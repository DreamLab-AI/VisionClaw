extends "res://scripts/spatial_volume.gd"
## Used via `const EmulatedVolume := preload("res://scripts/emulated_volume.gd")`
## (or `scenes/EmulatedVolume.tscn`) rather than a class_name, so a headless
## run with a stale global class cache still resolves it.

## The emulated spatial volume (ADR-2158 D2): every graph volume, on every
## runtime. It adds to the contract:
##   * Frame: a 12-edge wire box, unshaded, one draw call, scaled to `bounds`;
##     hidden when the volume is hidden or immersive.
##   * Handles: 8 corner grab points (Area3D + CollisionShape3D on
##     HANDLE_LAYER, never monitoring) plus one MultiMesh of markers, drawn
##     only while the volume is focused, interactable and bounded (one draw
##     call). The scene picks a handle with `handle_at`, by distance.
##   * The emulated grant policy: each requested axis is clamped to
##     [MIN_BOUNDS, MAX_BOUNDS] metres and granted.

const MIN_BOUNDS := 0.2
const MAX_BOUNDS := 4.0
const HANDLE_RADIUS := 0.04
## Physics layer 20, so a caller that does raycast against the handles can
## mask them in without picking anything else up.
const HANDLE_LAYER := 1 << 19
const FRAME_COLOUR := Color(0.55, 0.8, 1.0)
const FRAME_FOCUS_COLOUR := Color(1.0, 0.85, 0.35)
const HANDLE_COLOUR := Color(1.0, 0.85, 0.35)

static var _frame_mesh: ArrayMesh
static var _frame_material: StandardMaterial3D
static var _focus_material: StandardMaterial3D
static var _handle_shape: SphereShape3D
static var _handle_mesh: SphereMesh

var frame: MeshInstance3D
var handles: Node3D
var _handle_areas: Array[Area3D] = []
var _handle_marks: MultiMeshInstance3D


## Clamps each axis to [MIN_BOUNDS, MAX_BOUNDS].
func _policy_bounds(b: Vector3) -> Vector3:
	return b.clamp(Vector3.ONE * MIN_BOUNDS, Vector3.ONE * MAX_BOUNDS)


func _on_state_changed() -> void:
	if frame == null:
		return
	frame.scale = bounds
	var bounded := bounds_mode == BoundsMode.BOUNDED
	frame.visible = visible_state and bounded
	frame.material_override = _focus_material if focused else _frame_material
	var live := visible_state and interactable and bounded and not is_closed
	for i: int in _handle_areas.size():
		var area: Area3D = _handle_areas[i]
		var corner := corner_sign(i) * bounds * 0.5
		area.position = corner
		area.input_ray_pickable = live
		area.collision_layer = HANDLE_LAYER if live else 0
		_handle_marks.multimesh.set_instance_transform(i, Transform3D(Basis.IDENTITY, corner))
	_handle_marks.visible = live and focused


func _build_extra() -> void:
	_ensure_shared()
	frame = get_node_or_null("Frame") as MeshInstance3D
	if frame == null:
		frame = MeshInstance3D.new()
		frame.name = "Frame"
		add_child(frame)
	frame.mesh = _frame_mesh
	frame.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	handles = get_node_or_null("Handles") as Node3D
	if handles == null:
		handles = Node3D.new()
		handles.name = "Handles"
		add_child(handles)
	_handle_areas.clear()
	for i: int in 8:
		var area := handles.get_node_or_null("Handle%d" % i) as Area3D
		if area == null:
			area = Area3D.new()
			area.name = "Handle%d" % i
			var shape := CollisionShape3D.new()
			shape.shape = _handle_shape
			area.add_child(shape)
			handles.add_child(area)
		area.monitoring = false
		area.monitorable = false
		area.collision_mask = 0
		area.set_meta(&"handle_index", i)
		_handle_areas.append(area)
	_handle_marks = handles.get_node_or_null("Marks") as MultiMeshInstance3D
	if _handle_marks == null:
		_handle_marks = MultiMeshInstance3D.new()
		_handle_marks.name = "Marks"
		handles.add_child(_handle_marks)
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = _handle_mesh
	mm.instance_count = 8
	_handle_marks.multimesh = mm
	_handle_marks.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_on_state_changed()


## Draw calls this volume adds outside its content: the frame, plus the
## handle markers while they show.
func overhead_draw_calls() -> int:
	if frame == null:
		return 0
	return (1 if frame.visible and visible else 0) + (1 if _handle_marks.visible and visible else 0)


static func _ensure_shared() -> void:
	if _frame_mesh != null:
		return
	var v := PackedVector3Array()
	var c: Array[Vector3] = []
	for i: int in 8:
		c.append(corner_sign(i) * 0.5)
	# The 12 edges join corners whose indices differ in exactly one bit.
	for i: int in 8:
		for bit: int in [1, 2, 4]:
			if (i & bit) == 0:
				v.append(c[i])
				v.append(c[i | bit])
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = v
	_frame_mesh = ArrayMesh.new()
	_frame_mesh.add_surface_from_arrays(Mesh.PRIMITIVE_LINES, arrays)
	_frame_material = _unshaded(FRAME_COLOUR)
	_focus_material = _unshaded(FRAME_FOCUS_COLOUR)
	_handle_shape = SphereShape3D.new()
	_handle_shape.radius = HANDLE_RADIUS
	_handle_mesh = SphereMesh.new()
	_handle_mesh.radius = HANDLE_RADIUS * 0.6
	_handle_mesh.height = HANDLE_RADIUS * 1.2
	_handle_mesh.radial_segments = 8
	_handle_mesh.rings = 4
	_handle_mesh.material = _unshaded(HANDLE_COLOUR)


static func _unshaded(col: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.albedo_color = col
	return m
