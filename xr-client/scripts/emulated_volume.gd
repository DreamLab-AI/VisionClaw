extends "res://scripts/spatial_volume.gd"
## Used via `const EmulatedVolume := preload("res://scripts/emulated_volume.gd")`
## (or `scenes/EmulatedVolume.tscn`) rather than a class_name, so a headless
## run with a stale global class cache still resolves it.

## The emulated spatial volume (ADR-2158 D2): every graph volume, on every
## runtime. It adds to the contract:
##   * Frame: a 12-edge wire box, unshaded, one draw call, scaled to `bounds`;
##     hidden when the volume is hidden or immersive. With `ribbon_frames` on
##     (EyeCandy tier 2) the same one call draws 12 camera-facing edge ribbons
##     (24 triangles) instead of 1 px lines; low cost and tier 0 keep the lines.
##     `set_frame_hidden(true)` hides it whatever else holds (a portal entered).
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

## Frame style, for every volume: ribbons when true. Set with
## `set_ribbon_frames`, which also restyles the volumes that already exist.
static var ribbon_frames: bool = false
static var _frame_mesh: ArrayMesh
static var _frame_material: StandardMaterial3D
static var _focus_material: StandardMaterial3D
static var _ribbon_mesh: ArrayMesh
static var _ribbon_material: ShaderMaterial
static var _ribbon_focus_material: ShaderMaterial
static var _handle_shape: SphereShape3D
static var _handle_mesh: SphereMesh

var frame: MeshInstance3D
var handles: Node3D
var _handle_areas: Array[Area3D] = []
var _handle_marks: MultiMeshInstance3D
var _frame_hidden := false


## Clamps each axis to [MIN_BOUNDS, MAX_BOUNDS].
func _policy_bounds(b: Vector3) -> Vector3:
	return b.clamp(Vector3.ONE * MIN_BOUNDS, Vector3.ONE * MAX_BOUNDS)


func _on_state_changed() -> void:
	if frame == null:
		return
	frame.scale = bounds
	var bounded := bounds_mode == BoundsMode.BOUNDED
	frame.visible = visible_state and bounded and not portal_room and not _frame_hidden
	frame.material_override = _frame_material_for(focused)
	var live := visible_state and interactable and bounded and not is_closed and not portal_room
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
	frame.mesh = _ribbon_mesh if ribbon_frames else _frame_mesh
	frame.extra_cull_margin = 0.05 # the ribbons overhang the unit box
	frame.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_to_group(&"xr_volume_frames")
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


## Hide or show the frame whatever the volume's state (a portal volume entered
## hides its own frame). The frame shows again when it is cleared and the volume
## is still visible and bounded.
func set_frame_hidden(hidden: bool) -> void:
	if _frame_hidden == hidden:
		return
	_frame_hidden = hidden
	_on_state_changed()


func is_frame_hidden() -> bool:
	return _frame_hidden


## Triangles the frame draws now: 24 as ribbons, 0 as lines.
func frame_triangles() -> int:
	return 24 if ribbon_frames else 0


## Switch every volume, existing and future, between line and ribbon frames.
## `tree` is any SceneTree; the volumes join the "xr_volume_frames" group.
static func set_ribbon_frames(tree: SceneTree, on: bool) -> void:
	ribbon_frames = on
	if tree != null:
		tree.call_group(&"xr_volume_frames", &"restyle_frame")


## Re-read `ribbon_frames`: mesh and material for the current focus state.
func restyle_frame() -> void:
	if frame == null:
		return
	_ensure_shared()
	frame.mesh = _ribbon_mesh if ribbon_frames else _frame_mesh
	frame.material_override = _frame_material_for(focused)


func _frame_material_for(is_focused: bool) -> Material:
	if ribbon_frames:
		return _ribbon_focus_material if is_focused else _ribbon_material
	return _focus_material if is_focused else _frame_material


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
	_ribbon_mesh = _build_ribbon_mesh()
	_ribbon_material = _ribbon(FRAME_COLOUR)
	_ribbon_focus_material = _ribbon(FRAME_FOCUS_COLOUR)
	_handle_shape = SphereShape3D.new()
	_handle_shape.radius = HANDLE_RADIUS
	_handle_mesh = SphereMesh.new()
	_handle_mesh.radius = HANDLE_RADIUS * 0.6
	_handle_mesh.height = HANDLE_RADIUS * 1.2
	_handle_mesh.radial_segments = 8
	_handle_mesh.rings = 4
	_handle_mesh.material = _unshaded(HANDLE_COLOUR)


## The 12 edges as quads: four vertices an edge (both ends, both sides), 24
## triangles, one surface. Layout is documented in frame_ribbon.gdshader.
static func _build_ribbon_mesh() -> ArrayMesh:
	var v := PackedVector3Array()
	var col := PackedColorArray()
	var uv := PackedVector2Array()
	var idx := PackedInt32Array()
	var c: Array[Vector3] = []
	for i: int in 8:
		c.append(corner_sign(i) * 0.5)
	for i: int in 8:
		for bit: int in [1, 2, 4]:
			if (i & bit) != 0:
				continue
			var axis: float = 0.0 if bit == 1 else (0.5 if bit == 2 else 1.0)
			var base := v.size()
			for end: Vector3 in [c[i], c[i | bit]]:
				for side: float in [0.0, 1.0]:
					v.append(end)
					col.append(Color(axis, 0.0, 0.0, 1.0))
					uv.append(Vector2(side, 0.0))
			# vertices: 0 = start/left, 1 = start/right, 2 = end/left, 3 = end/right
			idx.append_array(PackedInt32Array([base, base + 1, base + 2, base + 1, base + 3, base + 2]))
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = v
	arrays[Mesh.ARRAY_COLOR] = col
	arrays[Mesh.ARRAY_TEX_UV] = uv
	arrays[Mesh.ARRAY_INDEX] = idx
	var m := ArrayMesh.new()
	m.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return m


static func _ribbon(col: Color) -> ShaderMaterial:
	var m := ShaderMaterial.new()
	m.shader = preload("res://materials/frame_ribbon.gdshader")
	m.set_shader_parameter("frame_color", Color(col.r, col.g, col.b, 0.9))
	return m


static func _unshaded(col: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.albedo_color = col
	return m
