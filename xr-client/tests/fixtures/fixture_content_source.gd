extends RefCounted
## Used via `const FixtureContentSource := preload("res://tests/fixtures/fixture_content_source.gd")`.

## A fixed point set behind the duck-typed content-source interface
## (phase-1 contract §3.3), for agent B's volume tests. It draws every point
## as one instance of a single MultiMesh under the volume's Content node, so
## a drawn node centre is exactly `position_of(id)` in graph units.

var points: PackedVector3Array
## Excluded from `content_aabb` while >= 0 (the grabbed-node exemption that
## the real source applies, E7).
var grabbed_id := -1
var instance: MultiMeshInstance3D
var mount_count := 0
var _id: StringName


func _init(pts: PackedVector3Array = PackedVector3Array(), id: StringName = &"fixture:0b0001") -> void:
	points = pts
	_id = id


func content_id() -> StringName:
	return _id


func content_ids() -> PackedInt32Array:
	var ids := PackedInt32Array()
	ids.resize(points.size())
	for i: int in points.size():
		ids[i] = i
	return ids


## Full extent over every point except `grabbed_id`; AABB() when none.
func content_aabb() -> AABB:
	var out := AABB()
	var first := true
	for i: int in points.size():
		if i == grabbed_id:
			continue
		if first:
			out = AABB(points[i], Vector3.ZERO)
			first = false
		else:
			out = out.expand(points[i])
	return out


func mount(parent: Node3D) -> void:
	unmount()
	instance = MultiMeshInstance3D.new()
	instance.name = "FixtureNodes"
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	var mesh := SphereMesh.new()
	mesh.radial_segments = 4
	mesh.rings = 2
	mm.mesh = mesh
	instance.multimesh = mm
	parent.add_child(instance)
	mount_count += 1
	_pack()


func unmount() -> void:
	if instance == null:
		return
	if instance.get_parent() != null:
		instance.get_parent().remove_child(instance)
	instance.free()
	instance = null


func position_of(id: int) -> Vector3:
	return points[id] if id >= 0 and id < points.size() else Vector3.ZERO


## Replaces the point set (a moving layout) and repacks.
func set_points(pts: PackedVector3Array) -> void:
	points = pts
	_pack()


func _pack() -> void:
	if instance == null:
		return
	var mm := instance.multimesh
	mm.instance_count = points.size()
	for i: int in points.size():
		mm.set_instance_transform(i, Transform3D(Basis.IDENTITY, points[i]))
