"""Local English ASR comparison on a fixed human-speech corpus.

Run in the isolated ASR comparison container. Offline latency is NOT streaming
latency. Native streaming measurements are explicitly labelled separately.
"""
import argparse
import io
import json
import re
import time
from pathlib import Path

import numpy as np
import soundfile as sf
import torch
from datasets import Audio, load_dataset
from jiwer import wer


def corpus(root):
    root.mkdir(parents=True, exist_ok=True)
    manifest = root / 'manifest.json'
    if manifest.exists():
        return json.loads(manifest.read_text())
    dataset = load_dataset('hf-internal-testing/librispeech_asr_dummy', 'clean', split='validation').cast_column('audio', Audio(decode=False))
    rows = []
    for row in list(dataset)[:12]:
        audio, sr = sf.read(io.BytesIO(row['audio']['bytes']), dtype='float32')
        target = root / (row['id']+'.wav')
        sf.write(target, audio, sr)
        rows.append(dict(path=str(target), id=row['id'], reference=row['text'], duration_s=len(audio)/sr))
    manifest.write_text(json.dumps(rows, indent=2))
    return rows


def normal(text):
    return re.sub(r'[^a-z0-9\s]', '', text.lower()).strip()


def main(args):
    torch.set_num_threads(4)
    rows = corpus(Path(args.corpus))
    if args.engine == 'whisper':
        from faster_whisper import WhisperModel
        model = WhisperModel('large-v3-turbo', device='cuda', compute_type='float16', cpu_threads=4)
        def transcribe(path):
            segments, _ = model.transcribe(path, language='en', beam_size=1, vad_filter=False)
            return ' '.join(s.text for s in segments)
    elif args.engine == 'qwen':
        from transformers import AutoModelForMultimodalLM, AutoProcessor
        model_id = 'Qwen/Qwen3-ASR-0.6B-hf'
        processor = AutoProcessor.from_pretrained(model_id)
        model = AutoModelForMultimodalLM.from_pretrained(model_id, dtype=torch.bfloat16).cuda().eval()
        def transcribe(path):
            inputs = processor.apply_transcription_request(audio=path, language='English').to(model.device, model.dtype)
            with torch.inference_mode():
                output = model.generate(**inputs, max_new_tokens=256, do_sample=False)
            return processor.decode(output[:, inputs['input_ids'].shape[1]:], return_format='transcription_only')[0]
    elif args.engine == 'nemotron-en':
        import nemo.collections.asr as nemo_asr
        model = nemo_asr.models.ASRModel.from_pretrained('nvidia/nemotron-speech-streaming-en-0.6b').cuda().eval()
        def transcribe(path):
            output = model.transcribe([path], batch_size=1, verbose=False)
            return output[0].text if hasattr(output[0], 'text') else str(output[0])
    else:
        from transformers import AutoModelForRNNT, AutoProcessor
        model_id = 'nvidia/nemotron-3.5-asr-streaming-0.6b'
        processor = AutoProcessor.from_pretrained(model_id)
        model = AutoModelForRNNT.from_pretrained(model_id).cuda().eval()
        def transcribe(path):
            audio, sr = sf.read(path, dtype='float32')
            inputs = processor(audio, sampling_rate=sr, language='en-US', return_tensors='pt').to(model.device, model.dtype)
            with torch.inference_mode():
                output = model.generate(**inputs)
            return processor.batch_decode(output.sequences, skip_special_tokens=True)[0]
    # Exclude one explicit model warm-up, not the slow cases from measured runs.
    transcribe(rows[0]['path'])
    for row in rows:
        for run in range(args.runs):
            torch.cuda.synchronize()
            start = time.perf_counter()
            try:
                transcript = transcribe(row['path'])
                torch.cuda.synchronize()
                elapsed = time.perf_counter()-start
                result = dict(engine=args.engine, mode='whole-utterance', run=run, **row,
                              transcript=transcript, latency_ms=elapsed*1000,
                              rtf=elapsed/row['duration_s'], wer=wer(normal(row['reference']),normal(transcript)))
            except Exception as exc:
                result = dict(engine=args.engine, run=run, **row, error=repr(exc))
            print(json.dumps(result), flush=True)
            with open(args.output,'a') as out:
                out.write(json.dumps(result)+'\n')


if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--engine', choices=['whisper','qwen','nemotron-en','nemotron35'], required=True)
    p.add_argument('--corpus', default='/tmp/stt-corpus')
    p.add_argument('--runs',type=int,default=2)
    p.add_argument('--output',required=True)
    main(p.parse_args())
