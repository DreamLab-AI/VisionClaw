"""Cancellation-aware Pocket TTS server for isolated latency trials.

One inference producer; bounded two-frame queue. Cancelling HTTP consumption
stops production after the current model frame and releases the inference lock.
"""
import asyncio
import concurrent.futures
import contextlib
import struct
import threading

from fastapi import FastAPI, Form
from fastapi.responses import StreamingResponse
from pocket_tts import TTSModel
import uvicorn


model = None
voice_state = None
gate = threading.Lock()


@contextlib.asynccontextmanager
async def lifespan(app):
    global model, voice_state
    model = TTSModel.load_model()
    voice_state = model.get_state_for_audio_prompt("alba")
    # Warm the model before health becomes ready.
    for _ in model.generate_audio_stream(voice_state, "Ready."):
        pass
    yield


app = FastAPI(lifespan=lifespan)


@app.get("/health")
def health():
    return {"status": "ok"}


@app.post("/tts")
async def tts(text: str = Form(...)):
    async def audio():
        loop = asyncio.get_running_loop()
        queue = asyncio.Queue(maxsize=2)
        stop = threading.Event()

        def put(value):
            future = asyncio.run_coroutine_threadsafe(queue.put(value), loop)
            while not stop.is_set():
                try:
                    future.result(timeout=0.05)
                    return True
                except concurrent.futures.TimeoutError:
                    pass
            future.cancel()
            return False

        def produce():
            while not gate.acquire(timeout=0.05):
                if stop.is_set():
                    return
            stream = None
            try:
                if stop.is_set():
                    return
                stream = model.generate_audio_stream(voice_state, text)
                for chunk in stream:
                    if stop.is_set():
                        break
                    pcm = (chunk.detach().cpu().numpy().clip(-1, 1) * 32767).astype("<i2").tobytes()
                    if not put(pcm):
                        break
            except Exception as exc:
                put(exc)
            finally:
                if stream is not None:
                    stream.close()
                gate.release()
                if not stop.is_set():
                    put(None)

        thread = threading.Thread(target=produce, daemon=True)
        thread.start()
        try:
            yield struct.pack("<4sI4s4sIHHIIHH4sI", b"RIFF", 0xFFFFFFFF, b"WAVE", b"fmt ", 16, 1, 1, 24000, 48000, 2, 16, b"data", 0xFFFFFFFF)
            while True:
                value = await queue.get()
                if value is None:
                    break
                if isinstance(value, Exception):
                    raise value
                yield value
        finally:
            stop.set()
    return StreamingResponse(audio(), media_type="audio/wav")


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8000)
