# How to deploy to production

**Goal:** ship a solid-pod-rs pod behind a TLS-terminating reverse
proxy with monitoring, backup, and sensible defaults.

## Minimum viable deployment

- TLS: terminated at a reverse proxy (nginx, caddy, traefik). Never
  serve plaintext in production; NIP-98 and DPoP both assume an
  authenticated channel.
- Pod process: your binary built around `solid-pod-rs` + an HTTP
  framework (actix-web, axum). Run as a non-root user.
- Storage: `FsBackend` on SSD for pods up to ~100 GB, S3-backed for
  larger or multi-instance deployments.
- Auth: NIP-98 only, or NIP-98 + Solid-OIDC (feature `oidc`).

## systemd unit

```ini
[Unit]
Description=solid-pod-rs pod
After=network.target

[Service]
Type=simple
User=pod
Group=pod
WorkingDirectory=/var/lib/mypod
Environment=RUST_LOG=solid_pod_rs=info,tower_http=info
ExecStart=/usr/local/bin/my-pod
Restart=on-failure
RestartSec=3s
LimitNOFILE=65535
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/mypod
PrivateTmp=true
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true

[Install]
WantedBy=multi-user.target
```

## nginx in front

```nginx
server {
  listen 443 ssl http2;
  server_name pod.example.com;

  ssl_certificate     /etc/letsencrypt/live/pod.example.com/fullchain.pem;
  ssl_certificate_key /etc/letsencrypt/live/pod.example.com/privkey.pem;

  # Solid WebSocket notifications
  location ~ ^/\.notifications/(websocket|subscription/) {
    proxy_pass http://127.0.0.1:8765;
    proxy_http_version 1.1;
    proxy_set_header Upgrade     $http_upgrade;
    proxy_set_header Connection  "upgrade";
    proxy_read_timeout 86400s;
  }

  location / {
    proxy_pass http://127.0.0.1:8765;
    proxy_set_header Host        $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto https;
  }
}
```

### Host header correctness

NIP-98 URL matching is byte-level (after trailing-slash
normalisation). Make sure `proxy_set_header Host $host` is set so the
`u` tag in the Nostr event resolves to the same URL the pod sees.

## TLS inside the pod

If you must terminate TLS in-process, let your HTTP framework handle
it; solid-pod-rs itself has no TLS knowledge. See actix-web's
`rustls` feature.

## Tracing

```rust
use tracing_subscriber::{fmt, EnvFilter};

fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .with_target(false)
    .json()
    .init();
```

Recommended log levels in production:

- `solid_pod_rs=info`
- Your HTTP framework: `warn` (info if debugging)
- `tower_http=info` for request logging

## Metrics

solid-pod-rs does not ship a metrics feature. Export from your HTTP
framework:

- Request count + latency histogram (per method / per status class).
- `wac_allow` deny count (hook into your WAC evaluator).
- Storage backend GET/PUT/DELETE counts.
- `WebhookChannelManager::active_subscriptions()` and
  `WebSocketChannelManager::active_subscriptions()`.

## Backups

### FS backend

Rsync the root directory. `.meta.json` sidecars **must** go with their
bodies or the pod will 404 on content-type discovery.

```bash
rsync -av --delete /var/lib/mypod/ backup:/backups/mypod-$(date +%F)/
```

### S3 backend

Enable bucket versioning + replication. Point-in-time restore is a
bucket-level restore + key-prefix filter.

## Rotating the NIP-98 clock tolerance

`verify_at` uses a 60 s window. If your clients drift further than
that, either:

1. Sync clocks with NTP (preferred).
2. Fork the verifier with an adjusted `TIMESTAMP_TOLERANCE` constant.
   Do not accept windows longer than 5 minutes — the whole point is
   to bound replay.

## Hardening checklist

- [ ] Run as non-root with minimal file-system access.
- [ ] `POST`/`PUT`/`PATCH` rate-limited at the proxy.
- [ ] Request bodies capped (64 KB NIP-98, realistic cap on resources).
- [ ] `/.acl` on the pod root **installed** — deny-by-default is
      easy to lock yourself out with.
- [ ] Backups tested with a restore drill.
- [ ] CSP / CORS headers on the proxy (pod itself is framework
      neutral).

## See also

- [how-to/configure-nip98-auth.md](configure-nip98-auth.md)
- [how-to/enable-solid-oidc.md](enable-solid-oidc.md)
- [how-to/scale-with-s3-backend.md](scale-with-s3-backend.md)
- [explanation/security-model.md](../explanation/security-model.md)
