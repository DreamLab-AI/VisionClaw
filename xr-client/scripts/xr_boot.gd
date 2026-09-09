extends Node3D

@onready var error_overlay: Node3D = $ErrorOverlay
@onready var error_label: Label3D = $ErrorOverlay/ErrorLabel

var _warnings: PackedStringArray = PackedStringArray()


func _ready() -> void:
	var xr_interface: XRInterface = XRServer.find_interface("OpenXR")
	if xr_interface == null:
		_show_error("OpenXR runtime not present.")
		return
	if not xr_interface.is_initialized():
		if not xr_interface.initialize():
			_show_error("OpenXR initialise() returned false.")
			return
	get_viewport().use_xr = true
	_probe_capabilities(xr_interface)
	if not _warnings.is_empty():
		push_warning("XRBoot warnings:\n%s" % "\n".join(_warnings))
	_transition_to_graph_scene()


func _probe_capabilities(xr_interface: XRInterface) -> void:
	if not xr_interface.has_method("get_capabilities"):
		_warnings.append("XR interface lacks get_capabilities() -- capability queries unavailable.")

	if not xr_interface.is_passthrough_supported():
		_warnings.append("Passthrough unsupported -- proceeding without passthrough overlay.")

	var hand_tracking_available: bool = false
	if xr_interface.has_method("get_capabilities"):
		var caps: int = xr_interface.get_capabilities()
		# XRInterface.XR_HAND_TRACKING = 16 (bit 4)
		hand_tracking_available = (caps & 16) != 0
	if not hand_tracking_available:
		_warnings.append("Hand tracking not available -- falling back to controller input.")

	# Eye-gaze policy (copresence brief; godot #113717 hazard). We only QUERY
	# support here, after OpenXR init -- we never enable the
	# XR_EXT_eye_gaze_interaction action-map binding blindly, which is the bug
	# that trips the action-map error. Head-gaze stays primary; GraphScene feeds
	# this same capability to the Rust gaze resolver, which degrades eye-gaze to
	# head when unsupported. SteamVR on the Vive and Quest 3 both return false here.
	var eye_gaze_supported: bool = false
	if xr_interface.has_method("is_eye_gaze_interaction_supported"):
		eye_gaze_supported = xr_interface.is_eye_gaze_interaction_supported()
	if not eye_gaze_supported:
		_warnings.append("Eye-gaze interaction unsupported -- head-gaze primary (extension left disabled).")


func _transition_to_graph_scene() -> void:
	var graph_scene := load("res://scenes/GraphScene.tscn")
	if graph_scene == null:
		_show_error("GraphScene.tscn missing.")
		return
	# Defer the swap: _ready() runs while the OpenXR vendor addon is still adding
	# XR nodes to the tree, so a synchronous change_scene_to_packed() trips
	# "Parent node is busy adding/removing children". Deferring runs it at idle.
	get_tree().change_scene_to_packed.call_deferred(graph_scene)


func _show_error(text: String) -> void:
	error_overlay.visible = true
	error_label.text = text
	push_error("XRBoot: %s" % text)
