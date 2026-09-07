extends Node3D
## Deterministic, offline material/instancing smoke scene; no backend connection.
## Reuses the production scene geometry, lighting, materials and environment.

var _frames: int = 0

func _ready() -> void:
    var hud := get_node_or_null("XROrigin3D/XRCamera3D/HUD")
    if hud != null:
        hud.visible = false
    var camera: Camera3D = $FlatFallbackCamera
    camera.current = true
    camera.position = Vector3(0, 1.65, 1.6)
    camera.look_at(Vector3(0, 1.25, -1.6))
    var palette := [Color("62c4d4"), Color("a99ce7"), Color("e9b56f"), Color("73cbb0")]
    var positions: Array[Vector3] = []
    var nodes: MultiMesh = $GraphRoot/NodesMulti.multimesh
    nodes.instance_count = 48
    for i: int in range(48):
        var cluster: int = i / 12
        var angle: float = float(i % 12) * TAU / 12.0
        var center := Vector3(-0.85 if cluster % 2 == 0 else 0.85, 0.88 + float(cluster / 2) * 0.8, -1.6)
        var point := center + Vector3(cos(angle) * 0.44, sin(angle) * 0.28, sin(angle * 3.0) * 0.3)
        positions.append(point)
        var size: float = 0.10 if i % 12 == 0 else 0.055
        nodes.set_instance_transform(i, Transform3D(Basis.IDENTITY.scaled(Vector3.ONE * size), point))
        nodes.set_instance_color(i, palette[cluster])
        nodes.set_instance_custom_data(i, Color(float(i % 12) / 11.0, 8.0 if i == 0 else 0.0, 1.0 if i == 12 else 0.0, 1.0))
    var edges: MultiMesh = $GraphRoot/EdgesMulti.multimesh
    edges.instance_count = 60
    for i: int in range(60):
        var a: Vector3 = positions[i % 48]
        var b: Vector3 = positions[(i + 1) % 48] if i < 48 else positions[(i + 12) % 48]
        var delta: Vector3 = b - a
        var direction := delta.normalized()
        var right := direction.cross(Vector3.FORWARD).normalized()
        if right.length_squared() < 0.001:
            right = Vector3.RIGHT
        var forward := right.cross(direction).normalized()
        var basis := Basis(right * 0.07, direction * delta.length(), forward * 0.07)
        edges.set_instance_transform(i, Transform3D(basis, (a + b) * 0.5))
        edges.set_instance_custom_data(i, Color(0, 0, 0, float(i % 4)))
    # Exercise the transparent node twin with the production per-instance alpha.
    var faded: MultiMesh = $GraphRoot/NodesFadedMulti.multimesh
    faded.instance_count = 1
    faded.set_instance_transform(0, Transform3D(Basis.IDENTITY.scaled(Vector3.ONE * 0.16), Vector3(0, 1.4, -1.35)))
    faded.set_instance_color(0, Color(0.85, 0.68, 0.95, 0.3))
    faded.set_instance_custom_data(0, Color(0.8, 0, 0, 1))
    var environment: Node3D = $SpatialEnvironment
    environment.call("set_visual_comfort", false, true)
    assert(not environment.get_node("SpatialFloor").visible, "low-cost removes ground grid")
    assert($GraphRoot/NodesMulti.material_override.next_pass == null, "low-cost removes node halo pass")
    assert(get_viewport().msaa_3d == Viewport.MSAA_DISABLED, "low-cost removes multisample cost")
    environment.call("set_visual_comfort", true, false)
    assert(environment.get_node("SpatialFloor").visible, "balanced restores ground grid")
    assert($GraphRoot/NodesMulti.material_override.next_pass != null, "balanced restores halo pass")
    assert($GraphRoot/NodesMulti.material_override.next_pass.get_shader_parameter("query_pulse_depth") == 0.0, "reduced motion stops query pulsing")
    assert($GraphRoot/EdgesMulti.material_override.get_shader_parameter("pulse_energy") == 0.0, "reduced motion stops edge pulses")
    var focus: MeshInstance3D = environment.get_node("GraphFocusRing")
    focus.visible = true
    focus.global_position = positions[12]
    focus.global_basis = camera.global_basis
    print("SPATIAL_COMFORT:low-cost disables halo/MSAA;balanced restores;reduced motion stops pulses")
    print("SPATIAL_FIXTURE:48 opaque,1 faded,60 edges,4 relation styles; production MultiMesh channels preserved")

func _process(_delta: float) -> void:
    _frames += 1
    if _frames == 24:
        await RenderingServer.frame_post_draw
        var image := get_viewport().get_texture().get_image()
        var output := OS.get_environment("XR_VISUAL_CAPTURE")
        if output.is_empty():
            output = "/tmp/visionclaw-spatial-preview.png"
        var result := image.save_png(output)
        print("SPATIAL_CAPTURE:%s result=%d" % [output, result])
        get_tree().quit(result)
