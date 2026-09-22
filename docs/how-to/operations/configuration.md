---
title: Configuration Guide
description: *[Guides](README.md)*
category: how-to
tags:
  - tutorial
  - api
  - api
  - docker
  - backend
updated-date: 2025-12-18
difficulty-level: intermediate
---


# Configuration Guide

*[Guides](README.md)*

This practical guide covers common configuration scenarios and use cases for VisionClaw. For comprehensive technical reference, see the .

## Quick Configuration Setup

### Basic Development Setup

Get VisionClaw running locally in development mode:

```bash
# 1. Copy environment template
cp .env-template .env

# 2. Set essential variables
cat >> .env << 'EOF'
# Core Settings
CLAUDE-FLOW-HOST=multi-agent-container
MCP-TCP-PORT=9500
HOST-PORT=3001

# Security (generate a strong secret!)
JWT-SECRET=your-very-secure-256-bit-secret-key-here-please-change-this

# Database
POSTGRES-PASSWORD=dev-password-change-in-production

# Development flags
DEBUG-MODE=true
RUST-LOG=debug
HOT-RELOAD=true
EOF

# 3. Start development environment
docker-compose --profile dev up
```

### Production Deployment

Configure for production deployment:

```bash
# 1. Generate secure secrets
JWT-SECRET=$(openssl rand -hex 32)
POSTGRES-PASSWORD=$(openssl rand -hex 24)
CLOUDFLARE-TUNNEL-TOKEN="your-cloudflare-tunnel-token"

# 2. Set production environment
cat > .env << EOF
# Production Configuration
ENVIRONMENT=production
DEBUG-MODE=false
RUST-LOG=info

# Security
JWT-SECRET=$JWT-SECRET
POSTGRES-PASSWORD=$POSTGRES-PASSWORD
CLOUDFLARE-TUNNEL-TOKEN=$CLOUDFLARE-TUNNEL-TOKEN

# Performance
MEMORY-LIMIT=16g
CPU-LIMIT=8.0
ENABLE-GPU=true
MAX-AGENTS=20

# Network
HOST-PORT=3001
DOMAIN=your-domain.com
EOF

# 3. Deploy with production configuration
docker-compose -f docker-compose.yml -f docker-compose.prod.yml --profile production up -d
```

## Common Configuration Scenarios

### GPU-Accelerated Setup

Enable GPU acceleration for better performance:

```bash
# Environment variables for GPU
ENABLE-GPU=true
NVIDIA-VISIBLE-DEVICES=0              # Use first GPU
CUDA-ARCH=89                          # RTX 40xx series
GPU-MEMORY-LIMIT=8g

# Start with GPU support
docker-compose -f docker-compose.yml -f docker-compose.gpu.yml up -d
```

Verify GPU is working:
```bash
# Check GPU status
docker exec -it visionclaw-container nvidia-smi

# Check GPU utilisation
curl http://localhost:3030/api/analytics/gpu-metrics
```

### Multi-Agent System Optimisation

Configure for large-scale multi-agent deployments:

```bash
# High-performance agent configuration
MAX-AGENTS=50
AGENT-TIMEOUT=600
TASK-QUEUE-SIZE=10000
WORKER-THREADS=16

# Memory allocation
MEMORY-LIMIT=32g
SHARED-MEMORY-SIZE=4g

# Neural enhancement
ENABLE-NEURAL-ENHANCEMENT=true
ENABLE-WASM-ACCELERATION=true
NEURAL-BATCH-SIZE=128
```

Update `data/settings.yaml` for agent-specific settings:
```yaml
system:
  agent-management:
    max-concurrent-agents: 50
    agent-spawn-timeout: 60
    agent-heartbeat-interval: 30
    enable-agent-persistence: true
    
  performance:
    enable-load-balancing: true
    task-distribution-strategy: "adaptive"
    resource-monitoring: true
```

### XR/VR Configuration

Enable extended reality features for Quest 3 and other XR devices:

```bash
# XR Environment variables
ENABLE-XR=true
QUEST3-SUPPORT=true
HAND-TRACKING=true
XR-RENDER-SCALE=1.2
XR-REFRESH-RATE=90
```

Configure XR settings in `data/settings.yaml`:
```yaml
xr:
  enabled: true
  client-side-enable-xr: true
  mode: "immersive-vr"
  space-type: "local-floor"
  quality: high
  render-scale: 1.2
  
  # Hand tracking
  enable-hand-tracking: true
  hand-mesh-enabled: true
  gesture-smoothing: 0.8
  
  # Comfort settings
  locomotion-method: teleport
  enable-passthrough-portal: true
  passthrough-opacity: 0.8
```

### Knowledge Graph Integration

Configure for vault and GitHub integration:

```bash
# GitHub integration
GITHUB-TOKEN=ghp-your-github-personal-access-token
GITHUB-SYNC-INTERVAL=300
ENABLE-GITHUB-WEBHOOKS=true

# Vault corpus source (ADR-2114 / ADR-2115: local by default, GitHub optional)
CORPUS_SOURCE=local
VAULT_ROOT=/vault/visionGraph
VAULT_BASE_PATHS=knowledge/pages,working/pages
```

Set up graph-specific visualisation in `data/settings.yaml`. The graph key is `knowledge`
(`GraphsSettings::knowledge`, `crates/visionclaw-domain/src/config/visualisation.rs`;
[ADR-2041](../../adr/ADR-2041-graph-settings-key-knowledge.md)). The former `logseq` key is
rejected like any other unknown key since
[ADR-2115](../../adr/ADR-2115-github-sync-is-optional-and-off.md) retired the read alias:
```yaml
visualisation:
  graphs:
    knowledge:
      physics:
        enabled: true
        spring-strength: 0.005
        repulsion-strength: 50.0
        damping: 0.9
        max-velocity: 1.0
        iterations: 200
      
      nodes:
        base-colour: '#a06522'
        node-size: 1.8
        enable-hologram: true
      
      labels:
        enable-labels: true
        show-metadata: true
        desktop-font-size: 1.2
```

## Performance Tuning

### Memory Optimisation

Configure memory usage for your available resources:

```bash
# For 16GB system
MEMORY-LIMIT=12g
SHARED-MEMORY-SIZE=2g
POSTGRES-SHARED-BUFFERS=1GB
REDIS-MAX-MEMORY=512mb

# For 32GB system
MEMORY-LIMIT=24g
SHARED-MEMORY-SIZE=4g
POSTGRES-SHARED-BUFFERS=2GB
REDIS-MAX-MEMORY=2gb

# For 64GB+ system
MEMORY-LIMIT=48g
SHARED-MEMORY-SIZE=8g
POSTGRES-SHARED-BUFFERS=4GB
REDIS-MAX-MEMORY=4gb
```

### CPU Configuration

Optimise CPU usage based on your hardware:

```bash
# For 4-core system
CPU-LIMIT=4.0
CPU-RESERVATION=2.0
WORKER-THREADS=4

# For 8-core system
CPU-LIMIT=8.0
CPU-RESERVATION=4.0
WORKER-THREADS=8

# For 16+ core system
CPU-LIMIT=16.0
CPU-RESERVATION=8.0
WORKER-THREADS=16
```

### Network Performance

Configure for high-throughput scenarios:

```bash
# High-performance networking
MAX-CONCURRENT-REQUESTS=5000
WS-CONNECTION-LIMIT=1000
RATE-LIMIT-MAX=10000

# WebSocket optimisation
WS-BINARY-CHUNK-SIZE=4096
WS-UPDATE-RATE=120
COMPRESSION-ENABLED=true
```

## Security Configuration

### Nostr Bead Provenance

VisionClaw publishes a signed Nostr event (kind 30001, NIP-33) to the JSS relay for each
completed brief → debrief cycle, providing cryptographic provenance. The
`BeadLifecycleOrchestrator` coordinates the full
lifecycle with retry, outcome classification, and learning capture (see
[ADR-034](../../archive/adr/ADR-034-needle-bead-provenance.md) and
[PRD](../../archive/prd/prd-bead-provenance-upgrade.md)).

```bash
# Bridge bot private key (64-char hex). Generate with: openssl rand -hex 32
VISIONCLAW_NOSTR_PRIVKEY=<64-char hex secret key>

# JSS integrated Nostr relay (default shown — matches docker-compose service name)
JSS_RELAY_URL=ws://jss:3030/relay

# DreamLab forum relay for NIP-29 group messages (bridge re-publishes here)
FORUM_RELAY_URL=wss://forum.dreamlab.ai/relay
```

Also required in JSS environment:
```bash
JSS_NOSTR=true   # Enables the JSS integrated relay (set in docker-compose.unified.yml)
```

Bridge bot public key (for forum relay whitelist):
```
eb47d8a792a4709329270a9f85f012326c61867a913791dc5f89dc7a0a760754
```

### Bead Retry Configuration

The publisher retries transient failures (timeout, connection error) with exponential backoff.
Permanent failures (signing error, relay rejection) fail immediately. See
`BeadRetryConfig`.

```bash
# Maximum publish attempts before marking as failed (default: 3)
BEAD_RETRY_MAX_ATTEMPTS=3

# Base delay in milliseconds for first retry (default: 1000)
BEAD_RETRY_BASE_DELAY_MS=1000

# Maximum delay cap in milliseconds (default: 10000)
BEAD_RETRY_MAX_DELAY_MS=10000

# Backoff multiplier — delay doubles each attempt (default: 2.0)
BEAD_RETRY_BACKOFF_MULTIPLIER=2.0
```

Backoff sequence with defaults: 1s → 2s → 4s (capped at 10s).

### Bridge Reconnection

The [`NostrBridge`](../../../src/services/nostr_bridge.rs) uses exponential backoff when
the JSS relay connection drops:

| Attempt | Delay | Notes |
|---------|-------|-------|
| 1 | 5s | Initial reconnect |
| 2 | 10s | |
| 3 | 20s | |
| 4 | 40s | |
| 5 | 80s | |
| 6 | 160s | |
| 7+ | 300s | Maximum cap |

If a reconnection succeeds and remains healthy for > 60 seconds, the backoff resets to 5s.
Monitor bridge health via `BridgeHealth::is_connected()` and `BridgeHealth::last_event_age_secs()`.

This key must be added to the forum relay's D1 allowlist before the bridge can publish.

---

### Authentication Setup

Configure Nostr-based authentication:

```bash
# Nostr authentication
AUTH-PROVIDER=nostr
AUTH-REQUIRED=true
SESSION-TIMEOUT=86400
```

Update authentication settings:
```yaml
auth:
  enabled: true
  provider: nostr
  required: true
  session-duration: 86400
  
  nostr:
    relay-urls:
      - "wss://relay.damus.io"
      - "wss://nos.lol" 
      - "wss://relay.snort.social"
    event-kinds: [1, 30023]
    max-event-size: 65536
```

### Security Hardening

Production security configuration:

```bash
# Security headers
HSTS-MAX-AGE=31536000
X-FRAME-OPTIONS=DENY
CONTENT-SECURITY-POLICY="default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'"

# CORS configuration
CORS-ORIGINS="https://your-domain.com,https://www.your-domain.com"
CORS-CREDENTIALS=true

# Rate limiting
RATE-LIMIT-ENABLED=true
RATE-LIMIT-MAX=1000
RATE-LIMIT-WINDOW=900

# SSL/TLS
SSL-CERT-PATH=/etc/ssl/certs/visionclaw.crt
SSL-KEY-PATH=/etc/ssl/private/visionclaw.key
```

### Database Security

Secure database configuration:

```bash
# PostgreSQL security
POSTGRES-SSL-MODE=require
POSTGRES-SSL-CERT=/certs/client.crt
POSTGRES-SSL-KEY=/certs/client.key
POSTGRES-CONNECTION-TIMEOUT=30
POSTGRES-STATEMENT-TIMEOUT=30000

# Redis security
REDIS-PASSWORD=your-secure-redis-password
REDIS-MAX-CONNECTIONS=100
REDIS-TIMEOUT=5
```

## AI Service Configuration

### Language Model Setup

Configure AI services for multi-agent capabilities:

```bash
# Primary AI services
OPENAI-API-KEY=sk-proj-your-openai-api-key
ANTHROPIC-API-KEY=sk-ant-your-anthropic-api-key
PERPLEXITY-API-KEY=pplx-your-perplexity-api-key

# Model configuration
DEFAULT-LLM-MODEL=gpt-4o
LLM-TEMPERATURE=0.7
LLM-MAX-TOKENS=4096
LLM-TIMEOUT=30
```

Configure model preferences in `data/settings.yaml`:
```yaml
# AI service settings
openai:
  model: "gpt-4o"
  max-tokens: 4096
  temperature: 0.7
  timeout: 30
  rate-limit: 1000

perplexity:
  model: "llama-3.1-sonar-small-128k-online"
  max-tokens: 4096
  temperature: 0.5
  timeout: 30
  rate-limit: 100

ragflow:
  agent-id: "your-ragflow-agent-id"
  timeout: 30
  max-retries: 3
  chunk-size: 512
  max-chunks: 100
```

### Voice and Audio Services

Enable voice interaction capabilities:

```bash
# Voice services
ENABLE-VOICE=true
VOICE-LANGUAGE=en-GB
STT-PROVIDER=whisper
TTS-PROVIDER=kokoro

# Kokoro TTS
KOKORO-DEFAULT-VOICE=af-heart
KOKORO-DEFAULT-FORMAT=mp3
KOKORO-SAMPLE-RATE=24000

# Whisper STT
WHISPER-MODEL=base
WHISPER-LANGUAGE=en
WHISPER-TEMPERATURE=0.0
```

## Environment-Specific Configurations

### Development Environment

Optimised for local development:

```bash
# .env.development
ENVIRONMENT=development
DEBUG-MODE=true
RUST-LOG=debug
HOT-RELOAD=true

# Relaxed security for development
CORS-ALLOW-ALL=true
DISABLE-HTTPS-REDIRECT=true
AUTH-REQUIRED=false

# Development services
MOCK-SERVICES=true
ENABLE-PROFILING=true
LOG-LEVEL=debug
```

### Staging Environment

Pre-production testing configuration:

```bash
# .env.staging
ENVIRONMENT=staging
DEBUG-MODE=false
RUST-LOG=info

# Staging-specific settings
ENABLE-METRICS=true
ENABLE-PROFILING=true
LOG-LEVEL=info

# Moderate security
AUTH-REQUIRED=true
RATE-LIMIT-MAX=5000
```

### Production Environment

Production-ready configuration:

```bash
# .env.production
ENVIRONMENT=production
DEBUG-MODE=false
RUST-LOG=warn

# Production optimisation
ENABLE-GPU=true
MAX-AGENTS=50
MEMORY-LIMIT=32g
CPU-LIMIT=16.0

# Strict security
AUTH-REQUIRED=true
RATE-LIMIT-MAX=10000
ENABLE-AUDIT-LOGGING=true
LOG-SENSITIVE-DATA=false
```

## Monitoring and Observability

### Metrics Configuration

Enable comprehensive monitoring:

```bash
# Metrics and monitoring
ENABLE-METRICS=true
METRICS-PORT=9090
PROMETHEUS-ENDPOINT=true

# Performance tracking
TRACK-PERFORMANCE=true
TRACK-USAGE=true
TRACK-ERRORS=true
TRACK-SECURITY-EVENTS=true
```

Configure monitoring in `data/settings.yaml`:
```yaml
system:
  monitoring:
    enable-metrics: true
    metrics-port: 9090
    metrics-interval: 15
    
    # Health checks
    health-check-interval: 30
    health-check-timeout: 10
    health-check-retries: 3
    
    # Performance monitoring
    track-performance: true
    track-resource-usage: true
    track-agent-metrics: true
```

### Logging Configuration

Configure structured logging:

```bash
# Logging configuration
LOG-LEVEL=info
LOG-FORMAT=json
LOG-FILE=/app/logs/visionclaw.log
LOG-ROTATION=daily
LOG-MAX-SIZE=100MB
LOG-MAX-FILES=10

# Structured logging options
LOG-JSON-PRETTY=false
LOG-INCLUDE-LOCATION=true
LOG-INCLUDE-THREAD=true
```

## Backup and Recovery

### Configuration Backup

Set up automatic configuration backups:

```bash
# Backup configuration
BACKUP-ENABLED=true
BACKUP-SCHEDULE="0 2 * * *"           # Daily at 2 AM
BACKUP-RETENTION-DAYS=30
BACKUP-LOCATION=/opt/backups/visionclaw
BACKUP-COMPRESSION=gzip

# What to backup
BACKUP-DATABASE=true
BACKUP-REDIS=true
BACKUP-USER-DATA=true
BACKUP-CONFIGURATION=true
```

### Disaster Recovery

Configure disaster recovery settings:

```yaml
system:
  disaster-recovery:
    enable-automatic-backup: true
    backup-interval: 86400          # 24 hours
    backup-retention: 2592000       # 30 days
    
    # Recovery settings
    enable-auto-recovery: true
    max-recovery-attempts: 3
    recovery-timeout: 300
    
    # Replication
    enable-replication: false
    replica-hosts: []
    replication-lag-threshold: 60
```

## Troubleshooting Common Issues

### Port Conflicts

Resolve port conflicts:
```bash
# Check for port conflicts
sudo netstat -tulpn | grep :3030
sudo lsof -i :3030

# Change ports if needed
HOST-PORT=3002
MCP-TCP-PORT=9501
METRICS-PORT=9091
```

### Memory Issues

Fix memory-related problems:
```bash
# Check memory usage
free -h
docker system df

# Adjust memory limits
MEMORY-LIMIT=8g                    # Reduce if insufficient
SHARED-MEMORY-SIZE=1g
POSTGRES-SHARED-BUFFERS=512MB
REDIS-MAX-MEMORY=256mb
```

### GPU Configuration Issues

Resolve GPU problems:
```bash
# Disable GPU if unavailable
ENABLE-GPU=false

# Or fix GPU configuration
NVIDIA-VISIBLE-DEVICES=all
CUDA-ARCH=86                       # Adjust for your GPU
NVIDIA-DRIVER-CAPABILITIES=compute,utility
```

### Database Connection Issues

Fix database connectivity:
```bash
# Check database status
docker-compose exec postgres pg-isready

# Reset database connection
docker-compose restart postgres
docker-compose restart visionclaw

# Verify connection settings
POSTGRES-HOST=postgres
POSTGRES-PORT=5432
POSTGRES-CONNECTION-TIMEOUT=30
```

## Configuration Validation

### Automated Validation

Use validation scripts to verify configuration:

```bash
# Validate environment variables
./scripts/validate-env.sh

# Validate YAML configuration
./scripts/validate-yaml.sh data/settings.yaml

# Test Docker configuration
./scripts/test-docker-config.sh

# Full configuration test
./scripts/test-full-config.sh
```

### Manual Verification

Manually verify key components:

```bash
# Test API health
curl http://localhost:3030/api/health

# Test WebSocket connection
wscat -c ws://localhost:3030/ws

# Test MCP connection
telnet localhost 9500

# Test GPU (if enabled)
docker exec -it visionclaw-container nvidia-smi

# Test database connection
docker-compose exec postgres psql -U visionclaw -d visionclaw -c "SELECT version();"
```

## Best Practices

### Configuration Management

1. **Use version control**: Keep configuration files in version control
2. **Environment separation**: Use different files for dev/staging/production
3. **Secret management**: Never commit secrets; use environment variables
4. **Documentation**: Document all custom configuration choices
5. **Validation**: Always validate configuration changes before deployment

### Performance Guidelines

1. **Start conservative**: Begin with lower resource limits and scale up
2. **Monitor resources**: Use monitoring to understand actual usage
3. **Profile regularly**: Enable profiling in staging to identify bottlenecks
4. **Optimise incrementally**: Make small changes and measure impact
5. **Plan for growth**: Configure with future scaling in mind

### Security Guidelines

1. **Principle of least privilege**: Only grant necessary permissions
2. **Regular updates**: Keep all components and dependencies updated
3. **Secure communications**: Use TLS/SSL for all external communications
4. **Audit trails**: Enable comprehensive logging and auditing
5. **Regular reviews**: Periodically review and update security configuration

---

This guide covers the most common configuration scenarios for VisionClaw. For advanced configuration options and complete technical reference, see the .

## Related Topics

-  - Comprehensive technical reference
- [Deployment Guide](../deployment.md) - Production deployment strategies
