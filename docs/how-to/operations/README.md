---
title: Operations Guide
description: Operating VisionClaw in production
category: how-to
diataxis: how-to
tags:
  - operations
  - monitoring
  - backup
updated-date: 2025-01-29
---

# Operations Guide

Operating VisionClaw in production environments.

## Contents

- [Configuration](configuration.md) - Environment and deployment configuration
- [Corpus ingest from the local vault](corpus-source-local.md) - The mounted vault source, and switching back to GitHub
- [Troubleshooting](troubleshooting.md) - Common issues and solutions
- [Security](security.md) - Security hardening
- [Telemetry & Logging](telemetry-logging.md) - Observability and alerting
- [Maintenance](maintenance.md) - Operational maintenance
- [Pipeline Admin API](pipeline-admin-api.md) - Pipeline administration
- [Operator Runbook](pipeline-operator-runbook.md) - Operational procedures

## Key Metrics

Monitor these key metrics:
- API response latency (p50, p95, p99)
- WebSocket connection count
- Graph-store query performance (embedded Oxigraph SPARQL)
- Memory and CPU utilization
- GitHub sync success rate

## Related

- [Configuration](../../reference/configuration.md)
- [Health API](../../reference/README.md)
