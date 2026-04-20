# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.x (latest minor) | Yes |
| 0.x (previous minor) | Yes |
| older | No |

## Reporting

Private disclosure preferred. Contact the maintainers via the channel listed at the repository's contact page on GitHub (Insights → Community Standards), or open a GitHub Security Advisory via the repository's "Security" tab → "Report a vulnerability".

Please do NOT open a public issue for suspected vulnerabilities.

## Process

1. Acknowledgement within 5 business days.
2. Assessment + scoped fix plan within 15 business days.
3. Coordinated disclosure within 90 days of initial report, or sooner if a public exploit exists.
4. CVE assignment where applicable; credit to the reporter on request.

## Scope

In scope: the `solid-pod-rs` crate, its default features, its documented public API, its CI/CD configuration.

Out of scope: downstream consumers' integrations (VisionClaw, community-forum-rs, etc.) — report those directly to those projects.
