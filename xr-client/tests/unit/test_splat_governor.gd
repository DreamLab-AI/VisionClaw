extends "res://addons/gut/test.gd"

# ADR-2170 decision 7: the splat tier governor steps by the ADR-2167 rule. Its stepping
# logic is Rust (rust/src/splat_governor.rs, unit-tested there); this suite feeds one
# series to it and to eye_candy.gd's GDScript `Governor`, parameterised for the splat
# tiers, and requires the same tier after every sample, so the two cannot drift apart.

const EyeCandy := preload("res://scripts/eye_candy.gd")
# Parity runs at 8 Hz so every window and hold is a sum of exactly representable steps
# (0.125 s): the GDScript governor accumulates in 64-bit floats and the Rust one in 32-bit,
# and 1/90 s sums land a hair either side of the thresholds in each. The 90 Hz behaviour is
# covered by the Rust unit tests.
const DT := 0.125
const HZ := 90.0


func _gd_gpu() -> Object:
	var g: Object = EyeCandy.Governor.gpu_mode()
	g.min_tier = 1 # the splat floor (see splat_governor.rs FLOOR_TIER)
	g.max_tier = 3
	g.tier = 1
	return g


func _gd_frame(budget_ms: float) -> Object:
	var g: Object = EyeCandy.Governor.frame_mode(budget_ms)
	g.min_tier = 1
	g.tier = 1
	return g


func _rust_gpu() -> Object:
	assert_true(ClassDB.class_exists("SplatGovernor"), "gdext library loaded")
	var g: Object = ClassDB.instantiate("SplatGovernor")
	g.use_gpu()
	return g


func _rust_frame(budget_ms: float) -> Object:
	var g: Object = ClassDB.instantiate("SplatGovernor")
	g.use_frame(budget_ms)
	return g


# A series of (ms, seconds) segments.
func _same_tier_throughout(gd: Object, rust: Object, segments: Array, label: String) -> int:
	var samples := 0
	for seg: Array in segments:
		for _i: int in int(round(float(seg[1]) / DT)):
			gd.feed(float(seg[0]), DT)
			rust.feed(float(seg[0]), DT, true)
			samples += 1
			if gd.tier != int(rust.tier()):
				fail_test("%s: sample %d (%.1f ms) GDScript tier %d, Rust tier %d" % [label, samples, float(seg[0]), gd.tier, int(rust.tier())])
				return samples
	return samples


func test_the_gpu_governor_matches_the_gdscript_governor() -> void:
	var series: Array = [
		[[5.0, 40.0]],                                     # calm: climbs 1 -> 2 -> 3
		[[9.0, 5.0], [5.0, 40.0], [9.0, 3.0], [5.0, 60.0]], # drops, doubled hold-off, recovery
		[[7.0, 30.0], [4.0, 20.0], [7.5, 10.0], [4.0, 90.0]],   # in between, then calm
		[[12.0, 6.0], [4.0, 200.0], [9.0, 8.0], [4.0, 200.0]], # to the floor and back, backoff to its cap and decay
		[[5.4, 30.0], [5.5, 30.0], [8.0, 10.0], [8.1, 3.0]],    # exactly on the thresholds
	]
	for i: int in series.size():
		var n := _same_tier_throughout(_gd_gpu(), _rust_gpu(), series[i], "gpu series %d" % i)
		assert_gt(n, 0)


func test_the_frame_time_governor_matches_the_gdscript_governor() -> void:
	var budget := 1000.0 / 90.0
	var series: Array = [
		[[budget, 60.0]],
		[[budget * 1.2, 5.0], [budget, 80.0], [budget * 1.2, 2.0], [budget, 80.0]],
		[[budget * 1.1, 30.0], [budget * 1.0, 25.0]],
	]
	for i: int in series.size():
		var n := _same_tier_throughout(_gd_frame(budget), _rust_frame(budget), series[i], "frame series %d" % i)
		assert_gt(n, 0)
	assert_eq(int(_rust_frame(budget).max_tier()), 2, "the frame fallback's ceiling is tier 2")


func test_it_climbs_only_while_a_splat_volume_is_entered() -> void:
	var g := _rust_gpu()
	for _i: int in int(30.0 * HZ):
		g.feed(2.0, 1.0 / HZ, false)
	assert_eq(int(g.tier()), 1, "thirty calm seconds, not entered: no climb")
	for _i: int in int(9.0 * HZ):
		g.feed(2.0, 1.0 / HZ, true)
	assert_eq(int(g.tier()), 2, "entered: the 8 s hold, then one step")


func test_low_cost_is_tier_zero_in_the_same_call() -> void:
	var g := _rust_gpu()
	assert_eq(int(g.tier()), 1)
	assert_true(g.set_low_cost(true))
	assert_eq(int(g.tier()), 0)
	assert_true(g.set_low_cost(false))
	assert_eq(int(g.tier()), 1)


func test_switching_to_frame_time_keeps_the_tier_capped_at_two() -> void:
	var g := _rust_gpu()
	for _i: int in int(18.0 * HZ):
		g.feed(2.0, 1.0 / HZ, true)
	assert_eq(int(g.tier()), 3)
	g.switch_to_frame(1000.0 / 90.0)
	assert_eq(String(g.signal()), "frame")
	assert_eq(int(g.tier()), 2)
