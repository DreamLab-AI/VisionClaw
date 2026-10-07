extends Node3D

## memory_flash bursts (WP3) — the headset counterpart of the desktop
## EmbeddingCloudLayer burst rings, in ONE pooled MultiMesh under a unit-scale
## effects root (world metres, never under GraphRoot; XR-client Invariant 8).
##
## Each RuVector access arrives as a `memory_flash` text frame; MemoryFlashCodec
## (Rust, semantic.rs) turns it into a descriptor with the desktop's semantic
## colour, scale, lifetime, motion and ring count. A burst uses `rings` slots,
## staggered by RING_STAGGER like the desktop. The pool never exceeds POOL_SIZE
## (64, the desktop's BURST_POOL_SIZE): when full, the oldest slot is recycled.
##
## Flat additive annulus with vertex-colour alpha — emission only, no
## post-process (Invariant 2 / ADR-2107), one draw call. Reduced motion (the
## comfort default) disables ring expansion: the ring holds a fixed size and
## only fades. `beat_pulse` (0..1) from beat_pulse.gd swells opacity on the beat.

const POOL_SIZE := 64
const RING_STAGGER := 0.14          # s between concentric rings (desktop RING_STAGGER)
const RING_INNER := 0.8             # desktop RingGeometry(0.8, 1.0)
const RING_OUTER := 1.0
const RING_SEGMENTS := 32
const PEAK_ALPHA := 0.85            # desktop opacity = (1 - t²) · 0.85
const REDUCED_SCALE := 0.6          # fraction of max_scale held under reduced motion
const BEAT_ALPHA_GAIN := 0.35
## Metres per desktop cloud unit. The desktop cloud spans ~±100 units; xr-cloud
## may set this to its own cloud scale so bursts sit in proportion to its points.
var unit_scale: float = 0.02

var reduced_motion: bool = true
var beat_pulse: float = 0.0

var _mmi: MultiMeshInstance3D = null
var _slots: Array = []   # [{pos, color, max, dur, implode, t, delay}] oldest first
var _head: Vector3 = Vector3.ZERO
var _spawned_total: int = 0


func _ready() -> void:
	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.vertex_color_use_as_albedo = true
	mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	mat.blend_mode = BaseMaterial3D.BLEND_MODE_ADD
	mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	mat.depth_draw_mode = BaseMaterial3D.DEPTH_DRAW_DISABLED
	var mesh := _annulus_mesh()
	mesh.surface_set_material(0, mat)
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.mesh = mesh
	mm.instance_count = 0
	_mmi = MultiMeshInstance3D.new()
	_mmi.name = "BurstMulti"
	_mmi.multimesh = mm
	_mmi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(_mmi)


# Flat ring in the XZ plane (normal +Y); _facing_transform turns +Y to the head.
func _annulus_mesh() -> ArrayMesh:
	var verts := PackedVector3Array()
	var idx := PackedInt32Array()
	for i: int in range(RING_SEGMENTS):
		var a: float = TAU * float(i) / float(RING_SEGMENTS)
		var d := Vector3(cos(a), 0.0, sin(a))
		verts.append(d * RING_INNER)
		verts.append(d * RING_OUTER)
	for i: int in range(RING_SEGMENTS):
		var j: int = (i + 1) % RING_SEGMENTS
		var a0: int = i * 2
		var b0: int = i * 2 + 1
		var a1: int = j * 2
		var b1: int = j * 2 + 1
		idx.append_array(PackedInt32Array([a0, b0, a1, a1, b0, b1]))
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = verts
	arrays[Mesh.ARRAY_INDEX] = idx
	var m := ArrayMesh.new()
	m.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return m


func set_head(p: Vector3) -> void:
	_head = p


## Spawn one burst at `pos` (world metres) from a MemoryFlashCodec descriptor.
func spawn(pos: Vector3, desc: Dictionary) -> void:
	var rings: int = clampi(int(desc.get("rings", 1)), 1, 3)
	var max_m: float = float(desc.get("max_scale", 3.2)) * unit_scale
	var dur: float = maxf(float(desc.get("duration", 1.6)), 0.05)
	var col: Color = desc.get("color", Color(0.6, 0.84, 1.0))
	var implode: bool = bool(desc.get("implode", false))
	for r: int in range(rings):
		if _slots.size() >= POOL_SIZE:
			_slots.pop_front()   # recycle the oldest slot
		_slots.append({
			"pos": pos, "color": col, "max": max_m, "dur": dur,
			"implode": implode, "t": 0.0, "delay": float(r) * RING_STAGGER,
		})
	_spawned_total += 1


## Live slots (≤ POOL_SIZE) — for tests and the HUD.
func slot_count() -> int:
	return _slots.size()


func spawned_total() -> int:
	return _spawned_total


func clear_all() -> void:
	_slots.clear()


## Desktop scale curve (EmbeddingCloudLayer): cubic ease-out expand, cubic implode.
static func burst_scale(t: float, max_scale: float, implode: bool) -> float:
	var u: float = clampf(t, 0.0, 1.0)
	var s: float = max_scale * pow(1.0 - u, 3.0) if implode else max_scale * (1.0 - pow(1.0 - u, 3.0))
	return maxf(s, 0.0005)


## World-space radius of one live slot now: the desktop curve, or a fixed
## REDUCED_SCALE of the peak under reduced motion (no ring expansion).
static func slot_scale(slot: Dictionary, reduced: bool) -> float:
	var mx: float = float(slot["max"])
	if reduced:
		return mx * REDUCED_SCALE
	var u: float = (float(slot["t"]) - float(slot["delay"])) / float(slot["dur"])
	return burst_scale(u, mx, bool(slot["implode"]))


## Snapshot of each drawn slot's radius (the headless renderer keeps no
## MultiMesh data to read back, so tests read this instead).
func drawn_scales() -> PackedFloat32Array:
	var out := PackedFloat32Array()
	for s: Dictionary in _slots:
		if float(s["t"]) >= float(s["delay"]):
			out.append(slot_scale(s, reduced_motion))
	return out


static func burst_alpha(t: float) -> float:
	var u: float = clampf(t, 0.0, 1.0)
	return (1.0 - u * u) * PEAK_ALPHA


## A stable stand-in position for a flash with no cloud point to land on: a
## point on a shell of `radius` around `centre`, hashed from key + namespace so
## the same memory always bursts in the same place.
static func ambient_position(key: String, ns: String, centre: Vector3, radius: float = 0.45) -> Vector3:
	var h: int = hash(ns + "\u001f" + key)
	var u: float = float(h & 0xFFFF) / 65535.0
	var v: float = float((h >> 16) & 0xFFFF) / 65535.0
	var theta: float = TAU * u
	var z: float = 2.0 * v - 1.0
	var r: float = sqrt(maxf(0.0, 1.0 - z * z))
	return centre + Vector3(r * cos(theta), z, r * sin(theta)) * radius


func _process(delta: float) -> void:
	if _slots.is_empty():
		if _mmi != null and _mmi.multimesh.instance_count != 0:
			_mmi.multimesh.instance_count = 0
		return
	var alive: Array = []
	for s: Dictionary in _slots:
		s["t"] = float(s["t"]) + delta
		if float(s["t"]) - float(s["delay"]) < float(s["dur"]):
			alive.append(s)
	_slots = alive
	_rebuild()


func _rebuild() -> void:
	if _mmi == null:
		return
	var mm: MultiMesh = _mmi.multimesh
	var visible: Array = []
	for s: Dictionary in _slots:
		if float(s["t"]) >= float(s["delay"]):
			visible.append(s)
	if mm.instance_count != visible.size():
		mm.instance_count = visible.size()
	var swell: float = 1.0 + BEAT_ALPHA_GAIN * clampf(beat_pulse, 0.0, 1.0)
	var i: int = 0
	for s: Dictionary in visible:
		var u: float = (float(s["t"]) - float(s["delay"])) / float(s["dur"])
		var sc: float = slot_scale(s, reduced_motion)
		var c: Color = s["color"]
		c.a = clampf(burst_alpha(u) * swell, 0.0, 1.0)
		mm.set_instance_transform(i, _facing_transform(s["pos"], sc))
		mm.set_instance_color(i, c)
		i += 1


func _facing_transform(pos: Vector3, scale: float) -> Transform3D:
	var to_head: Vector3 = _head - pos
	var basis := Basis.IDENTITY
	if to_head.length_squared() > 0.0001:
		var y: Vector3 = to_head.normalized()
		var up := Vector3.UP if absf(y.dot(Vector3.UP)) < 0.99 else Vector3.RIGHT
		var x: Vector3 = up.cross(y).normalized()
		var z: Vector3 = x.cross(y).normalized()
		basis = Basis(x, y, z)
	return Transform3D(basis.scaled(Vector3(scale, scale, scale)), pos)
