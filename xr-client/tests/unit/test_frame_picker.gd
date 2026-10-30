extends "res://addons/gut/test.gd"
const FramePicker := preload("res://scripts/frame_picker.gd")
const SpatialVolume := preload("res://scripts/spatial_volume.gd")

# Spatial-volume UX features 1 and 4: the ray against the volume boxes. Faces
# give "frame", corners within the handle radius give "handle" (via
# SpatialVolume.handle_at), handles beat faces within a volume, and the
# nearest volume wins across volumes.

var volumes: Dictionary


func _volume(id: StringName, at: Vector3, b := Vector3.ONE) -> Node3D:
	var v: Node3D = SpatialVolume.new()
	v.volume_id = id
	add_child_autofree(v)
	v.position = at
	v.set_runtime_bounds(b, SpatialVolume.BoundsMode.BOUNDED)
	return v


func before_each() -> void:
	volumes = {&"knowledge": _volume(&"knowledge", Vector3(0, 0, -3))}


func test_face_hit_is_frame_at_the_entry_point() -> void:
	var hit := FramePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes)
	assert_eq(hit.get("volume"), &"knowledge")
	assert_eq(hit.get("part"), "frame")
	assert_eq(hit.get("handle"), -1)
	assert_almost_eq(float(hit["distance"]), 2.5, 1e-4)
	assert_true((hit["world_pos"] as Vector3).is_equal_approx(Vector3(0, 0, -2.5)))


func test_corner_hit_is_a_handle_and_beats_the_face() -> void:
	# Aim at the +X +Y +Z corner (handle 7) of the box centred at (0, 0, -3).
	var corner := Vector3(0.5, 0.5, -2.5)
	var hit := FramePicker.pick(Vector3.ZERO, corner.normalized(), volumes)
	assert_eq(hit.get("part"), "handle")
	assert_eq(hit.get("handle"), 7)
	assert_true((hit["world_pos"] as Vector3).is_equal_approx(corner))
	assert_almost_eq(float(hit["distance"]), corner.length(), 1e-4)
	# 3 cm off the corner but inside 4 cm: still the handle.
	hit = FramePicker.pick(Vector3.ZERO, (corner + Vector3(0, 0.03, 0)).normalized(), volumes)
	assert_eq(hit.get("part"), "handle")
	# With a 1 cm radius the same ray falls back to the face it crosses.
	hit = FramePicker.pick(Vector3.ZERO, (corner - Vector3(0.03, 0.03, 0)).normalized(), volumes, 0.01)
	assert_eq(hit.get("part"), "frame")


func test_handle_radius_is_in_metres_under_a_scaled_parent() -> void:
	var parent := Node3D.new()
	add_child_autofree(parent)
	parent.scale = Vector3.ONE * 0.5
	var v: Node3D = SpatialVolume.new()
	parent.add_child(v)
	v.position = Vector3(0, 0, -6)  # world centre (0, 0, -3), world half 0.25
	v.set_runtime_bounds(Vector3.ONE, SpatialVolume.BoundsMode.BOUNDED)
	var corner := Vector3(0.25, 0.25, -2.75)
	# ~2.7 cm inside the corner on the front face: a handle at 4 cm, the face at 2 cm.
	var off := (corner - Vector3(0.01, 0.025, 0)).normalized()
	assert_eq(FramePicker.pick(Vector3.ZERO, off, {&"s": v}).get("part"), "handle", "2.7 cm < 4 cm")
	assert_eq(FramePicker.pick(Vector3.ZERO, off, {&"s": v}, 0.02).get("part"), "frame", "2.7 cm > 2 cm")


func test_miss_inside_and_behind() -> void:
	assert_eq(FramePicker.pick(Vector3.ZERO, Vector3(0, 1, 0), volumes), {}, "miss")
	assert_eq(FramePicker.pick(Vector3(0, 0, -3), Vector3(0, 0, -1), volumes), {}, "from inside")
	assert_eq(FramePicker.pick(Vector3(0, 0, -5), Vector3(0, 0, -1), volumes), {}, "behind")
	assert_eq(FramePicker.pick(Vector3.ZERO, Vector3.ZERO, volumes), {}, "zero direction")


func test_hidden_closed_and_immersive_volumes_have_no_frame() -> void:
	var v: Node3D = volumes[&"knowledge"]
	v.request_visible(false)
	assert_eq(FramePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes), {})
	v.request_visible(true)
	v.request_bounds_mode(SpatialVolume.BoundsMode.IMMERSIVE)
	assert_eq(FramePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes), {})
	v.request_bounds_mode(SpatialVolume.BoundsMode.BOUNDED)
	assert_eq(FramePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes).get("part"), "frame")
	v.close()
	assert_eq(FramePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes), {})


func test_non_interactable_volume_keeps_its_frame_but_not_its_handles() -> void:
	var v: Node3D = volumes[&"knowledge"]
	# 1 cm inside the corner on the front face: a handle while interactable.
	var near := Vector3(0.49, 0.49, -2.5).normalized()
	assert_eq(FramePicker.pick(Vector3.ZERO, near, volumes).get("part"), "handle")
	v.request_interactable(false)
	var hit := FramePicker.pick(Vector3.ZERO, near, volumes)
	assert_eq(hit.get("part"), "frame", "handle_at refuses a non-interactable volume")


func test_nearest_volume_wins() -> void:
	volumes[&"ontology"] = _volume(&"ontology", Vector3(0, 0, -6))
	assert_eq(FramePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes).get("volume"), &"knowledge")
	(volumes[&"knowledge"] as Node3D).request_visible(false)
	var hit := FramePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes)
	assert_eq(hit.get("volume"), &"ontology")
	assert_almost_eq(float(hit["distance"]), 5.5, 1e-4)


func test_rotated_box() -> void:
	var v: Node3D = volumes[&"knowledge"]
	v.rotation = Vector3(0, deg_to_rad(45.0), 0)
	# A 1 m cube turned 45° about Y shows an edge at z = -3 + √2/2.
	var hit := FramePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes, 0.0001)
	assert_almost_eq(float(hit["distance"]), 3.0 - sqrt(2.0) * 0.5, 1e-4)


func test_pick_volume_and_ray_box_directly() -> void:
	var v: Node3D = volumes[&"knowledge"]
	var one := FramePicker.pick_volume(Vector3.ZERO, Vector3(0, 0, -1), v, 0.04)
	assert_eq(one.get("part"), "frame")
	assert_false(one.has("volume"), "pick adds the id")
	assert_eq(FramePicker.pick_volume(Vector3.ZERO, Vector3(0, 0, -1), null, 0.04), {})
	var xf := Transform3D(Basis(), Vector3(0, 0, -3))
	assert_almost_eq(FramePicker.ray_box(Vector3.ZERO, Vector3(0, 0, -1), xf, Vector3.ONE * 0.5), 2.5, 1e-5)
	assert_eq(FramePicker.ray_box(Vector3(0, 0, -3), Vector3(0, 0, -1), xf, Vector3.ONE * 0.5), -1.0, "inside")
	assert_eq(FramePicker.ray_box(Vector3(2, 0, 0), Vector3(0, 0, -1), xf, Vector3.ONE * 0.5), -1.0, "parallel miss")
