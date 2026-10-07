extends RefCounted

## Node-mesh LOD wiring (PRD-008 §6: ≤ 100k triangles, ≤ 50 draw calls).
##
## Rust (`lod::split_node_tiers`, via BinaryProtocolClient.build_node_buffer_lod)
## keeps the full gem mesh for the nearest NEAR_CAP nodes within NEAR_RADIUS_M
## and returns every other drawn node as a far-tier instance. This script only
## builds the far-tier MultiMesh (a 1×1 quad with node_impostor.gdshader) and
## assigns buffers. Shared by GraphScene and perf/benchmark.gd so the benchmark
## measures the production path.

const IMPOSTOR_SHADER := preload("res://materials/node_impostor.gdshader")
const RIBBON_SHADER := preload("res://materials/edge_ribbon.gdshader")
## Near-field cylinder cap: 96 × 32 = 3 072 triangles (rust lod::DEFAULT_NEAR_EDGE_CAP).
const NEAR_EDGE_CAP: int = 96
## Edge midpoint radius for cylinders, world metres (same as the node gem tier).
const NEAR_EDGE_RADIUS_M: float = 1.0
## edge_flow uniforms the ribbon mirrors from the live cylinder material, so the
## comfort settings (reduced motion → pulse off, low cost → dimmer) reach both.
const EDGE_FLOW_PARAMS: Array[String] = ["flow_color", "base_alpha", "pulse_speed", "pulse_width",
	"pulse_energy", "pulse_gain", "subclass_dash_gap", "subclass_dash_freq", "subclass_dim",
	"untyped_dim", "subclass_color", "inferred_color", "inferred_dash_gap", "inferred_dash_freq",
	"inferred_dim"]
## Gem-tier cap: 96 × 576 measured triangles = 55 296 (rust lod::DEFAULT_NEAR_CAP).
const NEAR_CAP: int = 96
## Gem-tier radius in world metres (rust lod::DEFAULT_NEAR_RADIUS_M).
const NEAR_RADIUS_M: float = 1.0
const STRIDE: int = 20


## A MultiMeshInstance3D for the impostor tier: same 20-float instance layout as
## NodesMulti (transform + colour + custom), quad mesh, impostor material.
static func make_impostor_instance(node_name: String = "NodesImpostorMulti") -> MultiMeshInstance3D:
	var quad := QuadMesh.new()
	quad.size = Vector2(1.0, 1.0)
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.use_custom_data = true
	mm.mesh = quad
	var inst := MultiMeshInstance3D.new()
	inst.name = node_name
	inst.multimesh = mm
	inst.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	var mat := ShaderMaterial.new()
	mat.shader = IMPOSTOR_SHADER
	inst.material_override = mat
	return inst


## Assign a packed 20-float buffer to a MultiMesh instance, resizing it first.
static func assign(inst: MultiMeshInstance3D, buf: PackedFloat32Array) -> int:
	if inst == null or inst.multimesh == null:
		return 0
	var mm: MultiMesh = inst.multimesh
	var count: int = buf.size() / STRIDE
	if mm.instance_count != count:
		mm.instance_count = count
	if count > 0:
		mm.buffer = buf
	return count


## `assign` for any instance stride (edges and beams use 16, Invariant 3).
static func assign_stride(inst: MultiMeshInstance3D, buf: PackedFloat32Array, stride: int) -> int:
	if inst == null or inst.multimesh == null or stride <= 0:
		return 0
	var mm: MultiMesh = inst.multimesh
	var count: int = buf.size() / stride
	if mm.instance_count != count:
		mm.instance_count = count
	if count > 0:
		mm.buffer = buf
	return count


## A MultiMeshInstance3D for the far edge tier: a 1×1 quad per edge with the
## ribbon shader, 16-float instances (12 transform + 4 custom), like EdgesMulti.
static func make_ribbon_instance(node_name: String = "EdgesRibbonMulti") -> MultiMeshInstance3D:
	var quad := QuadMesh.new()
	quad.size = Vector2(1.0, 1.0)
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_custom_data = true
	mm.mesh = quad
	var inst := MultiMeshInstance3D.new()
	inst.name = node_name
	inst.multimesh = mm
	inst.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	var mat := ShaderMaterial.new()
	mat.shader = RIBBON_SHADER
	inst.material_override = mat
	return inst


## Copy the edge_flow uniforms from the cylinder material to the ribbon material.
static func sync_edge_params(src: Material, dst: Material) -> void:
	var a := src as ShaderMaterial
	var b := dst as ShaderMaterial
	if a == null or b == null:
		return
	for p: String in EDGE_FLOW_PARAMS:
		var v: Variant = a.get_shader_parameter(p)
		if v != null and b.get_shader_parameter(p) != v:
			b.set_shader_parameter(p, v)
