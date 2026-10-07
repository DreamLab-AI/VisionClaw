extends "res://addons/gut/test.gd"

# memory_flash bursts (scripts/memory_bursts.gd, WP3): one pooled MultiMesh of at
# most 64 ring slots under a unit-scale root; oldest slots recycle; the desktop's
# ease curves; reduced motion holds the ring size; MemoryFlashCodec decodes the
# frame with the desktop's semantic colour and shape.

const Bursts := preload("res://scripts/memory_bursts.gd")


func _make() -> Node3D:
	var b: Node3D = Bursts.new()
	add_child(b)
	await get_tree().process_frame
	return b


func _desc(rings: int = 1, implode: bool = false) -> Dictionary:
	return {"rings": rings, "max_scale": 4.0, "duration": 0.5, "implode": implode, "color": Color(0.2, 1, 0.1)}


func test_pool_never_exceeds_64_and_recycles_the_oldest() -> void:
	var b: Node3D = await _make()
	for i: int in range(40):
		b.spawn(Vector3(float(i), 1, -1), _desc(3))   # 120 ring slots requested
	assert_eq(b.slot_count(), Bursts.POOL_SIZE, "capped at 64 slots")
	assert_eq(b.spawned_total(), 40)
	# The newest burst survives; the oldest was recycled.
	var positions: Array = []
	for s: Dictionary in b._slots:
		positions.append((s["pos"] as Vector3).x)
	assert_true(positions.has(39.0), "newest burst kept")
	assert_false(positions.has(0.0), "oldest burst recycled")
	await get_tree().process_frame
	var mm: MultiMesh = (b.get_node("BurstMulti") as MultiMeshInstance3D).multimesh
	assert_lte(mm.instance_count, Bursts.POOL_SIZE, "drawn instances within the pool")
	b.queue_free()
	await get_tree().process_frame


func test_rings_stagger_then_expire() -> void:
	var b: Node3D = await _make()
	b.spawn(Vector3(0, 1, -1), _desc(3))
	await get_tree().process_frame
	var mm: MultiMesh = (b.get_node("BurstMulti") as MultiMeshInstance3D).multimesh
	assert_eq(b.slot_count(), 3, "three concentric rings")
	assert_lt(mm.instance_count, 3, "later rings wait on the stagger")
	await get_tree().create_timer(0.5 + 2.0 * Bursts.RING_STAGGER + 0.15).timeout
	await get_tree().process_frame
	assert_eq(b.slot_count(), 0, "all rings expired")
	assert_eq(mm.instance_count, 0)
	b.queue_free()
	await get_tree().process_frame


func test_curves_match_the_desktop_and_reduced_motion_holds_size() -> void:
	assert_almost_eq(Bursts.burst_scale(0.0, 2.0, false), 0.0005, 1e-6, "expand starts at a point")
	assert_almost_eq(Bursts.burst_scale(1.0, 2.0, false), 2.0, 1e-6, "expand peaks at max")
	assert_almost_eq(Bursts.burst_scale(0.5, 2.0, false), 2.0 * (1.0 - 0.125), 1e-6, "cubic ease-out")
	assert_almost_eq(Bursts.burst_scale(0.0, 2.0, true), 2.0, 1e-6, "implode starts large")
	assert_almost_eq(Bursts.burst_alpha(0.0), 0.85, 1e-6)
	assert_almost_eq(Bursts.burst_alpha(0.5), 0.75 * 0.85, 1e-6)
	var b: Node3D = await _make()
	b.reduced_motion = true
	b.spawn(Vector3(0, 1, -1), _desc(1))
	await get_tree().process_frame
	var s0: float = b.drawn_scales()[0]
	await get_tree().create_timer(0.2).timeout
	await get_tree().process_frame
	var s1: float = b.drawn_scales()[0]
	assert_almost_eq(s0, s1, 1e-4, "no ring expansion under reduced motion")
	assert_almost_eq(s0, 4.0 * b.unit_scale * Bursts.REDUCED_SCALE, 1e-4)
	b.reduced_motion = false
	await get_tree().process_frame
	var e0: float = b.drawn_scales()[0]
	await get_tree().create_timer(0.1).timeout
	await get_tree().process_frame
	assert_gt(b.drawn_scales()[0], e0, "the ring expands with motion allowed")
	b.queue_free()
	await get_tree().process_frame


func test_ambient_position_is_stable_and_on_the_shell() -> void:
	var c := Vector3(1, 2, 3)
	var p1: Vector3 = Bursts.ambient_position("k1", "patterns", c)
	var p2: Vector3 = Bursts.ambient_position("k1", "patterns", c)
	assert_eq(p1, p2, "same memory, same place")
	assert_almost_eq(p1.distance_to(c), 0.45, 1e-3, "on the 0.45 m shell")
	assert_ne(p1, Bursts.ambient_position("k2", "patterns", c), "different memories differ")


func test_codec_decodes_single_and_batch_with_semantic_colour() -> void:
	var one: Array = MemoryFlashCodec.parse('{"type":"memory_flash","data":{"key":"a","namespace":"","action":"store"}}')
	assert_eq(one.size(), 1)
	var d: Dictionary = one[0]
	assert_eq(String(d["action"]), "store")
	assert_eq(int(d["rings"]), 2, "store punches two rings")
	assert_eq((d["color"] as Color).to_html(false), "39ff14", "desktop store green")
	var del: Array = MemoryFlashCodec.parse('{"type":"memory_flash","data":[{"key":"x","action":"delete"},{"namespace":"n","action":"search"}]}')
	assert_eq(del.size(), 2, "batch form")
	assert_true(bool(del[0]["implode"]), "delete implodes")
	assert_eq(int(del[1]["rings"]), 3, "search ripples three rings")
	assert_eq(MemoryFlashCodec.parse('{"type":"other"}').size(), 0)
	assert_eq(MemoryFlashCodec.parse("junk").size(), 0)
