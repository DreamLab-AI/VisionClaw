---
title: Pipeline Operator Runbook
description: 1. [System Overview](#system-overview) 2. [Monitoring](#monitoring) 3. [Common Issues](#common-issues)
category: how-to
tags:
  - tutorial
  - api
  - api
  - docker
updated-date: 2025-12-18
difficulty-level: advanced
---


# Pipeline Operator Runbook

## Table of Contents

1. [System Overview](#system-overview)
2. [Monitoring](#monitoring)
3. [Common Issues](#common-issues)
4. [Incident Response](#incident-response)
5. [Maintenance Procedures](#maintenance-procedures)
6. [Performance Tuning](#performance-tuning)
7. [Troubleshooting Guide](#troubleshooting-guide)

## System Overview

The ontology processing pipeline transforms OWL data from GitHub into physics forces applied to the knowledge graph visualization.

### Pipeline Stages

1. **GitHub Sync** - Parse and store OWL data
<!-- lands with ontomark step 4 -->
2. **Reasoning** - Infer missing axioms with `ontomark-reason` (Whelk EL). The hand-rolled `CustomReasoner` transitive-closure stage is removed (ADR-2137, decision D5); the inferred-edge view and the physics constraint projection both read `ontomark-reason`'s inferred set
3. **Constraint Generation** - Convert axioms to physics forces
4. **GPU Upload** - Transfer constraints to CUDA
5. **Physics Simulation** - Apply forces to node positions
6. **Client Broadcasting** - Stream updates to WebSocket clients

### Key Metrics

- **Throughput**: 100 files/second (GitHub sync)
- **Latency**: P50 65ms, P95 120ms, P99 250ms (end-to-end)
- **Cache Hit Rate**: Target 85%+
- **GPU Utilization**: Target 60-80%
- **Error Rate**: Target <1%

## Monitoring

### Health Check Endpoints

```bash
# Liveness probe (lightweight — confirms process is running)
curl http://localhost:8080/api/healthz

# Readiness probe (confirms dependencies are ready — graph store, GPU, etc.;
# returns 503 with {"status":"not_ready","reason":...} while DEGRADED)
curl http://localhost:8080/api/readyz

# Full system diagnostics (detailed component-level health)
curl http://localhost:8080/api/health

# Application metrics (event bus stats, circuit breaker states, uptime)
curl http://localhost:8080/api/metrics

# Pipeline status
curl http://localhost:8080/api/admin/pipeline/status

# Pipeline metrics
curl http://localhost:8080/api/admin/pipeline/metrics
```

#### Kubernetes Health Check Configuration

For Kubernetes deployments, use the dedicated liveness and readiness probes:

```yaml
livenessProbe:
  httpGet:
    path: /api/healthz
    port: 8080
  initialDelaySeconds: 10
  periodSeconds: 15
  failureThreshold: 3
readinessProbe:
  httpGet:
    path: /api/readyz
    port: 8080
  initialDelaySeconds: 20
  periodSeconds: 10
  failureThreshold: 5
```

Use `/api/healthz` for liveness (restarts the pod if the process is hung) and `/api/readyz` for readiness (removes the pod from the Service load balancer until all dependencies are healthy). Do not use `/api/health` for probes — it performs full diagnostics and is too expensive for frequent polling.

### Key Dashboards

**Grafana Dashboard: Pipeline Overview**
- Pipeline throughput (ontologies/second)
- End-to-end latency (P50, P95, P99)
- Queue sizes (reasoning, constraints, GPU)
- Error rates by stage
- Cache hit rates

**Grafana Dashboard: GPU Monitoring**
- GPU memory usage
- CUDA kernel execution time
- GPU errors and fallbacks
- CPU fallback rate

**Grafana Dashboard: WebSocket Health**
- Connected clients
- Message throughput
- Dropped frames (backpressure)
- Client latency distribution

### Alert Rules

#### Critical Alerts (Page immediately)

**Pipeline Down**
```
alert: PipelineDown
expr: up{job="visionclaw-pipeline"} == 0
for: 2m
severity: critical
```

**High Error Rate**
```
alert: PipelineHighErrorRate
expr: rate(pipeline-errors-total[5m]) > 0.05
for: 5m
severity: critical
```

**GPU Unavailable**
```
alert: GPUUnavailable
expr: gpu-available == 0 AND cpu-fallback-rate > 0.8
for: 10m
severity: critical
```

#### Warning Alerts (Investigate within 1 hour)

**Cache Hit Rate Low**
```
alert: LowCacheHitRate
expr: reasoning-cache-hit-rate < 0.7
for: 15m
severity: warning
```

**High Latency**
```
alert: PipelineHighLatency
expr: histogram-quantile(0.95, pipeline-latency-ms) > 500
for: 10m
severity: warning
```

**Queue Backlog**
```
alert: PipelineQueueBacklog
expr: reasoning-queue-size > 50
for: 5m
severity: warning
```

## Common Issues

### Issue 1: Pipeline Stuck

**Symptoms**:
- `/api/admin/pipeline/status` shows "running" for >30 minutes
- No position updates to clients
- High reasoning queue size

**Diagnosis**:
```bash
# Check pipeline status
curl http://localhost:8080/api/admin/pipeline/status

# Check reasoning actor health
curl http://localhost:8080/api/health | jq '.components.reasoning-actor'

# Check logs for correlation ID
docker logs visionclaw-unified 2>&1 | grep "correlation-id"
```

**Resolution**:
```bash
# Option 1: Restart reasoning actor (graceful)
curl -X POST http://localhost:8080/api/admin/actors/reasoning/restart

# Option 2: Clear queue and restart pipeline
curl -X POST http://localhost:8080/api/admin/pipeline/pause
curl -X POST http://localhost:8080/api/admin/pipeline/clear-queues
curl -X POST http://localhost:8080/api/admin/pipeline/resume

# Option 3: Full system restart (last resort)
docker restart visionclaw-unified
```

### Issue 2: GPU Out of Memory

**Symptoms**:
- GPU errors in logs: `cudaErrorMemoryAllocation`
- High CPU fallback rate
- Constraint upload failures

**Diagnosis**:
```bash
# Check GPU memory
nvidia-smi

# Check GPU metrics
curl http://localhost:8080/api/admin/pipeline/metrics | jq '.gpu'

# Check constraint count
curl http://localhost:8080/api/admin/constraints/stats
```

**Resolution**:
```bash
# Option 1: Reduce constraint batch size
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"constraint-batch-size": 500}'

# Option 2: Clear GPU memory
curl -X POST http://localhost:8080/api/admin/gpu/clear-memory

# Option 3: Disable GPU temporarily
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"use-gpu-constraints": false}'

# Re-enable after resolution
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"use-gpu-constraints": true}'
```

### Issue 3: Cache Thrashing

**Symptoms**:
- Low cache hit rate (<50%)
- High reasoning latency
- Frequent cache misses in logs

**Diagnosis**:
```bash
# Check cache stats
curl http://localhost:8080/api/admin/pipeline/metrics | jq '.cache-stats'

# Check cache size
sqlite3 /var/lib/visionclaw/reasoning-cache.db "SELECT COUNT(*) FROM cache;"

# Check for checksum mismatches
docker logs visionclaw-unified 2>&1 | grep "Checksum mismatch"
```

**Resolution**:
```bash
# Option 1: Increase cache size
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"reasoning-cache-size-mb": 1000}'

# Option 2: Clear corrupted cache entries
curl -X POST http://localhost:8080/api/admin/cache/clear

# Option 3: Rebuild cache
curl -X POST http://localhost:8080/api/admin/cache/rebuild
```

### Issue 4: GitHub Sync Failures

**Symptoms**:
- 403 errors from GitHub API
- Sync stuck at same file count
- `failed to fetch files` errors

**Diagnosis**:
```bash
# Check GitHub API rate limit
curl -H "Authorization: token $GITHUB-TOKEN" \
  https://api.github.com/rate-limit

# Check sync service logs
docker logs visionclaw-unified 2>&1 | grep "GitHubSync"

# Verify GitHub token
echo $GITHUB-TOKEN | wc -c  # Should be ~40 characters
```

**Resolution**:
```bash
# Option 1: Wait for rate limit reset
# Check X-RateLimit-Reset header

# Option 2: Use authenticated requests
export GITHUB-TOKEN="ghp-your-token-here"
docker restart visionclaw-unified

# Option 3: Manual trigger with force
curl -X POST http://localhost:8080/api/admin/sync/trigger \
  -H "Content-Type: application/json" \
  -d '{"force": true}'
```

### Issue 5: WebSocket Client Overload

**Symptoms**:
- High dropped frame rate
- Client latency >100ms
- Backpressure warnings in logs

**Diagnosis**:
```bash
# Check connected clients
curl http://localhost:8080/api/admin/clients/count

# Check broadcast stats
curl http://localhost:8080/api/admin/websocket/stats

# Monitor client queue sizes
docker logs visionclaw-unified 2>&1 | grep "client queue full"
```

**Resolution**:
```bash
# Option 1: Reduce broadcast FPS
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"client-broadcast-fps": 20}'

# Option 2: Implement client-side throttling
# (Update client code to skip frames)

# Option 3: Disconnect idle clients
curl -X POST http://localhost:8080/api/admin/clients/disconnect-idle
```

## Incident Response

### Incident Severity Levels

**SEV 1 - Critical**
- Pipeline completely down
- No data flowing to clients
- GPU unavailable AND CPU fallback failing
- Response time: Immediate

**SEV 2 - Major**
- High error rate (>5%)
- Significant performance degradation (P95 >1s)
- Single component failure with degraded service
- Response time: <30 minutes

**SEV 3 - Minor**
- Low error rate (1-5%)
- Minor performance degradation
- Component warnings
- Response time: <2 hours

### Incident Response Checklist

1. **Acknowledge Alert**
   ```bash
   # Acknowledge in PagerDuty/Opsgenie
   # Post in #incidents Slack channel
   ```

2. **Assess Impact**
   ```bash
   # Check pipeline status
   curl http://localhost:8080/api/admin/pipeline/status

   # Check connected clients
   curl http://localhost:8080/api/admin/clients/count

   # Check error rate
   curl http://localhost:8080/api/admin/pipeline/metrics | jq '.error-rates'
   ```

3. **Mitigate**
   ```bash
   # Pause pipeline if necessary
   curl -X POST http://localhost:8080/api/admin/pipeline/pause \
     -H "Content-Type: application/json" \
     -d '{"reason": "SEV1 incident - investigating"}'

   # Enable CPU fallback
   curl -X POST http://localhost:8080/api/admin/pipeline/config \
     -H "Content-Type: application/json" \
     -d '{"use-gpu-constraints": false}'
   ```

4. **Investigate Root Cause**
   ```bash
   # Collect logs
   docker logs visionclaw-unified --since 1h > /tmp/incident-logs.txt

   # Check recent events
   curl http://localhost:8080/api/admin/pipeline/events/recent

   # Export metrics
   curl http://localhost:8080/api/admin/pipeline/metrics > /tmp/metrics.json
   ```

5. **Resolve**
   - Apply fix (restart, config change, code patch)
   - Verify resolution
   - Resume pipeline

6. **Post-Incident**
   - Document root cause
   - Create follow-up tasks
   - Update runbook

## Maintenance Procedures

### Scheduled Maintenance

**Weekly Maintenance (Sunday 02:00 UTC)**

```bash
# 1. Pause pipeline
curl -X POST http://localhost:8080/api/admin/pipeline/pause \
  -H "Content-Type: application/json" \
  -d '{"reason": "Weekly maintenance"}'

# 2. Backup the graph store (embedded Oxigraph — archive its RocksDB dataset; ADR-2004)
#    DATA_DIR defaults to ./data (Docker images set it to /app/data)
tar czf /backups/oxigraph-$(date +%Y%m%d).tgz -C "${DATA_DIR:-/app/data}" oxigraph

# 3. (No external DB cache to flush — Oxigraph is in-process. Optionally back up settings.)
sqlite3 "${DATA_DIR:-/app/data}/settings.db" ".backup /backups/settings-$(date +%Y%m%d).db"

# 4. Clear old cache entries (>30 days)
sqlite3 /var/lib/visionclaw/reasoning-cache.db \
  "DELETE FROM cache WHERE created-at < datetime('now', '-30 days');"

# 5. Restart service
docker restart visionclaw-unified

# 6. Wait for healthy
timeout 60 bash -c 'until curl -f http://localhost:8080/api/health; do sleep 2; done'

# 7. Resume pipeline
curl -X POST http://localhost:8080/api/admin/pipeline/resume

# 8. Verify
curl http://localhost:8080/api/admin/pipeline/status
```

### Database Maintenance

**Check Database Size**
```bash
# Embedded Oxigraph RocksDB dataset (ADR-2004)
du -sh "${DATA_DIR:-/app/data}/oxigraph/"
```

**Optimize Database**
```bash
# Oxigraph maintains SPO/POS/OSP indexes automatically — no manual index rebuild.
# To compact and reclaim space, stop the server and re-open the store (RocksDB
# compacts on open), or rebuild cleanly by deleting the dataset and re-running the sync:
#   1. docker stop visionclaw_container
#   2. rm -rf "${DATA_DIR:-/app/data}/oxigraph/"   # full reset (re-sync from GitHub on next boot)
#   3. docker start visionclaw_container
#
# Consistency: verify node/edge counts via the REST API after any reset.
curl http://localhost:8080/api/graph/data | jq '{nodes: (.nodes|length), edges: (.edges|length)}'
```

### Cache Maintenance

**Clear Stale Cache Entries**
```bash
# Delete entries older than 30 days
curl -X POST http://localhost:8080/api/admin/cache/clear-old \
  -H "Content-Type: application/json" \
  -d '{"max-age-days": 30}'
```

**Rebuild Cache**
```bash
# Clear all cache and rebuild from database
curl -X POST http://localhost:8080/api/admin/cache/rebuild
```

### GPU Maintenance

**Reset GPU State**
```bash
# Clear GPU memory
nvidia-smi --gpu-reset

# Restart CUDA services
systemctl restart nvidia-persistenced

# Verify
nvidia-smi
```

## Performance Tuning

### Tuning Reasoning Performance

**Increase Cache Size**
```bash
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"reasoning-cache-size-mb": 2000}'
```

**Adjust Reasoning Depth**
```bash
# Reduce for faster inference (less complete)
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"max-reasoning-depth": 5}'

# Increase for more complete inference (slower)
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"max-reasoning-depth": 20}'
```

### Tuning GPU Performance

**Batch Size Optimization**
```bash
# Test different batch sizes
for size in 250 500 1000 2000; do
  curl -X POST http://localhost:8080/api/admin/pipeline/config \
    -H "Content-Type: application/json" \
    -d "{\"constraint-batch-size\": $size}"

  sleep 60  # Run for 1 minute

  curl http://localhost:8080/api/admin/pipeline/metrics \
    | jq '.latencies.gpu-upload-p50-ms'
done
```

**Memory Pool Tuning**
```bash
# Increase pre-allocated memory
curl -X POST http://localhost:8080/api/admin/gpu/config \
  -H "Content-Type: application/json" \
  -d '{"memory-pool-size-mb": 1024}'
```

### Tuning WebSocket Performance

**Adjust Broadcast Rate**
```bash
# Lower FPS for bandwidth constrained clients
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"client-broadcast-fps": 20}'

# Higher FPS for low-latency requirements
curl -X POST http://localhost:8080/api/admin/pipeline/config \
  -H "Content-Type: application/json" \
  -d '{"client-broadcast-fps": 60}'
```

## Troubleshooting Guide

### Logs Analysis

**Find Errors by Correlation ID**
```bash
correlation-id="abc-123"
docker logs visionclaw-unified 2>&1 | grep "\[$correlation-id\]"
```

**Analyze Error Patterns**
```bash
# Count errors by type
docker logs visionclaw-unified 2>&1 \
  | grep ERROR \
  | awk '{print $5}' \
  | sort | uniq -c | sort -nr

# Recent errors
docker logs visionclaw-unified --since 1h 2>&1 | grep ERROR
```

### Performance Analysis

**Identify Slow Stages**
```bash
# Query metrics by stage
curl http://localhost:8080/api/admin/pipeline/metrics \
  | jq '{
    reasoning: .latencies.reasoning-p95-ms,
    constraints: .latencies.constraint-gen-p50-ms,
    gpu-upload: .latencies.gpu-upload-p50-ms,
    end-to-end: .latencies.end-to-end-p50-ms
  }'
```

**Trace Request Path**
```bash
# Get all events for correlation ID
correlation-id="abc-123"
curl http://localhost:8080/api/admin/pipeline/events/$correlation-id \
  | jq '.events[] | {type: .event-type, timestamp: .timestamp}'
```

### Circuit Breaker Status

**Check Circuit State**
```bash
# GPU circuit breaker
curl http://localhost:8080/api/admin/circuit-breakers/gpu

# Reasoning circuit breaker
curl http://localhost:8080/api/admin/circuit-breakers/reasoning
```

**Reset Circuit Breaker**
```bash
# Force reset to CLOSED
curl -X POST http://localhost:8080/api/admin/circuit-breakers/gpu/reset
```

## Emergency Procedures

### Complete System Recovery

```bash
#!/bin/bash
# emergency-recovery.sh

echo "EMERGENCY RECOVERY - $(date)"

# 1. Stop pipeline
curl -X POST http://localhost:8080/api/admin/pipeline/pause \
  -d '{"reason": "Emergency recovery"}'

# 2. Backup current state
mkdir -p /backups/emergency-$(date +%Y%m%d-%H%M%S)
cp /var/lib/visionclaw/*.db /backups/emergency-$(date +%Y%m%d-%H%M%S)/

# 3. Clear all queues
curl -X POST http://localhost:8080/api/admin/pipeline/clear-queues

# 4. Reset circuit breakers
curl -X POST http://localhost:8080/api/admin/circuit-breakers/reset-all

# 5. Clear GPU memory
curl -X POST http://localhost:8080/api/admin/gpu/clear-memory

# 6. Restart services
docker restart visionclaw-unified

# 7. Wait for healthy
timeout 120 bash -c 'until curl -f http://localhost:8080/api/health; do sleep 5; done'

# 8. Resume pipeline
curl -X POST http://localhost:8080/api/admin/pipeline/resume

# 9. Verify
curl http://localhost:8080/api/admin/pipeline/status

echo "RECOVERY COMPLETE - $(date)"
```

---

## Related Documentation

- [XR Architecture](../../explanation/xr-architecture.md)
- [Deployment Guide](../deployment.md)
- [Contributing Guidelines](../../CONTRIBUTING.md)
- [Agent Orchestration Guide](../agent-orchestration.md)

## Appendix

### Configuration Reference

**Pipeline Configuration**
```toml
[pipeline]
max-reasoning-queue = 10
max-constraint-queue = 5
max-gpu-queue = 3
max-retries = 3
initial-backoff-ms = 100
failure-threshold = 5
timeout-duration-secs = 30
reasoning-rate-limit = 10
gpu-upload-rate-limit = 5
client-broadcast-fps = 30
reasoning-cache-size-mb = 500
constraint-cache-size-mb = 200
```

### Contact Information

- **On-call Engineer**: PagerDuty rotation
- **Slack Channel**: #visionclaw-ops
- **Incident Management**: Jira Service Desk

### Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2025-01-03 | Initial operator runbook |
