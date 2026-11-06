extends RefCounted

## Priority-ranked label budget for the headset (ADR-2188 D5, ADR-2186).
##
## Labels are chosen by priority, not by who is closest: the scene gathers
## candidates in front of the eye within LABEL_REACH_M, Rust ranks them
## (`BinaryProtocolClient.rank_labels`: wire centrality, then degree, then id;
## greyed-out and unlabelled nodes dropped), and `select` here takes the pinned
## nodes first, then the ranked ones, skipping any whose projected direction
## falls in a cell already labelled (the coarse collision grid of ADR-2186 D2).
##
## Budget. The desktop default is 40 overview + 40 cursor + 100 selection
## labels (ADR-2186 D1). The headset's is DEFAULT_BUDGET = 8 ranked labels plus
## up to PINNED_MAX = 4 pinned ones (selected, hovered, one held per hand), so
## the pool stays at the 12 anchors it had before. Two limits set it:
##   * Draw calls. Every visible Label3D is its own draw call. The scene measures
##     31–34 of PRD-008's 50 (XR-client.md, LOD section), so labels have about 16.
##     Ranked labels draw a title only (1 call) and pinned ones title + detail
##     (2), so the worst case is 8 + 4 × 2 = 16, down from 12 × 2 = 24.
##   * Legibility. Titles are about 1.9 cm tall (LABEL_TITLE_PX); more than about
##     eight of them inside the central field of view overlap and stop reading.

const DEFAULT_BUDGET: int = 8
const PINNED_MAX: int = 4
const POOL_SIZE: int = DEFAULT_BUDGET + PINNED_MAX
## Candidates gathered per refresh (nearest within reach, before ranking).
const CANDIDATES: int = 64
## Reach of the label candidates in world metres, and where their fade starts.
const LABEL_REACH_M: float = 1.5
const LABEL_FULL_M: float = 1.0
## Labelled nodes within this distance turn see-through so their text reads through
## the sphere (the pre-ADR-2188 proximity reach); further out they stay opaque.
const SPHERE_FADE_M: float = 0.5
## Candidates outside this half-angle of the view direction are not labelled.
const VIEW_HALF_ANGLE_DEG: float = 55.0
## Collision grid cell, as an angle (about 2.5°: a short title at 1 m).
const CELL_RAD: float = 0.045


## Choose the labelled ids: `pinned` first (always shown, deduplicated, at most
## PINNED_MAX), then `ranked` in order until `budget` ranked labels are taken.
## `cells` maps id → Vector2i grid cell; a ranked id whose cell is taken by an
## earlier label is skipped. Ids without a cell never collide.
static func select(pinned: PackedInt32Array, ranked: PackedInt32Array, budget: int = DEFAULT_BUDGET,
		cells: Dictionary = {}) -> PackedInt32Array:
	var out := PackedInt32Array()
	var taken := {}
	var used := {}
	for id: int in pinned:
		if out.size() >= PINNED_MAX:
			break
		if used.has(id):
			continue
		used[id] = true
		out.append(id)
		if cells.has(id):
			taken[cells[id]] = true
	var n: int = 0
	for id: int in ranked:
		if n >= budget:
			break
		if used.has(id):
			continue
		if cells.has(id):
			var c: Vector2i = cells[id]
			if taken.has(c):
				continue
			taken[c] = true
		used[id] = true
		out.append(id)
		n += 1
	return out


## Grid cell of a world point seen from a camera transform, or null when it is
## behind the eye or outside VIEW_HALF_ANGLE_DEG.
static func cell_of(cam: Transform3D, world_pos: Vector3) -> Variant:
	var local: Vector3 = cam.affine_inverse() * world_pos
	var forward: float = -local.z
	if forward <= 1e-4:
		return null
	if local.length() > 0.0 and acos(clampf(forward / local.length(), -1.0, 1.0)) > deg_to_rad(VIEW_HALF_ANGLE_DEG):
		return null
	return Vector2i(floori(atan2(local.x, forward) / CELL_RAD), floori(atan2(local.y, forward) / CELL_RAD))


## Label opacity by distance: full within LABEL_FULL_M, 0 at LABEL_REACH_M.
static func fade(distance_m: float) -> float:
	return clampf(1.0 - (distance_m - LABEL_FULL_M) / maxf(LABEL_REACH_M - LABEL_FULL_M, 0.001), 0.0, 1.0)
