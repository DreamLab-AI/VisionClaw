extends Node3D
## World-fixed spatial reference and a single pooled graph focus marker.
## Never reparents/moves the camera, never traverses the complete graph per frame.

signal visual_comfort_changed(reduced_motion: bool, low_cost: bool)

const FOCUS_TIMEOUT_MSEC: int = 350
const FOCUS_WORLD_DIAMETER: float = 0.18
const HOVER_COLOR := Color(0.4, 0.9, 0.94, 0.88)
const GRAB_COLOR := Color(1.0, 0.72, 0.32, 0.95)

var _scene: Node3D
var _focus: MeshInstance3D
var _focus_material: ShaderMaterial
var _target_id: int = -1
var _last_target_msec: int = 0
var _reduced_motion: bool = true
var _low_cost: bool = false
var _node_materials: Array[Material] = []
var _node_meshes: Array[MultiMeshInstance3D] = []
var _edge_material: ShaderMaterial

func _ready() -> void:
    add_to_group("xr_visual_environment")
    _scene = get_parent() as Node3D
    # Scene-local duplicates prevent comfort choices from mutating shared assets.
    for path: String in ["GraphRoot/NodesMulti", "GraphRoot/NodesFadedMulti"]:
        var mesh: MultiMeshInstance3D = _scene.get_node_or_null(path)
        if mesh != null and mesh.material_override != null:
            var material: Material = mesh.material_override.duplicate(true)
            mesh.material_override = material
            _node_meshes.append(mesh)
            _node_materials.append(material)
    var edge_mesh: MultiMeshInstance3D = _scene.get_node_or_null("GraphRoot/EdgesMulti")
    if edge_mesh != null and edge_mesh.material_override is ShaderMaterial:
        _edge_material = edge_mesh.material_override.duplicate(true) as ShaderMaterial
        edge_mesh.material_override = _edge_material
    var floor_mesh := PlaneMesh.new()
    floor_mesh.size = Vector2(200.0, 200.0)
    var floor_material := ShaderMaterial.new()
    floor_material.shader = preload("res://materials/spatial_floor.gdshader")
    var floor_instance := MeshInstance3D.new()
    floor_instance.name = "SpatialFloor"
    floor_instance.mesh = floor_mesh
    floor_instance.material_override = floor_material
    floor_instance.position.y = -0.02
    floor_instance.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
    add_child(floor_instance)

    _focus_material = ShaderMaterial.new()
    _focus_material.shader = preload("res://materials/focus_ring.gdshader")
    var quad := QuadMesh.new()
    quad.size = Vector2(FOCUS_WORLD_DIAMETER, FOCUS_WORLD_DIAMETER)
    _focus = MeshInstance3D.new()
    _focus.name = "GraphFocusRing"
    _focus.mesh = quad
    _focus.material_override = _focus_material
    _focus.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
    _focus.visible = false
    add_child(_focus)
    var motion_value := OS.get_environment("XR_REDUCED_MOTION").to_lower().strip_edges()
    var reduced: bool = motion_value not in ["0", "false", "off"]
    set_visual_comfort(reduced, OS.get_environment("XR_VISUAL_QUALITY").to_lower() == "low")
    if _scene != null and _scene.has_signal("node_targeted_in_scene"):
        _scene.connect("node_targeted_in_scene", _on_node_targeted)
    _bind_hud.call_deferred()

func _on_node_targeted(node_id: int) -> void:
    _target_id = node_id
    _last_target_msec = Time.get_ticks_msec()

func _process(_delta: float) -> void:
    if _scene == null:
        return
    # Pure environment previews intentionally have no graph client.
    if not "_binary_client" in _scene:
        return
    var client: RefCounted = _scene.get("_binary_client") as RefCounted
    var graph_root: Node3D = _scene.get_node_or_null("GraphRoot")
    var camera := get_viewport().get_camera_3d()
    var grabbed: int = int(_scene.get("_grabbed_id"))
    var active_id: int = grabbed if grabbed >= 0 else _target_id
    var recent: bool = grabbed >= 0 or Time.get_ticks_msec() - _last_target_msec < FOCUS_TIMEOUT_MSEC
    _focus.visible = client != null and graph_root != null and camera != null and active_id >= 0 and recent
    if not _focus.visible:
        return
    var position_in_graph: Vector3 = client.call("node_position", active_id)
    _focus.global_position = graph_root.global_transform * position_in_graph
    # A world-space billboard remains at the target's actual depth in both eyes.
    _focus.global_basis = camera.global_basis
    # Production node buffers compensate GraphRoot scale; the largest centrality
    # node has 0.114 m diameter at factor 1, inside this 0.18 m marker.
    var size_factor: float = maxf(float(_scene.get("_node_size_factor")), 0.5)
    _focus.scale = Vector3.ONE * size_factor
    _focus_material.set_shader_parameter("focus_color", GRAB_COLOR if grabbed >= 0 else HOVER_COLOR)


## Called by the operator HUD, or seeded from the environment at scene creation.
func set_visual_comfort(reduced_motion: bool, low_cost: bool) -> void:
    _reduced_motion = reduced_motion
    _low_cost = low_cost
    var floor_instance := get_node_or_null("SpatialFloor") as MeshInstance3D
    if floor_instance != null:
        floor_instance.visible = not low_cost
    get_viewport().msaa_3d = Viewport.MSAA_DISABLED if low_cost else Viewport.MSAA_2X
    for i: int in range(_node_meshes.size()):
        # Retain the original halo resource so toggling low-cost off is reversible.
        var base: Material = _node_materials[i]
        var material: Material = base.duplicate()
        var halo: ShaderMaterial = base.next_pass as ShaderMaterial
        if low_cost:
            material.next_pass = null
        elif halo != null:
            var local_halo := halo.duplicate() as ShaderMaterial
            local_halo.set_shader_parameter("query_pulse_depth", 0.0 if reduced_motion else 0.12)
            material.next_pass = local_halo
        _node_meshes[i].material_override = material
    if _edge_material != null:
        _edge_material.set_shader_parameter("pulse_energy", 0.0 if reduced_motion or low_cost else 0.20)
        _edge_material.set_shader_parameter("base_alpha", 0.09 if low_cost else 0.16)
    visual_comfort_changed.emit(reduced_motion, low_cost)
    var hud: Node = _scene.get_node_or_null("XROrigin3D/XRCamera3D/HUD")
    if hud == null:
        hud = _scene.get_node_or_null("XROrigin3D/HUD")
    if hud != null and hud.has_method("set_visual_comfort"):
        hud.call("set_visual_comfort", reduced_motion, low_cost)

func get_visual_comfort() -> Dictionary:
    return {"reduced_motion": _reduced_motion, "low_cost": _low_cost}


func _bind_hud() -> void:
    var hud: Node = _scene.get_node_or_null("XROrigin3D/HUD")
    if hud == null:
        hud = _scene.get_node_or_null("XROrigin3D/XRCamera3D/HUD")
    if hud == null:
        return
    if hud.has_signal("control_pressed"):
        hud.connect("control_pressed", _on_visual_control)
    if hud.has_method("set_visual_comfort"):
        hud.call("set_visual_comfort", _reduced_motion, _low_cost)

func _on_visual_control(action: String) -> void:
    if action.begins_with("visual_motion:"):
        set_visual_comfort(action == "visual_motion:1", _low_cost)
    elif action.begins_with("visual_quality:"):
        set_visual_comfort(_reduced_motion, action == "visual_quality:1")
