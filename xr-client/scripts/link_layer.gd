extends Node3D
## Used via `const LinkLayer := preload("res://scripts/link_layer.gd")` rather
## than a class_name, so a headless run with a stale global class cache still
## resolves it (see backend_caps.gd).

## The cross-volume link layer (ADR-2164 D8, ADR-2158 D6).
##
## Every link that crosses a volume boundary (code → concept `annotates`,
## knowledge ↔ ontology `bridge`, later `dataspace`) is drawn here, in world
## space, by ONE MultiMeshInstance3D: one draw call, at most MAX_LINKS
## instances of a TRIS_PER_LINK-triangle segment, with the edge stride of 16
## floats (12 transform + 4 custom data, Invariant 3). The buffer is packed by
## the Rust `LinkPacker` (`render_store_pack.rs::pack_links`) on every `tick`: each
## end is the volume's content-to-world transform applied to the connection's
## `RenderStore::drawn_position_of`, the same fold-aware answer the edges draw
## with, read in the frame the volumes moved. This script keeps the records, their
## order, the generation purge and the per-volume and per-connection state, and
## uploads what the pack returns; it resolves no position and builds no instance.
##
## Records are client render state only (ADR-2164 D5): Dictionaries shaped
## `{source: {volume, conn, key, id}, target: {…}, kind, provenance: {…,
## generations: [src_gen, dst_gen]}}`. A record is valid only while both
## connections are on the generations it names: a reconnect on either side
## drops it before anything new is drawn (ADR-2164 D6, L4).
##
## Off (the `xr.cross_volume_links` switch, XrPrefs) means zero instances and
## an invisible node in the same frame, so it costs nothing.
##
## Highlight (spatial-volume UX, features 2 and 3): `set_highlight(keys)` draws
## every link touching a key brighter (HIGHLIGHT_LIFT toward white) and thicker
## (HIGHLIGHT_RADIUS_SCALE), and puts a ring marker (the same open prism, short
## and wide) on each end of each highlighted link. Markers are extra instances
## of the same MultiMesh, so the layer stays one draw call; `triangle_count()`
## charges links and markers, while `drawn_count()` and `counts_changed` stay
## links only. `drawn_segments()` exposes the last refresh's segments in world
## space for the link picker (packed every tick, so they are this frame's).

const MAX_LINKS := 512
const TRIS_PER_LINK := 12
const STRIDE := 16
const SIDES := 6  # open hexagonal prism: 6 quads = 12 triangles
## Mirrors of the Rust pack's constants (`LINK_RADIUS_M`, `LINK_HIGHLIGHT_RADIUS_SCALE`,
## `LINK_HIGHLIGHT_LIFT`, `LINK_MARKER_RADIUS_M`), which draw the instances; the tests pin the
## drawn buffer to them.
const RADIUS_M := 0.004
const HIGHLIGHT_RADIUS_SCALE := 2.5
const HIGHLIGHT_LIFT := 0.55
const MARKER_RADIUS_M := 0.018
## Draw order of kinds (ADR-2164 D8: ordered by kind, target key, source key).
const KIND_ORDER := ["annotates", "bridge", "dataspace"]
const KIND_COLOURS := {
	"annotates": Color(1.0, 0.72, 0.2),
	"bridge": Color(0.35, 0.85, 1.0),
	"dataspace": Color(0.9, 0.4, 1.0),
}
const VOL_UNMAPPED := 0  # absent, or cannot map content: its links count as unresolved
const VOL_HIDDEN := 1    # exists but hidden or closed: its links are skipped, not counted
const VOL_DRAWABLE := 2
const SHADER_CODE := """
shader_type spatial;
render_mode unshaded, cull_disabled;
varying vec4 link_colour;
void vertex() { link_colour = INSTANCE_CUSTOM; }
void fragment() { ALBEDO = link_colour.rgb; }
"""

signal counts_changed(drawn: int, unresolved: int, overflow: int)

var multimesh_instance: MultiMeshInstance3D
var _enabled := false
var _records := {}          # kind (String) → Array of record Dictionaries
var _external_unresolved := {}  # kind → int, from the producer (e.g. the join)
var _ordered: Array = []    # drawable order, capped at MAX_LINKS
# The Rust pack: owns the link table and the instance buffer. Records reach it as indices
# into `_conn_ids` and `_vol_ids`, built when the records change, not every tick.
var _packer: RefCounted = ClassDB.instantiate(&"LinkPacker")
var _conn_ids: Array = []   # distinct connection ids named by `_ordered`, index = packer connection
var _vol_ids: Array = []    # distinct volume ids named by `_ordered`, index = packer volume
var _overflow := 0
var _drawn := 0
var _unresolved := 0
var _last_volumes := {}
var _last_conns := {}
var _last_slots := {}
var _emitted := Vector3i(-1, -1, -1)
var _highlight := {}        # "conn|node_id" → true
var _markers := 0
# Last tick's segments, built from the pack on demand by `drawn_segments()` (the picker, tests).
var _segments_cache: Array = []
var _segments_cache_valid := false
# Record validity depends only on connection generations and on the records themselves, so the
# purge runs when either changed, not on every tick.
var _gen_sig := {}
var _purge_pending := true


func _init() -> void:
	name = "LinkLayer"
	multimesh_instance = MultiMeshInstance3D.new()
	multimesh_instance.name = "Links"
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = false
	mm.use_custom_data = true
	mm.mesh = _segment_mesh()
	mm.instance_count = 0
	multimesh_instance.multimesh = mm
	var mat := ShaderMaterial.new()
	var sh := Shader.new()
	sh.code = SHADER_CODE
	mat.shader = sh
	multimesh_instance.material_override = mat
	multimesh_instance.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(multimesh_instance)
	visible = false


## Turn the layer on or off. Off empties the MultiMesh and hides the node now.
func set_enabled(on: bool) -> void:
	if on == _enabled:
		return
	_enabled = on
	if on:
		visible = true
		return
	multimesh_instance.multimesh.instance_count = 0
	visible = false
	_markers = 0
	_clear_segments()
	_set_counts(0, _unresolved, _overflow)


func is_enabled() -> bool:
	return _enabled


## Replace every record of `kind`.
func set_records(kind: String, records: Array) -> void:
	_records[kind] = records.duplicate()
	_purge_pending = true
	_reorder()


## Unresolved count reported by the producer of `kind` (the join counts refs
## that found no node); added to the layer's own count in `counts_changed`.
func set_unresolved(kind: String, n: int) -> void:
	_external_unresolved[kind] = maxi(n, 0)


## Drop every record naming `conn_id` on either end (offline or reconnect),
## and redraw this frame.
func drop_connection(conn_id: StringName) -> void:
	var cid := String(conn_id)
	for kind: String in _records.keys():
		var kept: Array = []
		for r: Dictionary in _records[kind]:
			if _conn_of(r, "source") != cid and _conn_of(r, "target") != cid:
				kept.append(r)
		_records[kind] = kept
		if kept.is_empty():
			_external_unresolved.erase(kind)
	_purge_pending = true
	_reorder()
	_refresh(_last_volumes, _last_conns)


## Per frame. Packs every segment in Rust: each end is the volume's content-to-world transform
## applied to the connection store's drawn position of the node, read this frame.
func tick(_delta: float, volumes: Dictionary, conns: Dictionary, slots: Dictionary = {}) -> void:
	_last_volumes = volumes
	_last_conns = conns
	_last_slots = slots
	if not _enabled:
		return
	_refresh(volumes, conns)


## Links plus highlight markers: every instance is one TRIS_PER_LINK prism.
func triangle_count() -> int:
	return (_drawn + _markers) * TRIS_PER_LINK if _enabled else 0


func draw_calls() -> int:
	return 1 if _enabled and _drawn + _markers > 0 else 0


func marker_count() -> int:
	return _markers if _enabled else 0


## The segments drawn by the last refresh, in world space, in draw order:
## `[{a, b, record, from: {volume, conn, node_id}, to: {…}, highlighted}]`,
## `from` being the record's source end. Empty while the layer is off.
## Endpoints are as of the last tick.
func drawn_segments() -> Array:
	if not _enabled:
		return []
	if not _segments_cache_valid:
		var out: Array = []
		var pa: PackedVector3Array = _packer.segment_a()
		var pb: PackedVector3Array = _packer.segment_b()
		var which: PackedInt32Array = _packer.segment_links()
		for i: int in which.size():
			var r: Dictionary = _ordered[which[i]]
			out.append({
				"a": pa[i], "b": pb[i], "record": r,
				"from": _end_ref(r, "source"), "to": _end_ref(r, "target"),
				"highlighted": _is_highlighted(r),
			})
		_segments_cache = out
		_segments_cache_valid = true
	return _segments_cache


## Highlight every link touching any of `keys` (`[{conn, node_id}]`); `[]`
## clears. Redraws at once.
func set_highlight(keys: Array) -> void:
	var next := {}
	for k: Variant in keys:
		if k is Dictionary:
			next[_key_of(String((k as Dictionary).get("conn", "")), int((k as Dictionary).get("node_id", -1)))] = true
	if next == _highlight:
		return
	_highlight = next
	_sync_lit()
	if _enabled:
		_refresh(_last_volumes, _last_conns)


func highlight_keys() -> Array:
	var out: Array = []
	for k: String in _highlight:
		var cut := k.rfind("|")
		out.append({"conn": StringName(k.substr(0, cut)), "node_id": int(k.substr(cut + 1))})
	return out


## The ends opposite `key` (`{conn, node_id}`) of every current-generation
## record touching it, de-duplicated, in draw order: `[{volume, conn,
## node_id}]`. Read from the records, not the drawn set, so a counterpart whose
## volume is hidden or whose link is past the cap is still listed; the caller
## decides what to do with it.
func highlighted_counterparts(key: Dictionary) -> Array:
	var want := _key_of(String(key.get("conn", "")), int(key.get("node_id", -1)))
	var seen := {}
	var out: Array = []
	var all: Array = []
	for kind: String in _records:
		all.append_array(_records[kind])
	all.sort_custom(_before)
	for r: Dictionary in all:
		if not _last_conns.is_empty() and not _current(r, _last_conns):
			continue
		for pair: Array in [["source", "target"], ["target", "source"]]:
			if _end_key(r, pair[0]) != want:
				continue
			var other := _end_ref(r, pair[1])
			var ok := _key_of(String(other["conn"]), int(other["node_id"]))
			if ok == want or seen.has(ok):
				continue
			seen[ok] = true
			out.append(other)
	return out


func drawn_count() -> int:
	return _drawn


func overflow_count() -> int:
	return _overflow


func unresolved_count() -> int:
	return _unresolved


func record_count() -> int:
	var n := 0
	for kind: String in _records:
		n += (_records[kind] as Array).size()
	return n


# ---------------------------------------------------------------- internals

static func _key_of(conn: String, node_id: int) -> String:
	return "%s|%d" % [conn, node_id]


static func _end_key(r: Dictionary, end: String) -> String:
	var e: Variant = r.get(end)
	if not (e is Dictionary):
		return ""
	return _key_of(String((e as Dictionary).get("conn", "")), int((e as Dictionary).get("id", -1)))


## `{volume, conn, node_id}` for one end of a record.
static func _end_ref(r: Dictionary, end: String) -> Dictionary:
	var e: Dictionary = r.get(end, {}) if r.get(end) is Dictionary else {}
	return {
		"volume": StringName(String(e.get("volume", ""))),
		"conn": StringName(String(e.get("conn", ""))),
		"node_id": int(e.get("id", -1)),
	}


func _is_highlighted(r: Dictionary) -> bool:
	return not _highlight.is_empty() \
		and (_highlight.has(_end_key(r, "source")) or _highlight.has(_end_key(r, "target")))

static func _conn_of(r: Dictionary, end: String) -> String:
	var e: Variant = r.get(end)
	return String((e as Dictionary).get("conn", "")) if e is Dictionary else ""


static func _kind_rank(kind: String) -> int:
	var i := KIND_ORDER.find(kind)
	return i if i >= 0 else KIND_ORDER.size()


static func _order_key(r: Dictionary) -> Array:
	var kind := String(r.get("kind", ""))
	var s: Dictionary = r.get("source", {})
	var t: Dictionary = r.get("target", {})
	return [_kind_rank(kind), kind, String(t.get("key", "")), String(s.get("key", "")),
		int(t.get("id", 0)), int(s.get("id", 0))]


static func _before(a: Dictionary, b: Dictionary) -> bool:
	var ka := _order_key(a)
	var kb := _order_key(b)
	for i: int in ka.size():
		if ka[i] != kb[i]:
			return ka[i] < kb[i]
	return false


func _reorder() -> void:
	var all: Array = []
	for kind: String in _records:
		all.append_array(_records[kind])
	all.sort_custom(_before)
	_overflow = maxi(all.size() - MAX_LINKS, 0)
	_ordered = all.slice(0, MAX_LINKS)
	var conn_idx := {}
	var vol_idx := {}
	_conn_ids = []
	_vol_ids = []
	var ends := PackedInt32Array()
	var colours := PackedColorArray()
	ends.resize(_ordered.size() * 6)
	colours.resize(_ordered.size())
	for i: int in _ordered.size():
		var r: Dictionary = _ordered[i]
		var se: Dictionary = r.get("source", {})
		var te: Dictionary = r.get("target", {})
		ends[i * 6] = _index_of(conn_idx, _conn_ids, String(se.get("conn", "")))
		ends[i * 6 + 1] = _index_of(conn_idx, _conn_ids, String(te.get("conn", "")))
		ends[i * 6 + 2] = int(se.get("id", -1))
		ends[i * 6 + 3] = int(te.get("id", -1))
		ends[i * 6 + 4] = _index_of(vol_idx, _vol_ids, String(se.get("volume", "")))
		ends[i * 6 + 5] = _index_of(vol_idx, _vol_ids, String(te.get("volume", "")))
		colours[i] = KIND_COLOURS.get(String(r.get("kind", "")), Color.WHITE)
	_packer.set_links(ends, colours)
	_sync_lit()


## Index of `name` in `names`, appending it on first sight.
static func _index_of(index: Dictionary, names: Array, name: String) -> int:
	if not index.has(name):
		index[name] = names.size()
		names.append(name)
	return index[name]


## Tell the pack which links are highlighted.
func _sync_lit() -> void:
	var lit := PackedByteArray()
	lit.resize(_ordered.size())
	if not _highlight.is_empty():
		for i: int in _ordered.size():
			lit[i] = 1 if _is_highlighted(_ordered[i]) else 0
	_packer.set_lit(lit)


## Drop records whose generations no longer match their connections. A
## connection absent from `conns` drops nothing (its ends just do not draw).
## Returns whether anything was dropped.
func _purge_stale(conns: Dictionary) -> bool:
	var dropped := false
	for kind: String in _records.keys():
		var kept: Array = []
		for r: Dictionary in _records[kind]:
			if _current(r, conns):
				kept.append(r)
			else:
				dropped = true
		_records[kind] = kept
	return dropped


static func _conn_lookup(conns: Dictionary, cid: String) -> Object:
	var c: Variant = conns.get(StringName(cid), conns.get(cid))
	return c as Object


static func _current(r: Dictionary, conns: Dictionary) -> bool:
	var gens: Array = (r.get("provenance", {}) as Dictionary).get("generations", [])
	var ends := ["source", "target"]
	for i: int in 2:
		var conn := _conn_lookup(conns, _conn_of(r, ends[i]))
		if conn == null:
			continue
		if gens.size() == 2 and int(conn.get("generation")) != int(gens[i]):
			return false
	return true


static func _volume_lookup(volumes: Dictionary, vid: String) -> Object:
	var v: Variant = volumes.get(StringName(vid), volumes.get(vid))
	return v as Object


static func _drawable(v: Object) -> bool:
	if v == null or not v.has_method("content_to_world"):
		return false
	if v.get("is_closed") == true:
		return false
	return v.get("visible_state") != false


## A volume's state for this tick: [state, content-to-world Transform3D].
static func _volume_state(volumes: Dictionary, vid: String) -> Array:
	var v := _volume_lookup(volumes, vid)
	var state := VOL_UNMAPPED
	var xf := Transform3D.IDENTITY
	if v != null and v.has_method("content_to_world"):
		state = VOL_DRAWABLE if _drawable(v) else VOL_HIDDEN
		if state == VOL_DRAWABLE:
			xf = v.content_to_world()
	return [state, xf]


## The gdext client of a connection usable as an endpoint source this tick, or null (absent, no
## client, offline); the pack treats a null as unresolved.
static func _usable_client(conns: Dictionary, cid: String) -> Object:
	var c := _conn_lookup(conns, cid)
	if c == null or (c.has_method("is_connected_now") and not c.is_connected_now()):
		return null
	return c.get("client") as Object


func _clear_segments() -> void:
	_segments_cache = []
	_segments_cache_valid = false


## Whether the records could have gone stale: a record changed, or a connection's generation did.
func _needs_purge(conns: Dictionary) -> bool:
	var sig := {}
	for k: Variant in conns:
		var c: Object = conns[k] as Object
		if c != null:
			sig[k] = int(c.get("generation"))
	if _purge_pending or sig != _gen_sig:
		_gen_sig = sig
		_purge_pending = false
		return true
	return false


func _refresh(volumes: Dictionary, conns: Dictionary) -> void:
	if _needs_purge(conns) and _purge_stale(conns):
		_reorder()
	var mm := multimesh_instance.multimesh
	var unresolved := 0
	for kind: String in _external_unresolved:
		unresolved += int(_external_unresolved[kind])
	if not _enabled:
		mm.instance_count = 0
		_markers = 0
		_clear_segments()
		_set_counts(0, unresolved, _overflow)
		return
	var inv: Transform3D = global_transform.affine_inverse() if is_inside_tree() else Transform3D.IDENTITY
	var clients: Array = []
	for cid: String in _conn_ids:
		clients.append(_usable_client(conns, cid))
	var states := PackedByteArray()
	var xfs: Array = []
	var vol_slots := PackedInt32Array()
	for vid: String in _vol_ids:
		var vs := _volume_state(volumes, vid)
		states.append(vs[0])
		xfs.append(vs[1])
		vol_slots.append(int(_last_slots.get(vid, _last_slots.get(StringName(vid), 0))))
	var buf: PackedFloat32Array = _packer.pack(clients, states, xfs, vol_slots, inv)
	_segments_cache = []
	_segments_cache_valid = false
	var links: int = _packer.link_count()
	_markers = _packer.marker_count()
	var n := links + _markers
	mm.instance_count = n
	if n > 0:
		mm.buffer = buf
	_set_counts(links, unresolved + _packer.unresolved_count(), _overflow)


func _set_counts(drawn: int, unresolved: int, overflow: int) -> void:
	var now := Vector3i(drawn, unresolved, overflow)
	_drawn = drawn
	_unresolved = unresolved
	if now != _emitted:
		_emitted = now
		counts_changed.emit(drawn, unresolved, overflow)


## Open hexagonal prism along Y, radius 1, length 1: SIDES quads, 2·SIDES triangles.
static func _segment_mesh() -> ArrayMesh:
	var verts := PackedVector3Array()
	for s: int in SIDES:
		var a0 := TAU * float(s) / SIDES
		var a1 := TAU * float(s + 1) / SIDES
		var p0 := Vector3(cos(a0), 0.0, sin(a0))
		var p1 := Vector3(cos(a1), 0.0, sin(a1))
		var lo := Vector3(0, -0.5, 0)
		var hi := Vector3(0, 0.5, 0)
		verts.append_array([p0 + lo, p1 + lo, p1 + hi, p0 + lo, p1 + hi, p0 + hi])
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = verts
	var m := ArrayMesh.new()
	m.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return m
