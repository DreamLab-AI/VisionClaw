extends Node3D

## Geometric agent embodiment (PRD-023 WP-9 M3, ADR-130 Decision 4).
##
## The visible half of M3: a crystal-orb core, a translucent gaze cone oriented
## by the Rust attention output, and a screen-facing DID badge. Every threshold
## and state transition lives in Rust (`avatar_state.rs` → AgentAvatarNode); this
## script only wires those outputs to scene nodes and animates colour/motion.
## Body/face tracking are out of scope (neither the Vive/SteamVR rig nor a
## standalone headset exposes them to us) and never appear here.

# Activity ordinals mirror AgentActivity::as_u8 (agent_presence.rs:72).
const ACT_IDLE: int = 0
const ACT_WORKING: int = 1
const ACT_AWAITING: int = 2
const ACT_SPEAKING: int = 3
const ACT_DONE: int = 4

# LOD feature bits mirror lod.rs AGENT_FEAT_* (agent_feature_mask).
const FEAT_BADGE: int = 1
const FEAT_CONE: int = 2
const FEAT_CORE_MESH: int = 4
const FEAT_CORE_BILLBOARD: int = 8

# Legible-not-realistic state palette (copresence brief §1). Idle reads calm,
# working reads active, awaiting-approval reads a saturated amber that demands
# attention — the state the operator must act on.
const COLOR_IDLE: Color = Color(0.42, 0.6, 0.9)
const COLOR_WORKING: Color = Color(0.3, 0.9, 0.72)
const COLOR_AWAITING: Color = Color(1.0, 0.62, 0.12)
const COLOR_SPEAKING: Color = Color(0.7, 0.85, 1.0)
const COLOR_DONE: Color = Color(0.55, 0.55, 0.6)

const COLOR_VERIFIED: Color = Color(0.75, 1.0, 0.82)
const COLOR_UNVERIFIED: Color = Color(1.0, 0.72, 0.28)

# Gaze-cone length: narrow end at the core, opening forward along the gaze ray.
const CONE_LENGTH: float = 0.6

@onready var core: MeshInstance3D = $Core
@onready var core_billboard: MeshInstance3D = $CoreBillboard
@onready var gaze_cone: MeshInstance3D = $GazeCone
@onready var badge: Label3D = $Badge

# Rust AgentAvatarNode — owns the activity machine + gaze-attention model.
var _model: RefCounted = null
# Per-instance material so state colour never leaks across the avatar pool.
var _core_mat: StandardMaterial3D = null

var _activity: int = ACT_IDLE
var _feature_mask: int = FEAT_BADGE | FEAT_CONE | FEAT_CORE_MESH
var _time: float = 0.0
var _reduced_motion: bool = true

var _display_name: String = ""
var _did: String = ""
var _verified: bool = false


func _ready() -> void:
	_model = AgentAvatarNode.create()
	if _model != null and _model.has_signal("activity_changed"):
		_model.connect("activity_changed", Callable(self, "_on_activity_changed"))

	# Give the core its own material instance so per-agent state modulation does
	# not mutate the shared crystal_orb resource used by every other avatar.
	if core != null:
		var src: Material = core.material_override
		if src != null:
			_core_mat = src.duplicate()
			core.material_override = _core_mat
			core_billboard.material_override = _core_mat

	var comfort := get_tree().get_first_node_in_group("xr_visual_environment")
	if comfort != null:
		var state: Dictionary = comfort.call("get_visual_comfort")
		_reduced_motion = bool(state.get("reduced_motion", true))
		comfort.connect("visual_comfort_changed", _on_visual_comfort_changed)
	_apply_state_visual()
	_refresh_badge()


## Populate the DID badge. `verified` reflects whether the did:nostr passed the
## Schnorr-challenge check (ADR-130 Decision 6); an unverified claim renders
## visually distinct (amber + "?" mark) so it is never mistaken for a trusted
## identity.
func set_avatar_identity(display_name: String, did: String, verified: bool) -> void:
	_display_name = display_name
	_did = did
	_verified = verified
	_refresh_badge()


func set_verified(verified: bool) -> void:
	_verified = verified
	_refresh_badge()


## Apply an agent signal (ordinals mirror signal_from_i32, avatar_state.rs:418).
func apply_signal(signal_ordinal: int) -> void:
	if _model != null:
		_model.apply_signal(signal_ordinal)


## Advance the gaze-attention model one frame with the user-gaze test result.
func tick_attention(
	user_gazing_at_me: bool,
	dir_to_user: Vector3,
	has_deixis: bool,
	deixis_node: int,
	deixis_dir: Vector3,
	dt_us: int
) -> void:
	if _model != null:
		_model.tick_attention(user_gazing_at_me, dir_to_user, has_deixis, deixis_node, deixis_dir, dt_us)


func activity() -> int:
	return _activity


func set_alpha(a: float) -> void:
	if _core_mat == null:
		return
	_core_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA if a < 0.99 else BaseMaterial3D.TRANSPARENCY_DISABLED
	var c := _core_mat.albedo_color
	_core_mat.albedo_color = Color(c.r, c.g, c.b, a)


func set_activity_done() -> void:
	if _activity == ACT_DONE:
		return
	_activity = ACT_DONE
	_apply_state_visual()


## Drive per-feature visibility from the Rust LOD feature mask (lod.rs). Badge
## drops first, then the cone; the core swaps to a billboard before it culls.
func set_feature_mask(mask: int) -> void:
	_feature_mask = mask
	if badge != null:
		badge.visible = (mask & FEAT_BADGE) != 0
	if gaze_cone != null:
		gaze_cone.visible = (mask & FEAT_CONE) != 0
	if core != null:
		core.visible = (mask & FEAT_CORE_MESH) != 0
	if core_billboard != null:
		core_billboard.visible = (mask & FEAT_CORE_BILLBOARD) != 0
	visible = mask != 0


func _on_activity_changed(state: int) -> void:
	_activity = state
	_apply_state_visual()


func _process(delta: float) -> void:
	_time += delta
	_orient_gaze_cone()
	_animate_motion()


# Orient the gaze cone so its axis (the CylinderMesh +Y) lies along the Rust
# attention direction, narrow end at the core. Built basis-first (not look_at)
# because the mesh axis is +Y, not -Z.
func _orient_gaze_cone() -> void:
	if _model == null or gaze_cone == null or not gaze_cone.visible:
		return
	var gdir: Vector3 = _model.gaze_dir()
	if gdir.length() < 0.001:
		return
	var basis := _basis_from_y(gdir.normalized())
	# Offset the mesh forward by half its length so the narrow end sits at the
	# core and the cone opens along the gaze ray.
	gaze_cone.transform = Transform3D(basis, basis.y * (CONE_LENGTH * 0.5))


static func _basis_from_y(y_dir: Vector3) -> Basis:
	var y := y_dir.normalized()
	var up := Vector3.UP if absf(y.dot(Vector3.UP)) < 0.99 else Vector3.RIGHT
	var x := up.cross(y).normalized()
	var z := x.cross(y).normalized()
	return Basis(x, y, z)


# Colour + motion per activity (copresence brief §1): idle slow bob + dim,
# working steady pulse, awaiting-approval saturated + attention motion.
func _animate_motion() -> void:
	if core == null:
		return
	if _reduced_motion:
		core.position.y = 0.0
		_set_emission(0.55 if _activity == ACT_AWAITING else 0.3)
		return
	match _activity:
		ACT_IDLE:
			core.position.y = sin(_time * 1.2) * 0.02
			_set_emission(0.3)
		ACT_WORKING:
			core.position.y = 0.0
			var pulse: float = 0.5 + 0.5 * sin(_time * 6.0)
			_set_emission(0.4 + pulse * 0.5)
		ACT_AWAITING:
			# Saturated colour and a sharper attention bob — this is the state
			# the operator must act on.
			core.position.y = sin(_time * 3.5) * 0.035
			var att: float = 0.5 + 0.5 * sin(_time * 4.0)
			_set_emission(0.75 + att * 0.4)
		ACT_SPEAKING:
			core.position.y = 0.0
			var talk: float = 0.5 + 0.5 * sin(_time * 9.0)
			_set_emission(0.5 + talk * 0.35)
		ACT_DONE:
			core.position.y = sin(_time * 0.5) * 0.008
			_set_emission(0.12)


func _set_emission(energy: float) -> void:
	if _core_mat != null:
		_core_mat.emission_energy_multiplier = energy


func _apply_state_visual() -> void:
	if _core_mat == null:
		return
	var col: Color = COLOR_IDLE
	match _activity:
		ACT_WORKING:
			col = COLOR_WORKING
		ACT_AWAITING:
			col = COLOR_AWAITING
		ACT_SPEAKING:
			col = COLOR_SPEAKING
		ACT_DONE:
			col = COLOR_DONE
	_core_mat.albedo_color = Color(col.r, col.g, col.b, _core_mat.albedo_color.a)
	_core_mat.emission = col


func _refresh_badge() -> void:
	if badge == null:
		return
	var mark: String = "✓" if _verified else "?"
	var short: String = _short_did(_did)
	badge.text = "%s\n%s %s" % [_display_name, mark, short]
	badge.modulate = COLOR_VERIFIED if _verified else COLOR_UNVERIFIED


# "…<last8>" of the did:nostr pubkey hex. An empty DID reads "unverified" — an
# honest absence, never a fabricated identity.
static func _short_did(did: String) -> String:
	if did.strip_edges().is_empty():
		return "unverified"
	var pk: String = did
	var idx: int = did.rfind(":")
	if idx != -1:
		pk = did.substr(idx + 1)
	if pk.length() <= 8:
		return "…" + pk
	return "…" + pk.substr(pk.length() - 8)


func _on_visual_comfort_changed(reduced_motion: bool, _low_cost: bool) -> void:
	_reduced_motion = reduced_motion
