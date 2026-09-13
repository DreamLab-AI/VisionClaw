"""Speech-triggered interruption: real-time synthetic PCM -> existing Kyutai STT
pause predictor (<0.6) -> gateway cancel -> non-silent replacement PCM.
Excludes microphone hardware, echo cancellation, network beyond this host,
and speaker playback. The shared STT detector is identical for all TTS engines.
"""
import argparse
import array
import asyncio
import json
import struct
import time
from pathlib import Path

import aiohttp
import msgpack

from benchmark_voice_latency import LONG, SHORT
from voice_stream_gateway import pcm_stream, URLS


async def trial(session, args, pcm, run):
    async with session.ws_connect(args.url + "/voice") as gw, session.ws_connect(
        args.stt + "/api/asr-streaming", headers={"kyutai-api-key": "public_token"}
    ) as stt:
        ready = msgpack.unpackb((await stt.receive()).data)
        if ready["type"] != "Ready":
            raise RuntimeError(ready)
        await gw.send_json({"type": "speak", "engine": args.engine, "text": LONG})
        while True:
            msg = await gw.receive(timeout=30)
            if msg.type == aiohttp.WSMsgType.BINARY:
                break
            if msg.type == aiohttp.WSMsgType.TEXT and json.loads(msg.data)["type"] == "error":
                raise RuntimeError(json.loads(msg.data))
        await asyncio.sleep(0.3)
        queue = asyncio.Queue()
        async def drain():
            async for message in gw:
                await queue.put(message)
        drain_task = asyncio.create_task(drain())
        floats = [v / 32768 for v in array.array("h", pcm)]
        speech_offset = next((i/24000 for i, v in enumerate(floats) if abs(v)>0.005), 0)
        prelude = 0.8
        values = [0.0]*int(prelude*24000) + floats + [0.0]*48000
        start = time.perf_counter()
        async def feed():
            for i in range(0, len(values), 1920):
                await asyncio.sleep(max(0, start+i/24000-time.perf_counter()))
                await stt.send_bytes(msgpack.packb({"type":"Audio", "pcm":values[i:i+1920]}, use_single_float=True))
        feed_task = asyncio.create_task(feed())
        try:
            while True:
                raw = await stt.receive(timeout=15)
                if raw.type != aiohttp.WSMsgType.BINARY:
                    raise RuntimeError("STT closed")
                msg = msgpack.unpackb(raw.data)
                if msg["type"] == "Error":
                    raise RuntimeError(msg)
                elapsed = time.perf_counter()-start
                if elapsed < prelude:
                    continue
                if (msg["type"] == "Step" and msg["prs"][2] < .6) or msg["type"] == "Word":
                    detected = time.perf_counter()
                    trigger = msg["type"]
                    break
            await gw.send_json({"type": "cancel"})
            while True:
                response = await asyncio.wait_for(queue.get(), 10)
                if response.type == aiohttp.WSMsgType.TEXT and json.loads(response.data)["type"] == "cancelled":
                    ack = time.perf_counter()
                    cancelled_gen = json.loads(response.data)["generation"]
                    break
            await gw.send_json({"type":"speak", "engine":args.engine, "text":SHORT})
            stale = 0
            while True:
                response = await asyncio.wait_for(queue.get(), 30)
                if response.type == aiohttp.WSMsgType.TEXT:
                    if json.loads(response.data)["type"] == "error":
                        raise RuntimeError(json.loads(response.data))
                elif response.type == aiohttp.WSMsgType.BINARY:
                    gen = struct.unpack("!I", response.data[:4])[0]
                    if gen < cancelled_gen:
                        stale += 1
                    elif max(map(abs, array.array("h", response.data[4:])), default=0)>164:
                        replacement = time.perf_counter()
                        break
            await gw.send_json({"type":"cancel"})
            onset = start+prelude+speech_offset
            return dict(engine=args.engine, run=run, trigger=trigger,
                        speech_onset_to_detection_ms=(detected-onset)*1000,
                        speech_onset_to_cancel_ack_ms=(ack-onset)*1000,
                        speech_onset_to_replacement_ms=(replacement-onset)*1000,
                        stale_frames_after_ack=stale)
        finally:
            feed_task.cancel()
            drain_task.cancel()
            await asyncio.gather(feed_task, drain_task, return_exceptions=True)


async def main(args):
    async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=90)) as session:
        URLS["pocket"] = args.pocket
        fixture = Path(args.fixture)
        if fixture.exists():
            pcm = fixture.read_bytes()
        else:
            pcm = b"".join([chunk async for chunk in pcm_stream(session,"pocket","Stop. Please stop talking now.")])
            fixture.write_bytes(pcm)
        for run in range(args.runs):
            try:
                result = await trial(session,args,pcm,run)
            except Exception as exc:
                result = dict(engine=args.engine, run=run, error=repr(exc))
            print(json.dumps(result),flush=True)
            with Path(args.output).open("a") as f:
                f.write(json.dumps(result)+"\n")
            await asyncio.sleep(1)


if __name__ == "__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--url", default="http://127.0.0.1:8898")
    p.add_argument("--stt", default="http://172.21.0.7:8080")
    p.add_argument("--pocket", default="http://172.20.0.12:8000")
    p.add_argument("--engine",required=True)
    p.add_argument("--runs",type=int,default=5)
    p.add_argument("--output",required=True)
    p.add_argument("--fixture", default="/tmp/voice-bargein-fixture.pcm")
    asyncio.run(main(p.parse_args()))
