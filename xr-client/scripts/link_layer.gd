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
## floats (12 transform + 4 custom data, Invariant 3). Endpoints are refreshed
## at REFRESH_HZ, not every frame.
##
## Records are client render state only (ADR-2164 D5): Dictionaries shaped
## `{source: {volume, conn, key, id}, target: {…}, kind, provenance: {…,
## generations: [src_gen, dst_gen]}}`. A record is valid only while both
## connections are on the generations it names: a reconnect on either side
## drops it before anything new is drawn (ADR-2164 D6, L4).
##
## Off (the `xr.cross_volume_links` switch, XrPrefs) means zero instances and
## an invisible node in the same frame, so it costs nothing.

const MAX_LINKS := 512
const REFRESH_HZ := 10.0
const TRIS_PER_LINK := 12
const STRIDE := 16
const SIDES := 6  # open hexagonal prism: 6 quads = 12 triangles
const RADIUS_M := 0.004
const MIN_LENGTH_M := 1.0e-4
## Draw order of kinds (ADR-2164 D8: ordered by kind, target key, source key).
const KIND_ORDER := ["annotates", "bridge", "dataspace"]
const KIND_COLOURS := {
	"annotates": Color(1.0, 0.72, 0.2),
	"bridge": Color(0.35, 0.85, 1.0),
	"dataspace": Color(0.9, 0.4, 1.0),
}
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
var _overflow := 0
var _drawn := 0
var _unresolved := 0
var _accum := 0.0
var _dirty := true
var _last_volumes := {}
var _last_conns := {}
var _emitted := Vector3i(-1, -1, -1)


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
		_dirty = true
		return
	multimesh_instance.multimesh.instance_count = 0
	visible = false
	_set_counts(0, _unresolved, _overflow)


func is_enabled() -> bool:
	return _enabled


## Replace every record of `kind`.
func set_records(kind: String, records: Array) -> void:
	_records[kind] = records.duplicate()
	_reorder()


## Unresolved count reported by the producer of `kind` (the join counts refs
## that found no node); added to the layer's own count in `counts_changed`.
func set_unresolved(kind: String, n: int) -> void:
	_external_unresolved[kind] = maxi(n, 0)
	_dirty = true


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
	_reorder()
	_refresh(_last_volumes, _last_conns)


## Per frame. Refreshes endpoints at REFRESH_HZ (or at once after a change):
## each end is `volume.world_from_content(conn.node_position(id))`.
func tick(delta: float, volumes: Dictionary, conns: Dictionary) -> void:
	_last_volumes = volumes
	_last_conns = conns
	if not _enabled:
		return
	_accum += delta
	if not _dirty and _accum < 1.0 / REFRESH_HZ:
		return
	_accum = 0.0
	_refresh(volumes, conns)


func triangle_count() -> int:
	return _drawn * TRIS_PER_LINK if _enabled else 0


func draw_calls() -> int:
	return 1 if _enabled and _drawn > 0 else 0


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
	_dirty = true


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
	if v == null or not v.has_method("world_from_content"):
		return false
	if v.get("is_closed") == true:
		return false
	return v.get("visible_state") != false


## Whether either end sits in a volume that exists but is hidden or closed:
## such a link is not drawn and not counted as unresolved.
static func _hidden(r: Dictionary, volumes: Dictionary) -> bool:
	for end: String in ["source", "target"]:
		var e: Dictionary = r.get(end, {})
		var v := _volume_lookup(volumes, String(e.get("volume", "")))
		if v != null and v.has_method("world_from_content") and not _drawable(v):
			return true
	return false


## World position of one end, or null when its volume or connection is
## missing, its connection is offline, or the volume cannot map content.
func _endpoint(r: Dictionary, end: String, volumes: Dictionary, conns: Dictionary) -> Variant:
	var e: Dictionary = r.get(end, {})
	var v := _volume_lookup(volumes, String(e.get("volume", "")))
	var c := _conn_lookup(conns, String(e.get("conn", "")))
	if not _drawable(v) or c == null or not c.has_method("node_position"):
		return null
	if c.has_method("is_connected_now") and not c.is_connected_now():
		return null
	var p: Vector3 = c.node_position(int(e.get("id", -1)))
	return v.world_from_content(p)


func _refresh(volumes: Dictionary, conns: Dictionary) -> void:
	_dirty = false
	if _purge_stale(conns):
		_reorder()
		_dirty = false
	var mm := multimesh_instance.multimesh
	var unresolved := 0
	for kind: String in _external_unresolved:
		unresolved += int(_external_unresolved[kind])
	if not _enabled:
		mm.instance_count = 0
		_set_counts(0, unresolved, _overflow)
		return
	var inv: Transform3D = global_transform.affine_inverse() if is_inside_tree() else Transform3D.IDENTITY
	var buf := PackedFloat32Array()
	buf.resize(_ordered.size() * STRIDE)
	var n := 0
	for r: Dictionary in _ordered:
		if _hidden(r, volumes):
			continue
		var a: Variant = _endpoint(r, "source", volumes, conns)
		var b: Variant = _endpoint(r, "target", volumes, conns)
		if a == null or b == null:
			unresolved += 1
			continue
		var pa: Vector3 = inv * (a as Vector3)
		var pb: Vector3 = inv * (b as Vector3)
		var d := pb - pa
		if d.length() < MIN_LENGTH_M:
			continue
		_pack(buf, n, pa, pb, KIND_COLOURS.get(String(r.get("kind", "")), Color.WHITE))
		n += 1
	buf.resize(n * STRIDE)
	mm.instance_count = n
	if n > 0:
		mm.buffer = buf
	_set_counts(n, unresolved, _overflow)


## Write instance `i`: a unit segment along local Y (-0.5..0.5, radius 1)
## stretched from `a` to `b` with radius RADIUS_M, plus the kind colour.
static func _pack(buf: PackedFloat32Array, i: int, a: Vector3, b: Vector3, colour: Color) -> void:
	var y := b - a
	var dir := y.normalized()
	var ref := Vector3.UP if absf(dir.dot(Vector3.UP)) < 0.99 else Vector3.RIGHT
	var x := dir.cross(ref).normalized() * RADIUS_M
	var z := x.cross(dir).normalized() * RADIUS_M
	var o := (a + b) * 0.5
	var k := i * STRIDE
	# MultiMesh 3D layout: rows of the basis with the origin in the 4th column.
	var row := [x.x, y.x, z.x, o.x, x.y, y.y, z.y, o.y, x.z, y.z, z.z, o.z,
		colour.r, colour.g, colour.b, colour.a]
	for j: int in STRIDE:
		buf[k + j] = row[j]


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
