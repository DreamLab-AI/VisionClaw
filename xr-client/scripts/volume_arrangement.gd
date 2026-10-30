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
## A volume is only ever placed when it is added and by `reset_default`, so a
## user move (anything that sets the volume's transform) holds until then.
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

var _volumes: Array[Node3D] = []
var _yaws: Dictionary = {}          # volume_id → yaw degrees


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


## Puts every volume back on its ring position, dropping user moves. Leaves
## the arrangement scale alone (the shim restores that itself).
func reset_default() -> void:
	for v: Node3D in _volumes:
		v.transform = ring_transform(_yaws[v.volume_id])
