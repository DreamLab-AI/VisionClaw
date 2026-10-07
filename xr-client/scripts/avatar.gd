extends Node3D

## Default hand offsets relative to head when full hand tracking data is unavailable.
const DEFAULT_HAND_OFFSET_Y: float = -0.4
const DEFAULT_HAND_OFFSET_X: float = 0.25
const INTERPOLATION_SPEED: float = 12.0

const COLOR_VERIFIED: Color = Color(0.75, 1.0, 0.82)
const COLOR_UNVERIFIED: Color = Color(1.0, 0.72, 0.28)

@onready var head: MeshInstance3D = $Head
@onready var left_hand: MeshInstance3D = $LeftHand
@onready var right_hand: MeshInstance3D = $RightHand
@onready var nameplate: Label3D = $Head/Nameplate
@onready var did_badge: Label3D = $Head/DidBadge
@onready var voice_indicator: MeshInstance3D = $Head/VoiceIndicator

## Identity carried from the presence join (graph_scene.gd:431 sets `did` meta).
var _did: String = ""
var _verified: bool = false

## Current LOD level (0=High, 1=Med, 2=Low, 3=Culled).
var _lod_level: int = 0

## Target transforms for interpolation.
var _target_head_pos: Vector3 = Vector3.ZERO
var _target_head_basis: Basis = Basis.IDENTITY
var _target_left_pos: Vector3 = Vector3.ZERO
var _target_left_basis: Basis = Basis.IDENTITY
var _target_right_pos: Vector3 = Vector3.ZERO
var _target_right_basis: Basis = Basis.IDENTITY
var _has_left: bool = false
var _has_right: bool = false
var _has_received_pose: bool = false


func set_display_name(display_name: String) -> void:
	if nameplate != null:
		nameplate.text = display_name


## Render the actor's did:nostr identity (M1, PRD-023 WP-9). The badge shows the
## display name on the nameplate and a short-DID (…last8) here; `verified`
## reflects the Schnorr-challenge check (ADR-130 Decision 6). An unverified claim
## renders visually distinct (amber + "?") and is never mistaken for a trusted
## identity — the invariant-2 counter to a self-reported nameplate.
func set_avatar_identity(display_name: String, did: String, verified: bool) -> void:
	_did = did
	_verified = verified
	set_display_name(display_name)
	_refresh_did_badge()


func set_verified(verified: bool) -> void:
	_verified = verified
	_refresh_did_badge()


func _refresh_did_badge() -> void:
	if did_badge == null:
		return
	var mark: String = "✓" if _verified else "?"
	did_badge.text = "%s %s" % [mark, _short_did(_did)]
	did_badge.modulate = COLOR_VERIFIED if _verified else COLOR_UNVERIFIED


# "…<last8>" of the did:nostr pubkey hex; empty DID reads "unverified".
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


func apply_pose(
	head_pos: Vector3,
	head_rot: Quaternion,
	has_left: bool,
	has_right: bool,
	left_pos: Vector3 = Vector3.ZERO,
	left_rot: Quaternion = Quaternion.IDENTITY,
	right_pos: Vector3 = Vector3.ZERO,
	right_rot: Quaternion = Quaternion.IDENTITY
) -> void:
	_has_received_pose = true
	_target_head_pos = head_pos
	_target_head_basis = Basis(head_rot)
	_has_left = has_left
	_has_right = has_right

	if has_left:
		if left_pos == Vector3.ZERO and left_rot == Quaternion.IDENTITY:
			_target_left_pos = head_pos + Vector3(-DEFAULT_HAND_OFFSET_X, DEFAULT_HAND_OFFSET_Y, 0.0)
			_target_left_basis = Basis(head_rot)
		else:
			_target_left_pos = left_pos
			_target_left_basis = Basis(left_rot)

	if has_right:
		if right_pos == Vector3.ZERO and right_rot == Quaternion.IDENTITY:
			_target_right_pos = head_pos + Vector3(DEFAULT_HAND_OFFSET_X, DEFAULT_HAND_OFFSET_Y, 0.0)
			_target_right_basis = Basis(head_rot)
		else:
			_target_right_pos = right_pos
			_target_right_basis = Basis(right_rot)


func _process(delta: float) -> void:
	if not _has_received_pose:
		return
	var weight: float = clampf(INTERPOLATION_SPEED * delta, 0.0, 1.0)

	if head != null:
		head.transform.origin = head.transform.origin.lerp(_target_head_pos, weight)
		head.transform.basis = _slerp_rotation(head.transform.basis, _target_head_basis, weight)

	if left_hand != null:
		left_hand.visible = _has_left and _lod_level < 2
		if _has_left:
			left_hand.transform.origin = left_hand.transform.origin.lerp(_target_left_pos, weight)
			left_hand.transform.basis = _slerp_rotation(left_hand.transform.basis, _target_left_basis, weight)

	if right_hand != null:
		right_hand.visible = _has_right and _lod_level < 2
		if _has_right:
			right_hand.transform.origin = right_hand.transform.origin.lerp(_target_right_pos, weight)
			right_hand.transform.basis = _slerp_rotation(right_hand.transform.basis, _target_right_basis, weight)


# Rotation ease via quaternions. Basis.slerp casts both operands to Quaternion
# and requires them orthonormal; easing a Basis toward a target every frame lets
# float error accumulate until the cast fails, Godot logs an error per frame and
# the step is lost — at high frame rates (small delta, many frames) the head
# never reached its target (86° short after 3 000 frames, 2026-10-07). Slerping
# the rotation quaternions and rebuilding the basis keeps it orthonormal; any
# node scale is carried over unchanged.
static func _slerp_rotation(from: Basis, to: Basis, weight: float) -> Basis:
	var q: Quaternion = from.get_rotation_quaternion().slerp(to.get_rotation_quaternion(), weight)
	return Basis(q).scaled(from.get_scale())


func set_lod_level(level: int) -> void:
	_lod_level = level
	match level:
		0:  # High -- everything visible
			visible = true
			_set_label_visibility(true)
			if left_hand != null:
				left_hand.visible = _has_left
			if right_hand != null:
				right_hand.visible = _has_right
		1:  # Medium -- hide nameplate + DID badge (labels drop first)
			visible = true
			_set_label_visibility(false)
			if left_hand != null:
				left_hand.visible = _has_left
			if right_hand != null:
				right_hand.visible = _has_right
		2:  # Low -- hide labels and hands
			visible = true
			_set_label_visibility(false)
			if left_hand != null:
				left_hand.visible = false
			if right_hand != null:
				right_hand.visible = false
		3:  # Culled -- hide entirely
			visible = false


func _set_label_visibility(shown: bool) -> void:
	if nameplate != null:
		nameplate.visible = shown
	if did_badge != null:
		did_badge.visible = shown


func set_speaking(active: bool) -> void:
	if voice_indicator != null:
		voice_indicator.visible = active
