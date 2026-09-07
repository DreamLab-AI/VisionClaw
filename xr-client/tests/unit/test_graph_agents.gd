extends "res://addons/gut/test.gd"

# M3 agent lifecycle + M2 case routing in GraphScene (PRD-023 WP-9). Agents are
# distinct from human presence peers: spawned onto the proxemics arc, keyed by
# did:nostr, and carrying broker cases to the HUD intervention panel. The live
# 0x44 agent-presence wire and broker:new_case egress on the Godot socket are
# the named pending integration points; these drive the scene-side surface the
# wire will feed.


func _make_scene() -> Node3D:
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame
	return scene


func test_spawn_agent_adds_child_to_agent_spawner():
	var scene: Node3D = await _make_scene()
	var spawner: Node3D = scene.get_node("GraphRoot/AgentSpawner")
	var before := spawner.get_child_count()
	scene.spawn_agent("agent_1", "Planner", "did:nostr:" + "a".repeat(64), false)
	await get_tree().process_frame
	assert_eq(spawner.get_child_count(), before + 1, "spawning an agent adds a child to AgentSpawner")
	assert_true(scene._agents.has("agent_1"), "agent tracked by id")
	scene.queue_free()
	await get_tree().process_frame


func test_spawn_agent_is_idempotent():
	var scene: Node3D = await _make_scene()
	var spawner: Node3D = scene.get_node("GraphRoot/AgentSpawner")
	scene.spawn_agent("agent_dup", "A", "did:nostr:" + "b".repeat(64), false)
	await get_tree().process_frame
	var after_first := spawner.get_child_count()
	scene.spawn_agent("agent_dup", "A", "did:nostr:" + "b".repeat(64), false)
	await get_tree().process_frame
	assert_eq(spawner.get_child_count(), after_first, "re-spawning the same id is a no-op")
	scene.queue_free()
	await get_tree().process_frame


func test_despawn_agent_removes_child():
	var scene: Node3D = await _make_scene()
	var spawner: Node3D = scene.get_node("GraphRoot/AgentSpawner")
	scene.spawn_agent("agent_2", "Coder", "did:nostr:" + "c".repeat(64), false)
	await get_tree().process_frame
	var count_after_join := spawner.get_child_count()
	scene.despawn_agent("agent_2")
	await get_tree().process_frame
	await get_tree().process_frame  # queue_free is deferred
	assert_eq(spawner.get_child_count(), count_after_join - 1, "despawn removes the child")
	assert_false(scene._agents.has("agent_2"), "agent untracked after despawn")
	scene.queue_free()
	await get_tree().process_frame


func test_agent_case_publishes_count_and_state():
	var scene: Node3D = await _make_scene()
	scene.spawn_agent("agent_3", "Reviewer", "did:nostr:" + "d".repeat(64), false)
	await get_tree().process_frame
	scene.set_agent_case("agent_3", "case-9", "approve the enrichment")
	assert_true(scene._agent_cases.has("agent_3"), "case tracked against the agent")
	# The agent transitions toward awaiting-approval (a real Rust signal ordinal).
	var agent: Node3D = scene._agents["agent_3"]
	await get_tree().process_frame
	assert_eq(agent.activity(), 2, "an open case drives the agent to awaiting-approval")
	scene.clear_agent_case("agent_3")
	assert_false(scene._agent_cases.has("agent_3"), "clearing the case untracks it")
	scene.queue_free()
	await get_tree().process_frame


func test_selecting_an_agent_with_a_case_opens_the_panel():
	var scene: Node3D = await _make_scene()
	scene.spawn_agent("agent_4", "Judge", "did:nostr:" + "e".repeat(64), true)
	await get_tree().process_frame
	scene.set_agent_case("agent_4", "case-11", "merge decision")
	var agent: Node3D = scene._agents["agent_4"]
	var handle: int = agent.get_meta("handle")
	# Simulate the arbiter resolving this agent (the wire the Rust selection
	# signal drives on-device).
	scene._on_selection_made(handle, "did:nostr:" + "e".repeat(64), 0)
	await get_tree().process_frame
	var panel: PanelContainer = scene.hud.get_node("HudViewport/HudControl/InterventionPanel")
	assert_true(panel.visible, "selecting an agent with an open case opens the intervention panel")
	scene.queue_free()
	await get_tree().process_frame
