"""Real-time paced audio: native Nemotron streaming versus existing Kyutai STT.

Measures first text and final text arrival, not model loading or microphone/AEC.
One second of trailing silence is fed uniformly. Kyutai has no final-message
contract here; final text means last observed Word within that drain window.
"""
import argparse
import asyncio
import json
import threading
import time

import aiohttp
import msgpack
import numpy as np
import soundfile as sf
import torch
from scipy.signal import resample_poly
from jiwer import wer

from benchmark_stt_estate import corpus, normal
from pathlib import Path


async def kyutai(row, args):
    audio, sr = sf.read(row['path'], dtype='float32')
    audio = resample_poly(audio, 3, 2)
    samples = np.concatenate([audio, np.zeros(24000, dtype=np.float32)])
    async with aiohttp.ClientSession() as session, session.ws_connect(args.url+'/api/asr-streaming', headers={'kyutai-api-key':'public_token'}) as ws:
        ready = msgpack.unpackb((await ws.receive()).data)
        if ready['type'] != 'Ready':
            raise RuntimeError(ready)
        start = time.perf_counter()
        first = last = None
        words = []
        async def feed():
            for i in range(0,len(samples),1920):
                # Only deliver samples once a real microphone could collect them.
                await asyncio.sleep(max(0,start+min(i+1920,len(samples))/24000-time.perf_counter()))
                await ws.send_bytes(msgpack.packb({'type':'Audio','pcm':samples[i:i+1920].tolist()},use_single_float=True))
            await asyncio.sleep(.15)
            await ws.close()
        sender = asyncio.create_task(feed())
        try:
            async for event in ws:
                if event.type == aiohttp.WSMsgType.BINARY:
                    msg=msgpack.unpackb(event.data)
                    if msg['type']=='Word':
                        words.append(msg['text'])
                        last=time.perf_counter()
                        first=first or last
                    elif msg['type']=='Error':
                        raise RuntimeError(msg)
            await sender
        finally:
            sender.cancel()
            await asyncio.gather(sender, return_exceptions=True)
    return ' '.join(words), (first-start)*1000 if first else None, (last-start-row['duration_s'])*1000 if last else None


def nemotron(row, processor, model):
    from transformers import TextIteratorStreamer
    audio,sr=sf.read(row['path'],dtype='float32')
    audio=np.concatenate([audio,np.zeros(sr,dtype=np.float32)])
    first_inputs=processor(audio[:processor.num_samples_first_audio_chunk],sampling_rate=sr,is_streaming=True,is_first_audio_chunk=True,language='en-US',return_tensors='pt').to(model.device, model.dtype)
    start=time.perf_counter()
    def features():
        time.sleep(max(0,start+processor.num_samples_first_audio_chunk/sr-time.perf_counter()))
        yield first_inputs.input_features[:,:processor.num_mel_frames_first_audio_chunk,:]
        mel=processor.num_mel_frames_first_audio_chunk
        hop=processor.feature_extractor.hop_length
        n_fft=processor.feature_extractor.n_fft
        offset=mel*hop-n_fft//2
        while offset < len(audio):
            end=offset+processor.num_samples_per_audio_chunk
            chunk=audio[offset:end]
            if len(chunk)<processor.num_samples_per_audio_chunk:
                chunk=np.pad(chunk,(0,processor.num_samples_per_audio_chunk-len(chunk)))
            time.sleep(max(0,start+min(end,len(audio))/sr-time.perf_counter()))
            inputs=processor(chunk,sampling_rate=sr,is_streaming=True,is_first_audio_chunk=False,language='en-US',return_tensors='pt').to(model.device,model.dtype)
            yield inputs.input_features
            mel+=processor.num_mel_frames_per_audio_chunk
            offset=mel*hop-n_fft//2
    streamer=TextIteratorStreamer(processor.tokenizer,skip_special_tokens=True,timeout=45)
    errors=[]
    def generate():
        try:
            with torch.inference_mode():
                model.generate(**{**first_inputs,'input_features':features(),'streamer':streamer})
        except Exception as exc:
            errors.append(exc)
            streamer.end()
    thread=threading.Thread(target=generate)
    thread.start()
    pieces=[]
    first=last=None
    for text in streamer:
        if text.strip():
            last=time.perf_counter()
            first=first or last
        pieces.append(text)
    thread.join(timeout=5)
    if errors:
        raise errors[0]
    return ''.join(pieces), (first-start)*1000 if first else None,(last-start-row['duration_s'])*1000 if last else None


def main(args):
    rows=corpus(Path(args.corpus))
    if args.engine=='nemotron35':
        from transformers import AutoModelForRNNT, AutoProcessor
        torch.set_num_threads(4)
        processor=AutoProcessor.from_pretrained('nvidia/nemotron-3.5-asr-streaming-0.6b')
        processor.set_num_lookahead_tokens(args.lookahead)
        model=AutoModelForRNNT.from_pretrained('nvidia/nemotron-3.5-asr-streaming-0.6b').cuda().eval()
        nemotron(rows[0],processor,model)
    for row in rows[:args.limit]:
        try:
            result=asyncio.run(kyutai(row,args)) if args.engine=='kyutai' else nemotron(row,processor,model)
            text,first,last=result
            record=dict(engine=args.engine,mode='realtime-stream',**row,transcript=text,first_text_ms=first,last_text_after_speech_ms=last,wer=wer(normal(row['reference']),normal(text)))
        except Exception as exc:
            record=dict(engine=args.engine,**row,error=repr(exc))
        print(json.dumps(record),flush=True)
        with open(args.output,'a') as out:
            out.write(json.dumps(record)+'\n')


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--engine',choices=['kyutai','nemotron35'],required=True)
    p.add_argument('--corpus',default='/tmp/stt-corpus')
    p.add_argument('--url',default='http://172.21.0.7:8080')
    p.add_argument('--lookahead',type=int,default=3)
    p.add_argument('--limit',type=int,default=12)
    p.add_argument('--output',required=True)
    main(p.parse_args())
