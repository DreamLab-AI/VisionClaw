# Voice estate

Speech synthesis is owned by `agentbox/docker-compose.speech.yml`: one warm,
CPU-only `pocket-tts` service on `visionclaw_network`, started by both normal
Agentbox and VisionClaw launchers. HTTP `/v1/audio/speech` serves narration and
VisionClaw; incremental MessagePack `/api/tts_streaming` serves the web backend.

The `unmute/` checkout remains the source context for the web frontend/backend
and existing STT service. It no longer owns a speech synthesis container. Launch
the web stack using `agentbox/agentbox.sh voice up`, whose maintained service
definitions live in `agentbox/voice/compose.web.yml`. Do not launch the upstream
Compose file independently. See `agentbox/voice/README.md` for operator access.

Historical engine benchmarks and their retired harness live under
`docs/gap-close-evidence/voice-latency-2026-09-10/`; they are not deployment files.
