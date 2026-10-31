extends "res://addons/gut/test.gd"

# The visual extras (scripts/eye_candy.gd): the tier governor on synthetic load,
# what each tier draws and costs against the allocator's table, the comfort and
# low-cost switches, copy-on-write of the Environment, the ribbon frame, and a
# guard that nothing here reaches for a post-process.

const EyeCandy := preload("res://scripts/eye_candy.gd")
const EmulatedVolume := preload("res://scripts/emulated_volume.gd")
const VolumeScene := preload("res://scenes/EmulatedVolume.tscn")
const Environment_ := preload("res://scripts/spatial_environment.gd")
const GraphScene := preload("res://scripts/graph_scene.gd")


class FakePass:
	extends RefCounted
	var caps: Dictionary = {"extras_tier": 0}
	var demand: Array = []
	var applied: Array = []
	func set_extras_demand(tier: int, frames: int) -> void:
		demand.append([tier, frames])
	func set_extras_applied(tris: int, calls: int) -> void:
		applied.append([tris, calls])


class FakeBudget:
	extends RefCounted
	var layers: Dictionary = {}
	func tick(_delta: float, _client: RefCounted, l: Dictionary, _memory: Object, _measured: int, _bursts: bool = false, _volumes: Dictionary = {}) -> bool:
		layers = l
		return false


class FakeSource:
	extends RefCounted
	var counts: Dictionary = {}
	func demand() -> Dictionary:
		return {"nodes": 0, "edges": 0}


class FakeScene:
	extends Node3D
	var _budget: Object = null


func before_each() -> void:
	EmulatedVolume.ribbon_frames = false


func after_each() -> void:
	EmulatedVolume.ribbon_frames = false


func _scene_with_env() -> FakeScene:
	var s := FakeScene.new()
	var we := WorldEnvironment.new()
	we.name = "WorldEnvironment"
	var env := Environment.new()
	env.background_mode = Environment.BG_SKY
	env.sky = Sky.new()
	we.environment = env
	s.add_child(we)
	add_child_autofree(s)
	return s


func _candy(scene: Node) -> Node3D:
	var c: Node3D = EyeCandy.new()
	scene.add_child(c)
	c.setup(scene)
	return c


# ── governor ──

func _run(g, ms_for_second: Callable, seconds: int) -> void:
	for sec: int in seconds:
		var ms: float = ms_for_second.call(sec)
		for i: int in 90:
			g.feed(ms, 1.0 / 90.0)


func test_governor_climbs_one_tier_per_hold_on_a_quiet_gpu() -> void:
	var g := EyeCandy.Governor.gpu_mode()
	assert_eq(g.tier, 1, "starts on the free tier")
	_run(g, func(_s): return 3.0, 7)
	assert_eq(g.tier, 1, "not before the hold")
	_run(g, func(_s): return 3.0, 2)
	assert_eq(g.tier, 2)
	_run(g, func(_s): return 3.0, 80)
	assert_eq(g.tier, 4, "stops at the top tier")


func test_governor_hold_outlasts_the_budget_pass_peak_hold() -> void:
	var g := EyeCandy.Governor.gpu_mode()
	var FrameBudgetPass := preload("res://scripts/frame_budget_pass.gd")
	assert_gt(g.up_hold_sec, FrameBudgetPass.OTHER_PEAK_SEC, "a new tier settles in the budget before the next step is judged")
	assert_gt(EyeCandy.Governor.frame_mode(11.1).up_hold_sec, FrameBudgetPass.OTHER_PEAK_SEC)


func test_governor_steps_down_at_once_on_a_loaded_window() -> void:
	var g := EyeCandy.Governor.gpu_mode()
	g.tier = 3
	_run(g, func(_s): return 9.5, 1)
	assert_eq(g.tier, 2, "p95 over the limit: one window is enough")
	assert_eq(g.backoff, 2.0, "and the next climb waits twice as long")
	_run(g, func(_s): return 9.5, 5)
	assert_eq(g.tier, 1, "keeps stepping while it stays over, down to the free sky")
	_run(g, func(_s): return 9.5, 3)
	assert_eq(g.tier, 1, "and no further")


func test_a_loaded_governor_never_goes_below_the_sky() -> void:
	# The sky replaces the ProceduralSky draw: it costs nothing, so dropping it
	# frees nothing and swaps the whole background in one frame.
	for g in [EyeCandy.Governor.gpu_mode(), EyeCandy.Governor.frame_mode(1000.0 / 90.0)]:
		assert_eq(g.min_tier, EyeCandy.TIER_SKY)
		g.tier = g.max_tier
		_run(g, func(_s): return 40.0, 30)
		assert_eq(g.tier, EyeCandy.TIER_SKY, "heavily loaded for 30 s")
		assert_gt(int(g.steps["down"]), 0)
		var downs: int = int(g.steps["down"])
		_run(g, func(_s): return 40.0, 10)
		assert_eq(int(g.steps["down"]), downs, "no step is reported below the floor")
		assert_eq(g.tier, EyeCandy.TIER_SKY)
	# Low cost still takes the sky away: that is the controller's, not the governor's.
	var scene := _scene_with_env()
	var c := _candy(scene)
	c.set_comfort(true, true)
	assert_eq(c.wanted_tier(), 0)


func test_governor_does_not_flap_inside_the_hysteresis_band() -> void:
	var g := EyeCandy.Governor.gpu_mode()
	g.tier = 2
	# 6.5 ms is above the climb limit (5.5) and below the drop limit (8): hold.
	_run(g, func(_s): return 6.5, 120)
	assert_eq(g.tier, 2)
	assert_eq(int(g.steps["up"]) + int(g.steps["down"]), 0)
	# One spike in a window of quiet samples is not a missed window (p95).
	var spiky := EyeCandy.Governor.gpu_mode()
	spiky.tier = 2
	for i: int in 90 * 20:
		spiky.feed(30.0 if i % 90 == 0 else 3.0, 1.0 / 90.0)
	assert_ne(int(spiky.steps["down"]), 20, "a single slow sample a second is under p95")
	assert_eq(int(spiky.steps["down"]), 0)


func test_governor_backs_off_after_a_reversal() -> void:
	var g := EyeCandy.Governor.gpu_mode()
	# Quiet for 9 s then loaded, repeating: each failed climb doubles the hold.
	for cycle: int in 6:
		_run(g, func(_s): return 3.0, 9 * int(g.backoff))
		_run(g, func(_s): return 12.0, 1)
	assert_lte(g.backoff, EyeCandy.Governor.BACKOFF_MAX)
	assert_gte(g.backoff, 4.0, "repeated reversals lengthen the hold")
	assert_gt(int(g.steps["up"]), 0, "it did climb")
	assert_gt(int(g.steps["down"]), 0, "and was pushed back")


func test_frame_time_mode_is_conservative_and_capped() -> void:
	var g := EyeCandy.Governor.frame_mode(1000.0 / 90.0)
	assert_eq(g.max_tier, 2, "without a GPU timer the climb stops at the ribbon frames")
	_run(g, func(_s): return 11.2, 200)
	assert_eq(g.tier, 2)
	_run(g, func(_s): return 14.0, 1)
	assert_eq(g.tier, 1, "a missed-frame window steps down")
	var t := g.tier
	_run(g, func(_s): return 11.2, 25)
	assert_eq(g.tier, t, "and the climb back is held for 20 s x the back-off")


# ── tiers, cost and comfort ──

func test_tiers_draw_what_the_allocator_table_says() -> void:
	# Against the BUILT scene, not against the table itself: two real volume
	# frames, the real mote MultiMesh, the real sky. applied_cost() reads the
	# table, so it is not the oracle here.
	assert_true(ClassDB.class_exists("FrameBudget"), "gdext loaded")
	var scene := _scene_with_env()
	var vols: Array[Node3D] = []
	for i: int in 2:
		var v: Node3D = VolumeScene.instantiate()
		v.volume_id = StringName("cost%d" % i)
		scene.add_child(v)
		vols.append(v)
	var c := _candy(scene)
	c.set_comfort(true, false)
	assert_eq(c._visible_frames(), 2)
	var fb: Object = FrameBudget.new()
	var motes: MultiMeshInstance3D = c.get_node("AmbientMotes")
	var mote_tris: int = motes.multimesh.mesh.surface_get_array_len(0) / 3
	var line_calls: int = 0
	for v: Node3D in vols:
		line_calls += v.overhead_draw_calls()
	assert_eq(line_calls, 2, "each frame is one draw call as lines")
	for tier: int in 5:
		c.apply_tier(tier)
		assert_eq(c.applied_tier, tier)
		var built_tris: int = motes.multimesh.visible_instance_count * mote_tris if motes.visible else 0
		var frame_calls: int = 0
		for v: Node3D in vols:
			if v.frame.visible:
				built_tris += v.frame_triangles()
				frame_calls += 1
		# the extras' own calls: the motes' MultiMesh. The frames' calls are the
		# scene's (overhead_draw_calls), the same with lines or ribbons.
		var built_calls: int = 1 if motes.visible else 0
		assert_eq(frame_calls, 2, "tier %d: the frames are still one call each" % tier)
		var want: PackedInt32Array = fb.extras_cost(tier, 2)
		assert_eq(Vector2i(built_tris, built_calls), Vector2i(want[0], want[1]), "built scene against the table at tier %d" % tier)
		assert_eq(c.applied_cost(), Vector2i(want[0], want[1]))
		var n: int = 0 if tier < 3 else (512 if tier == 4 else 128)
		assert_eq(motes.visible, n > 0)
		assert_eq(motes.multimesh.visible_instance_count, n, "motes at tier %d" % tier)
		assert_eq(EmulatedVolume.ribbon_frames, tier >= 2)
		var env: Environment = scene.get_node("WorldEnvironment").environment
		assert_eq(env.sky == c._candy_sky, tier >= 1, "sky at tier %d" % tier)
		var total_overhead: int = 0
		for v: Node3D in vols:
			total_overhead += v.overhead_draw_calls()
		assert_eq(total_overhead, line_calls, "ribbons add no draw call over the lines")


func test_the_fallback_table_equals_the_allocators() -> void:
	assert_true(ClassDB.class_exists("FrameBudget"), "gdext loaded")
	var fb: Object = FrameBudget.new()
	for tier: int in 5:
		for frames: int in 10:
			var want: PackedInt32Array = fb.extras_cost(tier, frames)
			assert_eq(EyeCandy.fallback_cost(tier, frames), Vector2i(want[0], want[1]), "tier %d, %d frames" % [tier, frames])
	# And the instance without the library takes that branch.
	var c: Node3D = autofree(EyeCandy.new())
	c._fb = null
	assert_eq(c.cost_of(2, 3), EyeCandy.fallback_cost(2, 3))


func test_the_cost_table_object_is_made_once() -> void:
	var scene := _scene_with_env()
	var c := _candy(scene)
	var fb: Object = c._fb
	assert_not_null(fb)
	c.apply_tier(3)
	c.applied_cost()
	c.cost_of(4, 2)
	assert_same(c._fb, fb, "one FrameBudget for every lookup")


func test_the_probe_picks_gpu_time_only_on_a_majority_of_nonzero_readings() -> void:
	var c: Node3D = autofree(EyeCandy.new())
	c._probe_frames = EyeCandy.PROBE_FRAMES
	c._probe_nonzero = 1
	c._finish_probe()
	assert_eq(c.signal_mode, "frame", "one stray reading in 120 is not a timer")
	c._probe_nonzero = EyeCandy.PROBE_FRAMES / 2
	c._finish_probe()
	assert_eq(c.signal_mode, "frame", "exactly half is not a majority")
	c._probe_nonzero = EyeCandy.PROBE_FRAMES / 2 + 1
	c._finish_probe()
	assert_eq(c.signal_mode, "gpu")
	assert_eq(c._governor.max_tier, 4)
	assert_lte(c._governor.tier, EyeCandy.TIER_SKY)


func test_a_gpu_timer_that_goes_quiet_falls_back_to_frame_time() -> void:
	var c: Node3D = autofree(EyeCandy.new())
	c.signal_mode = "gpu"
	c._governor = EyeCandy.Governor.gpu_mode()
	c._governor.tier = 4
	for i: int in EyeCandy.GPU_ZERO_FALLBACK_FRAMES:
		c._feed_gpu(0.0, 1.0 / 90.0)
	assert_eq(c.signal_mode, "gpu", "not yet: 180 zeros in a row")
	assert_eq(c._governor.tier, 4, "and a zero is never fed as an idle GPU")
	assert_eq(int(c._governor.steps["up"]) + int(c._governor.steps["down"]), 0)
	c._feed_gpu(0.0, 1.0 / 90.0)
	assert_eq(c.signal_mode, "frame", "the 181st zero: the timer has stopped reporting")
	assert_eq(c._governor.max_tier, 2, "frame time vouches for the ribbons at most")
	assert_eq(c._governor.tier, 2, "tier 4 is cut to 2")
	# A reading in between resets the run.
	var d: Node3D = autofree(EyeCandy.new())
	d.signal_mode = "gpu"
	d._governor = EyeCandy.Governor.gpu_mode()
	d._governor.tier = 3
	for round: int in 3:
		for i: int in 150:
			d._feed_gpu(0.0, 1.0 / 90.0)
		d._feed_gpu(4.0, 1.0 / 90.0)
	assert_eq(d.signal_mode, "gpu", "a live reading restarts the count")
	assert_eq(d._governor.tier, 3)


func test_the_scene_counts_the_volume_frames_draw_calls_at_tier_0() -> void:
	# Before the hook, _tick_frame_budget summed nodes, edges, agents, links and
	# hulls only; the frames' calls were in the measured total but not in
	# layers.draw_calls, at tiers 0 and 1.
	var gs: Node3D = autofree(GraphScene.new())
	var budget := FakeBudget.new()
	gs._budget = budget
	gs._binary_client = RefCounted.new()
	var vols: Array[Node3D] = []
	for i: int in 3:
		var v: Node3D = VolumeScene.instantiate()
		v.volume_id = StringName("tick%d" % i)
		add_child_autofree(v)
		vols.append(v)
		gs._volumes[v.volume_id] = v
		gs._sources[v.volume_id] = FakeSource.new()
	assert_false(EmulatedVolume.ribbon_frames, "tier 0: line frames")
	gs._tick_frame_budget(0.016)
	assert_eq(int(budget.layers["draw_calls"]), 3, "three frames, three calls, at tier 0")
	EmulatedVolume.set_ribbon_frames(get_tree(), true)
	gs._tick_frame_budget(0.016)
	assert_eq(int(budget.layers["draw_calls"]), 3, "ribbons reuse the same call: the extras add none")
	vols[0].visible = false
	gs._tick_frame_budget(0.016)
	assert_eq(int(budget.layers["draw_calls"]), 2, "a hidden volume's frame is not drawn")


func test_the_mote_mesh_costs_what_the_table_charges() -> void:
	var scene := _scene_with_env()
	var c := _candy(scene)
	var motes: MultiMeshInstance3D = c.get_node("AmbientMotes")
	var mesh: ArrayMesh = motes.multimesh.mesh
	assert_eq(mesh.get_surface_count(), 1, "one surface, one draw call")
	assert_eq(mesh.surface_get_array_len(0), 3, "one triangle a mote")
	var spec: Dictionary = FrameBudget.new().extras_spec()
	assert_eq(motes.multimesh.instance_count, int(spec["motes_high"]))
	assert_eq(int(spec["mote_tris"]), 1)


func test_the_ribbon_frame_costs_what_the_table_charges() -> void:
	EmulatedVolume.ribbon_frames = true
	var v: Node3D = VolumeScene.instantiate()
	v.volume_id = &"knowledge"
	add_child_autofree(v)
	assert_eq(v.frame.mesh.get_surface_count(), 1, "one draw call")
	var idx: PackedInt32Array = v.frame.mesh.surface_get_arrays(0)[Mesh.ARRAY_INDEX]
	assert_eq(idx.size() / 3, int(FrameBudget.new().extras_spec()["frame_tris"]), "24 triangles")
	assert_eq(v.frame_triangles(), 24)
	assert_eq(v.frame.mesh.get_surface_count(), 1)
	assert_true(v.frame.material_override is ShaderMaterial)


func test_ribbon_frame_keeps_the_focus_colour_state() -> void:
	EmulatedVolume.ribbon_frames = true
	var v: Node3D = VolumeScene.instantiate()
	v.volume_id = &"knowledge"
	add_child_autofree(v)
	var idle: Color = v.frame.material_override.get_shader_parameter("frame_color")
	v.set_focused(true)
	var focus: Color = v.frame.material_override.get_shader_parameter("frame_color")
	assert_ne(idle, focus, "focused frames change colour")
	assert_eq(Color(focus.r, focus.g, focus.b), EmulatedVolume.FRAME_FOCUS_COLOUR)
	v.set_focused(false)
	assert_eq(v.frame.material_override.get_shader_parameter("frame_color"), idle)


func test_frames_follow_the_style_switch_and_new_volumes_inherit_it() -> void:
	var a: Node3D = VolumeScene.instantiate()
	a.volume_id = &"a"
	add_child_autofree(a)
	assert_eq(a.frame.mesh.get_surface_count(), 1)
	assert_eq(a.frame_triangles(), 0, "lines by default")
	EmulatedVolume.set_ribbon_frames(get_tree(), true)
	assert_eq(a.frame_triangles(), 24, "existing volumes restyle")
	var b: Node3D = VolumeScene.instantiate()
	b.volume_id = &"b"
	add_child_autofree(b)
	assert_eq(b.frame_triangles(), 24, "a volume made later takes the current style")
	EmulatedVolume.set_ribbon_frames(get_tree(), false)
	assert_eq(a.frame_triangles(), 0)
	assert_eq(b.frame_triangles(), 0)
	assert_eq(a.frame.mesh.surface_get_array_len(0), 24, "the line mesh again: 12 edges")


func test_set_frame_hidden_hides_the_frame_whatever_the_state() -> void:
	var v: Node3D = VolumeScene.instantiate()
	v.volume_id = &"knowledge"
	add_child_autofree(v)
	assert_true(v.frame.visible)
	v.set_frame_hidden(true)
	assert_false(v.frame.visible)
	assert_true(v.is_frame_hidden())
	v.set_focused(true)
	assert_false(v.frame.visible, "a focus change does not bring it back")
	assert_eq(v.overhead_draw_calls() , 0 if not v._handle_marks.visible else 1)
	v.set_frame_hidden(false)
	assert_true(v.frame.visible)


func test_low_cost_turns_every_extra_off() -> void:
	var scene := _scene_with_env()
	var c := _candy(scene)
	c.set_comfort(true, false)
	c.apply_tier(4)
	assert_eq(c.applied_tier, 4)
	c.set_comfort(true, true)
	assert_eq(c.applied_tier, 0)
	assert_eq(c.wanted_tier(), 0)
	assert_false((c.get_node("AmbientMotes") as MultiMeshInstance3D).visible)
	assert_false(EmulatedVolume.ribbon_frames, "frames are lines again")
	var env: Environment = scene.get_node("WorldEnvironment").environment
	assert_ne(env.sky, c._candy_sky, "the scene's own sky is back")
	c.apply_tier(4)
	assert_eq(c.applied_tier, 0, "and nothing re-enables them while low cost holds")
	c.set_comfort(true, false)
	assert_eq(c.wanted_tier() >= 0, true)


func test_reduced_motion_holds_the_motes_still() -> void:
	var scene := _scene_with_env()
	var c := _candy(scene)
	c.set_comfort(true, false)
	assert_eq(float(c._mote_material.get_shader_parameter("motion")), 0.0, "reduced motion: static")
	c.set_comfort(false, false)
	assert_eq(float(c._mote_material.get_shader_parameter("motion")), 1.0)
	c.set_comfort(true, false)
	assert_eq(float(c._mote_material.get_shader_parameter("motion")), 0.0)
	var fresh: Node3D = autofree(EyeCandy.new())
	assert_eq(fresh._reduced_motion, true, "reduced motion is the default")


func test_the_scene_environment_resource_is_never_mutated() -> void:
	var s := FakeScene.new()
	var we := WorldEnvironment.new()
	we.name = "WorldEnvironment"
	var shared := Environment.new()
	var sky := Sky.new()
	shared.sky = sky
	we.environment = shared
	s.add_child(we)
	add_child_autofree(s)
	var c := _candy(s)
	c.apply_tier(1)
	assert_eq(shared.sky, sky, "the shared resource keeps its sky")
	assert_ne(we.environment, shared, "the node holds a scene-local copy")
	assert_ne(we.environment.sky, sky)
	assert_false(we.environment.glow_enabled, "no glow, ever (it blanks the second eye)")


func test_the_extras_request_a_tier_and_are_bounded_by_the_grant() -> void:
	var scene := _scene_with_env()
	var pass_obj := FakePass.new()
	scene._budget = pass_obj
	var c := _candy(scene)
	c.set_comfort(true, false)
	c._forced_tier = 4
	c._apply_accum = 1.0
	c._process(0.016)
	assert_eq(pass_obj.demand.back()[0], 4, "the request goes to the pass")
	assert_eq(c.applied_tier, 0, "nothing is drawn before the pass grants it (fail closed)")
	pass_obj.caps = {"extras_tier": 2}
	c._apply_accum = 1.0
	c._process(0.016)
	assert_eq(c.applied_tier, 2)
	pass_obj.caps = {"extras_tier": 4}
	c._forced_tier = 1
	c._apply_accum = 1.0
	c._process(0.016)
	assert_eq(c.applied_tier, 1, "never above the request")
	assert_eq(pass_obj.applied.back(), [0, 0], "what is drawn is reported back so it is not charged twice")


func test_low_cost_withdraws_the_request_at_once_so_no_allowance_is_held() -> void:
	# ADR-2167 amended: the allocator holds back the requested tier's cost, so a
	# stale request would take graph gems for up to one pass tick after low cost.
	var scene := _scene_with_env()
	var pass_obj := FakePass.new()
	scene._budget = pass_obj
	var c := _candy(scene)
	c.set_comfort(true, false)
	c._forced_tier = 4
	c._apply_accum = 1.0
	c._process(0.016)
	assert_eq(pass_obj.demand.back()[0], 4)
	c.set_comfort(true, true)
	assert_eq(pass_obj.demand.back()[0], 0, "withdrawn in set_comfort, not at the next tick")
	c._apply_accum = 1.0
	c._process(0.016)
	assert_eq(pass_obj.demand.back()[0], 0, "and it stays 0 under low cost with a tier pinned")


## Source with its comments removed, so a banned name in prose does not count.
func _code_only(src: String, marker: String) -> String:
	var out := PackedStringArray()
	for line: String in src.split("\n"):
		var i := line.find(marker)
		out.append(line if i < 0 else line.substr(0, i))
	return "\n".join(out)


func test_no_extra_reaches_for_a_post_process_or_a_screen_read() -> void:
	for path: String in ["res://materials/frame_ribbon.gdshader", "res://materials/ambient_mote.gdshader", "res://materials/eye_candy_sky.gdshader"]:
		var src: String = _code_only(FileAccess.get_file_as_string(path), "//")
		assert_ne(src, "", path)
		for banned: String in ["SCREEN_TEXTURE", "DEPTH_TEXTURE", "hint_screen_texture", "hint_depth_texture", "NORMAL_ROUGHNESS_TEXTURE"]:
			assert_false(src.contains(banned), "%s must not use %s" % [path, banned])
	var script_src: String = _code_only(FileAccess.get_file_as_string("res://scripts/eye_candy.gd"), "#")
	for banned: String in ["SubViewport", "glow_enabled = true", "msaa_3d", "Compositor"]:
		assert_false(script_src.contains(banned), "eye_candy.gd must not use %s" % banned)
	# Static where it must be: the sky never reads TIME.
	assert_false(_code_only(FileAccess.get_file_as_string("res://materials/eye_candy_sky.gdshader"), "//").contains("TIME"))


func test_spatial_environment_owns_the_controller_and_forwards_comfort() -> void:
	var s := FakeScene.new()
	var we := WorldEnvironment.new()
	we.name = "WorldEnvironment"
	we.environment = Environment.new()
	s.add_child(we)
	var env_node: Node3D = Environment_.new()
	env_node.name = "SpatialEnvironment"
	s.add_child(env_node)
	add_child_autofree(s)
	var c: Node3D = env_node.get_eye_candy()
	assert_not_null(c, "EyeCandy lives under SpatialEnvironment: no graph_scene edit")
	assert_eq(c.get_parent(), env_node)
	env_node.set_visual_comfort(true, true)
	assert_eq(c.wanted_tier(), 0)
	env_node.set_visual_comfort(true, false)
	assert_gte(c.wanted_tier(), 0)
	assert_true(bool(env_node.get_visual_comfort()["reduced_motion"]))
