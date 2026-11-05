extends Node3D
## Used via `const SpacePortals := preload("res://scripts/space_portals.gd")`
## rather than a class_name, so a headless run with a stale global class cache
## still resolves it.

## Entering a spatial volume; the others become portals (ADR-2166, amends
## ADR-2158).
##
## The ring stays the default layout. This node adds one opt-in state on top of
## `VolumeArrangement`: one volume is *entered*, it stays BOUNDED but gets
## room-scale bounds (ROOM_BOUNDS, set through `set_runtime_bounds` so the
## emulated 4 m clamp is bypassed) and is centred on the user's floor position.
## Every other visible, open volume moves onto a portal ring outside that box,
## facing the user at eye height. A portal is the volume's real box and real
## content; the only addition is one membrane quad on its user-facing face,
## drawn for all portals by a single MultiMesh (one draw call). No second
## camera, render-to-texture, stencil or screen/depth sampling.
##
## The merged splat space (ADR-2178) is a second kind of entered state. The
## room is a staged splat volume at viewer scale (its bounds are the scene's
## extent times metres-per-unit, so the fit scale is exactly that), placed so
## the scene's start lands on the user's floor point and its focus lies ahead
## along the heading. One graph volume, the guest, is placed directly inside
## the room on the sight line from the user to the focus: no membrane, still
## pickable and draggable. Every other volume is hidden and shown again on
## leave. There are no portals.
##
## `IMMERSIVE` (1:1 graph units, and the native shim's mode mapping) is not
## used. Leaving restores the ring exactly from the snapshot taken at the first
## enter: transforms, bounds, mode, slots, moved flags and the arrangement's own
## transform.
##
## Transitions. Reduced motion on (the comfort default) is a dim, an instant
## snap while the view is dark, and the dim back: nothing around the user moves
## or scales while visible. That covers a portal appearing, disappearing or
## resizing while entered, and the room going away. Reduced motion off eases
## the boxes over EASE_SEC through `VolumeArrangement.ease_volume_to` when
## entering from the ring and when the portal ring is re-laid. Leaving and
## swapping always take the dim path, in either setting: both shrink a room's
## content, and E7 forbids an eased shrink. An arrangement that carries a
## two-hand transform cannot ease without jumping, so it takes the dim path too.

const VolumeArrangement := preload("res://scripts/volume_arrangement.gd")
const SpatialVolume := preload("res://scripts/spatial_volume.gd")

## Room-scale bounds of the entered volume, metres (full extents).
const ROOM_BOUNDS := Vector3(6.0, 3.0, 6.0)
## Clear gap between the room box and a portal's bounding circle, metres.
const PORTAL_MARGIN := 0.3
## Angle between neighbouring portals on the ring, degrees.
const PORTAL_STEP_DEG := 50.0
## Portal centre height when the caller gives no eye height, metres.
const DEFAULT_EYE_HEIGHT := VolumeArrangement.RING_HEIGHT
## Eye height above the floor is clamped to this range, metres.
const EYE_MIN := 0.8
const EYE_MAX := 2.2
## Walk-through: head inside a portal box this long enters it.
const DWELL_SEC := 0.25
## Walk-through re-arms after the head has been this far outside every portal.
const REARM_DISTANCE := 0.3
## Reduced-motion dim: seconds to go dark, and again to come back.
const FADE_SEC := 0.12
## Eased transition length, seconds (the brief's ceiling).
const EASE_SEC := 0.5
## How far in front of the camera the dim quad sits, metres.
const FADE_DISTANCE := 0.2
## Gap between the box face and its membrane quad, metres.
const MEMBRANE_LIFT := 0.003
## Most portals the ring holds (the MultiMesh capacity). `portal_layout` spreads
## n portals min(PORTAL_STEP_DEG, 360/n) degrees apart, which keeps every pair of
## portal boxes apart for up to MAX_PORTALS portals of any size up to the 4 m
## emulated clamp (tested). Past MAX_PORTALS the spacing is not guaranteed.
const MAX_PORTALS := 8
## Share of the near-cap split the non-entered volumes keep (stream XE hook).
const OTHER_WEIGHT := 0.15
const MEMBRANE_SHADER := "res://materials/portal_membrane.gdshader"
const MEMBRANE_PRIORITY := 10
const UNFOCUSED := 0.35
const DEFAULT_TINT := Color(0.8, 0.85, 1.0)

# Merged splat space (ADR-2178). The guest is a cube sized from the start-to-focus
# distance and kept within reach of the wand ray (graph_scene RAY_LENGTH, 5 m) from
# the start: its centre is at most GUEST_FAR from the user and its near face at
# least GUEST_NEAR_GAP away. It hangs on the sight line to the focus, between
# GUEST_FLOOR_GAP above the floor and GUEST_MAX_LIFT above the eye.
const GUEST_FRACTION := 0.3
const GUEST_EDGE_FRACTION := 0.25
const GUEST_EDGE_MIN := 1.2
const GUEST_EDGE_MAX := 3.0
const GUEST_NEAR_GAP := 1.0
const GUEST_FAR := 4.0
const GUEST_FLOOR_GAP := 0.3
const GUEST_MAX_LIFT := 1.5
## The viewer-scale multiplier steps the HUD offers (1.0 is the measured stage).
const MERGED_SCALES: Array[float] = [0.25, 0.5, 0.71, 1.0, 1.41, 2.0]
## Knowledge, ontology and Codescape portal tints.
const TINTS := {
	&"knowledge": Color(0.35, 0.75, 1.0),
	&"ontology": Color(0.75, 0.5, 1.0),
	&"codescape": Color(0.4, 1.0, 0.6),
}

enum Phase { IDLE, DIM_OUT, DIM_IN }

## Emitted after every enter, swap and leave; `entered_id` is &"" after a leave.
signal entered_changed(entered_id: StringName)

var arrangement: Node3D
## `() -> bool`: true while the native container arm owns the arrangement.
var native_check: Callable
## `(volume_id) -> bool`: true while that volume's connection is down.
var offline_check: Callable
## Mirrors the comfort node's `visual_comfort_changed(reduced_motion, …)`.
var reduced_motion := true:
	set(value):
		reduced_motion = value
		_set_motion_uniform()
## Walk-through (`xr.portal_walkthrough`); off by default.
var walkthrough_enabled := false
var membranes: MultiMeshInstance3D
var fade_quad: MeshInstance3D

var _entered: StringName = &""
var _guest: StringName = &""          # ADR-2178: the merged space's guest volume
var _stage: Dictionary = {}           # {metres_per_unit, focus, content} of the merged room
var _scale_mult := 1.0
var _hidden: Array[StringName] = []   # volumes the merged space hid, shown again on leave
var _snap: Dictionary = {}
var _user := Vector3.ZERO          # floor point, arrangement parent frame
var _heading := Vector3(0.0, 0.0, -1.0)
var _eye_y := DEFAULT_EYE_HEIGHT
var _phase := Phase.IDLE
var _phase_t := 0.0
var _pending: Callable = Callable()
var _signature: Array = []
var _armed := true
var _dwell := 0.0
var _dwell_id: StringName = &""
var _inst_xf: Array[Transform3D] = []   # what _update_membranes wrote, readable in headless
var _inst_col := PackedColorArray()
var _fade_material: StandardMaterial3D
var _membrane_material: ShaderMaterial


# --- pure maths ---------------------------------------------------------------

## XZ half-diagonal of a box, metres: the radius of its bounding circle.
static func half_diagonal_xz(b: Vector3) -> float:
	return 0.5 * Vector2(b.x, b.z).length()


## Radius of the portal ring about the user's floor point for a room of `room`
## and portals of `portal_bounds` (Array of Vector3): the largest of
## room half-diagonal + the portal's own half-diagonal + PORTAL_MARGIN.
static func ring_radius(room: Vector3, portal_bounds: Array) -> float:
	var r := 0.0
	for b: Vector3 in portal_bounds:
		r = maxf(r, half_diagonal_xz(room) + half_diagonal_xz(b) + PORTAL_MARGIN)
	return r


## Portal transforms, in the order of `portal_bounds`, in the frame `user` is
## given in. Portals sit on a ring about `user` (XZ), spread PORTAL_STEP_DEG
## apart (closer for 8 or more, so the ring never wraps onto itself) and centred on `heading`, at height `eye_y`, each turned so its +Z
## face looks back at the user.
static func portal_layout(user: Vector3, heading: Vector3, room: Vector3,
		portal_bounds: Array, eye_y: float) -> Array[Transform3D]:
	var out: Array[Transform3D] = []
	var fwd := Vector3(heading.x, 0.0, heading.z)
	fwd = Vector3(0.0, 0.0, -1.0) if fwd.length() < 1.0e-6 else fwd.normalized()
	var r := ring_radius(room, portal_bounds)
	var n := portal_bounds.size()
	# 50 degrees apart wraps past a full turn from 8 portals on: shrink the step.
	var step := minf(PORTAL_STEP_DEG, 360.0 / float(maxi(n, 1)))
	for i: int in n:
		var a := deg_to_rad((float(i) - 0.5 * float(n - 1)) * step)
		var dir := Basis(Vector3.UP, a) * fwd
		var pos := Vector3(user.x, eye_y, user.z) + dir * r
		out.append(Transform3D(Basis.looking_at(dir, Vector3.UP), pos))
	return out


## The entered room's transform: centred on the user's floor point, box bottom
## at `user.y`, turned to `heading`.
static func room_transform(user: Vector3, heading: Vector3, room: Vector3) -> Transform3D:
	var fwd := Vector3(heading.x, 0.0, heading.z)
	fwd = Vector3(0.0, 0.0, -1.0) if fwd.length() < 1.0e-6 else fwd.normalized()
	return Transform3D(Basis.looking_at(fwd, Vector3.UP), Vector3(user.x, user.y + room.y * 0.5, user.z))


## ADR-2178: the yaw basis that turns the stage's −Z onto `heading` (XZ only).
static func heading_basis(heading: Vector3) -> Basis:
	var fwd := Vector3(heading.x, 0.0, heading.z)
	fwd = Vector3(0.0, 0.0, -1.0) if fwd.length() < 1.0e-6 else fwd.normalized()
	return Basis.looking_at(fwd, Vector3.UP)


## The merged room's bounds: the staged scene's extent at viewer scale `s`
## (metres per scene unit times the multiplier). The fit then scales by exactly `s`.
static func merged_room_bounds(content: AABB, s: float) -> Vector3:
	return content.size * s


## The merged room's transform. The fit centres the content at the volume origin, so
## the origin goes where the content centre must be for the stage start (the content
## origin) to land on `user`, the floor point, with the stage's −Z along `heading`.
static func merged_room_transform(user: Vector3, heading: Vector3, content: AABB, s: float) -> Transform3D:
	var r := heading_basis(heading)
	return Transform3D(r, user + r * (content.get_center() * s))


## A stage point (scene units) in the room's parent frame, for the same placement.
static func stage_point(user: Vector3, heading: Vector3, s: float, p: Vector3) -> Vector3:
	return user + heading_basis(heading) * (p * s)


## {xf, bounds} for the guest: a cube on the sight line from the eye (`eye_y` above
## `user`'s floor point) to `focus`, in plan between the user and the focus, within
## reach of the wand ray, turned so its +Z face looks back at the user.
static func guest_placement(user: Vector3, eye_y: float, focus: Vector3) -> Dictionary:
	var eye := Vector3(user.x, eye_y, user.z)
	var flat := Vector3(focus.x - user.x, 0.0, focus.z - user.z)
	var dist := flat.length()
	var dir := Vector3(0.0, 0.0, -1.0) if dist < 1.0e-6 else flat / dist
	var edge := clampf(dist * GUEST_EDGE_FRACTION, GUEST_EDGE_MIN, GUEST_EDGE_MAX)
	var near := edge * 0.5 + GUEST_NEAR_GAP
	var d := clampf(dist * GUEST_FRACTION, near, maxf(near, GUEST_FAR))
	var slope := 0.0 if dist < 1.0e-6 else (focus.y - eye.y) / dist
	var lo := user.y + edge * 0.5 + GUEST_FLOOR_GAP
	var hi := maxf(lo, eye.y + GUEST_MAX_LIFT)
	var y := clampf(eye.y + slope * d, lo, hi)
	var pos := Vector3(user.x, 0.0, user.z) + dir * d
	pos.y = y
	return {"xf": Transform3D(Basis.looking_at(dir, Vector3.UP), pos), "bounds": Vector3.ONE * edge}


## The start after re-scaling by `ratio` about the floor point `about`: what was under
## `about` stays under it.
static func rescale_start(start: Vector3, about: Vector3, ratio: float) -> Vector3:
	var d := start - about
	return Vector3(about.x + d.x * ratio, start.y, about.z + d.z * ratio)


## The next multiplier step from `current` (`dir` −1 or +1), held at the ends.
static func step_scale(current: float, dir: int) -> float:
	var i := 0
	for k: int in MERGED_SCALES.size():
		if absf(MERGED_SCALES[k] - current) < absf(MERGED_SCALES[i] - current):
			i = k
	return MERGED_SCALES[clampi(i + signi(dir), 0, MERGED_SCALES.size() - 1)]


## Separating-axis test for two oriented boxes (`xf` places a box of full
## extents `size` centred on its origin). True when they overlap or touch.
static func boxes_intersect(xa: Transform3D, a: Vector3, xb: Transform3D, b: Vector3) -> bool:
	var ha := a * 0.5
	var hb := b * 0.5
	var ba := xa.basis.orthonormalized()
	var bb := xb.basis.orthonormalized()
	var axes: Array[Vector3] = []
	for i: int in 3:
		axes.append(ba[i])
		axes.append(bb[i])
	for i: int in 3:
		for j: int in 3:
			var c: Vector3 = ba[i].cross(bb[j])
			if c.length() > 1.0e-6:
				axes.append(c.normalized())
	var d := xb.origin - xa.origin
	for axis: Vector3 in axes:
		var ra := 0.0
		var rb := 0.0
		for k: int in 3:
			ra += ha[k] * absf(ba[k].dot(axis))
			rb += hb[k] * absf(bb[k].dot(axis))
		if absf(d.dot(axis)) > ra + rb:
			return false
	return true


# --- setup --------------------------------------------------------------------

## Binds this node to `arr` (it becomes a child, so membranes share the volumes'
## frame) and builds the membrane MultiMesh.
func bind(arr: Node3D) -> void:
	arrangement = arr
	if get_parent() != arr:
		if get_parent() != null:
			get_parent().remove_child(self)
		arr.add_child(self)
	_build_membranes()


## Adds the reduced-motion dim quad in front of `camera`. Optional.
func attach_fade(camera: Node3D) -> void:
	if camera == null or fade_quad != null:
		return
	_fade_material = StandardMaterial3D.new()
	_fade_material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	_fade_material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	_fade_material.cull_mode = BaseMaterial3D.CULL_DISABLED
	_fade_material.no_depth_test = true
	_fade_material.render_priority = 127
	_fade_material.albedo_color = Color(0.0, 0.0, 0.0, 0.0)
	var quad := QuadMesh.new()
	quad.size = Vector2(3.0, 3.0)
	fade_quad = MeshInstance3D.new()
	fade_quad.name = "PortalFade"
	fade_quad.mesh = quad
	fade_quad.material_override = _fade_material
	fade_quad.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	fade_quad.position = Vector3(0.0, 0.0, -FADE_DISTANCE)
	fade_quad.visible = false
	camera.add_child(fade_quad)


func _build_membranes() -> void:
	if membranes != null:
		return
	var quad := QuadMesh.new()
	quad.size = Vector2.ONE
	var shader: Shader = load(MEMBRANE_SHADER)
	_membrane_material = ShaderMaterial.new()
	_membrane_material.shader = shader
	_membrane_material.render_priority = MEMBRANE_PRIORITY
	quad.material = _membrane_material
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true            # before instance_count
	mm.mesh = quad
	mm.instance_count = MAX_PORTALS
	mm.visible_instance_count = 0
	membranes = MultiMeshInstance3D.new()
	membranes.name = "Membranes"
	membranes.multimesh = mm
	membranes.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	membranes.visible = false
	add_child(membranes)
	_set_motion_uniform()


func _set_motion_uniform() -> void:
	if _membrane_material != null:
		_membrane_material.set_shader_parameter("motion", 0.0 if reduced_motion else 1.0)


# --- state --------------------------------------------------------------------

func is_entered() -> bool:
	return _entered != &""


## The entered volume's id, or &"".
func entered_id() -> StringName:
	return _entered


func is_transitioning() -> bool:
	return _phase != Phase.IDLE


## The dim's opacity, 0 (clear) to 1 (dark).
func fade_alpha() -> float:
	match _phase:
		Phase.DIM_OUT:
			return _smooth(_phase_t / FADE_SEC)
		Phase.DIM_IN:
			return 1.0 - _smooth(_phase_t / FADE_SEC)
	return 0.0


## Whether `enter(id, …)` would be accepted. Refuses: no arrangement, the native
## arm active, an unknown, closed, hidden or offline volume, the volume already
## entered, or a transition in flight.
func can_enter(id: StringName) -> bool:
	if arrangement == null or _phase != Phase.IDLE or id == _entered or _guest != &"":
		return false
	if native_check.is_valid() and bool(native_check.call()):
		return false
	var v: Node3D = arrangement.volume(id)
	if v == null or v.is_closed or not v.visible_state:
		return false
	if offline_check.is_valid() and bool(offline_check.call(id)):
		return false
	return true


## Enters volume `id`, or swaps to it when another is entered (the current room
## becomes a portal). `user_world` is the head position and `heading_world` the
## way it faces, both in world space; `floor_world_y` is the floor (the XR
## origin's height), where the room's box bottom sits. Returns false and changes nothing when the
## request is refused (see `can_enter`).
func enter(id: StringName, user_world: Vector3, heading_world: Vector3 = Vector3(0.0, 0.0, -1.0),
		floor_world_y := 0.0) -> bool:
	if not can_enter(id):
		return false
	var first := _entered == &""
	if first:
		_snap = arrangement.snapshot()
	var inv := _parent_inverse()
	var head_local := inv * user_world
	var floor_local := (inv * Vector3(user_world.x, floor_world_y, user_world.z)).y
	_user = Vector3(head_local.x, floor_local, head_local.z)
	_eye_y = floor_local + clampf(head_local.y - floor_local, EYE_MIN, EYE_MAX)
	_heading = inv.basis * heading_world
	var old := _entered
	_entered = id
	_armed = false
	_dwell = 0.0
	_dwell_id = &""
	arrangement.layout_locked = true
	# A swap shrinks the outgoing room's content, and E7 forbids an eased shrink.
	var instant := reduced_motion or old != &"" \
			or not arrangement.transform.is_equal_approx(Transform3D.IDENTITY)
	var apply := func(duration: float) -> void:
		_apply_layout(old, duration, duration <= 0.0)
	if instant:
		_begin_dim(apply.bind(0.0))
	else:
		apply.call(EASE_SEC)
	entered_changed.emit(_entered)
	return true


## Leaves the entered space: the ring comes back exactly as it was before the
## first enter. Always through the dim, because the room's content shrinks and
## E7 forbids an eased shrink. False when nothing is entered or a transition is
## in flight.
func leave() -> bool:
	if _entered == &"" or _phase != Phase.IDLE:
		return false
	_end_space_through_dim()
	return true


## ADR-2178: true when `room_id` (a staged splat volume) and `guest_id` can be merged
## now: nothing is entered, no transition runs, both are open, shown and online, and
## `stage` carries a positive scale and a non-empty content box.
func can_enter_merged(room_id: StringName, guest_id: StringName, stage: Dictionary) -> bool:
	if _entered != &"" or room_id == guest_id or not can_enter(room_id):
		return false
	var g: Node3D = arrangement.volume(guest_id)
	if g == null or g.is_closed or not g.visible_state:
		return false
	if offline_check.is_valid() and bool(offline_check.call(guest_id)):
		return false
	var content: Variant = stage.get("content", null)
	return float(stage.get("metres_per_unit", 0.0)) > 0.0 and content is AABB \
			and (content as AABB).has_volume() and stage.get("focus", null) is Vector3


## Enters the merged space: `room_id` becomes the stage at `mult` times its measured
## scale, `guest_id` hangs between the user and the focus, the rest are hidden. Always
## through the dim (the room grows to tens of metres around the user). Arguments as
## for `enter`. False and no change when refused.
func enter_merged(room_id: StringName, guest_id: StringName, stage: Dictionary, mult: float,
		user_world: Vector3, heading_world: Vector3 = Vector3(0.0, 0.0, -1.0), floor_world_y := 0.0) -> bool:
	if not can_enter_merged(room_id, guest_id, stage):
		return false
	_snap = arrangement.snapshot()
	var inv := _parent_inverse()
	var head_local := inv * user_world
	var floor_local := (inv * Vector3(user_world.x, floor_world_y, user_world.z)).y
	_user = Vector3(head_local.x, floor_local, head_local.z)
	_eye_y = floor_local + clampf(head_local.y - floor_local, EYE_MIN, EYE_MAX)
	_heading = inv.basis * heading_world
	_entered = room_id
	_guest = guest_id
	_stage = stage.duplicate()
	_scale_mult = mult if mult > 0.0 else 1.0
	_armed = false
	_dwell = 0.0
	_dwell_id = &""
	arrangement.layout_locked = true
	_begin_dim(_apply_merged)
	entered_changed.emit(_entered)
	return true


func is_merged() -> bool:
	return _guest != &""


func merged_guest() -> StringName:
	return _guest


func merged_scale() -> float:
	return _scale_mult


## Viewer scale now: metres per scene unit times the multiplier (0 when not merged).
func viewer_scale() -> float:
	return float(_stage.get("metres_per_unit", 0.0)) * _scale_mult if _guest != &"" else 0.0


## Re-scales the merged space to `mult` about the floor point under `head_world`, so the
## user stays where they stand in the scene; through the dim. False when not merged,
## mid-transition or unchanged.
func set_merged_scale(mult: float, head_world: Vector3) -> bool:
	if _guest == &"" or _phase != Phase.IDLE or mult <= 0.0 or is_equal_approx(mult, _scale_mult):
		return false
	var about := _parent_inverse() * head_world
	about.y = _user.y
	_user = rescale_start(_user, about, mult / _scale_mult)
	_scale_mult = mult
	_begin_dim(_apply_merged)
	return true


## Ends the space through the dim from any state, including part-way through a
## dim: the restore lands in full dark and the opacity carries on from where it
## is, so nothing around the user moves or scales while visible.
func _end_space_through_dim() -> void:
	_entered = &""
	_dwell = 0.0
	match _phase:
		Phase.IDLE:
			_begin_dim(_apply_leave)
		Phase.DIM_OUT:
			# The dim already running goes dark first; swap what lands there.
			_pending = _apply_leave
		Phase.DIM_IN:
			# Go dark again from the current opacity (the smoothstep is symmetric).
			_pending = _apply_leave
			_phase = Phase.DIM_OUT
			_phase_t = FADE_SEC - _phase_t
	entered_changed.emit(_entered)


## Leaves at once with no transition. Only for Reset Layout, which rebuilds the
## ring anyway; a room that goes away ends through the dim instead.
func force_leave() -> void:
	if _entered == &"" and not arrangement_locked():
		return
	_phase = Phase.IDLE
	_pending = Callable()
	_set_fade(0.0)
	_entered = &""
	_apply_leave()
	entered_changed.emit(_entered)


func arrangement_locked() -> bool:
	return arrangement != null and arrangement.layout_locked


## Volume weights for the frame budget's near-cap split (stream XE's
## `FrameBudgetPass.set_volume_weights`): the entered volume 1.0, the rest
## OTHER_WEIGHT; all 1.0 when nothing is entered.
func budget_weights() -> Dictionary:
	var out := {}
	if arrangement == null:
		return out
	for v: Node3D in arrangement.volumes():
		var full: bool = _entered == &"" or v.volume_id == _entered or v.volume_id == _guest
		out[v.volume_id] = 1.0 if full else OTHER_WEIGHT
	return out


## The volumes currently drawn as portals, in layout order.
func portal_ids() -> Array[StringName]:
	var out: Array[StringName] = []
	if arrangement == null or _entered == &"" or _guest != &"":
		return out
	for v: Node3D in arrangement.volumes():
		if v.volume_id != _entered and not v.is_closed and v.visible_state:
			out.append(v.volume_id)
	return out


# --- layout -------------------------------------------------------------------

func _parent_inverse() -> Transform3D:
	var p := arrangement.get_parent() as Node3D if arrangement != null else null
	if p == null:
		return Transform3D.IDENTITY
	return (p.global_transform if p.is_inside_tree() else p.transform).affine_inverse()


# Applies the entered layout. `old` is the room being swapped out (or &"").
func _apply_layout(old: StringName, duration: float, snap_content: bool) -> void:
	if _entered == &"" or arrangement == null:
		return
	if duration <= 0.0:
		arrangement.transform = Transform3D.IDENTITY
	if old != &"" and old != _entered:
		var o: Node3D = arrangement.volume(old)
		if o != null:
			o.set_portal_room(false)
			_restore_bounds(o)
	var room: Node3D = arrangement.volume(_entered)
	if room == null:
		return
	room.set_portal_room(true)
	if not room.bounds.is_equal_approx(ROOM_BOUNDS) or room.bounds_mode != SpatialVolume.BoundsMode.BOUNDED:
		room.set_runtime_bounds(ROOM_BOUNDS, SpatialVolume.BoundsMode.BOUNDED)
	var room_xf := room_transform(_user, _heading, ROOM_BOUNDS)
	arrangement.ease_volume_to(_entered, room_xf, duration)
	_layout_portals(duration)
	if snap_content:
		room.snap_fit()
	_signature = _make_signature()
	_update_membranes()


# ADR-2178: lands the merged layout (always in full dark). The guest keeps its own
# fit rule inside its box, so the graph is drawn at the guest's scale, not the room's.
func _apply_merged() -> void:
	if _entered == &"" or _guest == &"" or arrangement == null:
		return
	arrangement.transform = Transform3D.IDENTITY
	var room: Node3D = arrangement.volume(_entered)
	var guest: Node3D = arrangement.volume(_guest)
	if room == null or guest == null:
		return
	var content: AABB = _stage["content"]
	var s := viewer_scale()
	room.set_portal_room(true)
	room.set_runtime_bounds(merged_room_bounds(content, s), SpatialVolume.BoundsMode.BOUNDED)
	arrangement.ease_volume_to(_entered, merged_room_transform(_user, _heading, content, s), 0.0)
	var focus := stage_point(_user, _heading, s, _stage["focus"])
	var g := guest_placement(_user, _eye_y, focus)
	guest.set_runtime_bounds(g["bounds"], SpatialVolume.BoundsMode.BOUNDED)
	arrangement.ease_volume_to(_guest, g["xf"], 0.0)
	for v: Node3D in arrangement.volumes():
		var id: StringName = v.volume_id
		if id != _entered and id != _guest and not v.is_closed and v.visible_state:
			v.request_visible(false)
			if not _hidden.has(id):
				_hidden.append(id)
	room.snap_fit()
	guest.snap_fit()
	_signature = _make_signature()
	_update_membranes()


# The dim's landing for a portal that appeared, went or resized: re-lay the ring
# and take the signature then, so a second change made during the dim is seen
# afterwards.
func _relayout_portals() -> void:
	if _entered == &"" or arrangement == null:
		return
	_layout_portals(0.0)
	_signature = _make_signature()
	_update_membranes()


func _layout_portals(duration: float) -> void:
	var ids := portal_ids()
	var sizes: Array = []
	for id: StringName in ids:
		sizes.append((arrangement.volume(id) as Node3D).bounds)
	var xfs := portal_layout(_user, _heading, ROOM_BOUNDS, sizes, _eye_y)
	for i: int in ids.size():
		arrangement.ease_volume_to(ids[i], xfs[i], duration)


func _restore_bounds(v: Node3D) -> void:
	var id: StringName = v.volume_id
	if _snap.has("bounds") and _snap["bounds"].has(id) and not v.is_closed:
		var b: Vector3 = _snap["bounds"][id]
		var m: int = int(_snap["modes"][id])
		if not v.bounds.is_equal_approx(b) or v.bounds_mode != m:
			v.set_runtime_bounds(b, m)


# Puts the ring back at once; only ever called in full dark (or by force_leave).
func _apply_leave() -> void:
	if arrangement == null or _snap.is_empty():
		return
	for v: Node3D in arrangement.volumes():
		v.set_portal_room(false)
	arrangement.restore(_snap, 0.0)
	arrangement.layout_locked = false
	for id: StringName in _hidden:
		var h: Node3D = arrangement.volume(id)
		if h != null and not h.is_closed:
			h.request_visible(true)
	_hidden.clear()
	_guest = &""
	_stage = {}
	for v: Node3D in arrangement.volumes():
		if not v.is_closed and v.visible_state:
			v.snap_fit()
	_snap = {}
	_signature = []
	_update_membranes()


# A change in which volumes are portals, or in their size, re-lays the ring.
func _make_signature() -> Array:
	var sig: Array = []
	if arrangement == null:
		return sig
	for v: Node3D in arrangement.volumes():
		sig.append([v.volume_id, v.is_closed, v.visible_state, v.bounds])
	return sig


# --- transitions --------------------------------------------------------------

func _begin_dim(pending: Callable) -> void:
	_pending = pending
	_phase = Phase.DIM_OUT
	_phase_t = 0.0


static func _smooth(u: float) -> float:
	var k := clampf(u, 0.0, 1.0)
	return k * k * (3.0 - 2.0 * k)


func _set_fade(a: float) -> void:
	if fade_quad == null:
		return
	fade_quad.visible = a > 0.001
	_fade_material.albedo_color = Color(0.0, 0.0, 0.0, a)


## Call once per frame, after `VolumeArrangement.tick`. Advances the dim,
## re-lays the ring when a portal's visibility or size changed, ends the space
## when its room can no longer be shown, and refreshes the membranes.
func tick(delta: float) -> void:
	if arrangement == null:
		return
	match _phase:
		Phase.DIM_OUT:
			_phase_t += maxf(delta, 0.0)
			if _phase_t >= FADE_SEC:
				_set_fade(1.0)
				if _pending.is_valid():
					_pending.call()
				_pending = Callable()
				_phase = Phase.DIM_IN
				_phase_t = 0.0
			else:
				_set_fade(fade_alpha())
		Phase.DIM_IN:
			_phase_t += maxf(delta, 0.0)
			if _phase_t >= FADE_SEC:
				_phase = Phase.IDLE
				_phase_t = 0.0
			_set_fade(fade_alpha())
	if _entered == &"":
		# The ring is the default: nothing to do once the membranes are hidden.
		if membranes != null and membranes.visible:
			_update_membranes()
		return
	var room: Node3D = arrangement.volume(_entered)
	if room == null or room.is_closed or not room.visible_state \
			or (offline_check.is_valid() and bool(offline_check.call(_entered))):
		_end_space_through_dim()
		return
	if _guest != &"":
		var g: Node3D = arrangement.volume(_guest)
		if _phase == Phase.IDLE and (g == null or g.is_closed or not g.visible_state):
			_end_space_through_dim()
		return
	if _phase == Phase.IDLE:
		var sig := _make_signature()
		if sig != _signature:
			if reduced_motion:
				_begin_dim(_relayout_portals)
			else:
				_layout_portals(EASE_SEC)
				_signature = sig
	_update_membranes()


func _update_membranes() -> void:
	if membranes == null:
		return
	var ids := portal_ids()
	var mm := membranes.multimesh
	var n := mini(ids.size(), MAX_PORTALS)
	mm.visible_instance_count = n
	membranes.visible = n > 0
	_inst_xf.resize(n)
	_inst_col.resize(n)
	for i: int in n:
		var v: Node3D = arrangement.volume(ids[i])
		var b: Vector3 = v.bounds
		var face := Transform3D(Basis.from_scale(Vector3(b.x, b.y, 1.0)), Vector3(0.0, 0.0, b.z * 0.5 + MEMBRANE_LIFT))
		var tint: Color = TINTS.get(ids[i], DEFAULT_TINT)
		tint.a = 1.0 if v.focused else UNFOCUSED
		_inst_xf[i] = v.transform * face
		_inst_col[i] = tint
		mm.set_instance_transform(i, _inst_xf[i])
		mm.set_instance_color(i, tint)


## The transform and colour (tint rgb, focus in alpha) written for membrane `i`.
func membrane_transform(i: int) -> Transform3D:
	return _inst_xf[i]


func membrane_color(i: int) -> Color:
	return _inst_col[i]


# --- walk-through -------------------------------------------------------------

## Metres from world point `p` to the nearest point of `v`'s box (0 inside).
static func distance_to_box(v: Node3D, p: Vector3) -> float:
	var xf: Transform3D = v.global_transform if v.is_inside_tree() else v.transform
	var local := xf.affine_inverse() * p
	var over: Vector3 = local.abs() - (v.bounds as Vector3) * 0.5
	# The inverse above gives local units. Restore per-axis world scale before
	# comparing to a world-space rearm margin (rotated boxes remain orthogonal).
	return (over.max(Vector3.ZERO) * xf.basis.get_scale().abs()).length()


## Call every frame with the head pose (world). With walk-through on and a space
## entered, the head inside a portal's box for DWELL_SEC enters it. After any
## enter the walk-through re-arms only once the head has been REARM_DISTANCE
## outside every portal. `floor_world_y` is the floor (the XR origin's height),
## as for `enter`. Returns the id entered this call, or &"".
func update_walkthrough(head_world: Vector3, heading_world: Vector3, delta: float,
		floor_world_y := 0.0) -> StringName:
	if not walkthrough_enabled or _entered == &"" or _phase != Phase.IDLE:
		_dwell = 0.0
		_dwell_id = &""
		return &""
	var ids := portal_ids()
	if not _armed:
		var clear := true
		for id: StringName in ids:
			var axis_scale := arrangement.global_transform.basis.get_scale().abs()
			var world_rearm := REARM_DISTANCE * maxf(axis_scale.x, maxf(axis_scale.y, axis_scale.z))
			if distance_to_box(arrangement.volume(id), head_world) <= world_rearm:
				clear = false
				break
		if clear:
			_armed = true
		_dwell = 0.0
		_dwell_id = &""
		return &""
	var inside: StringName = &""
	for id: StringName in ids:
		if distance_to_box(arrangement.volume(id), head_world) <= 0.0:
			inside = id
			break
	if inside == &"":
		_dwell = 0.0
		_dwell_id = &""
		return &""
	if inside != _dwell_id:
		_dwell_id = inside
		_dwell = 0.0
	_dwell += maxf(delta, 0.0)
	if _dwell < DWELL_SEC:
		return &""
	_dwell = 0.0
	_dwell_id = &""
	return inside if enter(inside, head_world, heading_world, floor_world_y) else &""


func is_walkthrough_armed() -> bool:
	return _armed
