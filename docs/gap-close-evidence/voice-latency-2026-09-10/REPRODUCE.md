# Reproducing the local voice baseline

Run from `/mnt/mldata/githubs/AR-AI-Knowledge-Graph`. This isolated comparison uses its own Compose file; it does not use or change the production application launcher. Existing `visionclaw_network`, `agentbox-voice_default`, Kyutai TTS/STT containers and model cache volumes are prerequisites on this host.

## Start/check the selected baseline

```sh
docker compose -f docker-compose.voice-latency.yml up -d
curl -fsS http://127.0.0.1:8898/health
docker compose -f docker-compose.voice-latency.yml ps
```

The gateway defaults to Pocket. Send JSON on `ws://127.0.0.1:8898/voice`:

```json
{"type":"speak","text":"Hello. I am ready to help you explore the knowledge graph."}
```

`started` specifies generation and sample rate. Binary messages contain a four-byte big-endian generation ID followed by mono signed little-endian 16-bit PCM, 24 kHz. Send `{"type":"cancel"}` for interruption. Flush the client playback queue on `cancelled` and each new `started` event; discard frames from earlier generations. `done` means upstream generation is complete, not that the speaker has drained. Other tested engine names are `moshi`, `kokoro`, `qwen`; their service must be running to select them. The service is unauthenticated and intentionally loopback-only; do not publish it publicly.

## Rebuild images

Exact current image IDs are in `image-manifest.txt`. Base tags can move; use those IDs/digests when repeating this host's run. Local images are not pushed to a registry.

Pocket base was built from a clean checkout at revision `0c2db3bdea7c991c568989cc11b503f14483fabc` using its upstream Dockerfile. For a fresh checkout:

```sh
git clone https://github.com/kyutai-labs/pocket-tts /mnt/nvme/githubs/pocket-tts-reproduction
git -C /mnt/nvme/githubs/pocket-tts-reproduction checkout --detach 0c2db3bdea7c991c568989cc11b503f14483fabc
docker build -t voice-latency-pocket:upstream /mnt/nvme/githubs/pocket-tts-reproduction
docker build -f config/Dockerfile.pocket-latency -t voice-latency-pocket:cancellable .
docker build -f config/Dockerfile.voice-stream-gateway -t voice-latency-gateway:tested .
docker compose -f docker-compose.voice-latency.yml up -d
```

Use a new checkout path if that directory already exists. Do not rebuild the base from the patched `/tmp/voice-pocket-tts` worktree and then apply the patch again. The upstream base uses a floating uv image and runtime model download; the exact tested local image/cache is more reproducible than a future fresh build. The final derived image applies the saved cancellation patch and includes the HTTP wrapper. No GPU is requested. The cached default Alba model is warmed before readiness.

Kokoros source/build is retained in `/mnt/nvme/githubs/Kokoros-latency-candidate` at revision `29e99ad5a5aa64b97e1e8e963e6d73b0267d796a`, with the patch already applied. A new clean worktree needs `config/voice-latency/kokoros-latency.patch` applied once before `cargo build --release`. The tested image uses staged host-ABI runtime files, not a portable multi-stage source build:

```sh
docker build -f config/Dockerfile.kokoros-latency -t voice-latency-kokoro:shared-voices /mnt/nvme/githubs/Kokoros-latency-candidate/.voice-latency-image
```

The retained `.voice-latency-image/runtime` contains the release binary at `/usr/bin/koko`, its `ldd` native dependencies including the dynamic loader, and eSpeak data at `/app/espeak-ng-data`. Re-stage all dependencies if recompiling on another ABI. The original checkpoints/data are mounted read-only at runtime. The existing comparison container can be stopped/started directly; the optional Compose `kokoro` profile is for a fresh deployment without that container-name conflict. Do not use `compose --profile kokoro up` against the existing manually launched comparison container without first resolving ownership.

Moshi's tested production container remains `agentbox-voice-tts-1`. The `voice-latency-moshi:snapshot` image preserves its compiled writable layer for trials; do not assume the original `moshi-server:latest` image has the compiled binary. Batch-1 settings are retained solely as rejected evidence.

Qwen's experimental container is stopped. `docker start voice-latency-qwen` resumes the exact tested command and mounted `config/qwen3-tts-latency.yaml`. It uses official `vllm/vllm-omni:v0.28.0`, GPU 0, the named `voice-latency-qwen-cache` volume and `vllm serve Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice --omni --host 0.0.0.0 --port 8000 --deploy-config /app/qwen3-tts-latency.yaml --trust-remote-code`. Wait for `/health` and model warmup before testing. Stop it afterwards to release GPU memory. Do not run comparison load against an engine simultaneously from multiple harnesses.

## Benchmark commands

The existing `/tmp/voice-latency-venv` has `aiohttp==3.14.3`, `msgpack==1.2.1`. For a new environment, use `python3 -m venv <new-path>` and install those packages. Scripts append output: use a new output filename for every run. Resolve container addresses afresh; hardcoded IP defaults in the barge-in harness are the recorded test environment, not stable service discovery.

```sh
/tmp/voice-latency-venv/bin/python scripts/benchmark_voice_interrupt.py --url http://127.0.0.1:8898 --engine pocket --runs 10 --output /tmp/new-pocket-interrupt.jsonl
/tmp/voice-latency-venv/bin/python scripts/benchmark_voice_bargein.py --engine pocket --stt http://172.21.0.7:8080 --fixture docs/gap-close-evidence/voice-latency-2026-09-10/samples/voice-bargein-fixture.pcm --output /tmp/new-pocket-bargein.jsonl
```

The barge-in fixture already exists, so Pocket's fixture-generation address is not used. For direct streaming, resolve the chosen engine's current network address, then:

```sh
/tmp/voice-latency-venv/bin/python scripts/benchmark_voice_latency.py --engine pocket --url http://CURRENT_POCKET_IP:8000 --runs 5 --label reproduction --output /tmp/new-pocket-direct.jsonl
/tmp/voice-latency-venv/bin/python scripts/summarize_voice_latency.py /tmp/new-pocket-interrupt.jsonl /tmp/new-pocket-bargein.jsonl /tmp/new-pocket-direct.jsonl
```

For other engines change `--engine` and the direct URL; gateway comparisons continue to use port 8898. `scripts/check_voice_sample.py` saves a WAV and STT transcript, accepting `--engine`, `--url`, `--stt`, `--output`. This is an intelligibility smoke test only.

## Regression checks

```sh
/tmp/voice-latency-venv/bin/python -m unittest discover -s tests -p test_voice_stream_gateway.py -v
docker cp scripts/check_pocket_cancellation.py voice-latency-pocket-tuned:/tmp/check_pocket_cancellation.py
docker exec voice-latency-pocket-tuned /app/.venv/bin/python /tmp/check_pocket_cancellation.py
```

The model check loads a separate model, closes ten long streams after the first chunk, and asserts that both generation threads have exited each time. Run it outside latency measurements. All ten passed on the recreated image. Gateway regression verifies default engine, upstream closure and generation fencing. Python compilation and Compose validation passed. Kokoros' library suite has the existing fixture failure documented in the report; do not describe all tests as passing.

## Stop / rollback

```sh
docker compose -f docker-compose.voice-latency.yml stop
```

This stops only the new baseline gateway and Pocket service. Existing production voice services are not Compose-owned by this file. Original temporary patched Pocket/gateway containers were stopped and renamed with `-prebaseline`; their writable layers remain available. Rejected trial containers, images and model cache volumes were retained, not deleted. No production routing configuration was switched, so no application rollback is required.
