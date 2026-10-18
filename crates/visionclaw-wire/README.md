# visionclaw-wire

The frozen VisionClaw position wire as byte-exact constructors: the 52-byte V3
node record, the V5 sequence envelope, the `0x34` acknowledgement and the `0x23`
agent-action batch.

The VisionClaw server, its web and Godot XR clients, and Codescape all test
against these functions, so no producer or consumer can drift from the bytes
the others expect. The crate has no dependencies. It is `no_std` + `alloc` with
the default `std` feature disabled.

## Layout

All integers and floats are little-endian.

| Frame | Layout | Size |
|---|---|---|
| V3 position | `[0x03]` (node record)* | `1 + 52n` |
| V5 position | `[0x05][u64 broadcast_seq]` (node record)* | `9 + 52n` |
| `0x34` acknowledgement | `[0x34][u64 sequence][u32 nodes_received][u64 timestamp_ms]` | 21 |
| `0x23` agent actions | `[0x23][u16 count]([u16 event_len][event bytes])*` | variable |

Node record (52 bytes):

| Offset | Type | Field |
|---|---|---|
| 0 | `u32` | `wire_id` (low 26 bits id, bits 26–31 class flags) |
| 4 | `f32 × 3` | `x`, `y`, `z` |
| 16 | `f32 × 3` | `vx`, `vy`, `vz` |
| 28 | `f32` | `sssp_distance` |
| 32 | `i32` | `sssp_parent` |
| 36 | `u32` | `cluster_id` |
| 40 | `f32` | `anomaly` |
| 44 | `u32` | `community_id` |
| 48 | `f32` | `centrality` |

Agent-action event body: `[u32 source_agent_id][u32 target_node_id][u8 action_type][u32 timestamp][u16 duration_ms][payload…]`
(15-byte header).

## Invariants

- The record is frozen at 52 bytes.
- V5 is purely additive: after the version byte and 8 sequence bytes, the body
  is byte-identical to V3.
- Position frames are all-or-nothing. A misaligned body, an unknown version or a
  truncated V5 sequence rejects the whole frame. `0x23` batches are a tolerant
  prefix parse.
- `0x34` and `0x23` frames carry no generic header.

## Example

```rust
use visionclaw_wire::{v5_frame, v5_sequence_of, NodeRecord};

let frame = v5_frame(42, &[NodeRecord::plain(1, [0.0, 1.0, 2.0])]);
assert_eq!(v5_sequence_of(&frame), Some(42));
```

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT licence](LICENSE-MIT), at your option.

VisionClaw itself is AGPL-3.0-only. This crate's text is the copyright holder's
own work, and the holder relicenses it here under MIT OR Apache-2.0 so that
independent producers and consumers of the wire can depend on it without
taking on the AGPL.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 licence, shall
be dual-licensed as above, without any additional terms or conditions.
