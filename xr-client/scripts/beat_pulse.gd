extends Node

## Beat clock and memory activity in the headset (WP3 / WP5 / WP8).
##
## GraphScene creates one of these and hands it `/wss` text frames; it owns
## everything else so the scene keeps only thin hooks:
##   * BeatPulse (Rust, pulse.rs/beat.rs): the desktop's relayed `beatClock`,
##     server-clock offset from JSON ping/pong (RTT/2, minimum-RTT sample), the
##     controller tap tempo and the opt-in microphone analyser;
##   * one `beat_pulse` uniform per frame on the node-halo and edge-flow
##     materials the meshes are ACTUALLY using (a uniform swell — no
##     post-process, Invariant 2), scaled down under reduced motion. Those are
##     scene-local duplicates made by spatial_environment.gd, replaced again on
##     every comfort toggle, so they are re-read from the meshes rather than
##     cached or taken from the shared .tres resources;
##   * memory_flash bursts (memory_bursts.gd) under the unit-scale effects root.
##
## Tap tempo: B/Y (Touch, Index, Cosmos, Focus 3) or a centred click of the LEFT
## trackpad/thumbstick (`primary_click`; Vive wands have no B/Y). Neither is bound
## elsewhere: trigger = grab/HUD, grip = panel/graph, menu or A/X = radial menu,
## stick or pad deflection = locomotion (a click is only a tap while the pad
## reads inside the locomotion dead zone, so it never moves the rig).
##
## Microphone (WP8): off by default, toggled on the Session page. While on, the
## HUD header shows a red "● MIC" badge. Audio stays in memory for analysis and
## is discarded; it is never recorded, stored or sent anywhere. Only a lock at
## high confidence, confirmed twice, may drive the pulse.

const MemoryBursts := preload("res://scripts/memory_bursts.gd")

const PING_INTERVAL_SEC := 2.0
const HUD_REFRESH_SEC := 0.25
const TAP_DEADZONE := 0.15           # = graph_scene LOCOMOTION_DEADZONE
const MIC_BUS := "BeatMic"
const HALO_MESHES: Array[String] = ["GraphRoot/NodesMulti", "GraphRoot/NodesFadedMulti"]
const EDGE_MESH := "GraphRoot/EdgesMulti"
const PULSE_EPSILON := 0.004

var reduced_motion: bool = true
var bursts_enabled: bool = true

var _bp: RefCounted = null           # Rust BeatPulse
var _client: RefCounted = null       # Rust BinaryProtocolClient (send_text)
var _hud: Node = null
var _left: XRController3D = null
var _right: XRController3D = null
var _bursts: Node3D = null
var _centre_fn: Callable = Callable()
var _scene: Node = null
var _pulse_targets: Array = []        # ShaderMaterials last written
var _last_pulse: float = -1.0
var _ping_t: float = 0.0
var _hud_t: float = 0.0
var _tap_was_down: bool = false
var _mic_player: AudioStreamPlayer = null
var _mic_capture: AudioEffectCapture = null
var _mic_on: bool = false
var _tap_total: int = 0


## Wire to the scene. `effects_root` must be a unit-scale node (never GraphRoot);
## `centre_fn` returns the world position bursts fall back to (graph centre).
func setup(scene: Node, client: RefCounted, hud: Node, left: XRController3D, right: XRController3D,
		effects_root: Node3D, centre_fn: Callable) -> void:
	_scene = scene
	_client = client
	_hud = hud
	_left = left
	_right = right
	_centre_fn = centre_fn
	# gdext classes are no_init: construct through their static create().
	_bp = BeatPulse.create()
	_bursts = MemoryBursts.new()
	_bursts.name = "MemoryBursts"
	if effects_root != null:
		effects_root.add_child(_bursts)
	else:
		add_child(_bursts)


func bursts() -> Node3D:
	return _bursts


func beat() -> RefCounted:
	return _bp


## Offer a `/wss` text frame (`type` already parsed). True when consumed here.
func on_text(json: String, type: String) -> bool:
	match type:
		"beatClock", "pong":
			if _bp != null:
				_bp.handle_text(json)
			return true
		"memory_flash":
			on_memory_flash(json)
			return true
	return false


func on_memory_flash(json: String) -> void:
	if not bursts_enabled or _bursts == null:
		return
	var descs: Array = MemoryFlashCodec.parse(json)
	if descs.is_empty():
		return
	# With the memory cloud shown and loaded (xr-cloud, memory_cloud_layer.gd),
	# a flash lands on its point(s) by the desktop's key → namespace rule and an
	# unmatched flash draws nothing, exactly as on the desktop; burst size follows
	# the cloud's scale. With no cloud on screen, a stable stand-in point on a
	# shell around the graph keeps memory activity visible in the headset.
	var cloud: Object = _scene.get("_memory_cloud") if _scene != null else null
	var cloud_live: bool = cloud != null and cloud.has_method("resolve_flash") \
		and bool(cloud.call("is_enabled")) and bool(cloud.call("has_snapshot"))
	var centre: Vector3 = _centre_fn.call() if _centre_fn.is_valid() else Vector3(0, 1.2, -1.2)
	if cloud_live:
		var root: Node3D = cloud.call("cloud_root")
		if root != null:
			_bursts.unit_scale = absf(root.global_transform.basis.get_scale().x)
	else:
		_bursts.unit_scale = MemoryBursts.DEFAULT_UNIT_SCALE
	for d: Dictionary in descs:
		if cloud_live:
			for row: int in cloud.call("resolve_flash", String(d["key"]), String(d["namespace"])):
				_bursts.spawn(cloud.call("world_point", row), d)
		else:
			_bursts.spawn(MemoryBursts.ambient_position(String(d["key"]), String(d["namespace"]), centre), d)


## HUD intents: "beat_tap", "beat_mic:1|0", "memory_bursts:1|0".
func on_hud_action(action: String) -> void:
	if action == "beat_tap":
		tap()
	elif action.begins_with("beat_mic:"):
		set_mic_enabled(action.ends_with(":1"))
	elif action.begins_with("memory_bursts:"):
		bursts_enabled = action.ends_with(":1")
		if not bursts_enabled and _bursts != null:
			_bursts.clear_all()


func tap() -> void:
	_tap_total += 1
	if _bp != null:
		_bp.tap()
	_refresh_hud()


func tap_total() -> int:
	return _tap_total


func mic_on() -> bool:
	return _mic_on


## Opt-in microphone. On Android the RECORD_AUDIO runtime permission is asked
## for here, on the operator's explicit press, never at start-up; a denial
## leaves the mic off and says so on the HUD.
func set_mic_enabled(on: bool) -> void:
	if on == _mic_on:
		_refresh_hud()
		return
	if on and OS.get_name() == "Android":
		var granted: PackedStringArray = OS.get_granted_permissions()
		if not granted.has("android.permission.RECORD_AUDIO"):
			OS.request_permissions()
			_notice("Microphone permission needed for beat sync — allow it, then turn Mic on again")
			_mic_on = false
			_refresh_hud()
			return
	_mic_on = on
	if on:
		_start_mic()
	else:
		_stop_mic()
	if _bp != null:
		_bp.set_mic_enabled(on)
	_refresh_hud()


func _start_mic() -> void:
	var bus: int = AudioServer.get_bus_index(MIC_BUS)
	if bus < 0:
		AudioServer.add_bus()
		bus = AudioServer.bus_count - 1
		AudioServer.set_bus_name(bus, MIC_BUS)
		AudioServer.add_bus_effect(bus, AudioEffectCapture.new())
	# Muted: the capture effect still sees the signal, the speakers never do.
	AudioServer.set_bus_mute(bus, true)
	_mic_capture = AudioServer.get_bus_effect(bus, 0) as AudioEffectCapture
	if _mic_capture != null:
		_mic_capture.clear_buffer()
	if _mic_player == null:
		_mic_player = AudioStreamPlayer.new()
		_mic_player.name = "BeatMicInput"
		_mic_player.stream = AudioStreamMicrophone.new()
		_mic_player.bus = MIC_BUS
		add_child(_mic_player)
	_mic_player.play()


func _stop_mic() -> void:
	if _mic_player != null:
		_mic_player.stop()
	if _mic_capture != null:
		_mic_capture.clear_buffer()


func _process(delta: float) -> void:
	_poll_tap_input()
	if _bp != null:
		_ping_t += delta
		if _ping_t >= PING_INTERVAL_SEC:
			_ping_t = 0.0
			if _client != null and _client.has_method("send_text"):
				_client.send_text(_bp.make_ping())
		if _mic_on and _mic_capture != null:
			var n: int = _mic_capture.get_frames_available()
			if n > 0:
				_bp.push_mic_frames(_mic_capture.get_buffer(n), AudioServer.get_mix_rate())
		_bp.process()
	var pulse: float = _bp.pulse(reduced_motion) if _bp != null else 0.0
	var targets: Array = pulse_materials()
	if absf(pulse - _last_pulse) > PULSE_EPSILON or targets != _pulse_targets:
		_last_pulse = pulse
		_pulse_targets = targets
		for m: ShaderMaterial in targets:
			m.set_shader_parameter("beat_pulse", pulse)
	if _bursts != null:
		_bursts.reduced_motion = reduced_motion
		_bursts.beat_pulse = pulse
		var cam: Node3D = get_viewport().get_camera_3d() if is_inside_tree() else null
		if cam != null:
			_bursts.set_head(cam.global_position)
	_hud_t += delta
	if _hud_t >= HUD_REFRESH_SEC:
		_hud_t = 0.0
		_refresh_hud()


## The halo and edge ShaderMaterials currently assigned to the graph meshes.
func pulse_materials() -> Array:
	var out: Array = []
	if _scene == null:
		return out
	for path: String in HALO_MESHES:
		var mi: MultiMeshInstance3D = _scene.get_node_or_null(path)
		if mi != null and mi.material_override != null and mi.material_override.next_pass is ShaderMaterial:
			out.append(mi.material_override.next_pass)
	var edges: MultiMeshInstance3D = _scene.get_node_or_null(EDGE_MESH)
	if edges != null and edges.material_override is ShaderMaterial:
		out.append(edges.material_override)
	return out


## Beat accessors read by xr-cloud's memory_cloud_layer.gd (`beat_source`):
## is_locked() — a clock is driving; beat_phase() — 0..1 within the beat, < 0
## when none; beat_pulse() — the raw 0..1 envelope (the reader applies its own
## reduced-motion policy).
func is_locked() -> bool:
	return _bp != null and bool(_bp.status().get("on", false))


func beat_phase() -> float:
	if not is_locked():
		return -1.0
	return float(_bp.status().get("phase", -1.0))


func beat_pulse() -> float:
	return float(_bp.pulse(false)) if _bp != null else 0.0


## Current pulse (0..1, reduced-motion scaled) for other layers.
func current_pulse() -> float:
	return maxf(_last_pulse, 0.0)


func _poll_tap_input() -> void:
	var down := false
	var hit: XRController3D = null
	for c: XRController3D in [_left, _right]:
		if c == null or not c.get_is_active():
			continue
		if c.is_button_pressed("by_button"):
			down = true
			hit = c
			break
		if c == _left and c.is_button_pressed("primary_click") \
				and c.get_vector2("primary").length() < TAP_DEADZONE:
			down = true
			hit = c
			break
	if down and not _tap_was_down:
		tap()
		if hit != null:
			hit.trigger_haptic_pulse("haptic", 0.0, 0.3, 0.04, 0.0)
	_tap_was_down = down


func _refresh_hud() -> void:
	if _hud == null:
		return
	var line: String = String(_bp.status_line()) if _bp != null else "Beat: unavailable"
	var mic_state: String = "off"
	if _bp != null and _mic_on:
		mic_state = String(_bp.status().get("mic_state", "listening"))
	if _hud.has_method("set_beat_status"):
		_hud.set_beat_status(line, _mic_on, mic_state)


func _notice(text: String) -> void:
	if _hud != null and _hud.has_method("flash_notice"):
		_hud.flash_notice(text)


func _exit_tree() -> void:
	_stop_mic()
	if _bp != null:
		_bp.set_mic_enabled(false)
