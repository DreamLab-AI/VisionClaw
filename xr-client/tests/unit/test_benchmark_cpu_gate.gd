extends "res://addons/gut/test.gd"
## The benchmark's CPU gate (perf/benchmark.gd cpu_gate): steady-state p99
## against the per-frame budgets, the warm-up window (first plan build) against
## its own explicit limits, so a load-time hitch still fails but never pollutes
## the steady-state budget.

const Bench := preload("res://perf/benchmark.gd")


func _series(n: int, steady: float, first: float) -> PackedFloat32Array:
	var a := PackedFloat32Array()
	a.resize(n)
	a.fill(steady)
	a[0] = first
	return a


func test_a_normal_first_build_passes_and_is_reported_separately() -> void:
	var g: Dictionary = Bench.cpu_gate(_series(2700, 0.5, 5.9), _series(2700, 1.0, 16.2))
	assert_true(g["pass_pack"])
	assert_true(g["pass_lod"])
	assert_true(g["pass_warmup"])
	assert_almost_eq(float(g["pack_cpu_p99"]), 0.5, 0.001, "steady state excludes the build")
	assert_almost_eq(float(g["warmup_pack_cpu_max"]), 5.9, 0.001)
	assert_almost_eq(float(g["warmup_lod_cpu_max"]), 16.2, 0.001)


func test_a_real_first_build_hitch_still_fails() -> void:
	var g: Dictionary = Bench.cpu_gate(_series(2700, 0.5, 5.0), _series(2700, 1.0, 60.0))
	assert_false(g["pass_warmup"], "a 60 ms first LOD build is a hitch")
	assert_true(g["pass_lod"], "but not a steady-state failure")


func test_a_sustained_steady_state_regression_fails() -> void:
	var pack := _series(2700, 0.5, 5.0)
	for k in range(500, 560):  # 2 % of frames at 4 ms: above p99
		pack[k] = 4.0
	var g: Dictionary = Bench.cpu_gate(pack, _series(2700, 1.0, 12.0))
	assert_false(g["pass_pack"])
	assert_true(g["pass_warmup"])


func test_limits_are_the_documented_ones() -> void:
	assert_eq(Bench.PACK_BUDGET_MS_P99, 2.0)
	assert_eq(Bench.LOD_BUILD_BUDGET_MS_P99, 3.0)
	assert_eq(Bench.WARMUP_PACK_LIMIT_MS, 12.0)
	assert_eq(Bench.WARMUP_LOD_BUILD_LIMIT_MS, 33.0)
