"""Local TTS comparison gateway with generation fencing and bounded audio lead.

WS /voice: {type:speak, text:..., engine:moshi|kokoro|pocket|qwen}, {type:cancel}.
Binary output: four-byte network-order generation ID followed by s16le 24kHz PCM.
On `cancelled` or a new `started` event the playback client MUST clear its buffer.
This is an isolated benchmark service, not a replacement for application auth.
"""
import asyncio
import os
import struct
import time

import aiohttp
from aiohttp import web
import msgpack

URLS = {k: os.getenv(k.upper() + "_URL", v) for k, v in {
    "kokoro": "http://voice-latency-kokoro:8880", "moshi": "http://agentbox-voice-tts-1:8080",
    "pocket": "http://voice-latency-pocket-tuned:8000", "qwen": "http://voice-latency-qwen:8000"
}.items()}


async def pcm_stream(session, engine, text):
    url = URLS[engine]
    if engine == "moshi":
        async with session.ws_connect(url + "/api/tts_streaming?format=PcmMessagePack&cfg_alpha=2&seed=42", headers={"kyutai-api-key": "public_token"}) as ws:
            while True:
                msg = msgpack.unpackb((await ws.receive()).data)
                if msg["type"] == "Ready":
                    break
                if msg["type"] == "Error":
                    raise RuntimeError(msg["message"])
            for word in text.split():
                await ws.send_bytes(msgpack.packb({"type": "Text", "text": word + " "}))
            await ws.send_bytes(msgpack.packb({"type": "Eos"}))
            async for event in ws:
                if event.type != aiohttp.WSMsgType.BINARY:
                    break
                msg = msgpack.unpackb(event.data)
                if msg["type"] == "Error":
                    raise RuntimeError(msg["message"])
                if msg["type"] == "Audio":
                    yield b"".join(struct.pack("<h", max(-32768, min(32767, int(v*32767)))) for v in msg["pcm"])
    else:
        if engine == "pocket":
            request = session.post(url + "/tts", data={"text": text})
        else:
            body = {"input": text, "voice": "af_heart", "model": "kokoro",
                    "stream": True, "response_format": "pcm", "speed": 1.0}
            if engine == "qwen":
                body.update(model="Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice", voice="ryan",
                            language="English", stream_format="audio")
            request = session.post(url + "/v1/audio/speech", json=body)
        async with request as response:
            response.raise_for_status()
            header = engine == "pocket"
            pending = b""
            async for data in response.content.iter_any():
                if header:
                    pending += data
                    pos = 12
                    while pos + 8 <= len(pending):
                        size = int.from_bytes(pending[pos+4:pos+8], "little")
                        if pending[pos:pos+4] == b"data":
                            data = pending[pos+8:]
                            header = False
                            break
                        pos += 8 + size + size % 2
                    if header:
                        continue
                yield data


async def voice(request):
    ws = web.WebSocketResponse(heartbeat=30)
    await ws.prepare(request)
    generation = 0
    active = None
    tasks = set()
    async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=120)) as session:
        async def produce(engine, text, gen):
            pending = b""
            due = None
            try:
                async for chunk in pcm_stream(session, engine, text):
                    pending += chunk
                    while len(pending) >= 960:
                        if gen != generation:
                            return
                        if due is None:
                            due = time.monotonic()
                        await asyncio.sleep(max(0, due - time.monotonic() - 0.040))
                        if gen != generation:
                            return
                        await ws.send_bytes(struct.pack("!I", gen) + pending[:960])
                        pending = pending[960:]
                        due += 0.020
                if pending and gen == generation:
                    await asyncio.sleep(max(0, (due or time.monotonic()) - time.monotonic() - 0.040))
                    if gen == generation:
                        await ws.send_bytes(struct.pack("!I", gen) + pending)
                if gen == generation:
                    await ws.send_json({"type": "done", "generation": gen})
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                if gen == generation and not ws.closed:
                    await ws.send_json({"type": "error", "generation": gen, "message": str(exc)})
        try:
            async for event in ws:
                if event.type != aiohttp.WSMsgType.TEXT:
                    continue
                message = event.json()
                if message.get("type") not in ("speak", "cancel"):
                    continue
                generation += 1
                if active:
                    active.cancel()
                if message["type"] == "cancel":
                    await ws.send_json({"type": "cancelled", "generation": generation})
                    continue
                engine = message.get("engine", os.getenv("DEFAULT_TTS_ENGINE", "pocket"))
                if engine not in URLS or not isinstance(message.get("text"), str):
                    await ws.send_json({"type": "error", "message": "Invalid engine or text"})
                    continue
                await ws.send_json({"type": "started", "generation": generation, "sample_rate": 24000})
                active = asyncio.create_task(produce(engine, message["text"][:10000], generation))
                tasks.add(active)
                active.add_done_callback(tasks.discard)
        finally:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
    return ws


app = web.Application()
app.router.add_get("/voice", voice)
app.router.add_get("/health", lambda request: web.json_response({"status": "ok"}))
if __name__ == "__main__":
    web.run_app(app, host=os.getenv("BIND", "127.0.0.1"), port=int(os.getenv("PORT", "8895")))
