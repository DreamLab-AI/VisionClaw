"""Direct-engine streaming benchmark; no microphone, LLM, or speaker timings.

Requires aiohttp and msgpack. Output is JSONL, including failures. Abort closes
the transport after the first non-silent frame, then immediately requests speech.
"""
import argparse
import asyncio
import array
import json
import time
from pathlib import Path

import aiohttp
import msgpack

SHORT = "Hello. I am ready to help you explore the knowledge graph."
LONG = " ".join(["The knowledge graph connects people, projects, and ideas. We can explore these relationships together, and find useful connections. Now let us examine the next group of nodes."] * 4)


async def request(session, engine, url, text, abort=False, cfg=2.0):
    start = time.perf_counter()
    first = audible = ready = None
    frames = []
    samples = 0
    close_ms = None
    if engine in ("kokoro", "pocket", "qwen"):
        if engine == "pocket":
            response = await session.post(url + "/tts", data={"text": text})
        else:
            body = {
            "model": "kokoro", "input": text, "voice": "af_heart",
            "stream": True, "response_format": "pcm", "speed": 1.0}
            if engine == "qwen":
                body.update(model="Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice", voice="ryan",
                            language="English", stream_format="audio")
            response = await session.post(url + "/v1/audio/speech", json=body)
        response.raise_for_status()
        ready = time.perf_counter() - start
        async def chunks():
            pending = b""
            header_done = engine != "pocket"
            async for chunk in response.content.iter_any():
                pending += chunk
                if not header_done:
                    # Streaming WAV: scan RIFF chunks; never count the header as audio.
                    if len(pending) < 12:
                        continue
                    pos = 12
                    while pos + 8 <= len(pending):
                        size = int.from_bytes(pending[pos+4:pos+8], "little")
                        if pending[pos:pos+4] == b"data":
                            pending = pending[pos+8:]
                            header_done = True
                            break
                        pos += 8 + size + (size % 2)
                    if not header_done:
                        continue
                n = len(pending) // 2 * 2
                if n:
                    pcm = array.array("h", pending[:n])
                    pending = pending[n:]
                    yield [v / 32768 for v in pcm]
        transport = response
    else:
        transport = await session.ws_connect(url + f"/api/tts_streaming?format=PcmMessagePack&cfg_alpha={cfg}&seed=42", headers={"kyutai-api-key": "public_token"})
        while True:
            msg = msgpack.unpackb((await transport.receive()).data)
            if msg["type"] == "Ready":
                break
            if msg["type"] == "Error":
                raise RuntimeError(msg)
        ready = time.perf_counter() - start
        for word in text.split():
            await transport.send_bytes(msgpack.packb({"type": "Text", "text": word + " "}))
        await transport.send_bytes(msgpack.packb({"type": "Eos"}))
        async def chunks():
            async for packet in transport:
                if packet.type != aiohttp.WSMsgType.BINARY:
                    break
                msg = msgpack.unpackb(packet.data)
                if msg["type"] == "Error":
                    raise RuntimeError(msg)
                if msg["type"] == "Audio":
                    yield msg["pcm"]
    try:
        async for pcm in chunks():
            now = time.perf_counter() - start
            if not pcm:
                continue
            if first is None:
                first = now
            frames.append((now, len(pcm)))
            samples += len(pcm)
            if audible is None and max(map(abs, pcm)) > 0.005:
                audible = now
            if abort and audible is not None:
                break
    finally:
        t = time.perf_counter()
        if engine != "moshi":
            transport.close()
        else:
            await transport.close()
        close_ms = (time.perf_counter() - t) * 1000
    # Simulate playout starting with 160ms lead; report missing audio time.
    underrun = 0.0
    end = (first or 0) + 0.160
    for arrival, count in frames:
        underrun += max(0, arrival - end)
        end = max(end, arrival) + count / 24000
    return dict(engine=engine, cfg=cfg, abort=abort, ready_ms=ready*1000,
                first_audio_ms=first*1000 if first is not None else None,
                first_nonsilent_ms=audible*1000 if audible is not None else None,
                elapsed_ms=(time.perf_counter()-start)*1000, audio_s=samples/24000,
                chunks=len(frames), simulated_underrun_ms=underrun*1000,
                close_ms=close_ms)


async def main(args):
    async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=90)) as session:
        for run in range(args.runs):
            for name, text, abort in [("short", SHORT, False), ("long", LONG, False), ("interrupt", LONG, True), ("replacement", SHORT, False)]:
                try:
                    result = await asyncio.wait_for(request(session, args.engine, args.url, text, abort, args.cfg), 90)
                except Exception as exc:
                    result = dict(error=repr(exc), engine=args.engine)
                result.update(run=run, scenario=name, label=args.label)
                print(json.dumps(result), flush=True)
                with Path(args.output).open("a") as f:
                    f.write(json.dumps(result) + "\n")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--engine", choices=["kokoro", "moshi", "pocket", "qwen"], required=True)
    p.add_argument("--url", required=True)
    p.add_argument("--cfg", type=float, default=2.0)
    p.add_argument("--runs", type=int, default=5)
    p.add_argument("--label", default="baseline")
    p.add_argument("--output", required=True)
    asyncio.run(main(p.parse_args()))
