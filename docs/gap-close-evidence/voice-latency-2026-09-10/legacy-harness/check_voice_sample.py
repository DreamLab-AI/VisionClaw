"""Save one short WAV and transcribe it with existing Kyutai STT (smoke test only)."""
import argparse
import array
import asyncio
import json
import wave
from pathlib import Path

import aiohttp
import msgpack

from benchmark_voice_latency import SHORT
from voice_stream_gateway import URLS, pcm_stream


async def main(args):
    URLS[args.engine] = args.url
    async with aiohttp.ClientSession() as session:
        pcm = b"".join([chunk async for chunk in pcm_stream(session, args.engine, SHORT)])
        with wave.open(args.output, "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(24000)
            wav.writeframes(pcm)
        async with session.ws_connect(args.stt + "/api/asr-streaming", headers={"kyutai-api-key": "public_token"}) as ws:
            ready = msgpack.unpackb((await ws.receive()).data)
            if ready["type"] != "Ready":
                raise RuntimeError(ready)
            values = [0.0]*19200 + [v/32768 for v in array.array("h", pcm)] + [0.0]*72000
            async def feed():
                for i in range(0, len(values), 1920):
                    await ws.send_bytes(msgpack.packb({"type": "Audio", "pcm": values[i:i+1920]}, use_single_float=True))
                    await asyncio.sleep(.08)
                await asyncio.sleep(.3)
                await ws.close()
            sender = asyncio.create_task(feed())
            words = []
            async for event in ws:
                if event.type == aiohttp.WSMsgType.BINARY:
                    msg = msgpack.unpackb(event.data)
                    if msg["type"] == "Word":
                        words.append(msg["text"])
            await sender
        result = dict(engine=args.engine, input=SHORT, transcript=" ".join(words), audio_s=len(pcm)/48000)
        print(json.dumps(result), flush=True)
        Path(args.output + ".json").write_text(json.dumps(result, indent=2)+"\n")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--engine", required=True)
    p.add_argument("--url", required=True)
    p.add_argument("--stt", default="http://172.21.0.7:8080")
    p.add_argument("--output", required=True)
    asyncio.run(main(p.parse_args()))
