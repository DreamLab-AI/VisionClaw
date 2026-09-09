extends Node3D

## Work-cue effects for embodied agents, in ONE pooled MultiMesh under the
## unit-scale AgentEffectsRoot (world metres, never under GraphRoot):
##   * a pulsing ring on every node an agent is currently working on
##   * an expanding, fading ring burst when an agent completes a task
## Solid unshaded geometry with vertex-colour alpha — reads without bloom and
## costs one draw call. Reduced motion freezes the pulse and shortens bursts.

const RING_INNER := 0.052
const RING_OUTER := 0.066
const PULSE_PERIOD := 1.75
const PULSE_AMP := 0.10
const BURST_SEC := 1.2
const BURST_SCALE := 4.0
const COLOR_WORK := Color(0.30, 0.90, 0.72, 0.85)
const COLOR_DONE := Color(0.80, 0.95, 1.00, 0.9)
# Hand-off packets: small solid beads that travel the real edge between the
# node an agent leaves and the node it moves to (the "work carried along an
# edge" cue revived from the 2025 desktop code).
const BEAD_RADIUS := 0.0075
const BEAD_SPEED := 0.45         # m/s
const BEAD_SPACING := 0.12       # m between beads of one packet
const BEAD_COUNT := 3
const COLOR_PACKET := Color(0.95, 0.95, 1.0, 1.0)

var reduced_motion: bool = false

var _mmi: MultiMeshInstance3D = null
var _beads_mmi: MultiMeshInstance3D = null
var _rings: Dictionary = {}   # key -> {pos: Vector3, color: Color}
var _bursts: Array = []       # [{pos, t, color}]
var _beads: Array = []        # [{from, to, t, dur, delay, color}]
var _time: float = 0.0
var _head: Vector3 = Vector3.ZERO


func _ready() -> void:
	var mesh := TorusMesh.new()
	mesh.inner_radius = RING_INNER
	mesh.outer_radius = RING_OUTER
	mesh.rings = 24
	mesh.ring_segments = 8
	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.vertex_color_use_as_albedo = true
	mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	mesh.material = mat
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.mesh = mesh
	_mmi = MultiMeshInstance3D.new()
	_mmi.name = "RingMulti"
	_mmi.multimesh = mm
	_mmi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(_mmi)

	var bead := SphereMesh.new()
	bead.radius = BEAD_RADIUS
	bead.height = BEAD_RADIUS * 2.0
	bead.radial_segments = 8
	bead.rings = 4
	var bead_mat := StandardMaterial3D.new()
	bead_mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	bead_mat.vertex_color_use_as_albedo = true
	bead.material = bead_mat
	var bmm := MultiMesh.new()
	bmm.transform_format = MultiMesh.TRANSFORM_3D
	bmm.use_colors = true
	bmm.mesh = bead
	_beads_mmi = MultiMeshInstance3D.new()
	_beads_mmi.name = "BeadMulti"
	_beads_mmi.multimesh = bmm
	_beads_mmi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(_beads_mmi)


func set_head(p: Vector3) -> void:
	_head = p


## Send a packet of beads from `from` to `to` (world metres) along a real edge.
## No-op under reduced motion (a travelling object is exactly what it forbids).
func send_packets(from: Vector3, to: Vector3, color: Color = COLOR_PACKET, count: int = BEAD_COUNT) -> void:
	if reduced_motion:
		return
	var dist: float = from.distance_to(to)
	if dist < 0.02:
		return
	var dur: float = dist / BEAD_SPEED
	for i: int in range(maxi(count, 1)):
		_beads.append({
			"from": from, "to": to, "t": 0.0, "dur": dur,
			"delay": float(i) * BEAD_SPACING / BEAD_SPEED, "color": color,
		})


func bead_count() -> int:
	return _beads.size()


func set_ring(key: String, pos: Vector3, color: Color = COLOR_WORK) -> void:
	_rings[key] = {"pos": pos, "color": color}


func clear_ring(key: String) -> void:
	_rings.erase(key)


func clear_all() -> void:
	_rings.clear()
	_bursts.clear()
	_beads.clear()


func burst(pos: Vector3, color: Color = COLOR_DONE) -> void:
	_bursts.append({"pos": pos, "t": 0.0, "color": color})


func ring_count() -> int:
	return _rings.size()


func burst_count() -> int:
	return _bursts.size()


func _process(delta: float) -> void:
	_time += delta
	var alive: Array = []
	for b: Dictionary in _bursts:
		b["t"] = float(b["t"]) + delta
		if float(b["t"]) < BURST_SEC:
			alive.append(b)
	_bursts = alive
	var live_beads: Array = []
	for b: Dictionary in _beads:
		b["t"] = float(b["t"]) + delta
		if float(b["t"]) < float(b["delay"]) + float(b["dur"]):
			live_beads.append(b)
	_beads = live_beads
	_rebuild()
	_rebuild_beads()


func _rebuild() -> void:
	if _mmi == null or _mmi.multimesh == null:
		return
	var mm: MultiMesh = _mmi.multimesh
	var count: int = _rings.size() + _bursts.size()
	if mm.instance_count != count:
		mm.instance_count = count
	if count == 0:
		return
	var i: int = 0
	var pulse: float = 1.0 if reduced_motion else 1.0 + PULSE_AMP * sin(TAU * _time / PULSE_PERIOD)
	for key: String in _rings:
		var r: Dictionary = _rings[key]
		mm.set_instance_transform(i, _facing_transform(r["pos"], pulse))
		mm.set_instance_color(i, r["color"])
		i += 1
	for b: Dictionary in _bursts:
		var u: float = clampf(float(b["t"]) / BURST_SEC, 0.0, 1.0)
		var s: float = 1.0 + (BURST_SCALE - 1.0) * (u if not reduced_motion else 0.0)
		var c: Color = b["color"]
		c.a *= 1.0 - u
		mm.set_instance_transform(i, _facing_transform(b["pos"], s))
		mm.set_instance_color(i, c)
		i += 1


func _rebuild_beads() -> void:
	if _beads_mmi == null or _beads_mmi.multimesh == null:
		return
	var mm: MultiMesh = _beads_mmi.multimesh
	# Beads still waiting on their spacing delay are not drawn.
	var visible_beads: Array = []
	for b: Dictionary in _beads:
		if float(b["t"]) >= float(b["delay"]):
			visible_beads.append(b)
	if mm.instance_count != visible_beads.size():
		mm.instance_count = visible_beads.size()
	var i: int = 0
	for b: Dictionary in visible_beads:
		var u: float = clampf((float(b["t"]) - float(b["delay"])) / maxf(float(b["dur"]), 0.001), 0.0, 1.0)
		var p: Vector3 = (b["from"] as Vector3).lerp(b["to"], u)
		mm.set_instance_transform(i, Transform3D(Basis.IDENTITY, p))
		mm.set_instance_color(i, b["color"])
		i += 1


# Ring lies in the plane facing the head so it reads as a halo, not a disc edge.
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
