# T1 — Class-flag bits in NodeId: ADR-08 vs PRD-08/DDD-07 collision

Status     : Resolved (recommendation pending acceptance)
Date       : 2026-05-16
Baseline   : `radical-rollback` @ `41979d33e` (current HEAD `b8300d94c`)
Owners     : Section 02 (Binary Protocol), Section 08 (Ontology), Section 07 (Bots)

## The conflict in one paragraph

ADR-08 §D6 declares `0x80000000=Agent, 0x40000000=Page,
**0x20000000=OntologyClass**, 0x10000000=OntologyProperty, 0x08000000=LinkedPage,
0x04000000=Axiom, 26-bit sequence`. PRD-08 §7 and DDD-07 ubiquitous-language
both pin the ontology-subtype region to mask **`0x1C000000`** (bits 26-28).
`0x20000000` is bit 29 — outside that mask. Mathematically:
`0x20000000 & 0x1C000000 == 0`, so `is_ontology_class(set_ontology_class_flag(id))`
returns false. The two specs cannot both hold.

## Current state in code

The baseline is `radical-rollback`. All citations are at HEAD `b8300d94c` of
that branch.

| Where                                                | Constant                              | Value         |
|------------------------------------------------------|---------------------------------------|---------------|
| `src/utils/binary_protocol.rs:16`                    | `AGENT_NODE_FLAG`                     | `0x80000000`  |
| `src/utils/binary_protocol.rs:17`                    | `KNOWLEDGE_NODE_FLAG`                 | `0x40000000`  |
| `src/utils/binary_protocol.rs:20`                    | `ONTOLOGY_TYPE_MASK`                  | `0x1C000000`  |
| `src/utils/binary_protocol.rs:21`                    | `ONTOLOGY_CLASS_FLAG`                 | `0x04000000`  |
| `src/utils/binary_protocol.rs:22`                    | `ONTOLOGY_INDIVIDUAL_FLAG`            | `0x08000000`  |
| `src/utils/binary_protocol.rs:23`                    | `ONTOLOGY_PROPERTY_FLAG`              | `0x10000000`  |
| `src/utils/binary_protocol.rs:27`                    | `NODE_ID_MASK`                        | `0x03FFFFFF`  |

Mask arithmetic verified: `0x80|0x40 & 0x1C000000 == 0` (top-level outside
mask); `0x04|0x08|0x10000000 == 0x1C000000` (three subtypes fully cover);
`0x03FFFFFF | 0x1C000000 | 0xC0000000 == 0xFFFFFFFF` (no overlap).

CUDA consumes class metadata via three buffers, not via flag bits on the id:

- `src/utils/visionflow_unified.cu:276-278` — kernel signature takes
  `class_id: const int*`, `class_charge: const float*`, `class_mass:
  const float*`. The id the kernel reads is a separately uploaded buffer.
- `src/utils/visionflow_unified.cu:337-344` — branches on
  `class_id[idx] == class_id[neighbor_idx]` for same-domain repulsion.
- `src/actors/gpu/gpu_resource_actor.rs:241-249` — `class_id` is derived
  from `node.metadata["source_domain"]` and mapped to small ints
  (`ai=1, bc=2, mv=3, rb=4, ngm=5, tc=6, _=>0`). These are domain
  cluster ids, **unrelated** to the wire flag bits.

`set_ontology_class_flag` / `set_ontology_property_flag` /
`set_ontology_individual_flag` are called only from
`src/handlers/socket_flow_handler/position_updates.rs:107-172` and
`src/utils/binary_protocol.rs:376-517`. They affect *only* the high 6 bits
of the broadcast id, never the GPU `class_id` buffer.

`NEXT_NODE_ID` (`src/models/node.rs:8`) allocates plain sequential `u32`
values from 1. Class bits are applied at wire-encode time, never stored.

### What main HEAD does

`git show main:src/utils/binary_protocol.rs` shows main has **removed flag
bits from the wire entirely**: "There is exactly ONE wire format. There is
no version negotiation, no flag-bit discriminator, no analytics column."
The 28-byte entry is raw `[u32 id, f32×6 pos+vel]`. radical-rollback
restored the flag bits because the broadcast classifier and `SUBCLASS_OF`
client rendering still depend on them. The forward path must pick one
encoding both branches can agree on.

## Surrounding intent

PRD-08 §7, DDD-07 ubiquitous-language, DDD-08, and ADR-08 §D6 together
have the bitfield serving three jobs:

1. **Wire classification** — client decodes class from id alone; 28-byte
   wire budget forbids adding a class byte.
2. **Geometry dispatch** — `Gem` (Page), `CrystalOrb` (OntologyClass),
   `AgentCapsule` (Agent), placeholder/axiom shapes branch on `NodeClass`
   derived from id with no lookup.
3. **Projection consistency** — `GraphTopology` sets class on `NodeId` at
   projection time; downstream consumers never re-derive.

ADR-08 calls out "67M IDs per class — more than enough". The intent is a
**fixed small taxonomy** (5-6 classes), not an open-ended hierarchy.
Ontology subtypes are a separate axis from the top-level
(Agent/Knowledge/Ontology) distinction. DDD-08 §C2 also requires the
`NodeId` *sequence* to survive `LinkedPage → Page` or `LinkedPage →
OntologyClass` upgrades — only class bits flip; sequence space is shared.

## Options evaluated

### A. Keep `0x1C000000` mask; relocate `OntologyClass` inside it

Pick 4 of 7 non-zero values from `{0x04, 0x08, 0x0C, 0x10, 0x14, 0x18,
0x1C}<<24` for the ontology-region classes.

- Pros: zero change to `ONTOLOGY_TYPE_MASK`, `NODE_ID_MASK`,
  `AGENT_NODE_FLAG`, `KNOWLEDGE_NODE_FLAG`. Three ontology classes
  already exist in code at 0x04/0x08/0x10; only Axiom needs a new slot.
  No GPU code touches these bits. No wire-format break. 3 headroom slots.
- Cons: ADR-08 §D6 wording is wrong and needs rewording.

### B. Widen mask to `0x3C000000` (4 bits) to accommodate `0x20000000`

- Pros: 4 bits = 16 subtypes; matches ADR-08 literal text.
- Cons: makes the mask irregular — agent (bit 31) and knowledge (bit 30)
  flank a 4-bit ontology region with no top-level "is-ontology" bit.
  Conceptually muddier than today; encourages a third widening later.
  Cannibalises sequence space (though `NODE_ID_MASK = 0x03FFFFFF` already
  reserves bits 26-31; no actual loss but the design becomes ad hoc).

### C. Drop bitfield-encoded subtypes; carry subtype in separate metadata

Keep 3 top-level classes (Agent / Knowledge / Ontology collapsed) in
flag bits; carry ontology subtype as u8 on domain `NodeId`, persisted by
Section 11 as RDF type triple.

- Pros: aligns with ADR-08 §D1 "one concept per thing". Extensible.
- Cons: wire grows 28 → 29 bytes/node OR the client must do an extra
  lookup for geometry dispatch, breaking PRD-08 intent §2. Bigger
  protocol swing than T1 should adjudicate.

### D. Hybrid: 2-bit top-level (Agent/Knowledge/Ontology/Reserved) + 3-bit subtype

Bits 30-31 = top-level (4 values), bits 26-28 = subtype-in-Ontology
(7 non-zero), 26-bit sequence preserved.

- Pros: uniform 2-bit top-level. Regular structure.
- Cons: changes `set_agent_flag`/`is_agent_node` semantics. **Wire ABI
  break** — every client decoder must change. Out of scope for a spec
  reconciliation.

## Recommended resolution

**Adopt Option A.** Keep the existing `ONTOLOGY_TYPE_MASK = 0x1C000000`
and the existing three ontology subtypes; allocate Axiom and LinkedPage
into the two remaining mask slots; drop `OntologyIndividual` from the
domain model (DDD-08 lists no such aggregate); rewrite ADR-08 §D6 to
match.

Final allocation:

| NodeClass         | Flag value     | Mask position             | Source             |
|-------------------|----------------|---------------------------|--------------------|
| `Agent`           | `0x80000000`   | bit 31 (top-level)        | unchanged          |
| `Page`            | `0x40000000`   | bit 30 (top-level)        | unchanged          |
| `OntologyClass`   | `0x04000000`   | mask `0x1C000000` slot 1  | was `ONTOLOGY_CLASS_FLAG` |
| `OntologyProperty`| `0x10000000`   | mask `0x1C000000` slot 4  | was `ONTOLOGY_PROPERTY_FLAG` |
| `LinkedPage`      | `0x08000000`   | mask `0x1C000000` slot 2  | repurposes `ONTOLOGY_INDIVIDUAL_FLAG` |
| `Axiom`           | `0x0C000000`   | mask `0x1C000000` slot 3  | NEW (currently unused) |
| `(reserved)`      | `0x14000000`   | mask `0x1C000000` slot 5  | headroom |
| `(reserved)`      | `0x18000000`   | mask `0x1C000000` slot 6  | headroom |
| `(reserved)`      | `0x1C000000`   | mask `0x1C000000` slot 7  | headroom |
| `Sequence`        | `0x03FFFFFF`   | bits 0-25 (26-bit space)  | unchanged |

`NodeClass::None` is the encoding `0x00000000` in the top 6 bits (a raw
sequence id with no class set) — the same as today's "unclassified
fallback" path that `get_node_type` returns `NodeType::Unknown` for.

### Proposed ADR-08 §D6 replacement wording

> Class bits encode `NodeClass` in the top 6 bits of the `u32` id:
> `0x80000000 = Agent` (bit 31), `0x40000000 = Page` (bit 30). The
> ontology region uses the mask `ONTOLOGY_TYPE_MASK = 0x1C000000`
> (bits 26-28) with these allocations: `0x04000000 = OntologyClass`,
> `0x08000000 = LinkedPage` (placeholder), `0x0C000000 = Axiom`,
> `0x10000000 = OntologyProperty`. Values `0x14000000`, `0x18000000`,
> `0x1C000000` are reserved for future ontology subtypes. The remaining
> 26 bits (`NODE_ID_MASK = 0x03FFFFFF`) are the per-class sequence,
> allocated by the atomic counter in `GraphStateActor`. Headroom: 67M
> sequences (shared across all classes; per ADR-08 §D1 the sequence is
> stable across a `LinkedPage → Page` or `LinkedPage → OntologyClass`
> upgrade — only the class bits change).

PRD-08 §7's existing "0x1C000000 ontology subtypes" sentence is correct
as-is. DDD-07's "0x1C000000 mask" phrasing is correct as-is. Both need a
forward reference to ADR-08 D6 once the latter is rewritten.

## Migration impact

- **Source code touched.** One file: `src/utils/binary_protocol.rs`.
  Rename `ONTOLOGY_INDIVIDUAL_FLAG` → `LINKED_PAGE_FLAG` (semantic only).
  Add `AXIOM_FLAG = 0x0C000000` + `set_axiom_flag`/`is_axiom`. Update
  `NodeType` enum: drop `OntologyIndividual`, add `LinkedPage` and
  `Axiom`. Update `get_node_type` branches at lines 183-197.
- **Callers touched.** `src/handlers/socket_flow_handler/position_updates.rs:107-172`:
  rename `OntologyIndividual` branch to `LinkedPage`; add `Axiom`. The
  upstream `node.node_type` string → flag setter classifier needs the
  same two-name change.
- **GPU code: NO CHANGE.** `class_id` in CUDA
  (`visionflow_unified.cu:276`) is the domain-cluster int from
  `gpu_resource_actor.rs:241-249`. The CUDA kernel never reads any flag
  constant. Collision is exclusively in the wire/domain metadata layer.
- **DB schema: NO CHANGE.** Persistence stores either full `u32` (Neo4j)
  or decoded `NodeClass + sequence` (Oxigraph). Neither changes.
- **Wire format: NO CHANGE.** Bit layout for `Agent`/`Page`/`OntologyClass`/
  `OntologyProperty` is unchanged. `LinkedPage` reuses `0x08000000` (was
  Individual). `Axiom` newly takes a previously-unused `0x0C000000`. No
  existing-value decoder breakage.
- **Tests touched.** `src/utils/binary_protocol.rs:1038-1051` — rename
  Individual refs to LinkedPage; add Axiom round-trip tests.
- **Docs touched.** ADR-08 §D6 (replacement above). PRD-08 §7 correct as
  written. DDD-08 §C2 invariant table consistent already.

Total effort: ~1 day mechanical work + tests. Zero risk to GPU pipeline,
zero risk to wire ABI, zero risk to persistence.
