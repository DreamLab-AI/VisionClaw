extends "res://addons/gut/test.gd"

# M3 geometric agent embodiment tests (PRD-023 WP-9, ADR-130 Decision 4).
# Exercises the visible-half GDScript: DID badge rendering, the LOD feature-mask
# drop order, activity-driven state, and the gaze-cone orientation basis. The
# state machine and thresholds are Rust-side (avatar_state.rs) and covered by
# cargo tests; these assert the scene wiring.

const FEAT_BADGE := 1
const FEAT_CONE := 2
const FEAT_CORE_MESH := 4
const FEAT_CORE_BILLBOARD := 8

const COLOR_VERIFIED := Color(0.75, 1.0, 0.82)
const COLOR_UNVERIFIED := Color(1.0, 0.72, 0.28)


func _make_agent() -> Node3D:
	var packed: PackedScene = load("res://scenes/AgentAvatar.tscn")
	var agent: Node3D = packed.instantiate()
	add_child(agent)
	await get_tree().process_frame
	return agent


func test_set_identity_renders_short_did_verified():
	var agent: Node3D = await _make_agent()
	var did := "did:nostr:" + "a".repeat(56) + "deadbeef"
	agent.set_avatar_identity("Planner", did, true)
	var badge: Label3D = agent.get_node("Badge")
	assert_true(badge.text.contains("Planner"), "badge shows the agent name")
	assert_true(badge.text.contains("…deadbeef"), "badge shows the …last8 short DID")
	assert_true(badge.text.contains("✓"), "verified badge carries the check mark")
	assert_eq(badge.modulate, COLOR_VERIFIED, "verified badge colour")
	agent.queue_free()
	await get_tree().process_frame


func test_set_identity_unverified_is_visually_distinct():
	var agent: Node3D = await _make_agent()
	agent.set_avatar_identity("Rogue", "did:nostr:" + "b".repeat(64), false)
	var badge: Label3D = agent.get_node("Badge")
	assert_true(badge.text.contains("?"), "unverified badge carries the query mark")
	assert_eq(badge.modulate, COLOR_UNVERIFIED, "unverified badge is amber, not the verified colour")
	assert_ne(badge.modulate, COLOR_VERIFIED, "unverified must never read as verified")
	agent.queue_free()
	await get_tree().process_frame


func test_empty_did_reads_unverified_never_fabricated():
	var agent: Node3D = await _make_agent()
	agent.set_avatar_identity("Anon", "", false)
	var badge: Label3D = agent.get_node("Badge")
	assert_true(badge.text.contains("unverified"), "an absent DID reads unverified, not a fake id")
	agent.queue_free()
	await get_tree().process_frame


func test_short_did_helper_variants():
	var agent: Node3D = await _make_agent()
	assert_eq(agent._short_did(""), "unverified", "empty → unverified")
	assert_eq(agent._short_did("did:nostr:0123456789abcdef"), "…89abcdef", "…last8 of the pubkey")
	assert_eq(agent._short_did("short"), "…short", "sub-8 pubkey kept whole")
	agent.queue_free()
	await get_tree().process_frame


func test_feature_mask_high_shows_everything():
	var agent: Node3D = await _make_agent()
	agent.set_feature_mask(FEAT_BADGE | FEAT_CONE | FEAT_CORE_MESH)
	assert_true(agent.get_node("Badge").visible, "badge visible at High")
	assert_true(agent.get_node("GazeCone").visible, "cone visible at High")
	assert_true(agent.get_node("Core").visible, "core mesh visible at High")
	assert_false(agent.get_node("CoreBillboard").visible, "billboard off at High")
	assert_true(agent.visible, "avatar visible at High")
	agent.queue_free()
	await get_tree().process_frame


func test_feature_mask_medium_drops_badge_first():
	var agent: Node3D = await _make_agent()
	agent.set_feature_mask(FEAT_CONE | FEAT_CORE_MESH)
	assert_false(agent.get_node("Badge").visible, "badge drops first at Medium")
	assert_true(agent.get_node("GazeCone").visible, "cone survives Medium")
	assert_true(agent.get_node("Core").visible, "core survives Medium")
	agent.queue_free()
	await get_tree().process_frame


func test_feature_mask_low_billboards_core():
	var agent: Node3D = await _make_agent()
	agent.set_feature_mask(FEAT_CORE_BILLBOARD)
	assert_false(agent.get_node("GazeCone").visible, "cone drops at Low")
	assert_false(agent.get_node("Core").visible, "core mesh off at Low")
	assert_true(agent.get_node("CoreBillboard").visible, "core billboards at Low")
	agent.queue_free()
	await get_tree().process_frame


func test_feature_mask_culled_hides_avatar():
	var agent: Node3D = await _make_agent()
	agent.set_feature_mask(0)
	assert_false(agent.visible, "avatar hidden when the feature mask is empty")
	agent.queue_free()
	await get_tree().process_frame


func test_apply_signal_drives_activity_state():
	var agent: Node3D = await _make_agent()
	# TaskStarted (ordinal 0) moves Idle → Working (activity 1). The Rust node
	# emits activity_changed synchronously; agent_avatar caches it.
	agent.apply_signal(0)
	await get_tree().process_frame
	assert_eq(agent.activity(), 1, "TaskStarted advances the avatar to Working")
	agent.queue_free()
	await get_tree().process_frame


# Work-layer embodiment (ADR-2109 / plan §2.2): role frame + accent, pointer
# instead of the social gaze cone, badge letters, gated caption, whole-body alpha.
func test_work_identity_uses_pointer_role_frame_and_gated_caption() -> void:
	var agent: Node3D = await _make_agent()
	agent.set_work_identity("Reviewer")
	agent.set_role("reviewer")
	agent.set_feature_mask(FEAT_BADGE | FEAT_CONE | FEAT_CORE_MESH)
	await get_tree().process_frame
	assert_eq(agent.role(), "reviewer")
	var frame: MeshInstance3D = agent.get_node("RoleFrame")
	assert_not_null(frame.mesh, "role frame mesh assigned")
	assert_true(frame.visible, "frame shows with the core mesh")
	assert_false(agent.get_node("GazeCone").visible, "work layer hides the social gaze cone even at High LOD")
	var pointer: MeshInstance3D = agent.get_node("Pointer")
	assert_false(pointer.visible, "no aim → no pointer")
	agent.set_aim(Vector3(1, 0, 0))
	await get_tree().process_frame
	assert_true(pointer.visible, "aim → pointer shows")
	assert_almost_eq(pointer.transform.basis.y, Vector3(1, 0, 0), Vector3(0.001, 0.001, 0.001), "pointer +Y follows the aim")
	assert_gt(pointer.position.x, agent.CORE_RADIUS, "pointer sits outside the core")
	var badge: Label3D = agent.get_node("Badge")
	assert_false(badge.fixed_size, "world-size badge")
	assert_true(badge.text.begins_with("RE  Reviewer"), "badge letters + name")
	assert_false(badge.text.contains("unverified"), "work layer never shows a DID line")
	agent.set_task_caption("Reviewing: Access policy")
	assert_true(badge.text.contains("Reviewing"), "caption shown by default")
	agent.set_caption_visible(false)
	assert_false(badge.text.contains("Reviewing"), "caption hidden when not announcing/selected")
	agent.set_alpha(0.3)
	assert_almost_eq(agent.alpha(), 0.3, 0.001)
	assert_almost_eq(badge.modulate.a, 0.3, 0.001, "badge fades with the body")
	assert_almost_eq((frame.material_override as StandardMaterial3D).albedo_color.a, 0.3, 0.001, "frame fades with the body")
	agent.set_alpha(1.0)
	assert_eq((frame.material_override as StandardMaterial3D).transparency, BaseMaterial3D.TRANSPARENCY_DISABLED, "opaque frame while working")
	agent.queue_free()
	await get_tree().process_frame


func test_conversation_layer_keeps_gaze_cone_and_generic_frame() -> void:
	var agent: Node3D = await _make_agent()
	agent.set_avatar_identity("Planner", "did:nostr:" + "a".repeat(64), true)
	agent.set_feature_mask(FEAT_BADGE | FEAT_CONE | FEAT_CORE_MESH)
	assert_true(agent.get_node("GazeCone").visible, "conversation layer keeps the gaze cone")
	assert_false(agent.get_node("Pointer").visible, "no pointer outside the work layer")
	assert_eq(agent.role(), "generic")
	assert_not_null((agent.get_node("RoleFrame") as MeshInstance3D).mesh, "generic hoop present")
	agent.queue_free()
	await get_tree().process_frame


func test_gaze_cone_basis_aligns_y_with_gaze():
	var agent: Node3D = await _make_agent()
	var gaze := Vector3(1.0, 0.0, -1.0).normalized()
	var basis: Basis = agent._basis_from_y(gaze)
	# The cone mesh axis is +Y; the built basis must map +Y onto the gaze ray.
	assert_almost_eq(basis.y, gaze, Vector3(0.001, 0.001, 0.001), "basis +Y follows the gaze direction")
	# Orthonormal: axes unit length and mutually perpendicular.
	assert_almost_eq(basis.x.length(), 1.0, 0.001, "x axis unit length")
	assert_almost_eq(basis.z.length(), 1.0, 0.001, "z axis unit length")
	assert_almost_eq(basis.x.dot(basis.y), 0.0, 0.001, "x ⟂ y")
	assert_almost_eq(basis.y.dot(basis.z), 0.0, 0.001, "y ⟂ z")
	agent.queue_free()
	await get_tree().process_frame
