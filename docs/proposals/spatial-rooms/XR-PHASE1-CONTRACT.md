# XR phase 1: interface contract for four parallel agents

Status: **binding for phase 1** (2026-10-30). Records: ADR-2158 (volumes, native arm, switch), ADR-2159 (one connection per source), ADR-2164 (code links). Drafted against `00669b7a2b8bca15702b8173df1dab268144c3ec`. All line numbers are at that revision.

This file lets four agents work at once without editing the same file. If an interface below is wrong, raise it with the lead. Do not edit another agent's file to work around it.

## 0. Ground rules

- **Ownership.** Each file has exactly one owner (§6). Agent A is the **only** agent that edits `xr-client/scripts/graph_scene.gd`, `hud.gd` and `xr-client/rust/src/binary_protocol.rs`. Agents B, C and D deliver new files, plus the exact integration calls A must add (§7).
- **Out of bounds for all four agents:**
  - `xr-client/project.godot`, `xr-client/scripts/xr_boot.gd`, `export_presets.cfg` and the Godot version facts and Invariant 1 in `docs/XR-client.md`. These belong to the engine-gate agent (ADR-2165).
  - Voice (ADR-2160), which stays deferred.
- **Order (ADR-2159, as refined at acceptance).**
  - A's **P3-pre preflight** (§2.1) runs first.
  - B, C and D write their new files and headless tests in parallel, against fixtures.
  - Nothing is wired into `graph_scene.gd` until P3-pre passes.
- **Engine.** Code must parse and run its headless tests on **both** 4.6.1 (today's engine) and 4.8 beta (ADR-2165). Never reference a 4.8-only class statically: use `ClassDB` and `Engine.get_singleton` by name.
- **Style.**
  - Scripts that the headless runner must resolve with a stale class cache use `preload` constants, not `class_name`, following the `backend_caps.gd` header. New scripts follow the same rule.
  - Tabs, typed GDScript, no `await` in the hot path.
- **Merge order.** C's bounds handling calls B's static `SpatialVolume.fit_scale`. C may develop against a stub with the same signature under `tests/fixtures/`, but C's own script must not copy the maths. B merges before C.
- **Tests.** GUT runs headless under `xr-client/tests/unit/test_*.gd`. `tests/gut_guard.sh` fails if a script is skipped, so every new test file must parse on both engines. Rust tests use `cargo test` in `xr-client/rust`. Codescape's tests live in `codescape/` with its own target directory (AGENTS.md).
- **Worktrees.** Use `~/workspace/wt/project-xr-<agent>` and remove it when done.

## 1. Shared vocabulary

| Term | Value |
|---|---|
| Volume ids (`StringName`) | `&"knowledge"`, `&"ontology"`, `&"codescape"` |
| Connection ids (`StringName`) | `&"visionclaw"`, `&"codescape"` (federated, later: `&"df:<node-key-hex>:<space-id>"`) |
| Node class codes (`binary_protocol.rs:91-98`) | `0` knowledge, `1` ontology (class/individual/property), `2` agent, `3` plain (every Codescape code node) |
| Class mask | bit `1 << class_code`; knowledge `0b0001`, ontology `0b0010`, Codescape `0b1000` |
| Bounds mode (app enum, `SpatialVolume.BoundsMode`) | `BOUNDED = 0`, `IMMERSIVE = 1`. Godot's and the spec's values are converted **only** in the native shim (§4) |
| Generation | `int`, per connection, `+1` on every (re)connect *and* every `initialGraphLoad`; wire ids are valid only within one generation |
| E3 bindings | knowledge ← (`visionclaw`, mask `0b0001`); ontology ← (`visionclaw`, mask `0b0010`); codescape ← (`codescape`, mask `0b1000`) |
| E3 arrangement | ring radius 1.6 m about the user's start position, at height 1.3 m. Yaw: knowledge −60°, ontology +60°, codescape 0°. Each default `bounds` is `Vector3(1.0, 1.0, 1.0)` m. The memory cloud is untouched (world furniture at 180°) |

## 2. `BackendConnection`: agent A

New file: `xr-client/scripts/backend_connection.gd`, `extends Node`. It is a Node so that it can own `HTTPRequest` children and be freed with the scene. One instance per content source, held by `graph_scene.gd` in `_connections: Dictionary[StringName, BackendConnection]`.

### 2.1 P3-pre preflight (first, blocking)

New file: `xr-client/rust/tests/two_clients.rs`. It starts two local WebSocket servers on the shared tokio runtime (`runtime.rs:12-20`) and drives two transports (`transport.rs` `ConnHandle`, the same path `connect_to_url` uses).

Each server sends an `initialGraphLoad` and then 200 V5 position frames. The test asserts:
- both clients receive every frame;
- both send `0x34` acks with independent sequences (ADR-2141);
- neither stalls the other by more than 100 ms;
- a main-thread-style `poll()` loop drains both within budget.

It must also show that the gdext statics tolerate two instances: list every `static`/`OnceLock` in `xr-client/rust/src` in the test's doc comment, with a verdict for each. Record the output in the commit message. If it fails, stop and report: ADR-2159 "Revisit" applies.

### 2.2 API

```gdscript
extends Node
const BackendCaps := preload("res://scripts/backend_caps.gd")

signal connected_changed(conn_id: StringName, connected: bool)
signal topology_changed(conn_id: StringName, generation: int)        # after each initialGraphLoad
signal text_received(conn_id: StringName, msg_type: String, json: String)  # every non-topology text frame
signal refused(conn_id: StringName, cap: String, label: String)      # a gated request was refused locally
signal generation_changed(conn_id: StringName, generation: int)

var conn_id: StringName
var mode: String                 # "visionclaw" | "codescape"
var ws_base: String              # e.g. ws://host:4000 or ws://host:4000/codescape
var generation: int = 0
var caps: Dictionary             # BackendCaps.for_mode(mode)
var client: RefCounted           # BinaryProtocolClient (gdext); never shared

static func make(conn_id: StringName, mode: String, ws_base: String, nostr_secret_hex: String) -> Node
func open() -> void              # connect /wss; resets reconnect backoff
func close() -> void             # cancel every HTTPRequest, close socket, drop pins it owns (A6 hygiene), generation += 1
func poll(delta: float) -> void  # scene calls once per frame; pumps client, reconnect backoff (2 s → 60 s)
func is_connected_now() -> bool
func supports(cap: String) -> bool
func gate(cap: String, label: String, quiet := false) -> bool   # false ⇒ emits `refused`, sends nothing
func http_base() -> String       # ws→http swap, XR_BACKEND_HTTP override, codescape prefix (moved helpers)
func node_ids_for_mask(mask: int) -> PackedInt32Array          # wraps client (§2.4)
func render_aabb_for_ids(ids: PackedInt32Array, exclude_id: int) -> AABB  # full extent, graph units; empty AABB if none
func node_position(id: int) -> Vector3                           # graph units, current generation
func send_drag_start(id: int, p: Vector3) -> bool                # gated: false on codescape, nothing sent (E9)
func send_drag_update(id: int, p: Vector3) -> bool
func send_drag_end(id: int) -> bool
func iri_table() -> Dictionary   # {iri: PackedInt32Array of wire ids}, current generation (§2.4, consumed by D)
func code_refs() -> Array        # [{key, id, path, is_file, refs: [[iri, line], …]}] (§2.4, consumed by D)
```

### 2.3 What moves out of `graph_scene.gd`

These move into `BackendConnection`, at their `00669b7a2` lines:
- `_binary_client` (`:91`), `_backend_mode` (`:93`), `_caps` (`:82`) and `_graph_ws_url` (`:166`);
- the graph reconnect state (`:175-176`), `_connect_graph` (`:1590-1625`), `_schedule_graph_reconnect` (`:3096`) and the graph half of `_tick_reconnect` (`:3074`);
- `_backend_base` and `_with_backend_prefix` (`:1533-1544`) and `_http_base` (`:1547-1557`);
- the per-connection half of `_abandon_backend_requests` (`:877-906`);
- `backend_supports` and `_gate` (`:858-875`), which become `conn.supports` and `conn.gate`.

The scene keeps:
- presence: `_presence_client`, `_presence_ws_url` and its reconnect, one socket per joined room (ADR-2159 D3);
- `_gate_for_volume(volume_id, cap, label)`, which looks up the volume's connection;
- the HUD notice on `refused`.

`_switch_backend` (`:909-926`) is **deleted**. The HUD "Backend" control becomes per-volume show/hide plus "focus volume", and a hidden volume stops packing. Every one of the 15 `_gate` call sites names its volume. Where a call site has no volume (the HUD's global tabs), it uses the focused volume.

### 2.4 Additions to `binary_protocol.rs` (A implements; B and D consume)

These are additive, and existing methods are unchanged. Each one gets a unit test in the same file's `cfg(test)` module.
- `#[func] fn node_ids_for_mask(&self, mask: i64) -> PackedInt32Array`: ids in the render store whose class code bit is set.
- `#[func] fn render_aabb_ids(&self, ids: PackedInt32Array, exclude_id: i64) -> PackedFloat32Array`: the full (0th to 100th percentile) AABB over `ids`, as `[minx, miny, minz, maxx, maxy, maxz]`, or empty.
- `NodeMetaWire` gains `iris: Vec<String>` and `ontology_refs: String`. In `parse_initial_graph`, `iris` is filled by `crate::link_index::extract_iris(node_json)` and `ontology_refs` from `metadata.ontology_refs` (D writes both functions, §5.2).
- `#[func] fn iri_table(&self) -> Dictionary` (`iri → PackedInt32Array`, because one IRI may sit on several nodes) and `#[func] fn code_refs(&self) -> Array` (`is_file` is `node_type == "file"`), both built through `crate::link_index` and cached per topology.

## 3. `SpatialVolume`: agent B

New files:
- `xr-client/scripts/spatial_volume.gd`: the contract and the shared fit maths;
- `xr-client/scripts/emulated_volume.gd` (`extends "res://scripts/spatial_volume.gd"`);
- `xr-client/scripts/volume_arrangement.gd`;
- `xr-client/scenes/EmulatedVolume.tscn`;
- the test fixture `xr-client/tests/fixtures/fixture_content_source.gd`.

### 3.1 Node shape

```
EmulatedVolume (Node3D, emulated_volume.gd)          ← placed by VolumeArrangement; user-movable
├── Frame      (MeshInstance3D)   12-edge wire box, unshaded, 1 draw call; hidden when !visible_state
├── Handles    (Node3D)           8 corner grab spheres (Area3D + CollisionShape3D) for resize
└── Content    (Node3D)           fit transform lives here (scale, centre, optional yaw); content attaches under it
VolumeArrangement (Node3D, volume_arrangement.gd)    ← child of graph_scene root; native shim may re-host it
└── <EmulatedVolume> × 3
```

### 3.2 API

```gdscript
extends Node3D   # spatial_volume.gd
enum BoundsMode { BOUNDED = 0, IMMERSIVE = 1 }
const MIN_EXTENT := 1.0e-6
const FIT_EASE := 0.15

signal bounds_changed(volume_id: StringName, bounds: Vector3, mode: int)   # granted or runtime-driven
signal bounds_request_denied(volume_id: StringName, requested: Vector3)
signal bounds_mode_request_denied(volume_id: StringName, mode: int)
signal visible_changed(volume_id: StringName, visible: bool)
signal interactable_changed(volume_id: StringName, interactable: bool)
signal closed(volume_id: StringName)
signal fit_changed(volume_id: StringName, scale: float, centre: Vector3)
signal focus_changed(volume_id: StringName, focused: bool)
signal selection_changed(volume_id: StringName, conn_id: StringName, node_id: int)  # -1 = cleared

var volume_id: StringName
var bounds: Vector3 = Vector3.ONE        # full extents, metres
var bounds_mode: int = BoundsMode.BOUNDED
var visible_state := true
var interactable := true
var is_closed := false
var content: Node3D                      # the Content child
var undo_yaw_deg := 0.0                  # applied on Content (ADR-2158 D9, yaw)

static func fit_scale(bounds: Vector3, content_extent: Vector3) -> float
    # min over axes i with content_extent[i] >= MIN_EXTENT of bounds[i] / content_extent[i];
    # 1.0 if no axis qualifies (single point or empty). Pure; the E7 tests target this.
func request_bounds(b: Vector3) -> void          # emulated: clamps to [0.2, 4.0] m per axis, grants → bounds_changed; else denied
func request_bounds_mode(m: int) -> void
func request_visible(v: bool) -> void
func set_runtime_bounds(b: Vector3, m: int) -> void  # runtime-driven change (native shim, resize handle); un-latches manual fit, eases
func attach(source: Object) -> void              # see 3.3; at most one source per volume
func detach() -> void
func fit_into(content_aabb: AABB) -> void        # sets target scale/centre; IMMERSIVE ⇒ scale 1.0
func tick(delta: float) -> void                  # scene calls per frame: re-reads source AABB every FIT_RECOMPUTE frames, eases Content
func world_from_content(p: Vector3) -> Vector3   # graph units → world (link layer uses this)
func contains_world(p: Vector3, tol := 1.0e-4) -> bool
func begin_resize(handle: int, ctrl: Node3D) -> void
func update_resize(ctrl: Node3D) -> void
func end_resize() -> void
func set_focused(f: bool) -> void
func select(conn_id: StringName, node_id: int) -> void
```

**The fit rule (E7).** With `R = Basis(Vector3.UP, deg_to_rad(undo_yaw_deg))`, `s = fit_scale(bounds, (Transform3D(R, Vector3.ZERO) * aabb).size)` and `c = aabb.get_center()`, the transform is `Content.transform = Transform3D(R.scaled(Vector3.ONE * s), -(R * c) * s)`. That is: yaw first, then measure, then scale, then centre on the volume origin.
- The AABB is the **full** extent of the attached ids, not a percentile box, excluding only the node currently grabbed.
- During a resize the content may be outside the box for at most one frame. The ease applies to the scale and centre targets, and the box test in the GUT suite uses the target transform.

### 3.3 How content attaches

A content source is any object with these methods (duck-typed):

```gdscript
func content_id() -> StringName                  # e.g. &"visionclaw:0b0001"
func content_ids() -> PackedInt32Array           # wire ids this volume draws (current generation)
func content_aabb() -> AABB                      # full extent over content_ids(), graph units
func mount(parent: Node3D) -> void               # create/reparent its MultiMeshInstance3D nodes under parent
func unmount() -> void
func position_of(id: int) -> Vector3             # graph units
```

In E3, A implements the real source, `xr-client/scripts/connection_volume_source.gd` (owner A). It wraps a `BackendConnection` and a class mask, and packs that mask's ids into its own node, edge and impostor `MultiMeshInstance3D`s. B's tests use `tests/fixtures/fixture_content_source.gd`, which uses a fixed point set and a single MultiMesh.

### 3.4 `VolumeArrangement`

```gdscript
func add_volume(v: Node3D, yaw_deg: float) -> void   # places on the §1 ring
func volume(id: StringName) -> Node3D
func volumes() -> Array
func arrangement_aabb() -> AABB                      # world AABB of all volume boxes (native shim fits this)
func set_arrangement_scale(s: float) -> void         # native shim only
func reset_default() -> void                          # back to the ring; a user move overrides until then
```

## 4. Native container shim: agent C

New file: `xr-client/scripts/native_container_shim.gd`, `extends Node`. It holds every Godot spatial-container name and enum, and no other file may mention them.

```gdscript
const SETTING := "visionclaw/xr/native_spatial_container"           # bool; absent ⇒ false (no project.godot edit in phase 1)
const GODOT_SETTING := "xr/openxr/extensions/spatial_container/enabled"
const ACTIVATE_TIMEOUT_SEC := 3.0

signal mode_decided(mode: String, reason: String)    # "native" | "emulated"
signal fallback(reason: String)                       # native → emulated during the session
signal container_bounds_changed(bounds: Vector3, app_mode: int)

func evaluate(env: Dictionary = {}) -> Dictionary   # {mode, reason}; pure given env. Keys (each optional; an absent key reads the real value):
                                                     #   setting: bool, is_android: bool, godot_setting: bool,
                                                     #   engine_version: Dictionary (Engine.get_version_info shape),
                                                     #   class_exists: bool, ext: Object (fake extension)
func start(arrangement: Node3D) -> void              # emulated arrangement already exists; native only re-hosts/fits it
func tick(delta: float) -> void                      # activation timeout
static func to_app_mode(godot_mode: int) -> int      # Godot BOUNDED 0 / IMMERSIVE 1 → app enum (identity today, by table)
static func from_spec_mode(spec_mode: int) -> int    # spec 1/2 → app 0/1 (for logs and future bindings)
```

`evaluate` takes the first reason that applies, in this order:
1. `setting_off`
2. `not_android` (`!OS.has_feature("android")`)
3. `godot_setting_off`
4. `engine_lt_4_8` (`Engine.get_version_info()`)
5. `class_missing` (`!ClassDB.class_exists("OpenXRSpatialContainerExtension")`)
6. `no_instance` (neither `Engine.get_singleton` nor a static `get_singleton` yields one)
7. `extension_disabled` (`!is_enabled()`, which also covers a runtime without `XR_EXT_spatial_container`)
8. otherwise `native`.

**After start.**
- If `is_spatial_container_active()` is still false after `ACTIVATE_TIMEOUT_SEC`, emit `fallback("container_inactive")`. A `closed` signal emits `fallback("closed")`.
- **Signal names.** Connect each signal by checking for its master name and its Android-guide name with `has_signal`:
  - `spatial_container_bounds_changed`, or `…_bounds_mode_changed`;
  - `…visible_changed`, or `…visibility_changed`;
  - `…interactable_changed`, or `…interactability_changed`;
  - `…closed`.
- **On `bounds_changed`:** fit the arrangement with `fit_scale(container_bounds, arrangement_aabb().size)`, using B's static (no copy), and set it through `set_arrangement_scale`. Immersive mode means scale 1.
- **On fallback:** restore the arrangement to scale 1 at the ring default and leave every connection alone.

C must also record, from a 4.8-beta headless run on the ADR-2165 branch, the actual class, singleton and signal names found. Put them in the test file's header comment. This discharges research risk 2 as far as headless can, because the extension can never be *active* without an Android XR runtime.

## 5. Code links and the link layer: agent D

### 5.1 Codescape side (in the `codescape/` repository; the lead bumps the pin)

- `crates/codescape-ingest/src/annotations.rs` (new): the grammar, binding and roll-up exactly as ADR-2164 D1 and D2 specify. It produces `Vec<Annotation { node: u32, path, line, iri }>` from file text plus the snapshot's spans. It reads files the ingest already reads, and adds no new mount.
- Server: a side table in repo state, rebuilt for `changed_paths` on each delta. `xr::initial_graph_load` (`codescape-server/src/xr.rs:21-61`) adds `metadata.ontology_refs` when the node has refs. The route `GET /api/graph/ontology-refs?repo=` is added to `routes/mod.rs:405-418` and to its module doc table.
- No new crate dependency. `tests/isolation.rs` must pass unchanged.
- **Root-repository docs, in the same change as the submodule pin bump.**
  - Add the `ontology_refs` bullet to `docs/PROTOCOL-registry.md` (§ "Second producer: Codescape").
  - Re-verify ADR-2143 and bump its `verified_commit`, because `scripts/adr-index-gen.js` flags ADR-2143 stale when that file changes.

### 5.2 Client Rust (`xr-client/rust`)

`src/link_index.rs` (new; D adds exactly one line, `pub mod link_index;`, to `lib.rs`) is pure and holds no gdext types:

```rust
pub fn extract_iris(node: &serde_json::Value) -> Vec<String>;          // metadata.class_iri, metadata.page_iri, top-level owl_class_iri; dedup
pub fn parse_ontology_refs(s: &str) -> Vec<(String, u32)>;              // "<iri>#L<n> …" → [(iri, n)], malformed tokens skipped
pub struct CodeRef { pub key: String, pub id: u32, pub path: String, pub is_file: bool, pub refs: Vec<(String, u32)> }
pub struct LinkRecord { /* ADR-2164 D5 fields, kind = "annotates" */ }
pub struct JoinResult { pub records: Vec<LinkRecord>, pub unresolved: usize }
pub fn join(code: &[CodeRef], code_gen: i64, iri_ids: &HashMap<String, Vec<u32>>, vc_gen: i64,
            class_of: impl Fn(u32) -> u8) -> JoinResult;
    // De-duplicate on (iri, path, line), preferring !is_file. Emit one record per target id.
    // Volume from class code: 0 knowledge, 1 ontology.
```

### 5.3 Client GDScript

- `xr-client/scripts/xr_prefs.gd` (new, static helpers):
  - `cross_volume_links() -> bool` reads `user://xr_prefs.cfg` `[xr]`, key `cross_volume_links_debug` when `OS.is_debug_build()` and `cross_volume_links_release` otherwise, and returns `OS.is_debug_build()` when that key is absent;
  - `set_cross_volume_links(on: bool) -> void` writes only the running build type's key;
  - `static func _key_for(debug: bool) -> String` is exposed so that tests can cover both build types headless.
- `xr-client/scripts/link_layer.gd` (new, `extends Node3D`):

```gdscript
const MAX_LINKS := 512
const REFRESH_HZ := 10.0
const TRIS_PER_LINK := 12
signal counts_changed(drawn: int, unresolved: int, overflow: int)
func set_enabled(on: bool) -> void                 # off ⇒ multimesh.instance_count = 0 and visible = false this frame
func is_enabled() -> bool
func set_records(kind: String, records: Array) -> void   # replaces that kind's set; Dictionaries in ADR-2164 D5 shape
func drop_connection(conn_id: StringName) -> void  # removes every record naming conn_id (offline / reconnect)
func tick(delta: float, volumes: Dictionary, conns: Dictionary) -> void   # 10 Hz endpoint refresh via volume.world_from_content(conn.node_position(id))
func triangle_count() -> int                       # for the frame-budget outside-layer charge
func draw_calls() -> int                           # 0 when off or empty, else 1
```

- `xr-client/scripts/bridge_link_source.gd` (new, pure):
  - `records(conn, ids_by_volume) -> Array` gives knowledge↔ontology edges from `client.get_edges()` whose two ends sit in different volumes, as `kind: "bridge"`.
  - It returns at most 256 of them, ranked by `get_edge_weights()`.

## 6. File-ownership table

| Agent | Owns: new files | Owns: edits to existing files | Must not touch |
|---|---|---|---|
| **A** multi-backend | `scripts/backend_connection.gd`, `scripts/connection_volume_source.gd`, `rust/tests/two_clients.rs`, `tests/unit/test_backend_connection.gd`, `tests/unit/test_connection_volume_source.gd` | **sole editor of** `scripts/graph_scene.gd`, `scripts/hud.gd`, `rust/src/binary_protocol.rs`, `rust/src/frame_budget.rs`, `scripts/frame_budget_pass.gd`, `scripts/backend_caps.gd`, `scenes/GraphScene.tscn`; updates `tests/unit/test_backend_switch.gd` (the switch is gone) | B, C, D files; `project.godot`, `xr_boot.gd` |
| **B** volumes | `scripts/spatial_volume.gd`, `scripts/emulated_volume.gd`, `scripts/volume_arrangement.gd`, `scenes/EmulatedVolume.tscn`, `tests/fixtures/fixture_content_source.gd`, `tests/unit/test_spatial_volume.gd`, `tests/unit/test_volume_arrangement.gd` | none | everything else |
| **C** native shim | `scripts/native_container_shim.gd`, `tests/unit/test_native_container_shim.gd`, `tests/fixtures/fake_spatial_container_ext.gd` | none (no `project.godot` edit; the setting defaults to absent ⇒ off) | everything else |
| **D** code links | Codescape: `crates/codescape-ingest/src/annotations.rs` + its edits to `codescape-ingest/src/lib.rs`, `codescape-server/src/xr.rs`, `routes/mod.rs`, repo state (Codescape repository, own commit). Client: `rust/src/link_index.rs`, `scripts/link_layer.gd`, `scripts/xr_prefs.gd`, `scripts/bridge_link_source.gd`, `tests/unit/test_link_layer.gd`, `tests/unit/test_xr_prefs.gd`, `tests/unit/test_bridge_link_source.gd`. Seed `@ontology` annotations (ADR-2164 D9) in estate files outside every path in this table | one line in `rust/src/lib.rs` | `binary_protocol.rs` (A wires it), `graph_scene.gd`, the root `members` and any root↔codescape path dependency (ADR-2143) |

## 7. Integration points A wires into `graph_scene.gd`

A wires these after P3-pre passes and after B, C and D have merged their files.

1. **`_ready` (`:461`).**
   - Replace the single `BinaryProtocolClient.create()` (`:479`) with `BackendConnection.make(&"visionclaw", …)` and `make(&"codescape", …)`. Add both as children, and connect `connected_changed`, `topology_changed`, `text_received` and `refused`.
   - Presence (`:480`) stays as it is.
2. **Build the arrangement.** Instantiate `VolumeArrangement` with three `EmulatedVolume`s, using the ids, yaws and bounds in §1. Attach a `connection_volume_source` per E3 binding to each. Set `undo_yaw_deg` once the yaw check is done (ADR-2158 D9).
3. **Native shim.**
   - Create `native_container_shim` and call `evaluate()`.
   - On `native`, call `start(arrangement)`. On `fallback(reason)`, show a HUD notice in debug builds only and continue.
   - Log the `{mode, reason}` once at boot.
4. **`_physics_process` (`:1636`).**
   - Call `conn.poll(delta)` for each connection.
   - Call `volume.tick(delta)` for each visible volume. This replaces `_fit_graph_to_view` (`:1920`), which is deleted.
   - Call `link_layer.tick(delta, volumes, conns)`.
5. **On `topology_changed(conn_id, gen)`.**
   - Call `link_layer.drop_connection(conn_id)`.
   - If both connections are up, call `link_layer.set_records("annotates", <D's join over conn.code_refs() and vc.iri_table()>)` and `set_records("bridge", bridge_link_source.records(…))`.
6. **On `connected_changed(conn_id, false)`.** Call `link_layer.drop_connection(conn_id)`.
7. **Frame budget.**
   - Split the allocator per visible volume (ADR-2159 D4; A's design, tested against `xr-client/perf/`).
   - Add `link_layer.triangle_count()` and `draw_calls()` to the scene-measured outside-layer charge.
8. **HUD.**
   - Add a "Cross-volume links" toggle that calls `xr_prefs.set_cross_volume_links(on)` and `link_layer.set_enabled(on)`.
   - Show the `counts_changed` figures in the debug strip.
   - Replace the Backend toggle with per-volume show/hide and focus.
9. **Gates.** Re-point the 15 `_gate` sites to `_gate_for_volume`. Drag in the Codescape volume must be refused with the HUD notice, and no `nodeDrag` may be sent (E9).

## 8. Tests each agent adds

| Agent | Test | Must show |
|---|---|---|
| A | `rust/tests/two_clients.rs` (cargo) | §2.1 preflight |
| A | `test_backend_connection.gd` (GUT) | per-connection caps; the Codescape `send_drag_*` returns false and sends nothing (a fake client records calls) (E9); `close()` cancels requests and bumps the generation; `refused` fires with a label; two connections never share a client |
| A | `test_connection_volume_source.gd` | a mask selects only its class; full-extent AABB; mount and unmount leave no orphan nodes |
| A | `binary_protocol.rs` `cfg(test)` | `node_ids_for_mask`, `render_aabb_ids`, `iri_table` and `code_refs` on fixture frames |
| B | `test_spatial_volume.gd` | `fit_scale` cases: isotropic; anisotropic (each axis checked); one axis below 1e-6 ignored; all axes below 1e-6 ⇒ 1.0. **E7:** after fit, every fixture point's world centre is inside the box. Resize every frame: no point outside for more than one frame. Immersive ⇒ scale 1. Request granted, denied and clamped; `closed` stops ticking; `undo_yaw_deg` is applied |
| B | `test_volume_arrangement.gd` | ring placement at −60/+60/0; a user move overrides; `reset_default` restores; `arrangement_aabb` |
| C | `test_native_container_shim.gd` | each `evaluate` reason in order, driven through the `env` keys (the real headless engine gives `setting_off`); `native` with a fake active extension; activation timeout ⇒ `fallback("container_inactive")`; `closed` ⇒ fallback with the arrangement restored; the enum tables; both signal-name variants connect |
| D | `codescape-ingest` tests | ADR-2164 L1 |
| D | `codescape-server` tests | ADR-2164 L2 (`ontology_refs` present only on annotated nodes; the route; `tests/isolation.rs` unchanged and green) |
| D | `link_index.rs` `cfg(test)` | L3: `extract_iris`; `parse_ontology_refs` with malformed tokens; `join` exact records plus the `unresolved` count; symbol beats file on a duplicate (iri, path, line); one IRI on two nodes gives two records; class-to-volume mapping |
| D | `test_link_layer.gd`, `test_xr_prefs.gd`, `test_bridge_link_source.gd` | L4 and L5: the build default; the stored preference for the running build type overrides it, and the other type's key is ignored; off ⇒ 0 instances in the same frame; `drop_connection` empties; cap 512 with an overflow count; `draw_calls() <= 1`; the bridge cap is 256 and only cross-volume edges qualify |

**Phase-1 exit.** All of the above are green on the HP host headless (GUT, plus `gut_guard.sh`) and in `cargo test`, with the outputs pasted in each commit message. After integration, A runs the E8 headset session, and D runs ADR-2164 L6 on the live estate.
