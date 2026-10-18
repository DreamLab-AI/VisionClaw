//! Canonical wire fixtures (ADR-2018 consumer freshness, ADR-2019 per-opcode
//! frame policy), re-exported from the dependency-free [`visionclaw_wire`]
//! crate so existing `visionclaw_protocol::wire_fixtures::*` callers keep
//! compiling. New code should depend on `visionclaw-wire` directly.

pub use visionclaw_wire::*;
