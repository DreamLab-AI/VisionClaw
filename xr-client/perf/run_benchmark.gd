extends SceneTree

# Headless entry point. Invoked as:
#   godot --headless --path xr-client --script perf/run_benchmark.gd
# Loads the benchmark scene, lets it run for 30s real time, then quits with the
# scene's pass/fail exit code (0 pass, 1 fail). The scene itself prints the
# "[XR_PERF_RESULT]={...}" JSON line that the xr-perf-regression bin consumes
# (xr-client/rust/perf-regression).
#   godot --path xr-client --script perf/run_benchmark.gd -- duration=10 memory_rows=20000

const SCENE_PATH := "res://perf/benchmark_scene.tscn"

func _initialize() -> void:
	var packed: PackedScene = load(SCENE_PATH)
	if packed == null:
		push_error("benchmark scene missing at %s" % SCENE_PATH)
		quit(2)
		return
	var inst := packed.instantiate()
	# Optional overrides after `--`: duration=<s>, memory_rows=<n> (0 = graph only),
# route_hops=<n> (default 12; 63 = a 64-node route), route_sidecar=<n> (default 5).
	for arg in OS.get_cmdline_user_args():
		var kv := arg.trim_prefix("--").split("=")
		if kv.size() != 2:
			continue
		match kv[0]:
			"duration":
				inst.set_meta("duration_seconds", float(kv[1]))
			"memory_rows":
				inst.set_meta("memory_cloud_rows", int(kv[1]))
			"route_hops":
				inst.set_meta("memory_route_hops", int(kv[1]))
			"route_sidecar":
				inst.set_meta("memory_route_sidecar", int(kv[1]))
	get_root().add_child(inst)
