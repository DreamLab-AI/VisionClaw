extends Node3D
## Used via `const VolumeArrangement := preload("res://scripts/volume_arrangement.gd")`
## rather than a class_name, so a headless run with a stale global class
## cache still resolves it.

## The default arrangement of spatial volumes (ADR-2158 D7 and D9, phase-1
## contract §1 and §3.4): a ring of RING_RADIUS metres about the user's start
## position (this node's origin), at RING_HEIGHT metres.
##
## Yaw convention. ADR-2135's vertex angle is a yaw about +Y, measured from the
## view axis (`visionclaw-tri-layout` lib.rs: knowledge −60° front-left,
## ontology +60° front-right). The XR user stands at the ring centre facing −Z,
## so a yaw θ places a volume at `(sin θ, 0, −cos θ) × RING_RADIUS`: knowledge
## front-left, ontology front-right, Codescape (0°) straight ahead. Each volume
## is turned by −θ about +Y so its +Z face looks back at the centre.
##
## A volume is only ever placed when it is added, by `reset_default` and by
## `bring_to_front`, so a user move (anything that sets the volume's
## transform) holds until one of those.
##
## Slots. Every volume holds one ring slot (a yaw). At add time its slot is its
## default yaw, so the slots are a permutation of the default yaws. A grip move
## (`move_volume` or `mark_moved`) takes a box off its slot position but keeps
## the slot assignment, so `bring_to_front` always has a slot to hand the
## previous front occupant. `reset_default` restores the default slots.
##
## Easing. `bring_to_front` eases the two boxes it swaps over `duration`
## seconds (smoothstep in time) through `tick`, which the scene calls every
## frame. Position lerps and rotation slerps; the basis is rebuilt at unit
## scale every step, so repeated calls never compound scale. `tick` writes only
## the boxes that are easing and allocates nothing.
##
## Frames. `arrangement_aabb` and `set_arrangement_scale` work in this node's
## own unscaled frame: the box AABB is measured before this node's scale, so a
## native shim that fits it and then sets the scale gets the same answer on
## every refit instead of compounding.

const SpatialVolume := preload("res://scripts/spatial_volume.gd")

const RING_RADIUS := 1.6
const RING_HEIGHT := 1.3
## E3 defaults (contract §1).
const DEFAULT_YAWS := {&"knowledge": -60.0, &"ontology": 60.0, &"codescape": 0.0}
const DEFAULT_BOUNDS := Vector3.ONE
## The slot `bring_to_front` moves a box to: straight ahead.
const FRONT_YAW := 0.0
## Default ease time for `bring_to_front`, seconds.
const FRONT_EASE_SEC := 0.35

var _volumes: Array[Node3D] = []
var _yaws: Dictionary = {}          # volume_id → default yaw degrees
var _slots: Dictionary = {}         # volume_id → current slot yaw degrees
var _moved: Dictionary = {}         # volume_id → true while off its slot by a user move
var _default_bounds: Dictionary = {}  # volume_id → bounds at add time
# Per-volume ease, parallel to _volumes (preallocated in add_volume).
var _ease_t := PackedFloat32Array()     # elapsed seconds; < 0 = idle
var _ease_dur := PackedFloat32Array()
var _ease_from: Array[Transform3D] = []
var _ease_to: Array[Transform3D] = []
var _easing := 0


## The default transform for a volume at `yaw_deg` on the ring.
static func ring_transform(yaw_deg: float) -> Transform3D:
	var t := deg_to_rad(yaw_deg)
	var origin := Vector3(sin(t) * RING_RADIUS, RING_HEIGHT, -cos(t) * RING_RADIUS)
	return Transform3D(Basis(Vector3.UP, -t), origin)


## Adds `v` (a SpatialVolume) as a child and places it on the ring at
## `yaw_deg`. A second volume with the same id replaces nothing: it is refused.
func add_volume(v: Node3D, yaw_deg: float) -> void:
	if v == null:
		return
	var id: StringName = v.volume_id
	if _yaws.has(id):
		push_error("VolumeArrangement: volume %s already added" % id)
		return
	if v.get_parent() != self:
		if v.get_parent() != null:
			v.get_parent().remove_child(v)
		add_child(v)
	_volumes.append(v)
	_yaws[id] = yaw_deg
	_slots[id] = yaw_deg
	_default_bounds[id] = v.bounds
	_ease_t.append(-1.0)
	_ease_dur.append(0.0)
	_ease_from.append(Transform3D.IDENTITY)
	_ease_to.append(Transform3D.IDENTITY)
	v.transform = ring_transform(yaw_deg)


func volume(id: StringName) -> Node3D:
	for v: Node3D in _volumes:
		if v.volume_id == id:
			return v
	return null


## The volumes in the order they were added.
func volumes() -> Array:
	return _volumes.duplicate()


## The default ring transform of volume `id` (identity if unknown).
func default_transform(id: StringName) -> Transform3D:
	return ring_transform(_yaws[id]) if _yaws.has(id) else Transform3D.IDENTITY


## AABB of the boxes of every visible, open volume, in this node's unscaled
## frame (see the header). Empty AABB when there are none.
func arrangement_aabb() -> AABB:
	var out := AABB()
	var first := true
	for v: Node3D in _volumes:
		if v.is_closed or not v.visible_state:
			continue
		var b: Vector3 = v.bounds
		var box: AABB = v.transform * AABB(-b * 0.5, b)
		if first:
			out = box
			first = false
		else:
			out = out.merge(box)
	return out


## Scales the whole arrangement about its origin (native shim only). Ignores a
## non-finite or non-positive scale.
func set_arrangement_scale(s: float) -> void:
	if not is_finite(s) or s <= 0.0:
		return
	scale = Vector3.ONE * s


func arrangement_scale() -> float:
	return scale.x


## Puts every volume back on its default ring slot, dropping user moves,
## slot swaps and eases. Leaves the arrangement scale alone (the shim restores
## that itself). With `restore_bounds` each volume also ends any resize and
## gets back the bounds it had when added (the HUD's "Reset layout").
func reset_default(restore_bounds := false) -> void:
	for i: int in _volumes.size():
		var v: Node3D = _volumes[i]
		var id: StringName = v.volume_id
		_ease_t[i] = -1.0
		_slots[id] = _yaws[id]
		v.transform = ring_transform(_yaws[id])
		if restore_bounds and not v.is_closed:
			v.end_resize()
			v.set_runtime_bounds(_default_bounds.get(id, DEFAULT_BOUNDS), v.bounds_mode)
	_moved.clear()
	_easing = 0


# --- slots, moves and bring-to-front (UX features 4 and 5) --------------------

## The ring slot volume `id` holds now (degrees), or NAN if unknown.
func slot_yaw(id: StringName) -> float:
	return float(_slots[id]) if _slots.has(id) else NAN


## The volume holding slot `yaw_deg`, or &"" if none.
func occupant(yaw_deg: float) -> StringName:
	for v: Node3D in _volumes:
		if is_equal_approx(float(_slots[v.volume_id]), yaw_deg):
			return v.volume_id
	return &""


## True while `id` sits off its slot because the user moved it.
func is_moved(id: StringName) -> bool:
	return _moved.has(id)


## Records a user move of `id` (its transform is the user's until a reset or
## `bring_to_front`) and cancels any ease on it, so the next tick does not drag
## it back.
func mark_moved(id: StringName) -> void:
	var i := _index_of(id)
	if i < 0:
		return
	_moved[id] = true
	_cancel_ease(i)


## Sets `id`'s transform from a user move, in this node's own frame. The basis
## is orthonormalised so a box stays at unit scale whatever the caller's maths
## carried in from the arrangement scale.
func move_volume(id: StringName, local_xf: Transform3D) -> void:
	var v := volume(id)
	if v == null:
		return
	mark_moved(id)
	v.transform = Transform3D(local_xf.basis.orthonormalized(), local_xf.origin)


## Moves volume `id` to the FRONT_YAW slot. The previous front occupant takes
## the slot `id` held. Both ease over `duration` seconds (0 or less snaps) and
## both lose their moved flag. Returns false (and does nothing) for an unknown
## id, or when `id` already sits unmoved and at rest in the front slot.
func bring_to_front(id: StringName, duration: float = FRONT_EASE_SEC) -> bool:
	var i := _index_of(id)
	if i < 0:
		return false
	var mine: float = _slots[id]
	if is_equal_approx(mine, FRONT_YAW) and not _moved.has(id) and _ease_t[i] < 0.0:
		return false
	var other := occupant(FRONT_YAW)
	if other != &"" and other != id:
		_slots[other] = mine
		_moved.erase(other)
		_start_ease(_index_of(other), ring_transform(mine), duration)
	_slots[id] = FRONT_YAW
	_moved.erase(id)
	_start_ease(i, ring_transform(FRONT_YAW), duration)
	return true


## True while `id` (or, with &"", any volume) is easing.
func is_easing(id: StringName = &"") -> bool:
	if id == &"":
		return _easing > 0
	var i := _index_of(id)
	return i >= 0 and _ease_t[i] >= 0.0


## Advances every running ease by `delta` seconds. Does nothing when idle.
func tick(delta: float) -> void:
	if _easing == 0:
		return
	for i: int in _volumes.size():
		if _ease_t[i] < 0.0:
			continue
		var t := _ease_t[i] + maxf(delta, 0.0)
		var dur := _ease_dur[i]
		var u := 1.0 if dur <= 0.0 else clampf(t / dur, 0.0, 1.0)
		_volumes[i].transform = ease_transform(_ease_from[i], _ease_to[i], u)
		if u >= 1.0:
			_cancel_ease(i)
		else:
			_ease_t[i] = t


## Smoothstep blend from `a` to `b` at `u` in [0, 1]: the origin lerps, the
## rotation slerps and the result is unit scale (no compounding).
static func ease_transform(a: Transform3D, b: Transform3D, u: float) -> Transform3D:
	var k := clampf(u, 0.0, 1.0)
	k = k * k * (3.0 - 2.0 * k)
	var qa := a.basis.orthonormalized().get_rotation_quaternion()
	var qb := b.basis.orthonormalized().get_rotation_quaternion()
	return Transform3D(Basis(qa.slerp(qb, k)), a.origin.lerp(b.origin, k))


func _start_ease(i: int, to: Transform3D, duration: float) -> void:
	if i < 0:
		return
	if duration <= 0.0:
		_cancel_ease(i)
		_volumes[i].transform = to
		return
	if _ease_t[i] < 0.0:
		_easing += 1
	_ease_t[i] = 0.0
	_ease_dur[i] = duration
	_ease_from[i] = _volumes[i].transform
	_ease_to[i] = to


func _cancel_ease(i: int) -> void:
	if _ease_t[i] >= 0.0:
		_ease_t[i] = -1.0
		_easing -= 1


func _index_of(id: StringName) -> int:
	for i: int in _volumes.size():
		if _volumes[i].volume_id == id:
			return i
	return -1
