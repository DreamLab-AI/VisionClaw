# solid-pod-rs-git

Git HTTP backend for [`solid-pod-rs`](../solid-pod-rs/).

**Not yet implemented. Target milestone: v0.5.0.**

This crate reserves the namespace under the workspace for the Git HTTP
smart-protocol backend per ADR-056 §D2 and
[`docs/design/jss-parity/06-library-surface-context.md`](../../docs/design/jss-parity/06-library-surface-context.md).

Scope when populated: `info/refs`, `upload-pack`, `receive-pack`, path-
traversal hardening, `receive.denyCurrentBranch=updateInstead`, and WAC
integration. See PARITY-CHECKLIST row 100 and GAP-ANALYSIS §E.1.

## Licence

**AGPL-3.0-only**.
