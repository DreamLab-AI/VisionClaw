extends "res://addons/gut/test.gd"
const BackendCaps := preload("res://scripts/backend_caps.gd")

# ADR-2143 A6: the per-backend capability table that gates server-bound HUD
# controls. Pure data; no scene, no gdext library needed.


func test_visionclaw_grants_everything() -> void:
	var caps: Dictionary = BackendCaps.for_mode("visionclaw")
	assert_eq(caps.size(), BackendCaps.ALL_KEYS.size(), "one flag per key")
	for key: String in BackendCaps.ALL_KEYS:
		assert_true(caps[key], "visionclaw grants %s" % key)
	assert_eq(BackendCaps.hidden_tabs(caps), [] as Array[String], "no tab hidden")
	assert_eq(BackendCaps.disabled_actions(caps), [] as Array[String], "no control greyed")


func test_codescape_grants_only_its_route_table() -> void:
	var caps: Dictionary = BackendCaps.for_mode("codescape")
	for key: String in BackendCaps.ALL_KEYS:
		assert_eq(bool(caps[key]), BackendCaps.CODESCAPE_SUPPORTED.has(key), "codescape flag for %s" % key)
	# Forces (all physics PUTs) and Swarm (demo) have no live control left, so
	# they are hidden; the mixed tabs stay and grey per control.
	assert_eq(BackendCaps.hidden_tabs(caps), ["forces", "swarm", "pins"] as Array[String])
	var greyed: Array[String] = BackendCaps.disabled_actions(caps)
	for action: String in ["reset_layout", "spread_plus", "flat_toggle", "fold_plus", "cycles_toggle", "layout_mode_cycle", "radial_dag", "memory_cloud_toggle", "toggle_demo", "unpin_all"]:
		assert_true(greyed.has(action), "%s greyed on codescape" % action)
		assert_false(BackendCaps.action_allowed(caps, action), "%s refused on codescape" % action)
	# Dynamic actions are matched by prefix.
	for action: String in ["forces_step:2:-1", "memory_hit:0", "memory_preset:3", "memory_typed:ns|hello"]:
		assert_false(BackendCaps.action_allowed(caps, action), "%s refused on codescape" % action)
	# Client-side controls are never in the table.
	for action: String in ["edges_plus", "node_size_plus", "color_mode_toggle", "hulls_cycle", "agent_offsets_toggle", "beat_tap", "type_toggle:knowledge:0"]:
		assert_false(greyed.has(action), "%s stays live on codescape" % action)
		assert_eq(BackendCaps.cap_for_action(action), "", "%s needs no capability" % action)
		assert_true(BackendCaps.action_allowed(caps, action))


func test_unknown_mode_is_treated_as_the_reference_server() -> void:
	var caps: Dictionary = BackendCaps.for_mode("something-new")
	for key: String in BackendCaps.ALL_KEYS:
		assert_true(caps[key], "unknown mode grants %s" % key)


func test_missing_key_is_granted_not_hidden() -> void:
	assert_true(BackendCaps.supports({}, BackendCaps.FOLD), "absent flag grants")
	assert_false(BackendCaps.supports({BackendCaps.FOLD: false}, BackendCaps.FOLD))
	assert_eq(BackendCaps.hidden_tabs({BackendCaps.DEMO: false}), ["swarm"] as Array[String])


func test_every_gated_action_names_a_known_capability() -> void:
	for action: String in BackendCaps.ACTION_CAPS:
		assert_true(BackendCaps.ALL_KEYS.has(BackendCaps.ACTION_CAPS[action]), "%s maps to a listed key" % action)
	for prefix: String in BackendCaps.ACTION_PREFIX_CAPS:
		assert_true(BackendCaps.ALL_KEYS.has(BackendCaps.ACTION_PREFIX_CAPS[prefix]), "%s maps to a listed key" % prefix)
	for tab: String in BackendCaps.TAB_CAPS:
		assert_true(BackendCaps.ALL_KEYS.has(BackendCaps.TAB_CAPS[tab]), "%s maps to a listed key" % tab)
