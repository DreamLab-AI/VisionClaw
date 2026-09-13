"""Run inside the patched Pocket image: cancelled streams must join both workers."""
import json
import threading
import time

from pocket_tts import TTSModel


model = TTSModel.load_model()
voice = model.get_state_for_audio_prompt("alba")
for _ in model.generate_audio_stream(voice, "Ready."):
    pass
baseline = {thread.ident for thread in threading.enumerate()}
results = []
for run in range(10):
    stream = model.generate_audio_stream(voice, "This is a long response that should stop when interrupted. " * 12)
    next(stream)
    start = time.perf_counter()
    stream.close()
    leaked = [thread.name for thread in threading.enumerate() if thread.ident not in baseline]
    assert not leaked, leaked
    results.append(dict(run=run, close_ms=(time.perf_counter()-start)*1000, leaked_threads=leaked))
print(json.dumps(results))
