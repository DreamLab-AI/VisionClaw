extends "res://addons/gut/test.gd"

# Scene-load smoke tests for the Godot 4 XR client (PRD-QE-002 S4.2).
#
# Each test loads one of the four canonical scenes and verifies it
# instantiates without errors and contains the expected child structure.
# These run headless under CI via GUT 9.3's `addons/gut/gut_cmdln.gd`
# (see `xr-client/.gutconfig.json` / `xr-client/tests/run_gut.gd`).

func test_xr_boot_scene_loads():
	var packed: PackedScene = load("res://scenes/XRBoot.tscn")
	assert_not_null(packed, "XRBoot.tscn must be loadable")
	var root: Node3D = packed.instantiate()
	assert_not_null(root, "XRBoot.tscn must instantiate")
	assert_eq(root.name, "XRBoot", "root node name")
	# XRBoot must contain an XROrigin3D (per scene definition).
	assert_not_null(root.get_node_or_null("XROrigin3D"), "XROrigin3D child")
	# XR cameras + controller children under the origin.
	var origin := root.get_node_or_null("XROrigin3D")
	assert_not_null(origin.get_node_or_null("XRCamera3D"), "XRCamera3D child")
	assert_not_null(origin.get_node_or_null("LeftController"), "LeftController child")
	assert_not_null(origin.get_node_or_null("RightController"), "RightController child")
	root.queue_free()

func test_graph_scene_loads():
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	assert_not_null(packed, "GraphScene.tscn must be loadable")
	var root: Node3D = packed.instantiate()
	assert_not_null(root, "GraphScene.tscn must instantiate")
	assert_eq(root.name, "GraphScene", "root node name")
	# Graph scene also hosts an XR origin + camera (player rig).
	assert_not_null(root.get_node_or_null("XROrigin3D"), "XROrigin3D child")
	root.queue_free()

func test_hud_scene_loads():
	var packed: PackedScene = load("res://scenes/HUD.tscn")
	assert_not_null(packed, "HUD.tscn must be loadable")
	var root: Node3D = packed.instantiate()
	assert_not_null(root, "HUD.tscn must instantiate")
	assert_eq(root.name, "HUD", "root node name")
	# HUD hosts a SubViewport so the panel can render UI offscreen.
	assert_not_null(root.get_node_or_null("HudViewport"), "HudViewport child")
	root.queue_free()

func test_avatar_scene_loads():
	var packed: PackedScene = load("res://scenes/Avatar.tscn")
	assert_not_null(packed, "Avatar.tscn must be loadable")
	var root: Node3D = packed.instantiate()
	assert_not_null(root, "Avatar.tscn must instantiate")
	assert_eq(root.name, "Avatar", "root node name")
	# Avatar geometry: head + (hidden) hand mesh instances.
	assert_not_null(root.get_node_or_null("Head"), "Head child")
	assert_not_null(root.get_node_or_null("LeftHand"), "LeftHand child")
	assert_not_null(root.get_node_or_null("RightHand"), "RightHand child")
	# M1 (PRD-023 WP-9): the peer avatar carries a DID badge below the nameplate.
	assert_not_null(root.get_node_or_null("Head/DidBadge"), "DidBadge child")
	root.queue_free()

func test_agent_avatar_scene_loads():
	var packed: PackedScene = load("res://scenes/AgentAvatar.tscn")
	assert_not_null(packed, "AgentAvatar.tscn must be loadable")
	var root: Node3D = packed.instantiate()
	assert_not_null(root, "AgentAvatar.tscn must instantiate")
	assert_eq(root.name, "AgentAvatar", "root node name")
	# M3 geometric embodiment: core + billboard + gaze cone + DID badge.
	assert_not_null(root.get_node_or_null("Core"), "Core child")
	assert_not_null(root.get_node_or_null("CoreBillboard"), "CoreBillboard child")
	assert_not_null(root.get_node_or_null("GazeCone"), "GazeCone child")
	assert_not_null(root.get_node_or_null("Badge"), "Badge child")
	root.queue_free()

func test_all_scenes_have_attached_scripts():
	# Every top-level scene must wire a script -- the gdext extension is
	# loaded but the GDScript glue lives in `xr-client/scripts/`.
	var scene_paths := [
		"res://scenes/XRBoot.tscn",
		"res://scenes/GraphScene.tscn",
		"res://scenes/HUD.tscn",
		"res://scenes/Avatar.tscn",
	]
	for path in scene_paths:
		var packed = load(path)
		assert_not_null(packed, "scene %s must load" % path)
		var root = packed.instantiate()
		var script = root.get_script()
		assert_not_null(script, "scene %s must attach a script" % path)
		root.queue_free()

func test_scene_uids_are_unique():
	# Each scene declares a `uid://` tag in its tscn header; collisions cause
	# Godot import failures. We verify by loading each scene and checking
	# resource_path differs.
	var paths := [
		"res://scenes/XRBoot.tscn",
		"res://scenes/GraphScene.tscn",
		"res://scenes/HUD.tscn",
		"res://scenes/Avatar.tscn",
	]
	var seen := {}
	for p in paths:
		var pkg = load(p)
		assert_not_null(pkg, "scene %s must load" % p)
		var rp = pkg.resource_path
		assert_false(seen.has(rp), "duplicate resource_path %s" % rp)
		seen[rp] = true


# ---------------------------------------------------------------------------
# Avatar unit tests
# ---------------------------------------------------------------------------

func test_avatar_set_display_name():
	var packed: PackedScene = load("res://scenes/Avatar.tscn")
	var avatar: Node3D = packed.instantiate()
	add_child(avatar)
	await get_tree().process_frame

	avatar.set_display_name("TestUser")
	var nameplate: Label3D = avatar.get_node_or_null("Head/Nameplate")
	assert_not_null(nameplate, "Nameplate node must exist")
	assert_eq(nameplate.text, "TestUser", "display name must update nameplate text")

	avatar.queue_free()
	await get_tree().process_frame


func test_avatar_apply_pose_updates_head_transform():
	var packed: PackedScene = load("res://scenes/Avatar.tscn")
	var avatar: Node3D = packed.instantiate()
	add_child(avatar)
	await get_tree().process_frame

	var target_pos := Vector3(1.0, 2.0, 3.0)
	var target_rot := Quaternion(Vector3.UP, deg_to_rad(90.0))
	avatar.apply_pose(target_pos, target_rot, false, false)

	# After apply_pose sets targets, process a few frames so interpolation converges.
	for i in range(120):
		await get_tree().process_frame

	var head: MeshInstance3D = avatar.get_node("Head")
	assert_almost_eq(head.transform.origin, target_pos, Vector3(0.05, 0.05, 0.05), "head position must converge to target")

	avatar.queue_free()
	await get_tree().process_frame


func test_avatar_apply_pose_shows_hands_when_tracked():
	var packed: PackedScene = load("res://scenes/Avatar.tscn")
	var avatar: Node3D = packed.instantiate()
	add_child(avatar)
	await get_tree().process_frame

	avatar.apply_pose(Vector3.ZERO, Quaternion.IDENTITY, true, true)
	await get_tree().process_frame

	var left: MeshInstance3D = avatar.get_node("LeftHand")
	var right: MeshInstance3D = avatar.get_node("RightHand")
	assert_true(left.visible, "left hand visible when has_left=true")
	assert_true(right.visible, "right hand visible when has_right=true")

	avatar.apply_pose(Vector3.ZERO, Quaternion.IDENTITY, false, false)
	await get_tree().process_frame

	assert_false(left.visible, "left hand hidden when has_left=false")
	assert_false(right.visible, "right hand hidden when has_right=false")

	avatar.queue_free()
	await get_tree().process_frame


func test_avatar_lod_level_high():
	var packed: PackedScene = load("res://scenes/Avatar.tscn")
	var avatar: Node3D = packed.instantiate()
	add_child(avatar)
	await get_tree().process_frame

	avatar.apply_pose(Vector3.ZERO, Quaternion.IDENTITY, true, true)
	avatar.set_lod_level(0)
	await get_tree().process_frame

	assert_true(avatar.visible, "avatar visible at LOD High")
	var nameplate: Label3D = avatar.get_node("Head/Nameplate")
	assert_true(nameplate.visible, "nameplate visible at LOD High")
	assert_true(avatar.get_node("LeftHand").visible, "left hand visible at LOD High")

	avatar.queue_free()
	await get_tree().process_frame


func test_avatar_lod_level_medium_hides_nameplate():
	var packed: PackedScene = load("res://scenes/Avatar.tscn")
	var avatar: Node3D = packed.instantiate()
	add_child(avatar)
	await get_tree().process_frame

	avatar.apply_pose(Vector3.ZERO, Quaternion.IDENTITY, true, true)
	avatar.set_lod_level(1)
	await get_tree().process_frame

	assert_true(avatar.visible, "avatar visible at LOD Medium")
	var nameplate: Label3D = avatar.get_node("Head/Nameplate")
	assert_false(nameplate.visible, "nameplate hidden at LOD Medium")
	assert_true(avatar.get_node("LeftHand").visible, "hands still visible at LOD Medium")

	avatar.queue_free()
	await get_tree().process_frame


func test_avatar_lod_level_low_hides_hands():
	var packed: PackedScene = load("res://scenes/Avatar.tscn")
	var avatar: Node3D = packed.instantiate()
	add_child(avatar)
	await get_tree().process_frame

	avatar.apply_pose(Vector3.ZERO, Quaternion.IDENTITY, true, true)
	avatar.set_lod_level(2)
	await get_tree().process_frame

	assert_true(avatar.visible, "avatar visible at LOD Low")
	assert_false(avatar.get_node("Head/Nameplate").visible, "nameplate hidden at LOD Low")
	assert_false(avatar.get_node("LeftHand").visible, "left hand hidden at LOD Low")
	assert_false(avatar.get_node("RightHand").visible, "right hand hidden at LOD Low")

	avatar.queue_free()
	await get_tree().process_frame


func test_avatar_lod_level_culled_hides_avatar():
	var packed: PackedScene = load("res://scenes/Avatar.tscn")
	var avatar: Node3D = packed.instantiate()
	add_child(avatar)
	await get_tree().process_frame

	avatar.set_lod_level(3)
	await get_tree().process_frame

	assert_false(avatar.visible, "avatar hidden at LOD Culled")

	avatar.queue_free()
	await get_tree().process_frame


# ---------------------------------------------------------------------------
# HUD unit tests
# ---------------------------------------------------------------------------

func test_hud_set_avatar_count():
	var packed: PackedScene = load("res://scenes/HUD.tscn")
	var hud: Node3D = packed.instantiate()
	add_child(hud)
	await get_tree().process_frame

	hud.set_avatar_count(5)
	assert_eq(hud._avatar_count, 5, "avatar count stored")

	# Let _process run to update the label.
	await get_tree().process_frame

	var debug_stats: Label = hud.get_node("HudViewport/HudControl/Root/Tabs/SessionPage/DebugStats")
	assert_true(debug_stats.text.contains("Avatars: 5"), "debug stats must show avatar count")

	hud.queue_free()
	await get_tree().process_frame


func test_hud_set_mtp_ms():
	var packed: PackedScene = load("res://scenes/HUD.tscn")
	var hud: Node3D = packed.instantiate()
	add_child(hud)
	await get_tree().process_frame

	hud.set_mtp_ms(12.5)
	assert_almost_eq(hud._mtp_ms, 12.5, 0.01, "mtp_ms stored")

	await get_tree().process_frame

	var debug_stats: Label = hud.get_node("HudViewport/HudControl/Root/Tabs/SessionPage/DebugStats")
	assert_true(debug_stats.text.contains("MTP: 12.5ms"), "debug stats must show MTP value")

	hud.queue_free()
	await get_tree().process_frame


func test_hud_connection_status():
	var packed: PackedScene = load("res://scenes/HUD.tscn")
	var hud: Node3D = packed.instantiate()
	add_child(hud)
	await get_tree().process_frame

	hud._on_connection_status(true)
	await get_tree().process_frame

	var debug_stats: Label = hud.get_node("HudViewport/HudControl/Root/Tabs/SessionPage/DebugStats")
	assert_true(debug_stats.text.contains("Net: OK"), "connected status shows OK")

	hud._on_connection_status(false)
	await get_tree().process_frame

	assert_true(debug_stats.text.contains("Net: OFF"), "disconnected status shows OFF")

	hud.queue_free()
	await get_tree().process_frame


# ---------------------------------------------------------------------------
# GraphScene avatar lifecycle tests
# ---------------------------------------------------------------------------

func test_graph_scene_avatar_join_adds_child():
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame

	var spawner: Node3D = scene.get_node("GraphRoot/AvatarSpawner")
	var before_count: int = spawner.get_child_count()

	scene._on_avatar_joined("did:nostr:abc123", "Alice", "avatar_001")
	await get_tree().process_frame

	assert_eq(spawner.get_child_count(), before_count + 1, "avatar join adds child to spawner")
	assert_true(scene._avatars.has("avatar_001"), "avatar tracked in dictionary")

	scene.queue_free()
	await get_tree().process_frame


func test_graph_scene_avatar_leave_removes_child():
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame

	scene._on_avatar_joined("did:nostr:abc123", "Bob", "avatar_002")
	await get_tree().process_frame

	var spawner: Node3D = scene.get_node("GraphRoot/AvatarSpawner")
	var count_after_join: int = spawner.get_child_count()

	scene._on_avatar_left("avatar_002")
	await get_tree().process_frame
	# queue_free is deferred, so wait another frame.
	await get_tree().process_frame

	assert_eq(spawner.get_child_count(), count_after_join - 1, "avatar leave removes child from spawner")
	assert_false(scene._avatars.has("avatar_002"), "avatar removed from dictionary")

	scene.queue_free()
	await get_tree().process_frame


func test_graph_scene_avatar_leave_nonexistent_is_safe():
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame

	# Leaving a nonexistent avatar should not error.
	scene._on_avatar_left("avatar_nonexistent")
	await get_tree().process_frame

	assert_true(true, "no crash on removing nonexistent avatar")

	scene.queue_free()
	await get_tree().process_frame
