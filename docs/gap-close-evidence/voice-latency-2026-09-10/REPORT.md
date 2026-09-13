# Local voice latency baseline — 10 September 2026

## Decision

Use **patched Pocket TTS, warm and Dockerised, as the single-user English TTS baseline**. It won the interruption tests on this host, needs no GPU, and its cancellation fix survives container recreation. Keep the existing Kyutai STT detector. Do not replace speech synthesis with LiveKit: LiveKit is the media transport layer, not a TTS engine.

The isolated baseline is running at `ws://127.0.0.1:8898/voice`, defaulting to Pocket. This is a benchmark gateway, **not an application production cutover**. The existing Unmute/Moshi deployment and original host Kokoros service remain unchanged. Browser buffer flushing, authentication, microphone echo cancellation and application routing still need integration before a production switch.

## Comparable interaction results

Milliseconds, median / nearest-rank p95. Each engine received the same text, transport pacing and cancellation protocol. First speech means arrival of a 20 ms PCM frame containing samples above the amplitude threshold—not the first HTTP byte or a silent packet.

| Engine/configuration | Request → first non-silent frame | Cancel → non-silent replacement | Speech onset → non-silent replacement |
|---|---:|---:|---:|
| Pocket, cancellation patched, CPU | **204 / 214** | **200 / 275** | **294 / 300** |
| Kyutai TTS/Moshi, existing batch 2, GPU | 308 / 325 | 277 / 286 | 401 / 406 |
| Kokoros, latest + shared voices, CPU | 1,001 / 1,034 | 1,093 / 1,151 | 1,022 / 1,050 |
| Qwen3-TTS 0.6B, vLLM-Omni + chunk ramp, GPU | 491 / 1,566 | 499 / **8,653** | 1,056 / **4,440** |

First two columns: 10 scripted interactions per engine, cancel after receiving 500 ms of audio. Last column: five interactions per engine, a shared prerecorded phrase streamed in real time into the existing Kyutai STT pause predictor. Different cancellation timings explain differences between the columns. With these small sample counts, p95 is the observed maximum; it is not a production tail-latency estimate.

All 60 final interactions completed without protocol errors and with **zero stale-generation frames after cancellation acknowledgement**. Acknowledgement medians were 0.47–0.69 ms. These are local network/control measurements, NOT proof that physical speakers become silent within 1 ms. Clients must discard queued audio on cancellation; the gateway alone cannot do that.

## Streaming results

Direct engine calls, complete short/long prompts followed by immediate abort/replacement. These figures exclude gateway pacing, and a buffer containing non-silent samples may contain leading silence. Do not mix them with the paced table above.

| Engine | Short first-audio packet, median | Long first-audio packet, median | Long synthesis / generated duration, medians | Simulated underrun |
|---|---:|---:|---:|---:|
| Pocket patched, n=5 | 168 ms | 192 ms | 18.14 s / 38.72 s | 0 ms |
| Moshi existing, n=3 | 193 ms | 165 ms | 10.74 s / 44.40 s | 0 ms |
| Kokoros patched, n=5 | 324 ms | 738 ms | 8.84 s / 47.70 s | 0 ms |
| Qwen ramp, n=3 | 79 ms | 90 ms | 7.50 s / 52.48 s | 0 ms |

Underrun is a simulated sink with 160 ms initial lead, not a browser audio measurement. Moshi and Qwen short sets include a first-after-idle/startup outlier (461 ms and 2,298 ms respectively), retained in raw evidence. Model download/load/startup time was not benchmarked systematically. Different voices and speaking rates produce different durations; this is a deployment comparison, not an architecture-only controlled experiment.

Qwen illustrates why first packet is insufficient: its long non-silent buffer arrived at 162 ms median, but paced speech arrived substantially later because of leading silence. The ramp removed the initial streaming gaps seen with a 1→25-frame jump, yet did not eliminate severe repeated-interruption stalls. Their exact server-side cause remains unproven; queued/in-flight generation is a hypothesis, not a confirmed diagnosis.

All four short WAV samples transcribed back to “Hello, I am ready to help you explore the knowledge graph.” using the existing STT. This is an intelligibility smoke test, not listening preference, prosody, multilingual accuracy or comprehensive quality evaluation. Samples and transcripts are in [samples](samples/).

## What was changed and tested

### Pocket

Tested upstream revision `0c2db3bdea7c991c568989cc11b503f14483fabc`, default English model/Alba voice, one Torch inference thread. Pocket is a CPU-oriented local TTS implementation. [Upstream](https://github.com/kyutai-labs/pocket-tts)

The first HTTP cancellation wrapper was insufficient: internal autoregressive and decoder threads continued running. Repeated replacement latency rose from approximately 284 to 1,111 ms. The final patch propagates cancellation to both workers and joins them before releasing the single-model inference lock. The wrapper has a bounded two-item output queue and warms the model before health becomes ready. Ten direct model cancellations in the rebuilt image left no additional Python threads; close/join took 3.3–35.3 ms in that check. This is a single-inference service; multi-user concurrency and queueing capacity are not certified.

Changes are preserved in `config/voice-latency/pocket-cancellation.patch`, `scripts/pocket_tts_cancellable.py` and `config/Dockerfile.pocket-latency`. The running container was recreated from the final image, not left dependent on a writable-layer hot patch.

### Kokoros

Fetched upstream and compiled revision `29e99ad5a5aa64b97e1e8e963e6d73b0267d796a` in `/mnt/nvme/githubs/Kokoros-latency-candidate`; original source checkout remains intact. [Upstream](https://github.com/lucasjinreal/Kokoros)

Replaced deep per-request voice/style map copies with `Arc` sharing, added disconnected-stream checks to prevent further dispatch, and removed unconditional unused headless audio-library links. Kept ONNX automatic thread selection: explicit 2- and 8-thread trials regressed latency. The current synchronous ONNX chunk cannot be preempted midway, so HTTP cancellation does not imply immediate compute cancellation.

Original short first-packet results were 381–440 ms; the final five-trial median is 324 ms. The latest upstream alone was not faster, and long/replacement behaviour did not improve uniformly. The final Docker runtime includes its tested native libraries and eSpeak data. The earlier missing-eSpeak-data trial produced invalid shortened speech and is **excluded** from recommendations.

Release build and speech smoke tests passed. `cargo test -p kokoros --lib` compiled but finished 1 passed / 1 failed: the unchanged upstream `test_tokenize` feeds `heɪ ðɪs ɪz ˈlʌvliː!` while expecting the tokens for `Hello!`. That fixture was not edited or hidden. This is not a clean upstream test suite.

### Moshi / Kyutai TTS

The deployed Rust 0.6.4 / Python 0.2.13 versions already match the checked upstream versions at revision `e6a55d2722a65870ef52a6c9f6ecfc0e90f38362`. No unsupported “upgrade gain” is claimed. Here “Moshi” means the Kyutai TTS server used by Unmute, not a benchmark of the full native speech-to-speech dialogue model. [Upstream](https://github.com/kyutai-labs/moshi)

An isolated batch-1 snapshot on GPU 2 repeatedly returned `No free channels` during immediate replacement. Rejected it and retained existing batch 2 / CFG 2. The main improvement evaluated for Moshi is the shared generation-fenced, bounded-lead gateway, not a model recompilation. The production Unmute speed 1.4 / 0.5 s uninterruptible settings were not changed; the isolated tests call the TTS engine directly and use a separate detector harness.

### Qwen3-TTS and wider current alternatives

Deployed official `vllm/vllm-omni:v0.28.0`, `Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice`, Ryan/English on GPU 0. Tested asynchronous audio chunks, initial chunk 1 and ramp `[1,2,4,8,16,25]`, CUDA graphs and stage-local memory budgets. These follow the upstream serving/optimization approach; published H200 numbers are not measurements of this host. [Serving API](https://github.com/vllm-project/vllm-omni/blob/main/docs/serving/speech_api.md), [optimization documentation](https://github.com/vllm-project/vllm-omni/blob/main/docs/design/qwen3_omni_tts_performance_optimization.md)

Chatterbox Turbo, CosyVoice and Fish-family candidates were considered in the upstream survey, but not deployed or benchmarked here. This decision is the best measured baseline among **four deployed candidates**, not an exhaustive or universal SOTA ranking. Additional voice-cloning/multilingual/quality requirements could change the choice.

## Existing integration and LiveKit finding

- `src/services/speech_service.rs:74` defaults to Kokoro. Lines 355–459 POST `{kokoro.apiUrl}/v1/audio/speech`, with fallback `http://kokoro-tts-container:8880` and default format MP3.
- The original actual service is a host process, `./target/release/koko --instances 1 openai --ip 172.20.0.1 --port 8880`, working in `/mnt/nvme/githubs/Kokoros`. No original Kokoros Docker container was running. `172.20.0.1` is the `visionclaw_network` bridge gateway.
- The tested Kokoros streaming path emits raw PCM s16le/24 kHz regardless of MP3 preference. The application needs an explicit codec contract; assuming streamed MP3 is unsafe.
- Active voice containers are the `agentbox-voice-*` Unmute stack. No LiveKit server container was found on this Docker host. `client/src/services/LiveKitVoiceService.ts` exists, but no invocation was found; another service mentions it only in documentation. This does not rule out an external LiveKit deployment.
- LiveKit can carry audio if selected as the common transport, but it does not remove the need for STT/TTS or fix cancellation of already queued speech. Do not make a transport migration solely to chase a model first-packet number.

## Method and limitations

Host: dual Xeon Gold 6154, 72 logical CPUs; GPU 0 RTX A6000, GPUs 1/2 RTX 6000 Ada, each 48 GB. Existing GPU workloads remained present; no production service was stopped to improve benchmark numbers. Pocket/Kokoros used CPU; Moshi used existing GPU 1; Qwen GPU 0. This is representative of this shared host, not an isolated lab.

The direct script measures request start, first PCM packet, first buffer exceeding amplitude 0.005, generated samples, completion and simulated underruns. Gateway tests split into 20 ms PCM frames, pace with 40 ms lead (up to approximately 60 ms in the initial burst), attach monotonically increasing generation IDs, cancel upstream and suppress stale generations. Paced non-silent threshold is 164/32768. Both thresholds are simple audibility proxies.

Speech-triggered tests feed the same saved synthetic interruption at 80 ms/frame, with 800 ms silence prelude, through the existing STT stream. Detection is pause probability `prs[2] < 0.6` or a word event; all final trials triggered on a Step. Median detection took 102–104 ms. This is **not** real-microphone mouth-to-ear latency: no acoustic echo, noisy room, speaker drain, LLM generation, WebRTC jitter, browser scheduler or WAN is included. Text was supplied in full; incremental LLM-token-to-speech latency was not tested.

Before production consolidation: connect the selected TTS to the real application voice route, implement/verify client buffer flushing and stale-generation handling, then measure actual mic→detection→speaker silence and mic→LLM→speaker reply under representative concurrent load. The local baseline is ready for that integration; those end-to-end acceptance tests remain outstanding.

## Evidence and operations

- [Raw JSONL](raw/) includes rejected trials, not just winners. Only `*-interrupt-final.jsonl`, `*-bargein-final.jsonl`, the four direct sets in the streaming table, and the explicitly labelled recreation smoke test support final claims. `voice-kokoro-docker8.jsonl` is invalid due to missing phoneme data. `voice-moshi-tuned.jsonl` is the rejected batch-1 configuration. Older interruption files measured first packet only; do not compare them as non-silent latency.
- [Summary](summary.json), [exact image IDs](image-manifest.txt), [reproduction instructions](REPRODUCE.md).
- Baseline Pocket and gateway are Compose-managed, with the gateway exposed only on loopback. The tuned Kokoros comparison container also remains running on `visionclaw_network` without a published host port.
- Rejected Qwen, batch-1 Moshi and stock Pocket trials are stopped, releasing their runtime resources. Temporary host candidate servers/gateways were terminated. Old test containers/images/caches are retained for recovery/reproduction; no production containers or model data were deleted.
- User-owned changes to `agentbox` and `docs/dream-cycle/LEDGER.md` were preserved. No application API/data structs were changed.
