extends RefCounted

## Overview-first rendering rules for the XR graph (ADR-2188 D1, D2, D5).
##
## Pure functions and constants only, so the GUT suite can pin them headless.
## The shaders that draw the overview evaluate the same maths per vertex:
## `impostor_world_size` ↔ materials/node_impostor.gdshader, `ribbon_width` and
## `length_fade` ↔ materials/edge_ribbon.gdshader. The near tiers (gem nodes,
## cylinder edges) stay the close-range level of detail chosen by Rust
## `lod::split_tiers` (NodeLod.NEAR_CAP / NEAR_RADIUS_M); `node_tier` mirrors it.
##
## No bloom anywhere (ADR-2188 D3, XR-client.md Invariant 2): emphasis is colour,
## size and an outline ring, set by the Rust highlight mask.

const NodeLod := preload("res://scripts/node_lod.gd")

## Pixels per radian of the eye buffer. VIVE Pro: 1440 px over about 110° → ~750.
const DEFAULT_PX_PER_RAD: float = 750.0
## Overview node pixel floor (shader `min_px`) and its growth cap in metres.
const NODE_MIN_PX: float = 3.0
const NODE_MAX_FLOOR_M: float = 0.05
## Overview edge width in pixels (shader `width_px`).
const EDGE_WIDTH_PX: float = 1.5
## Length fade (ADR-2181 D2, the cosmos.gl defaults): alpha 1 below NEAR,
## FADE_MIN above FAR, smoothstep between, by projected length in pixels.
const FADE_NEAR_PX: float = 50.0
const FADE_FAR_PX: float = 150.0
const FADE_MIN: float = 0.25
## A greyed edge keeps this fraction of its alpha (shader `grey_alpha`; with the
## edge base alpha 0.35 that is about the 0.1 ADR-2183 D3 starts from).
const GREY_ALPHA: float = 0.3

## Most edges one volume draws: the PRD-008 triangle budget, not a buffer size.
## Ribbons are 2 triangles, so 20 000 ribbons are 40 000 of the 100 000; Rust
## `lod::scene_triangle_estimate` pins the whole scene (13 164 nodes, both near
## tiers full, 32 hulls) at ≤ 97 k with this many. Beyond it the heaviest edges are
## drawn and the HUD says how many were not (ADR-2179 P4: no silent cap).
const EDGE_DRAW_CEILING: int = 20000

const TIER_GEM := 0
const TIER_OVERVIEW := 1


## Pixels per radian at the centre of an eye buffer `size_px` across an FOV of
## `fov_deg` along the same axis (width with horizontal FOV, or height with vertical).
static func px_per_rad(size_px: float, fov_deg: float) -> float:
	if size_px <= 0.0 or fov_deg <= 0.0 or fov_deg >= 180.0:
		return DEFAULT_PX_PER_RAD
	return size_px / (2.0 * tan(deg_to_rad(fov_deg) * 0.5))


## Metres one pixel covers at `depth_m` from the eye.
static func metres_per_px(depth_m: float, ppr: float) -> float:
	return maxf(depth_m, 0.0) / maxf(ppr, 1.0)


## World diameter the impostor is drawn at: its own size, grown so it never
## projects below `min_px`, by at most `max_floor_m`. Metric up close (no pop at
## the gem boundary), pixel-floored at overview range.
static func impostor_world_size(s_world: float, depth_m: float, ppr: float = DEFAULT_PX_PER_RAD,
		min_px: float = NODE_MIN_PX, max_floor_m: float = NODE_MAX_FLOOR_M) -> float:
	var floor_m: float = minf(min_px * metres_per_px(depth_m, ppr), max_floor_m)
	return maxf(s_world, floor_m)


## Projected size of a world diameter in pixels.
static func projected_px(s_world: float, depth_m: float, ppr: float = DEFAULT_PX_PER_RAD) -> float:
	var m: float = metres_per_px(depth_m, ppr)
	return s_world / m if m > 0.0 else INF


## Ribbon width in metres at an end `depth_m` away: `width_px` pixels, never
## thinner than the world cylinder it replaces.
static func ribbon_width(world_width: float, depth_m: float, ppr: float = DEFAULT_PX_PER_RAD,
		width_px: float = EDGE_WIDTH_PX) -> float:
	return maxf(world_width, width_px * metres_per_px(depth_m, ppr))


## Edge alpha multiplier from its projected length in pixels.
static func length_fade(len_px: float) -> float:
	return lerpf(1.0, FADE_MIN, smoothstep(FADE_NEAR_PX, FADE_FAR_PX, len_px))


## Projected length in pixels of the segment a→b seen from `eye`.
static func projected_length_px(a: Vector3, b: Vector3, eye: Vector3, ppr: float = DEFAULT_PX_PER_RAD) -> float:
	var da: Vector3 = a - eye
	var db: Vector3 = b - eye
	if da.length() < 1e-6 or db.length() < 1e-6:
		return 0.0
	return da.angle_to(db) * ppr


## Which tier a node is drawn in, mirroring Rust `lod::split_tiers` without its
## 10 % hysteresis: the gem when it is among the nearest `near_cap` drawn nodes
## (`rank_by_distance`, 0 = nearest) and within `near_radius_m`; else the overview.
static func node_tier(distance_m: float, rank_by_distance: int,
		near_cap: int = NodeLod.NEAR_CAP, near_radius_m: float = NodeLod.NEAR_RADIUS_M) -> int:
	if rank_by_distance < near_cap and distance_m <= near_radius_m:
		return TIER_GEM
	return TIER_OVERVIEW


## Edges a volume asks for: sized from the data (its own edge count), bounded by
## the triangle-budget ceiling. The MultiMesh instance buffers are sized each frame
## from what was packed, so there is no fixed buffer capacity to clip against.
## `share` (0..1, Look › Edges) asks for that fraction of them, heaviest first.
static func edge_draw_limit(total: int, share: float = 1.0) -> int:
	var want: int = ceili(float(maxi(total, 0)) * clampf(share, 0.0, 1.0))
	return mini(want, EDGE_DRAW_CEILING)


## Edges not drawn out of `total`, whatever the cause (the ceiling, or an end not drawn).
static func dropped(total: int, drawn: int) -> int:
	return maxi(total - maxi(drawn, 0), 0)


## HUD readout for the dropped-edge counter; empty when nothing was dropped.
static func edge_readout(drawn: int, total: int) -> String:
	if dropped(total, drawn) <= 0:
		return ""
	return "Edges %s of %s drawn" % [_thousands(maxi(drawn, 0)), _thousands(total)]


static func _thousands(n: int) -> String:
	var s: String = str(n)
	var out: String = ""
	var k: int = 0
	for i: int in range(s.length() - 1, -1, -1):
		out = s[i] + out
		k += 1
		if k % 3 == 0 and i > 0:
			out = " " + out
	return out
