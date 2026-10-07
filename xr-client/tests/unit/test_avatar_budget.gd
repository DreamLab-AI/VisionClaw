extends "res://addons/gut/test.gd"

# Remote avatars sit inside FrameBudget's other_tris. The head was a default
# SphereMesh (64 x 32, 4 224 triangles) for a 12 cm sphere; 16 x 8 matches the
# graph's gem nodes (lod.rs SPHERE_TRIS = 288).

func _tris(mi: MeshInstance3D) -> int:
	return mi.mesh.get_faces().size() / 3


func test_avatar_meshes_are_budget_sized() -> void:
	var av: Node3D = (load("res://scenes/Avatar.tscn") as PackedScene).instantiate()
	add_child(av)
	assert_eq(_tris(av.get_node("Head") as MeshInstance3D), 288, "head = a gem node's sphere")
	assert_lte(_tris(av.get_node("Head/VoiceIndicator") as MeshInstance3D), 80, "voice dot")
	av.queue_free()
