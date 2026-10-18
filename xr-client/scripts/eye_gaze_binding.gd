extends RefCounted

# Runtime-only XR_EXT_eye_gaze_interaction action-map binding (ADR-2036).
#
# openxr_action_map.tres deliberately carries NO eye-gaze binding: a static
# /user/eyes_ext binding is bound blindly at boot and breaks headsets that lack
# the extension (Quest 3; godot #113717). The binding is built here instead and
# added to an action map only after the capability query has succeeded.
# UNVERIFIED ON DEVICE: no headset was available when this was written (L-5).

const PROFILE_PATH := "/interaction_profiles/ext/eye_gaze_interaction"
const GAZE_BINDING_PATH := "/user/eyes_ext/input/gaze_ext/pose"
const GAZE_TOPLEVEL_PATH := "/user/eyes_ext"
const POSE_ACTION_NAME := "default_pose"


# True only when the OpenXR interface reports the extension as usable.
static func is_supported(xr_interface: XRInterface) -> bool:
	if xr_interface == null:
		return false
	if not xr_interface.has_method("is_eye_gaze_interaction_supported"):
		return false
	return bool(xr_interface.is_eye_gaze_interaction_supported())


# Adds the eye-gaze interaction profile to `action_map` iff `supported`.
# Idempotent. Returns true when the map now carries the eye-gaze binding.
static func apply(action_map: OpenXRActionMap, supported: bool) -> bool:
	if not supported or action_map == null:
		return false
	if action_map.find_interaction_profile(PROFILE_PATH) != null:
		return true
	var pose_action: OpenXRAction = _find_pose_action(action_map)
	if pose_action == null:
		push_warning("EyeGazeBinding: '%s' action not found; eye-gaze left unbound." % POSE_ACTION_NAME)
		return false

	var toplevels: PackedStringArray = pose_action.toplevel_paths
	if not toplevels.has(GAZE_TOPLEVEL_PATH):
		toplevels.append(GAZE_TOPLEVEL_PATH)
		pose_action.toplevel_paths = toplevels

	var binding := OpenXRIPBinding.new()
	binding.action = pose_action
	binding.binding_path = GAZE_BINDING_PATH

	var profile := OpenXRInteractionProfile.new()
	profile.interaction_profile_path = PROFILE_PATH
	profile.add_binding(binding)
	action_map.add_interaction_profile(profile)
	return true


static func _find_pose_action(action_map: OpenXRActionMap) -> OpenXRAction:
	for action_set: OpenXRActionSet in action_map.get_action_sets():
		for action: OpenXRAction in action_set.get_actions():
			if action.resource_name == POSE_ACTION_NAME:
				return action
	return null
