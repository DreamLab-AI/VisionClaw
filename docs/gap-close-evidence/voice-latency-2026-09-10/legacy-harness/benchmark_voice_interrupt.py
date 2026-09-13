"""Measure a paced PCM gateway with scripted barge-in; no microphone or speaker.

Cancels after 500ms of audio, checks for stale frames after the acknowledgement,
then requests a replacement. Client buffer clearing is modelled, not acoustic.
"""
import argparse
import array
import asyncio
import json
import struct
import time
from pathlib import Path

import aiohttp

from benchmark_voice_latency import LONG, SHORT


async def trial(session, url, engine, run):
    async with session.ws_connect(url + "/voice") as ws:
        start = time.perf_counter()
        await ws.send_json({"type": "speak", "engine": engine, "text": LONG})
        first = None
        nonsilent = None
        received = 0
        gen = None
        while received < 24000:
            msg = await ws.receive(timeout=30)
            if msg.type == aiohttp.WSMsgType.TEXT:
                event = json.loads(msg.data)
                if event["type"] == "error":
                    raise RuntimeError(event)
                if event["type"] == "started":
                    gen = event["generation"]
            elif msg.type == aiohttp.WSMsgType.BINARY:
                if first is None:
                    first = time.perf_counter()
                if nonsilent is None and max(map(abs, array.array("h", msg.data[4:])), default=0) > 164:
                    nonsilent = time.perf_counter()
                received += len(msg.data)-4
            else:
                raise RuntimeError("Transport closed before audio")
        cancel = time.perf_counter()
        await ws.send_json({"type": "cancel"})
        in_flight = 0
        while True:
            msg = await ws.receive(timeout=30)
            if msg.type == aiohttp.WSMsgType.BINARY:
                in_flight += len(msg.data)-4
            elif msg.type == aiohttp.WSMsgType.TEXT and json.loads(msg.data)["type"] == "cancelled":
                break
        ack = time.perf_counter()
        await ws.send_json({"type": "speak", "engine": engine, "text": SHORT})
        stale = 0
        replacement_first = None
        while True:
            msg = await ws.receive(timeout=30)
            if msg.type == aiohttp.WSMsgType.BINARY:
                if struct.unpack("!I", msg.data[:4])[0] == gen:
                    stale += 1
                else:
                    if replacement_first is None:
                        replacement_first = time.perf_counter()
                    if max(map(abs, array.array("h", msg.data[4:])), default=0) > 164:
                        replacement = time.perf_counter()
                        break
            elif msg.type == aiohttp.WSMsgType.TEXT and json.loads(msg.data)["type"] == "error":
                raise RuntimeError(json.loads(msg.data))
            elif msg.type not in (aiohttp.WSMsgType.TEXT, aiohttp.WSMsgType.BINARY):
                raise RuntimeError("Transport closed before replacement")
        await ws.send_json({"type": "cancel"})
        return dict(engine=engine, run=run, first_audio_ms=(first-start)*1000,
                    first_nonsilent_ms=(nonsilent-start)*1000 if nonsilent else None,
                    cancel_ack_ms=(ack-cancel)*1000,
                    cancel_to_replacement_first_packet_ms=(replacement_first-cancel)*1000,
                    cancel_to_replacement_ms=(replacement-cancel)*1000,
                    in_flight_audio_ms=in_flight/48, stale_frames_after_ack=stale)


async def main(args):
    async with aiohttp.ClientSession() as session:
        for run in range(args.runs):
            try:
                result = await trial(session, args.url, args.engine, run)
            except Exception as exc:
                result = dict(engine=args.engine, run=run, error=repr(exc))
            print(json.dumps(result), flush=True)
            with Path(args.output).open("a") as f:
                f.write(json.dumps(result)+"\n")
            await asyncio.sleep(1)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--url", default="http://127.0.0.1:8895")
    p.add_argument("--engine", required=True)
    p.add_argument("--runs", type=int, default=10)
    p.add_argument("--output", required=True)
    asyncio.run(main(p.parse_args()))
