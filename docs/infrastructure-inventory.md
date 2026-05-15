# Turbo Flow Infrastructure Inventory

> Container: `agentbox` (Nix-based, replacing legacy `multi-agent-docker-agentic-workstation`) | OS: CachyOS Linux 7.0.1 | CUDA 13.2 | 32 CPU / 64GB RAM | Updated 2026-05-05
>
> **Note:** `multi-agent-docker` is on deprecation track per [ADR-058](adr/ADR-058-mad-to-agentbox-migration.md). New features land in `agentbox/`.

## Users & Accounts

| User | UID | Home | Purpose | Switch |
|------|-----|------|---------|--------|
| devuser | 1000 | /home/devuser | Primary dev, Claude Code, main workspace | - |
| gemini-user | 1001 | /home/gemini-user | Google Gemini API, gemini-flow daemon | `as-gemini` |
| openai-user | 1002 | /home/openai-user | OpenAI Codex MCP | `as-openai` |
| zai-user | 1003 | /home/zai-user | Z.AI service (port 9600) | `as-zai` |
| deepseek-user | 1004 | /home/deepseek-user | DeepSeek reasoning (pending) | `as-deepseek` |

## Model Vendors & Models

| Vendor | Model | Endpoint | Speed | Cost | Context | Reasoning | Best For |
|--------|-------|----------|-------|------|---------|-----------|----------|
| Anthropic | Claude Opus 4.6 | Direct (OAuth) | Slow | High | 200K | Excellent | Complex analysis, architecture |
| Anthropic | Claude Sonnet 4.6 | Direct (OAuth) | Medium | Medium | 200K | Very Good | Balanced production tasks |
| Anthropic | Claude Haiku 4.5 | Direct (OAuth) | Fast | Very Low | 200K | Good | Simple tasks, high throughput |
| Z.AI | GLM-4.7 | localhost:9600 | Fast | Very Low | 4K | Good | Cost-effective coding, prototyping |
| OpenAI | GPT-5.4 | Codex MCP socket | Medium | Medium | 128K+ | Excellent | Advanced reasoning, code review |
| Google | Gemini | MCP socket | Fast | Low | 1M+ | Good | Multimodal, long context |
| Qwen (LAN) | Qwen3.5-122B-A10B | 192.168.2.48:8080 | 37 tok/s gen, 442 tok/s prompt | Free (local) | 262K | Excellent | Reasoning, code gen, open-source |
| DeepSeek | DeepSeek-R1 | Pending | Medium | Low | 128K | Excellent | Reasoning, cost-effective |

## Skills (102 in project, 65 in user config)

| Category | Skills |
|----------|--------|
| AI/ML | pytorch-ml, cuda, deepseek-reasoning, comfyui, comfyui-3d, reasoningbank-learning, reasoningbank-patterns |
| Agentic | agentic-qe, agentic-lightning, agentic-jujutsu, hive-mind-advanced, hooks-automation, swarm-coordination, swarm-memory |
| AgentDB | agentdb-core, agentdb-learning, agentdb-memory, agentdb-patterns, agentdb-search |
| 3D/Graphics | blender, comfyui, comfyui-3d, pbr-rendering, wasm-js |
| Browser | chrome-devtools, playwright, agent-browser |
| Code Quality | pair-programming, webapp-testing, build-with-quality, qe-suite |
| DevOps | docker-manager, docker-orchestrator, kubernetes-ops, linux-admin, tmux-ops |
| Documents | pdf, xlsx, docx, pptx, latex-documents, docs-alignment |
| Flow Nexus | flow-nexus-core, flow-nexus-sandbox, flow-nexus-swarm |
| GIS/EDA | qgis, kicad, ngspice |
| GitHub | github-code-review, github-pr-manager, github-issues, github-actions, git-architect |
| Media | imagemagick, ffmpeg-processing, slack-gif-creator, text-processing |
| Networking | network-analysis, web-summary, gemini-url-context, perplexity |
| Platform | skill-builder, skill-creator, mcp-builder, prd2build |
| UI/UX | ui-ux-pro-max-skill (16 sub-components), theme-factory |
| V3 | v3-architecture, v3-memory, v3-performance, v3-security, v3-integration, v3-coordination, v3-neural, v3-testing |
| Strategy | wardley-maps, ontology-design, ontology-reasoning |
| Terminal | console-buddy, host-webserver-debug |
| Open Source | fossflow |
| Nostr | logseq-formatted |

## Claude-Flow V3 Hooks (26 registered)

| Hook | Type | Trigger | Purpose |
|------|------|---------|---------|
| pre-edit | PreToolUse | Before file edit | Validate changes |
| post-edit | PostToolUse | After file edit | Neural training, pattern capture |
| pre-command | PreToolUse | Before bash cmd | Command validation |
| post-command | PostToolUse | After bash cmd | Result capture |
| pre-task | PreToolUse | Before task spawn | Task routing |
| post-task | PostToolUse | After task complete | Result storage |
| route | Intelligence | Task received | 3-tier model routing (Booster/Haiku/Opus) |
| explain | Intelligence | On demand | Explain hook decisions |
| metrics | Analytics | Continuous | Performance tracking |
| init | System | Startup | Initialize hooks system |
| notify | Coordination | Cross-agent | Inter-agent notifications |
| session-start | SessionStart | Session begin | Restore state |
| session-end | SessionEnd | Session close | Persist state, generate summary |
| session-restore | SessionStart | Resume | Load previous session |
| pretrain | Intelligence | On demand | Pre-train neural patterns |
| build-agents | Intelligence | On demand | Generate agent configs |
| transfer | Intelligence | On demand | Knowledge transfer between agents |
| intelligence | Intelligence | Status check | Intelligence subsystem status |
| intelligence_trajectory_start | Intelligence | SONA begin | Start trajectory tracking |
| intelligence_trajectory_step | Intelligence | SONA step | Record trajectory step |
| intelligence_trajectory_end | Intelligence | SONA end | Consolidate trajectory |
| intelligence_pattern_store | Intelligence | Pattern found | Store pattern in memory |
| intelligence_pattern_search | Intelligence | Lookup | Search stored patterns |
| intelligence_stats | Analytics | On demand | Intelligence statistics |
| intelligence_learn | Intelligence | After task | Active learning cycle |
| intelligence_attention | Intelligence | Routing | Attention-based task routing |

## Memory Systems

| System | Backend | Features | Access |
|--------|---------|----------|--------|
| RuVector PostgreSQL | postgres:17.5 @ ruvector-postgres:5432 | pgvector + HNSW, 384-dim embeddings, 1.17M+ entries | `$RUVECTOR_PG_CONNINFO` |
| Claude-Flow MCP Memory | AgentDB (HNSW-indexed) | Semantic search, vector storage, namespaced | `mcp__claude-flow__memory_*` tools |
| Claude Auto-Memory | File-based @ `~/.claude/projects/*/memory/` | Per-project persistent notes | MEMORY.md + topic files |
| Session State | claude-flow sessions | Save/restore conversation state | `claude-flow session save/restore` |

## MCP Servers

| MCP Server | Port/Socket | Status | Purpose |
|------------|-------------|--------|---------|
| claude-flow | npx @claude-flow/cli | Active | Agent orchestration, memory, tasks, swarms, hooks |
| ruv-swarm | npx ruv-swarm | Active | Swarm coordination, DAA, neural |
| flow-nexus | npx flow-nexus | Active | Cloud platform, sandboxes, workflows, challenges |
| ruflo | /usr/local/bin/ruflo mcp | Active | Core flow tooling |
| aqe-mcp | /usr/local/bin/aqe-mcp | Active | Agentic QE testing tools |
| agent-browser | daemon on localhost | Active | Playwright browser automation |
| mcp-gateway | TCP :9500 / WS :3002 | Active | Gateway routing |
| openai-codex-mcp | Unix socket | Active | OpenAI codex_generate, codex_review |
| gemini-flow | Unix socket | Active | Google Gemini integration |
| blender-mcp | Supervised | Active | Blender 3D modeling |
| qgis-mcp | Supervised | Active | QGIS geospatial |
| web-summary-mcp | Supervised | Stopped (on-demand) | Web page summarization |
| imagemagick-mcp | Supervised | Stopped (on-demand) | Image manipulation |
| kicad-mcp | Supervised | Stopped (on-demand) | Electronics design |
| ngspice-mcp | Supervised | Stopped (on-demand) | Circuit simulation |
| pbr-mcp | Supervised | Stopped (on-demand) | PBR rendering |
| playwright-mcp | Supervised | Stopped (on-demand) | Browser automation (fallback) |
| perplexity-mcp | Supervised | Stopped (on-demand) | Web search & research |
| jupyter-notebooks-mcp | Supervised | Stopped (on-demand) | Notebook execution |
| host-webserver-debug-mcp | Supervised | Stopped (on-demand) | Host server debugging |

## External Docker Containers

| Container | Image | Status | Ports | Purpose |
|-----------|-------|--------|-------|---------|
| agentic-workstation | multi-agent-docker-agentic-workstation | Healthy | 2222, 5901, 8080, 9090 | This container |
| visionclaw_container | ar-ai-knowledge-graph-visionclaw | Healthy | 3001, 4000 | VisionClaw knowledge graph app |
| visionclaw-neo4j | neo4j:5.13.0 | Healthy | 7474, 7687 | Graph database |
| visionclaw-jss | ar-ai-knowledge-graph-jss | Unhealthy | 3030 | JSS service |
| kokoro-tts-container | kokoro-fastapi-gpu | Up | 8880 | GPU text-to-speech |
| docker-es01-1 | elasticsearch:8.11.3 | Healthy | 1200 | Search & analytics |
| docker-minio-1 | quay.io/minio/minio | Healthy | 9000, 9001 | S3-compatible storage |
| docker-redis-1 | valkey/valkey:8 | Healthy | 6379 | Cache / session store |
| comfyui | comfyui-sam3d | Healthy | 8188 | AI image generation |
| vircadia_world_postgres | postgres:17.5-alpine | Unhealthy | 5432 | Virtual world DB |
| whisper-webui | whisper-webui | Up | 7860 | Speech-to-text |
| buildx_buildkit_mybuilder0 | moby/buildkit | Up | - | Docker buildkit |

## Local Tools & Runtimes

| Tool | Version | Path |
|------|---------|------|
| Claude Code | 2.1.47 | /usr/local/bin/claude |
| Claude Flow | v3 | /usr/local/bin/claude-flow |
| Ruflo | 3.5.14 | /usr/local/bin/ruflo |
| Gemini Flow | 2.1.0-alpha.1 | /usr/local/bin/gemini-flow |
| Agent Browser | 0.7.6 | /usr/local/bin/agent-browser |
| OpenAI CLI | 6.27.0 | /usr/local/bin/openai |
| Node.js | 23.11.1 | /usr/local/bin/node |
| npm | 11.11.0 | /usr/local/bin/npm |
| Python | 3.12.8 | /opt/venv/bin/python3 |
| PyTorch | 2.10.0+cu130 | /opt/venv (with CUDA 13) |
| Cargo (Rust) | 1.94.0 | /usr/bin/cargo |
| Git | 2.53.0 | /usr/bin/git |
| tmux | latest | /usr/bin/tmux |
| code-server | 4.96.2 | /usr/bin/code-server |
| Docker CLI | installed | /usr/bin/docker |
| Xvfb | installed | /usr/bin/Xvfb |
| x11vnc | installed | /usr/bin/x11vnc |
| qwen-ask | local | /usr/local/bin/qwen-ask |
| qwen-chat | local | /usr/local/bin/qwen-chat |

## Agent Templates (610)

| Category | Count (approx) | Examples |
|----------|----------------|---------|
| Development & Engineering | 150+ | backend-api-code-writer, react-19-specialist, typescript-specialist, python-specialist, microservices-distributed-systems |
| AI/ML & Automation | 70+ | tensorflow-machine-learning, pytorch-deep-learning, ai-ml-engineering, anomaly-detection |
| Business & Strategy | 90+ | business-growth-scaling, competitive-differentiation, revenue-growth-manager, customer-journey-orchestration |
| Security & Compliance | 60+ | security-architecture, penetration-testing, compliance-auditor |
| Integration & APIs | 80+ | api-integration-architect, payment-processor (Stripe, Alipay, Apple Pay, Amazon Pay, Authorize.net) |
| Testing & QA | 40+ | acceptance-criteria-agent, atdd-specialist, automated-insights-reporting |
| Infrastructure & DevOps | 50+ | kubernetes-orchestration, argocd-gitops, aws-cloud, backup-restore |
| Research & Analysis | 30+ | advanced-research-engine, analytics-insights-engineer, attention-pattern |
| Specialized | 40+ | accessibility-specialist, angular-specialist, astro-static, async-rust, autonomous-refactoring |

## Tmux Session Layout (12 windows)

| Window | Name | Purpose |
|--------|------|---------|
| 0 | Claude-Main | Primary interactive workspace |
| 1 | Claude-Agent | Agent execution |
| 2 | Services | supervisord monitoring |
| 3 | Development | Python/Rust/CUDA dev |
| 4 | Logs | Service logs (split pane) |
| 5 | System | htop monitoring |
| 6 | VNC-Status | VNC server info |
| 7 | SSH-Shell | General shell |
| 8 | Gemini-Shell | gemini-user shell |
| 9 | OpenAI-Shell | openai-user shell |
| 10 | ZAI-Shell | zai-user shell |
| 11 | DeepSeek-Shell | deepseek-user shell |

## Service Ports

| Port | Service | Access | Auth |
|------|---------|--------|------|
| 22 (→2222) | SSH | Public | devuser:turboflow |
| 5901 | VNC | Public | None |
| 8080 | code-server | Public | Password |
| 9090 | Management API | Public | X-API-Key header |
| 9600 | Z.AI | Internal | None |
| 9500 | MCP Gateway (TCP) | Internal | None |
| 3002 | MCP Gateway (WS) | Internal | None |
| 3001 | HTTPS Bridge | Internal | None |

## Docker Networks

| Network | Driver | Purpose |
|---------|--------|---------|
| bridge | bridge | Default Docker |
| visionclaw_network | bridge | RuVector/ML platform |
| vircadia_internal_network | bridge | Virtual world (internal) |
| vircadia_public_network | bridge | Virtual world (public) |
