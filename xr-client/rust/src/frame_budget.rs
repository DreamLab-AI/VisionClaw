//! One frame triangle and draw-call budget shared by every XR layer.
//!
//! The headset budget (`perf/README.md`, PRD-008) is 100 000 triangles and 50
//! draw calls a frame. The graph's LOD tiers (`lod.rs`, `hulls.rs`), the memory
//! cloud (`memory_cloud.rs`) and the query route (`memory_route.rs`) all draw
//! from it, so one allocator decides their caps instead of each layer guessing
//! what the others leave.
//!
//! Two passes:
//!
//! 1. **Minimums**, in priority order: triangles outside every budgeted layer
//!    (HUD, avatars, controllers; measured by the scene), the graph's far tiers (an impostor quad
//!    per node, a ribbon quad per edge, labelled nodes on the full mesh), the
//!    route at one sample per hop, a cloud floor that still reads as a cloud,
//!    and a minimum near field of gem nodes.
//! 2. **Growth** up to each layer's demand, in the same priority: the route to
//!    full curve detail, the cloud to its sprite cap, the memory_flash ring
//!    pool (only when no cloud is shown), then the graph's near
//!    detail (hulls, then gem nodes, then cylinder edges; the reverse of the
//!    order they give way).
//!
//! Both passes work inside [`TRI_LIMIT`] / [`DRAW_CALL_LIMIT`]: 5 % of each
//! budget is held back for frame-to-frame variance.
//!
//! The visual extras (`eye_candy.gd`: sky, ribbon volume frames, ambient motes)
//! hold a bounded **allowance** ([`allocate_with_extras`], ADR-2167 as amended by
//! the owner's decision of 2026-10-31). After the minimums, the route, cloud,
//! burst pool and hulls have been served, the cost of the *requested* tier
//! (capped at tier 4) is held back before the graph's gem and cylinder share, so
//! the extras show at production density. The graph loses at most
//! `ceil(allowance / GEM_STEP)` gems; the cloud, route, burst and hull caps are
//! unchanged. When the minimums overrun, or any memory layer or the hulls fall
//! short of their demand, the allowance is 0 and the extras fall back to last
//! claim (only what the graph left). A tier the draw-call guard refuses reserves
//! nothing, and their draw calls are checked against every other layer's final
//! count.
//!
//! The **splat account** (ADR-2170 decision 7, accepted by the owner) sits outside
//! [`TRI_LIMIT`]. A Gaussian splat is a camera-facing quad (2 triangles), so even
//! the smallest tier would spend more than the whole graph leaves. Splat
//! triangles are therefore *reported* ([`FrameCaps::splat_tris`]) and never
//! charged: they are not in `tris_total`, they never set `over_budget`, and the
//! graph's caps are bit-identical with or without a splat demand (tested).
//! Splat **draw calls** are charged, one per visible splat volume and at most
//! [`SPLAT_MAX_VOLUMES`], and they are reserved from [`DRAW_CALL_LIMIT`] *before*
//! the extras allowance, so the extras can never take the calls splats need. A
//! volume the limit has no call for is not granted. Tiers 0 to 3 are 0, 25k, 50k
//! and 100k splats; only an *entered* splat volume (ADR-2166, the graph becomes
//! portals) may draw above [`SPLAT_UNENTERED_MAX_TIER`].
//!
//! Near-tier costs are charged as the increment over the far tier that the
//! node or edge would otherwise use. If the minimums alone exceed the budget
//! the minimums are still returned, with `over_budget` set, so the benchmark
//! reports it rather than a layer silently vanishing.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use crate::hulls::{DEFAULT_MAX_HULLS, MAX_TRIS_PER_HULL};
use crate::lod::{
    CYLINDER_TRIS_PER_EDGE, DEFAULT_NEAR_CAP, DEFAULT_NEAR_EDGE_CAP, GEM_TRIS_PER_NODE,
    IMPOSTOR_TRIS_PER_NODE, RIBBON_TRIS_PER_EDGE,
};
use crate::memory_cloud::{DEFAULT_SPRITE_CAP, TRIANGLES_PER_SPRITE};
use crate::memory_route::{ring_cap_for_budget, route_triangles_at, ROUTE_RING_CAP};

/// Frame triangle budget.
pub const TRI_BUDGET: usize = 100_000;
/// Frame draw-call budget.
pub const DRAW_CALL_BUDGET: usize = 50;
/// 5 % of the triangle budget held back for frame-to-frame variance (label
/// glyphs, controller models, a hull rebuild landing mid-frame).
pub const TRI_RESERVE: usize = TRI_BUDGET / 20;
/// 5 % of the draw-call budget, rounded down to whole calls.
pub const DRAW_CALL_RESERVE: usize = DRAW_CALL_BUDGET / 20;
/// What the allocator hands out: the budgets less the reserves.
pub const TRI_LIMIT: usize = TRI_BUDGET - TRI_RESERVE;
pub const DRAW_CALL_LIMIT: usize = DRAW_CALL_BUDGET - DRAW_CALL_RESERVE;
/// memory_flash ring pool (xr-pulse `memory_bursts.gd`): POOL_SIZE slots, a
/// RING_SEGMENTS-sided annulus each (2 triangles a segment), one draw call.
/// Used only when no cloud is shown; with the cloud on, bursts restyle
/// sprites (`set_row_emphasis`) and cost nothing. A GUT test checks these
/// against the built mesh.
pub const BURST_POOL_SLOTS: usize = 64;
pub const BURST_TRIS_PER_SLOT: usize = 2 * 32;
pub const BURST_DRAW_CALLS: usize = 1;
/// Fewest cloud sprites that still read as a cloud (a quarter of the cap).
pub const CLOUD_MIN_SPRITES: usize = 2_000;
/// Fewest gem nodes, so the near field never vanishes.
pub const GEM_MIN: usize = 16;
/// Draw calls the cloud adds when shown (one sprite MultiMesh).
pub const CLOUD_DRAW_CALLS: usize = 1;
/// Draw calls a shown route adds (tube surface, bead discs, rings).
pub const ROUTE_DRAW_CALLS: usize = 3;

/// Highest extras tier (see [`extras_cost`]).
pub const EXTRAS_MAX_TIER: usize = 4;
/// Volume frames the extras layer prices; more are charged as this many.
pub const EXTRAS_MAX_FRAMES: usize = 8;
/// A ribbon frame: 12 edges, one quad (2 triangles) each, one draw call per
/// volume (`emulated_volume.gd`). A GUT test checks it against the built mesh.
pub const FRAME_RIBBON_TRIS: usize = 12 * 2;
/// Ambient motes at tier 3 and tier 4: one MultiMesh, one triangle per mote,
/// one draw call (`eye_candy.gd`). A GUT test checks them against the node.
pub const MOTES_LOW: usize = 128;
pub const MOTES_HIGH: usize = 512;
pub const MOTE_TRIS: usize = 1;

const GEM_STEP: usize = GEM_TRIS_PER_NODE - IMPOSTOR_TRIS_PER_NODE;
const CYLINDER_STEP: usize = CYLINDER_TRIS_PER_EDGE - RIBBON_TRIS_PER_EDGE;

/// What the graph wants drawn this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphDemand {
    /// Drawn nodes.
    pub nodes: usize,
    /// Drawn edges.
    pub edges: usize,
    /// Hulls present (0 when hulls are off).
    pub hulls: usize,
    /// Triangles in the current hull mesh, as measured.
    pub hull_tris: usize,
    /// Labelled nodes that must stay on the full gem mesh.
    pub faded: usize,
    /// The graph's own draw calls.
    pub draw_calls: usize,
    /// Triangles outside every budgeted layer (HUD, avatars, controllers,
    /// labels), measured by the scene; reserved before anything else.
    pub other_tris: usize,
}

/// The query route being shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteShape {
    /// Path rows, root to answer.
    pub rows: usize,
    /// Sidecar marks.
    pub sidecar: usize,
}

/// What the memory layers want drawn this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryDemand {
    /// Snapshot rows the cloud would draw; 0 when the cloud is hidden.
    pub cloud_rows: usize,
    /// The route, when one is shown.
    pub route: Option<RouteShape>,
    /// Ring-pool slots that may draw (`BURST_POOL_SLOTS` when bursts are on
    /// and no cloud is shown, else 0).
    pub burst_slots: usize,
}

/// What the visual extras (`eye_candy.gd`) want drawn this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtrasDemand {
    /// Requested tier, 0 = off (see [`extras_cost`]).
    pub tier: usize,
    /// Volume frames that would draw ribbons.
    pub frames: usize,
}

/// Triangles and draw calls the extras cost at `tier` with `frames` volume
/// frames. Tiers add to each other, ordered by triangle cost so that something
/// still shows when the graph has used every spare triangle:
///
/// | tier | adds | triangles | draw calls |
/// |------|------|-----------|------------|
/// | 0 | nothing | 0 | 0 |
/// | 1 | static gradient sky with fixed stars, on the Environment | 0 | 0 |
/// | 2 | ribbon volume frames | 24 a frame | 0 (the frame's own call is counted by the scene) |
/// | 3 | 128 ambient motes | 128 | 1 |
/// | 4 | 512 ambient motes | 512 | 1 (the same call) |
///
/// Totals with three frames: tier 2 costs 72, tier 3 costs 200 and tier 4 costs
/// 584 triangles; tier 4 is the largest allowance the allocator holds back.
///
/// The sky has no geometry (it is the scene's background), so it costs
/// per-pixel work only. Tiers above [`EXTRAS_MAX_TIER`] and frames above
/// [`EXTRAS_MAX_FRAMES`] clamp.
pub fn extras_cost(tier: usize, frames: usize) -> (usize, usize) {
    let tier = tier.min(EXTRAS_MAX_TIER);
    let frames = frames.min(EXTRAS_MAX_FRAMES);
    let mut tris = 0;
    let mut calls = 0;
    if tier >= 2 {
        // The ribbons replace the 1 px lines in the same draw call, and the
        // scene counts that call (`overhead_draw_calls`), so tier 2 adds none.
        tris += frames * FRAME_RIBBON_TRIS;
    }
    if tier >= 3 {
        let motes = if tier >= 4 { MOTES_HIGH } else { MOTES_LOW };
        tris += motes * MOTE_TRIS;
        calls += 1;
    }
    (tris, calls)
}

/// Highest splat tier (see [`splat_count`]).
pub const SPLAT_MAX_TIER: usize = 3;
/// Splats drawn at each tier (ADR-2170 decision 7): the headset variants
/// `xr-1`, `xr-2` and `xr-3` (`visionclaw_splat::VariantName::headset_tier`).
pub const SPLAT_TIER_COUNTS: [usize; SPLAT_MAX_TIER + 1] = [0, 25_000, 50_000, 100_000];
/// A splat is one camera-facing quad.
pub const SPLAT_TRIS_PER_SPLAT: usize = 2;
/// Splat volumes that may draw at once, one draw call each.
pub const SPLAT_MAX_VOLUMES: usize = 2;
/// The highest tier a splat volume that is not the entered room may draw.
pub const SPLAT_UNENTERED_MAX_TIER: usize = 1;

/// Splats drawn at `tier` (clamped to [`SPLAT_MAX_TIER`]).
pub fn splat_count(tier: usize) -> usize {
    SPLAT_TIER_COUNTS[tier.min(SPLAT_MAX_TIER)]
}

/// What the splat volumes want drawn this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SplatDemand {
    /// Requested tier, 0 = off (low cost, no splat volume, or the governor).
    pub tier: usize,
    /// Splat volumes that are visible and loaded; more than
    /// [`SPLAT_MAX_VOLUMES`] are charged as that many.
    pub volumes: usize,
    /// True while a splat volume is the entered room (ADR-2166). That volume is
    /// the first of `volumes`; it alone may draw above
    /// [`SPLAT_UNENTERED_MAX_TIER`].
    pub entered: bool,
}

/// What the splat account granted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SplatGrant {
    /// Volumes granted a draw call.
    pub volumes: usize,
    /// Tier of the first granted volume (the entered room when `entered`).
    pub tier: usize,
    /// Tier every other granted volume draws.
    pub portal_tier: usize,
    /// Splats drawn across the granted volumes.
    pub count: usize,
    /// `count` quads at 2 triangles; reported, never charged to the graph.
    pub tris: usize,
    /// Draw calls spent (one per granted volume).
    pub draw_calls: usize,
}

/// The splat account's grant for `d` when every other layer already makes
/// `other_calls` draw calls. Pure; [`allocate_with_splats`] reserves the result
/// before the extras.
///
/// The first volume draws `min(d.tier, 3)` when `d.entered` and
/// `min(d.tier, SPLAT_UNENTERED_MAX_TIER)` otherwise; every further volume draws
/// at most [`SPLAT_UNENTERED_MAX_TIER`]. Volumes are granted in order while a
/// call remains under [`DRAW_CALL_LIMIT`]; tier 0 grants nothing.
pub fn splat_grant(d: &SplatDemand, other_calls: usize) -> SplatGrant {
    let tier = d.tier.min(SPLAT_MAX_TIER);
    if tier == 0 {
        return SplatGrant::default();
    }
    let room = DRAW_CALL_LIMIT.saturating_sub(other_calls);
    let volumes = d.volumes.min(SPLAT_MAX_VOLUMES).min(room);
    if volumes == 0 {
        return SplatGrant::default();
    }
    let first = if d.entered {
        tier
    } else {
        tier.min(SPLAT_UNENTERED_MAX_TIER)
    };
    let portal_tier = tier.min(SPLAT_UNENTERED_MAX_TIER);
    let count = splat_count(first) + (volumes - 1) * splat_count(portal_tier);
    SplatGrant {
        volumes,
        tier: first,
        portal_tier,
        count,
        tris: count * SPLAT_TRIS_PER_SPLAT,
        draw_calls: volumes,
    }
}

/// Caps for the graph's near tiers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphCaps {
    /// Gem-tier node cap (`lod::DEFAULT_NEAR_CAP` at most).
    pub gem_nodes: usize,
    /// Cylinder-tier edge cap (`lod::DEFAULT_NEAR_EDGE_CAP` at most).
    pub cylinder_edges: usize,
    /// Hulls to draw.
    pub max_hulls: usize,
}

/// One frame's allocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameCaps {
    pub graph: GraphCaps,
    /// Cloud sprite cap (`memory_cloud::CloudState::set_cap`).
    pub cloud_sprites: usize,
    /// Triangles granted to the route.
    pub route_tris: usize,
    /// Centreline sample cap for the route (`memory_route::ActiveRoute::set_ring_cap`).
    pub route_ring_cap: usize,
    /// Ring-pool slots the bursts may draw.
    pub burst_slots: usize,
    /// Triangles all layers spend at these caps (worst case for the graph's
    /// near tiers: every cap filled).
    pub tris_total: usize,
    /// Draw calls all layers spend (extras included).
    pub draw_calls: usize,
    /// Extras tier granted (0 when none was asked for or nothing fitted).
    pub extras_tier: usize,
    /// Triangles the granted extras spend (counted in `tris_total`).
    pub extras_tris: usize,
    /// Draw calls the granted extras spend (counted in `draw_calls`).
    pub extras_draw_calls: usize,
    /// Triangles held back from the graph's near detail for the extras (the
    /// allowance; counted in `extras_tris`). 0 under pressure, when the extras
    /// only buy what the graph left.
    pub extras_allowance: usize,
    /// Splat volumes granted a draw call (counted in `draw_calls`).
    pub splat_volumes: usize,
    /// Tier the first granted splat volume draws (the entered room when one is
    /// entered).
    pub splat_tier: usize,
    /// Tier every other granted splat volume draws.
    pub splat_portal_tier: usize,
    /// Splats drawn across the granted volumes.
    pub splat_count: usize,
    /// Triangles those splats draw. Reported only: outside `tris_total`,
    /// `TRI_LIMIT` and `over_budget` (ADR-2170 decision 7).
    pub splat_tris: usize,
    /// Draw calls the splats spend (counted in `draw_calls`, reserved before
    /// the extras).
    pub splat_draw_calls: usize,
    /// The minimums alone exceed a budget.
    pub over_budget: bool,
}

/// Divide the frame budget between the layers (see the module docs), with no
/// visual extras: [`allocate_with_extras`] asking for tier 0.
pub fn allocate(g: &GraphDemand, m: &MemoryDemand) -> FrameCaps {
    allocate_with_extras(g, m, &ExtrasDemand::default())
}

/// [`allocate`] plus the visual-extras layer, in this order:
///
/// 1. the hard minimums (far tiers, route floor, cloud floor, gem floor);
/// 2. growth of the route, cloud, burst pool and hulls, as in [`allocate`];
/// 3. the **allowance**: the highest tier at or below `min(x.tier, 4)` whose
///    triangles fit in what is left and whose draw calls keep the frame at or
///    under [`DRAW_CALL_LIMIT`] is paid for here, before the near detail;
/// 4. the graph's gem nodes, then cylinder edges, from the remainder.
///
/// The allowance is never above the cost of the requested tier, so a governor
/// or comfort setting that pins a lower tier reserves less and the difference
/// stays with the near detail. The cloud, route, burst and hull caps equal
/// [`allocate`]'s and the graph loses at most `ceil(allowance / GEM_STEP)` gems
/// (tested). When the minimums overrun, or the route, cloud, burst pool or hulls
/// get less than they asked for, the allowance is 0 ([`FrameCaps::extras_allowance`])
/// and the extras are the last claim: the highest tier the graph's leftover
/// pays for, so only the zero-cost tiers remain when nothing is left. Draw calls
/// are checked against every other layer's final count, with the burst pool at
/// its granted size.
pub fn allocate_with_extras(g: &GraphDemand, m: &MemoryDemand, x: &ExtrasDemand) -> FrameCaps {
    allocate_with_splats(g, m, x, &SplatDemand::default())
}

/// [`allocate_with_extras`] plus the splat account (ADR-2170 decision 7; see the
/// module docs). The splat draw calls are reserved from [`DRAW_CALL_LIMIT`]
/// after every other layer's calls and *before* the extras are priced, so an
/// extras tier that needs a call the splats hold is refused instead. The
/// triangle side is untouched: every graph, cloud, route, burst, hull and extras
/// field equals what [`allocate_with_extras`] returns for the same inputs
/// whenever the splat calls leave the extras their room (tested), and the splat
/// triangles are only reported in [`FrameCaps::splat_tris`].
pub fn allocate_with_splats(
    g: &GraphDemand,
    m: &MemoryDemand,
    x: &ExtrasDemand,
    sd: &SplatDemand,
) -> FrameCaps {
    let faded = g.faded.min(g.nodes);
    let far = g.other_tris
        + g.nodes * IMPOSTOR_TRIS_PER_NODE
        + g.edges * RIBBON_TRIS_PER_EDGE
        + faded * GEM_STEP;

    let route_min = m
        .route
        .map_or(0, |r| route_triangles_at(r.rows, r.sidecar, 0));
    let cloud_want = m.cloud_rows.min(DEFAULT_SPRITE_CAP);
    let cloud_min = cloud_want.min(CLOUD_MIN_SPRITES);
    let gem_candidates = g.nodes - faded;
    let gem_max = gem_candidates.min(DEFAULT_NEAR_CAP);
    let gem_min = gem_max.min(GEM_MIN);

    let minimums = far + route_min + cloud_min * TRIANGLES_PER_SPRITE + gem_min * GEM_STEP;
    let over_tris = minimums > TRI_LIMIT;
    let mut rem = TRI_LIMIT.saturating_sub(minimums);

    // route to full detail
    let (route_tris, route_ring_cap) = match m.route {
        Some(r) => {
            let cap = ring_cap_for_budget(r.rows, r.sidecar, route_min + rem);
            let t = route_triangles_at(r.rows, r.sidecar, cap);
            rem -= t - route_min;
            (t, cap)
        }
        None => (0, ROUTE_RING_CAP),
    };

    // cloud to its cap
    let cloud_sprites = cloud_want.min(cloud_min + rem / TRIANGLES_PER_SPRITE);
    rem -= (cloud_sprites - cloud_min) * TRIANGLES_PER_SPRITE;

    // burst ring pool, as many slots as fit
    let burst_slots = m
        .burst_slots
        .min(BURST_POOL_SLOTS)
        .min(rem / BURST_TRIS_PER_SLOT);
    rem -= burst_slots * BURST_TRIS_PER_SLOT;

    // hulls: all of the measured mesh, or as many worst-case hulls as fit
    let hulls_present = g.hulls.min(DEFAULT_MAX_HULLS);
    let (max_hulls, hull_cost) = if hulls_present == 0 || g.hull_tris == 0 {
        (hulls_present, 0)
    } else if g.hull_tris <= rem {
        (hulls_present, g.hull_tris)
    } else {
        let n = (rem / MAX_TRIS_PER_HULL).min(hulls_present);
        (n, n * MAX_TRIS_PER_HULL)
    };
    rem -= hull_cost;

    // draw calls every other layer makes at its granted size
    let base_calls = g.draw_calls
        + if cloud_sprites > 0 {
            CLOUD_DRAW_CALLS
        } else {
            0
        }
        + if m.route.is_some() {
            ROUTE_DRAW_CALLS
        } else {
            0
        }
        + if burst_slots > 0 { BURST_DRAW_CALLS } else { 0 };
    // splat calls come off the limit before the extras can ask for any
    let splat = splat_grant(sd, base_calls);
    let held_calls = base_calls + splat.draw_calls;
    let fits_calls = |calls: usize| held_calls + calls <= DRAW_CALL_LIMIT.max(held_calls);
    let top = x.tier.min(EXTRAS_MAX_TIER);

    // extras allowance (ADR-2167, owner decision 2026-10-31): the cost of the
    // requested tier, capped at the top tier, is held back from the near-detail
    // share. It is taken only when every hard minimum and every memory layer and
    // the hulls got what they asked for, so it can never push anything below
    // its minimum; under pressure it is 0 and the extras fall back to last claim.
    let pressure = over_tris
        || (m.route.is_some() && route_ring_cap < ROUTE_RING_CAP)
        || cloud_sprites < cloud_want
        || burst_slots < m.burst_slots.min(BURST_POOL_SLOTS)
        || max_hulls < hulls_present;
    let mut extras_tier = 0;
    let (mut extras_tris, mut extras_draw_calls) = (0, 0);
    if !pressure {
        // A tier the draw-call guard refuses reserves nothing: its triangles
        // stay with the near detail below.
        for t in (1..=top).rev() {
            let (tris, calls) = extras_cost(t, x.frames);
            if tris <= rem && fits_calls(calls) {
                extras_tier = t;
                (extras_tris, extras_draw_calls) = (tris, calls);
                break;
            }
        }
        rem -= extras_tris;
    }

    // gem nodes, then cylinder edges
    let gem_nodes = gem_max.min(gem_min + rem / GEM_STEP);
    rem -= (gem_nodes - gem_min) * GEM_STEP;
    let cylinder_edges = g.edges.min(DEFAULT_NEAR_EDGE_CAP).min(rem / CYLINDER_STEP);
    rem -= cylinder_edges * CYLINDER_STEP;

    // last claim: with no allowance held, the extras buy what the graph left
    if pressure {
        for t in (1..=top).rev() {
            let (tris, calls) = extras_cost(t, x.frames);
            if tris <= rem && fits_calls(calls) {
                extras_tier = t;
                (extras_tris, extras_draw_calls) = (tris, calls);
                break;
            }
        }
        rem -= extras_tris;
    }
    let spent = TRI_LIMIT.saturating_sub(rem);
    let tris_total = if over_tris {
        minimums + hull_cost + extras_tris
    } else {
        spent
    };
    let draw_calls = held_calls + extras_draw_calls;

    FrameCaps {
        graph: GraphCaps {
            gem_nodes,
            cylinder_edges,
            max_hulls,
        },
        cloud_sprites,
        route_tris,
        route_ring_cap,
        burst_slots,
        tris_total,
        draw_calls,
        extras_tier,
        extras_tris,
        extras_draw_calls,
        extras_allowance: if pressure { 0 } else { extras_tris },
        splat_volumes: splat.volumes,
        splat_tier: splat.tier,
        splat_portal_tier: splat.portal_tier,
        splat_count: splat.count,
        splat_tris: splat.tris,
        splat_draw_calls: splat.draw_calls,
        over_budget: over_tris || base_calls > DRAW_CALL_LIMIT,
    }
}

/// Split one near-tier cap (gem nodes or cylinder edges) between the visible volumes
/// (ADR-2159 D4). The budget stays one budget: [`allocate`] runs once on the summed demand of
/// every visible volume, and this divides the resulting cap.
///
/// `demands[i]` is volume *i*'s candidate count (its drawn non-faded nodes, or its drawn
/// edges); a hidden volume passes 0 and gets 0. Rule:
/// 1. **Floor.** Each volume first gets `min(demand, floor)`, so no visible volume loses its
///    near field entirely. If the floors alone exceed `total`, the cap is dealt one at a time
///    in volume order instead.
/// 2. **Proportional.** The rest is shared in proportion to each volume's unmet demand
///    (largest-remainder rounding, ties to the lower index), never past a volume's demand.
///    Whatever a capped volume cannot use is offered to the others until nothing is left
///    or every demand is met.
///
/// Invariants (tested): `sum(out) <= total`, `out[i] <= demands[i]`, and
/// `sum(out) == min(total, sum(demands))`.
pub fn split_near_caps(total: usize, demands: &[usize], floor: usize) -> Vec<usize> {
    let mut out = vec![0usize; demands.len()];
    let want: usize = demands.iter().sum();
    let mut left = total.min(want);
    let floors: Vec<usize> = demands.iter().map(|&d| d.min(floor)).collect();
    if floors.iter().sum::<usize>() > left {
        // Deal round-robin up to each floor.
        while left > 0 {
            let mut gave = false;
            for i in 0..out.len() {
                if left == 0 {
                    break;
                }
                if out[i] < floors[i] {
                    out[i] += 1;
                    left -= 1;
                    gave = true;
                }
            }
            if !gave {
                break;
            }
        }
        return out;
    }
    for i in 0..out.len() {
        out[i] = floors[i];
        left -= floors[i];
    }
    while left > 0 {
        let unmet: Vec<usize> = demands.iter().zip(&out).map(|(&d, &o)| d - o).collect();
        let unmet_sum: usize = unmet.iter().sum();
        if unmet_sum == 0 {
            break;
        }
        let pool = left.min(unmet_sum);
        let mut shares: Vec<usize> = Vec::with_capacity(out.len());
        let mut rems: Vec<(usize, usize)> = Vec::with_capacity(out.len());
        let mut given = 0usize;
        for (i, &u) in unmet.iter().enumerate() {
            let num = pool * u;
            let share = (num / unmet_sum).min(u);
            shares.push(share);
            rems.push((num % unmet_sum, i));
            given += share;
        }
        // Largest remainder first; ties to the lower index.
        rems.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut extra = pool - given;
        for &(_, i) in &rems {
            if extra == 0 {
                break;
            }
            if shares[i] < unmet[i] {
                shares[i] += 1;
                extra -= 1;
            }
        }
        let round: usize = shares.iter().sum();
        for i in 0..out.len() {
            out[i] += shares[i];
        }
        left -= round;
        if round == 0 {
            break;
        }
    }
    out
}

/// Weight clamp: NaN and negatives are 0, infinities and huge values are capped so
/// products stay finite.
fn clean_weight(w: f64) -> f64 {
    if w.is_nan() || w <= 0.0 {
        0.0
    } else {
        w.min(1e12)
    }
}

/// [`split_near_caps`] with a weight per volume (ADR-2159 D4, portals): the
/// proportional step shares in proportion to *unmet demand x weight*, that is
/// drawn x weight at the start, instead of unmet demand alone.
///
/// * `weights[i]` is volume *i*'s weight; a missing entry is 1.0, and NaN or a
///   negative is 0.
/// * **Identity.** An empty `weights`, or weights equal across every volume that
///   has demand, take the unweighted path unchanged: the result is exactly
///   [`split_near_caps`]'s (tested). A hidden volume's weight never counts.
/// * **Floor.** Every volume still gets `min(demand, floor)` first, weight or no
///   weight, exactly as the unweighted split does (and when the cap cannot cover
///   the floors, the unweighted round-robin deals it).
/// * A zero weight asks for the floor only. When nobody with weight has unmet
///   demand left, the remaining cap goes to the zero-weight volumes unweighted,
///   so `sum(out) == min(total, sum(demands))` still holds.
pub fn split_near_caps_weighted(
    total: usize,
    demands: &[usize],
    weights: &[f64],
    floor: usize,
) -> Vec<usize> {
    let w: Vec<f64> = (0..demands.len())
        .map(|i| clean_weight(weights.get(i).copied().unwrap_or(1.0)))
        .collect();
    let mut live = demands
        .iter()
        .zip(&w)
        .filter(|(d, _)| **d > 0)
        .map(|(_, w)| *w);
    let equal = match live.next() {
        None => true,
        Some(first) => live.all(|x| x == first),
    };
    let want: usize = demands.iter().sum();
    let left0 = total.min(want);
    let floors: Vec<usize> = demands.iter().map(|&d| d.min(floor)).collect();
    if equal || floors.iter().sum::<usize>() > left0 {
        return split_near_caps(total, demands, floor);
    }
    let mut out = floors.clone();
    let mut left = left0 - floors.iter().sum::<usize>();
    while left > 0 {
        let unmet: Vec<usize> = demands.iter().zip(&out).map(|(&d, &o)| d - o).collect();
        let unmet_sum: usize = unmet.iter().sum();
        if unmet_sum == 0 {
            break;
        }
        let mut a: Vec<f64> = unmet.iter().zip(&w).map(|(&u, &w)| u as f64 * w).collect();
        let mut a_sum: f64 = a.iter().sum();
        if a_sum <= 0.0 {
            // only zero-weight volumes still want cap: share it unweighted
            a = unmet.iter().map(|&u| u as f64).collect();
            a_sum = unmet_sum as f64;
        }
        let pool = left.min(unmet_sum);
        let mut shares = vec![0usize; out.len()];
        let mut fracs: Vec<(f64, usize)> = Vec::with_capacity(out.len());
        for i in 0..out.len() {
            let exact = pool as f64 * a[i] / a_sum;
            let share = (exact.floor() as usize).min(unmet[i]);
            shares[i] = share;
            fracs.push((exact - exact.floor(), i));
        }
        let mut given: usize = shares.iter().sum();
        // floating-point guard: never hand out more than the pool
        while given > pool {
            let i = (0..shares.len()).max_by_key(|&i| shares[i]).unwrap_or(0);
            shares[i] -= 1;
            given -= 1;
        }
        // largest remainder first; ties to the lower index
        fracs.sort_by(|x, y| {
            y.0.partial_cmp(&x.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(x.1.cmp(&y.1))
        });
        let mut extra = pool - given;
        for &(_, i) in &fracs {
            if extra == 0 {
                break;
            }
            if a[i] > 0.0 && shares[i] < unmet[i] {
                shares[i] += 1;
                extra -= 1;
            }
        }
        let round: usize = shares.iter().sum();
        for i in 0..out.len() {
            out[i] += shares[i];
        }
        left -= round;
        if round == 0 {
            break;
        }
    }
    out
}

// ── Godot adapter ──

#[cfg(not(test))]
use godot::prelude::*;

/// A [`FrameCaps`] as the dictionary GDScript reads.
#[cfg(not(test))]
fn caps_dictionary(c: &FrameCaps) -> Dictionary {
    let mut d = Dictionary::new();
    d.set("gem_nodes", c.graph.gem_nodes as i64);
    d.set("cylinder_edges", c.graph.cylinder_edges as i64);
    d.set("max_hulls", c.graph.max_hulls as i64);
    d.set("cloud_sprites", c.cloud_sprites as i64);
    d.set("route_tris", c.route_tris as i64);
    d.set("route_ring_cap", c.route_ring_cap as i64);
    d.set("burst_slots", c.burst_slots as i64);
    d.set("tris_total", c.tris_total as i64);
    d.set("draw_calls", c.draw_calls as i64);
    d.set("extras_tier", c.extras_tier as i64);
    d.set("extras_tris", c.extras_tris as i64);
    d.set("extras_draw_calls", c.extras_draw_calls as i64);
    d.set("extras_allowance", c.extras_allowance as i64);
    d.set("splat_volumes", c.splat_volumes as i64);
    d.set("splat_tier", c.splat_tier as i64);
    d.set("splat_portal_tier", c.splat_portal_tier as i64);
    d.set("splat_count", c.splat_count as i64);
    d.set("splat_tris", c.splat_tris as i64);
    d.set("splat_draw_calls", c.splat_draw_calls as i64);
    d.set("over_budget", c.over_budget);
    d
}

/// Godot face of [`allocate`]: `FrameBudget.new().allocate(...)`.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(base = RefCounted)]
pub struct FrameBudget {
    base: Base<RefCounted>,
}

// Hand-written so `FrameBudget.new()` still works; the derive's generated
// `init` spelled the struct literal `base: base`.
#[cfg(not(test))]
#[godot_api]
impl IRefCounted for FrameBudget {
    fn init(base: Base<RefCounted>) -> Self {
        Self { base }
    }
}

#[cfg(not(test))]
#[godot_api]
impl FrameBudget {
    /// Caps for one frame. `other_tris` = triangles outside the budgeted layers
    /// (HUD, avatars, controllers); `route_rows` 0 = no route shown;
    /// `cloud_rows` 0 = cloud hidden. Returns gem_nodes, cylinder_edges, max_hulls,
    /// cloud_sprites, route_tris, route_ring_cap, tris_total, draw_calls,
    /// over_budget.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn allocate(
        &self,
        nodes: i64,
        edges: i64,
        hulls: i64,
        hull_tris: i64,
        faded: i64,
        other_tris: i64,
        graph_draw_calls: i64,
        cloud_rows: i64,
        route_rows: i64,
        route_sidecar: i64,
        burst_slots: i64,
    ) -> Dictionary {
        let u = |v: i64| v.max(0) as usize;
        let g = GraphDemand {
            nodes: u(nodes),
            edges: u(edges),
            hulls: u(hulls),
            hull_tris: u(hull_tris),
            faded: u(faded),
            draw_calls: u(graph_draw_calls),
            other_tris: u(other_tris),
        };
        let route = (route_rows >= 2).then(|| RouteShape {
            rows: u(route_rows),
            sidecar: u(route_sidecar),
        });
        let m = MemoryDemand {
            cloud_rows: u(cloud_rows),
            route,
            burst_slots: u(burst_slots),
        };
        caps_dictionary(&allocate(&g, &m))
    }

    /// [`allocate`] plus the visual-extras layer: the same eleven arguments, then
    /// the requested `extras_tier` (0 = off) and the volume `extras_frames` that
    /// would draw ribbons. The result adds `extras_tier`, `extras_tris` and
    /// `extras_draw_calls`; every other key equals `allocate`'s.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn allocate_with_extras(
        &self,
        nodes: i64,
        edges: i64,
        hulls: i64,
        hull_tris: i64,
        faded: i64,
        other_tris: i64,
        graph_draw_calls: i64,
        cloud_rows: i64,
        route_rows: i64,
        route_sidecar: i64,
        burst_slots: i64,
        extras_tier: i64,
        extras_frames: i64,
    ) -> Dictionary {
        let u = |v: i64| v.max(0) as usize;
        let g = GraphDemand {
            nodes: u(nodes),
            edges: u(edges),
            hulls: u(hulls),
            hull_tris: u(hull_tris),
            faded: u(faded),
            draw_calls: u(graph_draw_calls),
            other_tris: u(other_tris),
        };
        let route = (route_rows >= 2).then(|| RouteShape {
            rows: u(route_rows),
            sidecar: u(route_sidecar),
        });
        let m = MemoryDemand {
            cloud_rows: u(cloud_rows),
            route,
            burst_slots: u(burst_slots),
        };
        let x = ExtrasDemand {
            tier: u(extras_tier),
            frames: u(extras_frames),
        };
        caps_dictionary(&allocate_with_extras(&g, &m, &x))
    }

    /// [`allocate_with_extras`] plus the splat account (ADR-2170): the same
    /// thirteen arguments, then a `splat` dictionary {tier: requested tier, 0 =
    /// off; volumes: visible loaded splat volumes; entered: whether a splat
    /// volume is the entered room} (gdext methods take at most 15 arguments, so
    /// the demand travels as one). The result adds `splat_volumes`, `splat_tier`, `splat_portal_tier`,
    /// `splat_count`, `splat_tris` and `splat_draw_calls`; `splat_tris` is
    /// reported only and is not in `tris_total`.
    #[func]
    #[allow(clippy::too_many_arguments)]
    fn allocate_with_splats(
        &self,
        nodes: i64,
        edges: i64,
        hulls: i64,
        hull_tris: i64,
        faded: i64,
        other_tris: i64,
        graph_draw_calls: i64,
        cloud_rows: i64,
        route_rows: i64,
        route_sidecar: i64,
        burst_slots: i64,
        extras_tier: i64,
        extras_frames: i64,
        splat: Dictionary,
    ) -> Dictionary {
        let u = |v: i64| v.max(0) as usize;
        let g = GraphDemand {
            nodes: u(nodes),
            edges: u(edges),
            hulls: u(hulls),
            hull_tris: u(hull_tris),
            faded: u(faded),
            draw_calls: u(graph_draw_calls),
            other_tris: u(other_tris),
        };
        let route = (route_rows >= 2).then(|| RouteShape {
            rows: u(route_rows),
            sidecar: u(route_sidecar),
        });
        let m = MemoryDemand {
            cloud_rows: u(cloud_rows),
            route,
            burst_slots: u(burst_slots),
        };
        let x = ExtrasDemand {
            tier: u(extras_tier),
            frames: u(extras_frames),
        };
        let field = |key: &str| {
            splat
                .get(key)
                .and_then(|v| v.try_to::<i64>().ok())
                .unwrap_or(0)
        };
        let sd = SplatDemand {
            tier: u(field("tier")),
            volumes: u(field("volumes")),
            entered: splat
                .get("entered")
                .and_then(|v| v.try_to::<bool>().ok())
                .unwrap_or(false),
        };
        caps_dictionary(&allocate_with_splats(&g, &m, &x, &sd))
    }

    /// The splat account's constants: max_tier, tier_counts (index = tier),
    /// tris_per_splat, max_volumes, unentered_max_tier.
    #[func]
    fn splat_spec(&self) -> Dictionary {
        let mut d = Dictionary::new();
        d.set("max_tier", SPLAT_MAX_TIER as i64);
        let counts: Vec<i64> = SPLAT_TIER_COUNTS.iter().map(|&c| c as i64).collect();
        d.set("tier_counts", PackedInt64Array::from(counts.as_slice()));
        d.set("tris_per_splat", SPLAT_TRIS_PER_SPLAT as i64);
        d.set("max_volumes", SPLAT_MAX_VOLUMES as i64);
        d.set("unentered_max_tier", SPLAT_UNENTERED_MAX_TIER as i64);
        d
    }

    /// [triangles, draw calls] of extras `tier` with `frames` volume frames
    /// ([`extras_cost`]).
    #[func]
    fn extras_cost(&self, tier: i64, frames: i64) -> PackedInt32Array {
        let (t, c) = extras_cost(tier.max(0) as usize, frames.max(0) as usize);
        PackedInt32Array::from(&[t as i32, c as i32][..])
    }

    /// The extras layer's constants: max_tier, max_frames, frame_tris, motes_low,
    /// motes_high, mote_tris.
    #[func]
    fn extras_spec(&self) -> Dictionary {
        let mut d = Dictionary::new();
        d.set("max_tier", EXTRAS_MAX_TIER as i64);
        d.set("max_frames", EXTRAS_MAX_FRAMES as i64);
        d.set("frame_tris", FRAME_RIBBON_TRIS as i64);
        d.set("motes_low", MOTES_LOW as i64);
        d.set("motes_high", MOTES_HIGH as i64);
        d.set("mote_tris", MOTE_TRIS as i64);
        d
    }

    /// [slots, triangles per slot] the allocator assumes for the burst pool.
    #[func]
    fn burst_pool_spec(&self) -> PackedInt32Array {
        PackedInt32Array::from(&[BURST_POOL_SLOTS as i32, BURST_TRIS_PER_SLOT as i32][..])
    }

    /// [`split_near_caps`]: share `total` between volumes by `demands` (0 = hidden) with a
    /// per-volume `floor`. Returns one cap per demand, in order.
    #[func]
    fn split_near_caps(
        &self,
        total: i64,
        demands: PackedInt64Array,
        floor: i64,
    ) -> PackedInt64Array {
        let d: Vec<usize> = demands
            .as_slice()
            .iter()
            .map(|&v| v.max(0) as usize)
            .collect();
        let out: Vec<i64> = split_near_caps(total.max(0) as usize, &d, floor.max(0) as usize)
            .into_iter()
            .map(|v| v as i64)
            .collect();
        PackedInt64Array::from(out.as_slice())
    }

    /// [`split_near_caps_weighted`]: as `split_near_caps`, with a weight per volume
    /// (a missing entry is 1.0). Empty or all-equal weights give exactly
    /// `split_near_caps`'s answer.
    #[func]
    fn split_near_caps_weighted(
        &self,
        total: i64,
        demands: PackedInt64Array,
        weights: PackedFloat64Array,
        floor: i64,
    ) -> PackedInt64Array {
        let d: Vec<usize> = demands
            .as_slice()
            .iter()
            .map(|&v| v.max(0) as usize)
            .collect();
        let out: Vec<i64> = split_near_caps_weighted(
            total.max(0) as usize,
            &d,
            weights.as_slice(),
            floor.max(0) as usize,
        )
        .into_iter()
        .map(|v| v as i64)
        .collect();
        PackedInt64Array::from(out.as_slice())
    }

    /// The gem floor the per-volume split uses ([`GEM_MIN`]).
    #[func]
    fn gem_min(&self) -> i64 {
        GEM_MIN as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_route::{route_triangles_for, MAX_PATH, MAX_SIDECAR};

    fn check_split(total: usize, demands: &[usize], floor: usize) -> Vec<usize> {
        let out = split_near_caps(total, demands, floor);
        assert_eq!(out.len(), demands.len());
        let sum: usize = out.iter().sum();
        assert!(sum <= total, "{out:?} exceeds {total}");
        for (o, d) in out.iter().zip(demands) {
            assert!(o <= d, "{out:?} exceeds demand {demands:?}");
        }
        assert_eq!(
            sum,
            total.min(demands.iter().sum()),
            "{out:?} leaves cap unused"
        );
        out
    }

    #[test]
    fn split_near_caps_is_proportional_above_the_floor() {
        // 80 gems over knowledge 6 000, ontology 2 000, codescape 0 (hidden).
        let out = check_split(80, &[6_000, 2_000, 0], GEM_MIN);
        assert_eq!(out[2], 0, "a hidden volume gets nothing");
        // Floors 16 + 16, the remaining 48 split 3:1 on unmet demand.
        assert_eq!(out, vec![52, 28, 0]);
    }

    #[test]
    fn split_near_caps_redistributes_what_a_small_volume_cannot_use() {
        let out = check_split(80, &[5, 10_000, 10_000], GEM_MIN);
        assert_eq!(out[0], 5, "a volume never gets past its demand");
        assert_eq!(out[1] + out[2], 75);
        assert!(out[1].abs_diff(out[2]) <= 1);
    }

    #[test]
    fn split_near_caps_deals_floors_when_the_cap_is_short() {
        let out = check_split(20, &[100, 100, 100], GEM_MIN);
        assert_eq!(out, vec![7, 7, 6], "dealt round-robin up to the floor");
        assert_eq!(check_split(0, &[100, 100], GEM_MIN), vec![0, 0]);
    }

    #[test]
    fn split_near_caps_single_volume_gets_the_whole_cap() {
        assert_eq!(check_split(96, &[20_000], GEM_MIN), vec![96]);
        assert_eq!(check_split(96, &[40], GEM_MIN), vec![40]);
        assert!(check_split(96, &[], GEM_MIN).is_empty());
    }

    #[test]
    fn split_near_caps_holds_its_invariants_over_a_sweep() {
        let shapes: [&[usize]; 6] = [
            &[1, 1, 1],
            &[0, 0, 0],
            &[3, 50, 7],
            &[13_164, 0, 900],
            &[17, 17],
            &[1, 2, 3, 4],
        ];
        for total in [0usize, 1, 2, 15, 16, 47, 80, 96, 500] {
            for d in shapes {
                for floor in [0usize, 1, GEM_MIN] {
                    check_split(total, d, floor);
                }
            }
        }
    }

    /// Production graph as xr-graph measures it: 13 164 nodes, 20 000 edges,
    /// 32 hulls (2 938 triangles), 6 draw calls.
    fn production() -> GraphDemand {
        GraphDemand {
            nodes: 13_164,
            edges: 20_000,
            hulls: 32,
            hull_tris: 2_938,
            faded: 0,
            draw_calls: 6,
            other_tris: 0,
        }
    }

    fn full_memory() -> MemoryDemand {
        MemoryDemand {
            cloud_rows: 20_000,
            route: Some(RouteShape {
                rows: MAX_PATH,
                sidecar: MAX_SIDECAR,
            }),
            burst_slots: 0,
        }
    }

    /// Recompute what the caps cost, independently of `allocate`'s bookkeeping.
    fn cost(g: &GraphDemand, m: &MemoryDemand, c: &FrameCaps) -> usize {
        let faded = g.faded.min(g.nodes);
        let gem = c.graph.gem_nodes.min(g.nodes - faded);
        let cyl = c.graph.cylinder_edges.min(g.edges);
        let hull = if c.graph.max_hulls >= g.hulls {
            g.hull_tris
        } else {
            c.graph.max_hulls * MAX_TRIS_PER_HULL
        };
        let route = m.route.map_or(0, |r| {
            route_triangles_at(r.rows, r.sidecar, c.route_ring_cap)
        });
        g.other_tris
            + (faded + gem) * GEM_TRIS_PER_NODE
            + (g.nodes - faded - gem) * IMPOSTOR_TRIS_PER_NODE
            + cyl * CYLINDER_TRIS_PER_EDGE
            + (g.edges - cyl) * RIBBON_TRIS_PER_EDGE
            + hull
            + c.cloud_sprites * TRIANGLES_PER_SPRITE
            + c.burst_slots * BURST_TRIS_PER_SLOT
            + route
    }

    #[test]
    fn production_scene_with_full_cloud_and_longest_route_fits() {
        let (g, m) = (production(), full_memory());
        let c = allocate(&g, &m);
        assert!(!c.over_budget);
        assert_eq!(
            cost(&g, &m, &c),
            c.tris_total,
            "bookkeeping matches an independent recount"
        );
        assert!(
            c.tris_total <= TRI_LIMIT,
            "{} inside the 5 % reserve",
            c.tris_total
        );
        assert!(c.draw_calls <= DRAW_CALL_LIMIT);
        assert_eq!(c.draw_calls, 6 + CLOUD_DRAW_CALLS + ROUTE_DRAW_CALLS);
        // priority: route and cloud whole, hulls whole, near detail takes the rest
        assert_eq!(c.route_tris, route_triangles_for(MAX_PATH, MAX_SIDECAR));
        assert_eq!(c.cloud_sprites, DEFAULT_SPRITE_CAP);
        assert_eq!(c.graph.max_hulls, 32);
        assert!(
            c.graph.gem_nodes >= GEM_MIN && c.graph.gem_nodes < DEFAULT_NEAR_CAP,
            "{:?}",
            c.graph
        );
        assert!(
            TRI_LIMIT - c.tris_total < CYLINDER_STEP.max(GEM_STEP),
            "limit used up, not padded"
        );
        assert_eq!(
            c.burst_slots, 0,
            "cloud shown: bursts restyle sprites, no pool"
        );
    }

    #[test]
    fn small_graph_without_memory_layers_gets_the_defaults() {
        let g = GraphDemand {
            nodes: 1_000,
            edges: 1_500,
            hulls: 12,
            hull_tris: 1_100,
            faded: 0,
            draw_calls: 6,
            other_tris: 0,
        };
        let c = allocate(&g, &MemoryDemand::default());
        assert_eq!(
            c.graph,
            GraphCaps {
                gem_nodes: DEFAULT_NEAR_CAP,
                cylinder_edges: DEFAULT_NEAR_EDGE_CAP,
                max_hulls: 12
            }
        );
        assert_eq!(c.cloud_sprites, 0);
        assert_eq!(c.route_tris, 0);
        assert_eq!(c.draw_calls, 6);
        assert_eq!(cost(&g, &MemoryDemand::default(), &c), c.tris_total);
    }

    #[test]
    fn layers_give_way_in_priority_order() {
        // Grow the graph's far floor until the minimums no longer fit; at every
        // step a lower-priority layer is at its minimum before a higher one gives.
        let m = full_memory();
        let route_full = route_triangles_for(MAX_PATH, MAX_SIDECAR);
        let mut prev: Option<FrameCaps> = None;
        for nodes in (1_000..40_000).step_by(250) {
            let g = GraphDemand {
                nodes,
                edges: nodes * 3 / 2,
                hulls: 32,
                hull_tris: 2_938,
                faded: 0,
                draw_calls: 6,
                other_tris: 0,
            };
            let c = allocate(&g, &m);
            // A higher-priority layer below its demand could not afford its next
            // step from what the lower layers took (leftovers trickle down).
            if c.graph.gem_nodes < DEFAULT_NEAR_CAP {
                assert!(
                    c.graph.cylinder_edges * CYLINDER_STEP < GEM_STEP,
                    "{nodes}: cylinders give way before gems"
                );
            }
            if c.graph.max_hulls < 32 {
                assert_eq!(
                    c.graph.gem_nodes, GEM_MIN,
                    "{nodes}: gems at minimum before hulls give"
                );
                assert!(
                    c.graph.cylinder_edges * CYLINDER_STEP < MAX_TRIS_PER_HULL,
                    "{nodes}"
                );
            }
            if c.cloud_sprites < DEFAULT_SPRITE_CAP {
                assert_eq!(
                    (c.graph.max_hulls, c.graph.gem_nodes, c.graph.cylinder_edges),
                    (0, GEM_MIN, 0),
                    "{nodes}: graph detail gone before the cloud shrinks"
                );
            }
            if c.route_tris < route_full {
                assert_eq!(
                    c.cloud_sprites, CLOUD_MIN_SPRITES,
                    "{nodes}: cloud at floor before the route thins"
                );
            }
            if !c.over_budget {
                assert!(c.tris_total <= TRI_LIMIT, "{nodes}: {}", c.tris_total);
                assert_eq!(cost(&g, &m, &c), c.tris_total, "{nodes}");
            }
            if let Some(p) = prev {
                assert!(
                    c.cloud_sprites <= p.cloud_sprites && c.route_tris <= p.route_tris,
                    "{nodes}: monotone"
                );
            }
            prev = Some(c);
        }
        assert!(
            prev.unwrap().over_budget,
            "the sweep reaches an impossible graph"
        );
    }

    #[test]
    fn a_thinner_route_is_used_before_it_overruns() {
        // a 13-row route wants 10 samples per hop; with little room it drops detail
        let g = GraphDemand {
            nodes: 21_625,
            edges: 21_625,
            hulls: 0,
            hull_tris: 0,
            faded: 0,
            draw_calls: 6,
            other_tris: 0,
        };
        let m = MemoryDemand {
            cloud_rows: 20_000,
            route: Some(RouteShape {
                rows: 13,
                sidecar: 5,
            }),
            burst_slots: 0,
        };
        let c = allocate(&g, &m);
        assert!(!c.over_budget);
        assert!(
            c.route_ring_cap < ROUTE_RING_CAP && c.route_ring_cap >= 13,
            "{}",
            c.route_ring_cap
        );
        assert_eq!(c.graph.gem_nodes, GEM_MIN);
        // the next detail step up would not have fitted in what trickled down
        let next = (c.route_ring_cap + 1..=ROUTE_RING_CAP)
            .map(|cap| route_triangles_at(13, 5, cap))
            .find(|&t| t > c.route_tris)
            .unwrap();
        let trickled = (c.cloud_sprites - CLOUD_MIN_SPRITES) * TRIANGLES_PER_SPRITE
            + c.graph.cylinder_edges * CYLINDER_STEP
            + (TRI_LIMIT - c.tris_total);
        assert!(
            next - c.route_tris > trickled,
            "next step {} vs {trickled}",
            next - c.route_tris
        );
        assert!(c.tris_total <= TRI_LIMIT);
        assert_eq!(cost(&g, &m, &c), c.tris_total);
    }

    #[test]
    fn impossible_graph_reports_over_budget_with_minimums() {
        let g = GraphDemand {
            nodes: 60_000,
            edges: 0,
            hulls: 32,
            hull_tris: 2_938,
            faded: 0,
            draw_calls: 6,
            other_tris: 0,
        };
        let c = allocate(&g, &full_memory());
        assert!(c.over_budget);
        assert_eq!(
            c.graph,
            GraphCaps {
                gem_nodes: GEM_MIN,
                cylinder_edges: 0,
                max_hulls: 0
            }
        );
        assert_eq!(c.cloud_sprites, CLOUD_MIN_SPRITES);
        assert_eq!(c.route_tris, route_triangles_at(MAX_PATH, MAX_SIDECAR, 0));
        assert!(c.tris_total > TRI_LIMIT);
    }

    #[test]
    fn faded_nodes_are_charged_the_full_mesh() {
        let base = production();
        let f = GraphDemand { faded: 10, ..base };
        let m = full_memory();
        let (a, b) = (allocate(&base, &m), allocate(&f, &m));
        assert_eq!(cost(&f, &m, &b), b.tris_total);
        assert!(b.tris_total <= TRI_LIMIT);
        assert_eq!(
            a.graph.gem_nodes - b.graph.gem_nodes,
            10,
            "each faded node displaces one gem"
        );
    }

    #[test]
    fn other_triangles_are_reserved_first() {
        let m = full_memory();
        let base = allocate(&production(), &m);
        let hud = GraphDemand {
            other_tris: 3_000,
            ..production()
        };
        let c = allocate(&hud, &m);
        assert!(!c.over_budget);
        assert_eq!(cost(&hud, &m, &c), c.tris_total);
        assert!(c.tris_total <= TRI_LIMIT);
        assert!(
            c.graph.gem_nodes < base.graph.gem_nodes,
            "near detail pays for the HUD"
        );
        assert_eq!(
            (c.cloud_sprites, c.route_tris),
            (base.cloud_sprites, base.route_tris),
            "memory layers keep priority"
        );
    }

    #[test]
    fn hidden_cloud_and_no_route_cost_nothing() {
        let c = allocate(&production(), &MemoryDemand::default());
        assert_eq!((c.cloud_sprites, c.route_tris), (0, 0));
        assert_eq!(c.draw_calls, 6);
        let small = allocate(
            &production(),
            &MemoryDemand {
                cloud_rows: 500,
                route: None,
                burst_slots: 0,
            },
        );
        assert_eq!(small.cloud_sprites, 500, "a small snapshot draws in full");
        assert_eq!(small.draw_calls, 7);
    }

    #[test]
    fn draw_call_overrun_is_reported_inside_the_reserve() {
        assert_eq!(
            (TRI_LIMIT, DRAW_CALL_LIMIT),
            (95_000, 48),
            "5 % of both budgets held back"
        );
        let g = GraphDemand {
            draw_calls: 44,
            ..production()
        };
        assert!(
            !allocate(&g, &full_memory()).over_budget,
            "44 + 4 memory calls = 48, at the limit"
        );
        let g = GraphDemand {
            draw_calls: 45,
            ..production()
        };
        assert!(allocate(&g, &full_memory()).over_budget, "45 + 4 > 48");
        assert!(!allocate(&g, &MemoryDemand::default()).over_budget);
        let g = GraphDemand {
            draw_calls: 48,
            ..production()
        };
        assert!(!allocate(&g, &MemoryDemand::default()).over_budget);
    }

    #[test]
    fn burst_pool_is_a_layer_after_the_cloud() {
        // cloud hidden: the ring pool draws, ahead of the graph's near detail
        let m = MemoryDemand {
            cloud_rows: 0,
            route: None,
            burst_slots: BURST_POOL_SLOTS,
        };
        let c = allocate(&production(), &m);
        assert_eq!(c.burst_slots, BURST_POOL_SLOTS);
        assert_eq!(c.draw_calls, 6 + BURST_DRAW_CALLS);
        assert_eq!(cost(&production(), &m, &c), c.tris_total);
        let without = allocate(&production(), &MemoryDemand::default());
        assert!(
            c.graph.gem_nodes < without.graph.gem_nodes,
            "near detail pays for the pool"
        );
        // a starved frame trims slots, not the cloud
        let g = GraphDemand {
            nodes: 20_000,
            edges: 20_000,
            ..production()
        };
        let m = MemoryDemand {
            cloud_rows: 20_000,
            route: None,
            burst_slots: BURST_POOL_SLOTS,
        };
        let c = allocate(&g, &m);
        assert!(!c.over_budget);
        assert!(c.burst_slots < BURST_POOL_SLOTS);
        assert_eq!(
            c.cloud_sprites, DEFAULT_SPRITE_CAP,
            "the cloud grows before the pool"
        );
        assert_eq!(c.graph.gem_nodes, GEM_MIN);
        assert_eq!(cost(&g, &m, &c), c.tris_total);
    }

    // ── weighted volume split ──

    fn check_weighted(
        total: usize,
        demands: &[usize],
        weights: &[f64],
        floor: usize,
    ) -> Vec<usize> {
        let out = split_near_caps_weighted(total, demands, weights, floor);
        assert_eq!(out.len(), demands.len());
        let sum: usize = out.iter().sum();
        assert!(sum <= total, "{out:?} exceeds {total} ({weights:?})");
        for (o, d) in out.iter().zip(demands) {
            assert!(o <= d, "{out:?} exceeds demand {demands:?}");
        }
        assert_eq!(
            sum,
            total.min(demands.iter().sum()),
            "{out:?} leaves cap unused ({demands:?} {weights:?})"
        );
        let floors: usize = demands.iter().map(|&d| d.min(floor)).sum();
        if floors <= total {
            for (o, d) in out.iter().zip(demands) {
                assert!(
                    *o >= (*d).min(floor),
                    "{out:?} drops a floor ({demands:?} {weights:?})"
                );
            }
        }
        out
    }

    const SHAPES: [&[usize]; 6] = [
        &[1, 1, 1],
        &[0, 0, 0],
        &[3, 50, 7],
        &[13_164, 0, 900],
        &[17, 17],
        &[1, 2, 3, 4],
    ];

    #[test]
    fn weighted_split_with_no_or_equal_weights_is_the_unweighted_split() {
        for total in [0usize, 1, 2, 15, 16, 47, 80, 96, 500] {
            for d in SHAPES {
                for floor in [0usize, 1, GEM_MIN] {
                    let plain = split_near_caps(total, d, floor);
                    assert_eq!(
                        split_near_caps_weighted(total, d, &[], floor),
                        plain,
                        "empty"
                    );
                    for w in [1.0f64, 2.5, 0.0, 1e-9] {
                        let ws = vec![w; d.len()];
                        assert_eq!(
                            split_near_caps_weighted(total, d, &ws, floor),
                            plain,
                            "all {w}: {total} {d:?} {floor}"
                        );
                    }
                }
            }
        }
        // A hidden volume's weight is irrelevant: only volumes with demand count.
        assert_eq!(
            split_near_caps_weighted(80, &[6_000, 2_000, 0], &[2.0, 2.0, 9.0], GEM_MIN),
            split_near_caps(80, &[6_000, 2_000, 0], GEM_MIN)
        );
    }

    #[test]
    fn weighted_split_shares_by_drawn_times_weight() {
        // Same drawn count, weights 3:1: floors 16 + 16, the other 48 split 3:1.
        let out = check_weighted(80, &[6_000, 6_000], &[3.0, 1.0], GEM_MIN);
        assert_eq!(out, vec![52, 28]);
        // The weight multiplies the drawn count, so a smaller heavy volume can win.
        let out = check_weighted(80, &[1_000, 3_000], &[4.0, 1.0], GEM_MIN);
        let plain = split_near_caps(80, &[1_000, 3_000], GEM_MIN);
        assert!(
            plain[0] < plain[1],
            "unweighted favours the bigger drawn count: {plain:?}"
        );
        assert!(
            out[0] > out[1],
            "weight 4 beats 3x the drawn count: {out:?}"
        );
        assert_eq!(out, vec![43, 37]);
        // A missing id counts as 1.0.
        let out = check_weighted(80, &[6_000, 6_000], &[3.0], GEM_MIN);
        assert_eq!(out, vec![52, 28]);
    }

    #[test]
    fn weighted_split_keeps_the_floor_for_a_zero_weight_volume() {
        let out = check_weighted(80, &[6_000, 6_000], &[0.0, 1.0], GEM_MIN);
        assert_eq!(
            out,
            vec![GEM_MIN, 80 - GEM_MIN],
            "zero weight is floor only"
        );
        // Nobody else wants the rest: the zero-weight volume takes it rather
        // than leaving cap unused.
        let out = check_weighted(80, &[100, 20], &[0.0, 1.0], GEM_MIN);
        assert_eq!(out, vec![60, 20]);
    }

    #[test]
    fn weighted_split_sanitises_bad_weights() {
        let w = [f64::NAN, -3.0, f64::INFINITY, 1.0];
        let out = check_weighted(200, &[500, 500, 500, 500], &w, GEM_MIN);
        assert_eq!(out.iter().sum::<usize>(), 200);
        // NaN and negative are zero weight: floor only while others want more.
        assert_eq!((out[0], out[1]), (GEM_MIN, GEM_MIN));
    }

    #[test]
    fn weighted_split_holds_its_invariants_over_a_sweep() {
        let weight_sets: [&[f64]; 6] = [
            &[1.0, 2.0, 3.0, 4.0],
            &[0.0, 5.0, 0.0, 0.5],
            &[0.25, 0.25, 8.0],
            &[f64::NAN, 1.0, -1.0],
            &[1e9, 1.0],
            &[7.0],
        ];
        for total in [0usize, 1, 2, 15, 16, 47, 80, 96, 500] {
            for d in SHAPES {
                for w in weight_sets {
                    for floor in [0usize, 1, GEM_MIN] {
                        check_weighted(total, d, w, floor);
                    }
                }
            }
        }
    }

    // ── extras layer ──

    /// Every extras tier's cost, recounted from the constants.
    #[test]
    fn extras_tiers_cost_what_the_constants_say() {
        assert_eq!(extras_cost(0, 3), (0, 0));
        assert_eq!(
            extras_cost(1, 3),
            (0, 0),
            "the sky is the Environment's: no triangle, no call"
        );
        assert_eq!(
            extras_cost(2, 3),
            (3 * FRAME_RIBBON_TRIS, 0),
            "the ribbons replace the lines in the frame's own call, which the scene counts"
        );
        assert_eq!(
            extras_cost(3, 3),
            (3 * FRAME_RIBBON_TRIS + MOTES_LOW * MOTE_TRIS, 1)
        );
        assert_eq!(
            extras_cost(4, 3),
            (3 * FRAME_RIBBON_TRIS + MOTES_HIGH * MOTE_TRIS, 1)
        );
        assert_eq!(
            extras_cost(9, 3),
            extras_cost(EXTRAS_MAX_TIER, 3),
            "tier clamps"
        );
        assert_eq!(
            extras_cost(4, 99),
            extras_cost(4, EXTRAS_MAX_FRAMES),
            "frames clamp"
        );
        let mut prev = (0, 0);
        for t in 0..=EXTRAS_MAX_TIER {
            let c = extras_cost(t, 2);
            assert!(c.0 >= prev.0 && c.1 >= prev.1, "tiers only add cost");
            prev = c;
        }
    }

    /// Triangles the plain allocator's near detail (gems above the minimum plus
    /// cylinders) spends: the pool an allowance is carved from.
    fn near_pool(g: &GraphDemand, c: &FrameCaps) -> usize {
        let gem_min = (g.nodes - g.faded.min(g.nodes))
            .min(DEFAULT_NEAR_CAP)
            .min(GEM_MIN);
        (c.graph.gem_nodes - gem_min) * GEM_STEP + c.graph.cylinder_edges * CYLINDER_STEP
    }

    #[test]
    fn the_allowance_costs_the_graph_at_most_its_size_in_gems() {
        // ADR-2167 (amended 2026-10-31): the cloud, route and burst caps and the
        // hulls are the plain allocator's; the graph loses at most
        // ceil(allowance / GEM_STEP) gems; the requested tier is granted whenever
        // the plain near pool could pay for it.
        let m = full_memory();
        let mut granted_somewhere = false;
        for nodes in (500..40_000).step_by(1_250) {
            for tier in 0..=EXTRAS_MAX_TIER {
                let g = GraphDemand {
                    nodes,
                    edges: nodes * 3 / 2,
                    hulls: 32,
                    hull_tris: 2_938,
                    faded: 0,
                    draw_calls: 6,
                    other_tris: 0,
                };
                let x = ExtrasDemand { tier, frames: 3 };
                for mem in [&m, &MemoryDemand::default()] {
                    let (a, b) = (allocate(&g, mem), allocate_with_extras(&g, mem, &x));
                    assert_eq!(
                        (
                            a.cloud_sprites,
                            a.route_tris,
                            a.route_ring_cap,
                            a.burst_slots,
                            a.graph.max_hulls
                        ),
                        (
                            b.cloud_sprites,
                            b.route_tris,
                            b.route_ring_cap,
                            b.burst_slots,
                            b.graph.max_hulls
                        ),
                        "{nodes} tier {tier}"
                    );
                    let (allowance, _) = extras_cost(tier, 3);
                    let bound = allowance.div_ceil(GEM_STEP);
                    assert!(
                        b.graph.gem_nodes + bound >= a.graph.gem_nodes
                            && b.graph.gem_nodes <= a.graph.gem_nodes,
                        "{nodes} tier {tier}: gems {} -> {}, bound {bound}",
                        a.graph.gem_nodes,
                        b.graph.gem_nodes
                    );
                    assert_eq!(a.extras_tier, 0, "plain allocate grants no extras");
                    if tier <= 1 {
                        assert_eq!(
                            b.extras_allowance, 0,
                            "tiers 0 and 1 reserve nothing: {nodes}"
                        );
                        assert_eq!(b.graph, a.graph, "and cost the graph nothing: {nodes}");
                    }
                    assert!(b.over_budget == a.over_budget);
                    assert!(b.draw_calls == a.draw_calls + b.extras_draw_calls);
                    assert!(
                        b.over_budget || b.tris_total <= TRI_LIMIT,
                        "{nodes} tier {tier}"
                    );
                    let pressured = a.graph.max_hulls < g.hulls
                        || (mem.route.is_some() && a.route_ring_cap < ROUTE_RING_CAP)
                        || a.cloud_sprites < mem.cloud_rows.min(DEFAULT_SPRITE_CAP);
                    if pressured {
                        assert_eq!(
                            b.graph, a.graph,
                            "{nodes} tier {tier}: pressure holds nothing back"
                        );
                    }
                    if !a.over_budget && !pressured && near_pool(&g, &a) >= allowance {
                        assert_eq!(
                            b.extras_tier, tier,
                            "{nodes} tier {tier}: the pool pays for the request"
                        );
                        granted_somewhere |= tier == EXTRAS_MAX_TIER;
                    }
                }
            }
        }
        assert!(granted_somewhere, "the sweep must reach a granted tier 4");
    }

    /// The benchmark's production inputs (`m_461_prod_*` JSON, 13 164 nodes): the
    /// measured `other_tris` 2 182 and 24 other draw calls, three volume frames'
    /// calls (`XR_BENCH_CANDY_FRAMES=3`; the benchmark defaults to two), 6 000 cloud rows,
    /// a 13-row route with 5 sidecar marks.
    fn benchmark_production() -> (GraphDemand, MemoryDemand) {
        let g = GraphDemand {
            other_tris: 2_182,
            draw_calls: 6 + 24 + 3, // graph + other + three volume frames
            ..production()
        };
        let m = MemoryDemand {
            cloud_rows: 6_000,
            route: Some(RouteShape {
                rows: 13,
                sidecar: 5,
            }),
            burst_slots: 0,
        };
        (g, m)
    }

    #[test]
    fn production_shape_grants_tier_four_for_at_most_two_gems() {
        // 13 164 nodes, three volume frames, the benchmark JSON caps (gems 35,
        // cylinders 7): before the allowance only the sky fitted.
        let (g, m) = benchmark_production();
        let plain = allocate(&g, &m);
        assert_eq!(
            (plain.graph.gem_nodes, plain.graph.cylinder_edges),
            (35, 7),
            "the JSON caps"
        );
        for tier in 0..=EXTRAS_MAX_TIER {
            let c = allocate_with_extras(&g, &m, &ExtrasDemand { tier, frames: 3 });
            assert_eq!(c.extras_tier, tier, "tier {tier} is granted");
            assert_eq!(
                c.extras_allowance, c.extras_tris,
                "tier {tier}: the grant is a held-back allowance"
            );
            assert!(!c.over_budget && c.tris_total <= TRI_LIMIT && c.draw_calls <= DRAW_CALL_LIMIT);
            let lost = plain.graph.gem_nodes - c.graph.gem_nodes;
            assert!(lost <= 2, "tier {tier} costs {lost} gems");
            assert_eq!(
                (c.cloud_sprites, c.route_ring_cap),
                (plain.cloud_sprites, plain.route_ring_cap)
            );
        }
        let c = allocate_with_extras(&g, &m, &ExtrasDemand { tier: 4, frames: 3 });
        assert_eq!(c.extras_tris, 24 * 3 + 512);
    }

    #[test]
    fn a_tier_the_call_guard_refuses_returns_its_allowance_to_the_graph() {
        // 48 of 48 calls: the motes' call is refused, so only tier 2 is held
        // back (no triangles with 0 frames) and the graph keeps everything.
        let g = GraphDemand {
            draw_calls: 44,
            ..production()
        };
        let x = ExtrasDemand { tier: 4, frames: 3 };
        let (plain, c) = (
            allocate(&g, &full_memory()),
            allocate_with_extras(&g, &full_memory(), &x),
        );
        assert_eq!(c.extras_tier, 2);
        assert_eq!(
            c.graph.gem_nodes, plain.graph.gem_nodes,
            "no motes granted: nothing reserved for them"
        );
        assert_eq!(c.extras_tris, 72);
        assert!(plain.graph.gem_nodes - c.graph.gem_nodes <= 72usize.div_ceil(GEM_STEP));
        // a pinned lower tier reserves only itself
        let c = allocate_with_extras(
            &production(),
            &full_memory(),
            &ExtrasDemand { tier: 2, frames: 3 },
        );
        let p = allocate(&production(), &full_memory());
        assert_eq!(c.extras_tris, 72);
        assert!(p.graph.gem_nodes - c.graph.gem_nodes <= 72usize.div_ceil(GEM_STEP));
    }

    #[test]
    fn the_allowance_is_zero_under_minimum_overrun_or_pressure() {
        let x = ExtrasDemand {
            tier: EXTRAS_MAX_TIER,
            frames: 3,
        };
        // pressure: the memory layers cannot reach their caps, so the graph's
        // near pool is empty and the extras only have last claim
        let g = GraphDemand {
            nodes: 24_000,
            edges: 40_000,
            ..production()
        };
        let m = full_memory();
        let plain = allocate(&g, &m);
        let c = allocate_with_extras(&g, &m, &x);
        assert!(plain.cloud_sprites < DEFAULT_SPRITE_CAP || plain.route_ring_cap < ROUTE_RING_CAP);
        assert_eq!(
            c.graph, plain.graph,
            "pressure: the graph is the plain allocator's"
        );
        assert_eq!(
            (c.cloud_sprites, c.route_ring_cap, c.route_tris),
            (plain.cloud_sprites, plain.route_ring_cap, plain.route_tris)
        );
        assert_eq!(
            c.extras_tier, 1,
            "last claim: nothing is left, the free sky shows"
        );
        assert_eq!((c.extras_tris, c.extras_allowance), (0, 0));
        // a hull that does not fit in full is pressure too
        let g = GraphDemand {
            hull_tris: 40_000,
            nodes: 44_000,
            edges: 0,
            ..production()
        };
        let (p, c) = (
            allocate(&g, &MemoryDemand::default()),
            allocate_with_extras(&g, &MemoryDemand::default(), &x),
        );
        assert!(p.graph.max_hulls < g.hulls);
        assert_eq!(c.graph, p.graph);
        assert_eq!(c.extras_allowance, 0, "short hulls: no allowance");
        // a minimum overrun likewise (see also extras_drop_first_...)
        let g = GraphDemand {
            nodes: 60_000,
            edges: 0,
            ..production()
        };
        let c = allocate_with_extras(&g, &full_memory(), &x);
        assert!(c.over_budget);
        assert_eq!((c.extras_allowance, c.extras_tris), (0, 0));
        // and a case with none of it holds the full cost
        let (g, m) = benchmark_production();
        assert_eq!(
            allocate_with_extras(&g, &m, &x).extras_allowance,
            extras_cost(4, 3).0
        );
    }

    /// Every shape in which `short` is the only pressure: no hulls, no cloud,
    /// so neither can mask the term under test (R12 F3).
    fn only_pressure_shapes(
        m: &MemoryDemand,
        x: &ExtrasDemand,
        short: impl Fn(&FrameCaps) -> bool,
    ) -> usize {
        let mut found = 0;
        for nodes in (100..40_000).step_by(7) {
            let g = GraphDemand {
                nodes,
                edges: 40_000,
                hulls: 0,
                hull_tris: 0,
                ..production()
            };
            let (plain, c) = (allocate(&g, m), allocate_with_extras(&g, m, x));
            if c.over_budget || !short(&c) {
                continue;
            }
            found += 1;
            assert_eq!(
                c.extras_allowance, 0,
                "{nodes} nodes: a short layer takes no allowance"
            );
            assert_eq!(
                c.graph, plain.graph,
                "{nodes} nodes: the graph is the plain allocator's"
            );
        }
        found
    }

    #[test]
    fn a_short_route_alone_takes_no_allowance() {
        let m = MemoryDemand {
            cloud_rows: 0,
            route: Some(RouteShape {
                rows: 13,
                sidecar: 5,
            }),
            burst_slots: 0,
        };
        let x = ExtrasDemand { tier: 2, frames: 2 };
        let n = only_pressure_shapes(&m, &x, |c| c.route_ring_cap < ROUTE_RING_CAP);
        assert!(n > 0, "the sweep must reach a short-route shape");
    }

    #[test]
    fn a_short_burst_pool_alone_takes_no_allowance() {
        let m = MemoryDemand {
            cloud_rows: 0,
            route: None,
            burst_slots: BURST_POOL_SLOTS,
        };
        let x = ExtrasDemand { tier: 2, frames: 2 };
        let n = only_pressure_shapes(&m, &x, |c| c.burst_slots < BURST_POOL_SLOTS);
        assert!(n > 0, "the sweep must reach a short-burst shape");
    }

    #[test]
    fn extras_are_granted_in_full_where_the_budget_allows_and_hold_back_their_cost() {
        // A small graph leaves most of the budget: every tier is granted.
        let small = GraphDemand {
            nodes: 1_000,
            edges: 1_500,
            hulls: 12,
            hull_tris: 1_100,
            faded: 0,
            draw_calls: 6,
            other_tris: 0,
        };
        let x = ExtrasDemand {
            tier: EXTRAS_MAX_TIER,
            frames: 3,
        };
        let c = allocate_with_extras(&small, &MemoryDemand::default(), &x);
        let want = extras_cost(EXTRAS_MAX_TIER, 3);
        assert_eq!(c.extras_tier, EXTRAS_MAX_TIER);
        assert_eq!((c.extras_tris, c.extras_draw_calls), want);
        assert_eq!(c.draw_calls, 6 + want.1);
        assert!(!c.over_budget && c.tris_total <= TRI_LIMIT);

        // The production graph used to spend every triangle on near detail and
        // leave only the sky; the allowance now reserves the request first, and
        // the whole frame still fits.
        let c = allocate_with_extras(&production(), &full_memory(), &x);
        assert_eq!(c.extras_tier, EXTRAS_MAX_TIER);
        assert!(c.tris_total <= TRI_LIMIT && !c.over_budget);
        assert!(c.extras_tris <= c.tris_total);
    }

    #[test]
    fn extras_are_held_by_the_draw_call_limit() {
        // 44 graph calls + 4 memory calls = 48 of 48: the motes' call would be 49.
        let g = GraphDemand {
            nodes: 1_000,
            edges: 1_000,
            hulls: 0,
            hull_tris: 0,
            faded: 0,
            draw_calls: 44,
            other_tris: 0,
        };
        let x = ExtrasDemand {
            tier: EXTRAS_MAX_TIER,
            frames: 0,
        };
        let c = allocate_with_extras(&g, &full_memory(), &x);
        assert_eq!(c.draw_calls, 48 + c.extras_draw_calls);
        assert!(c.draw_calls <= DRAW_CALL_LIMIT && !c.over_budget);
        assert_eq!(
            c.extras_tier, 2,
            "no frames: tiers 1 and 2 cost nothing; motes need 1 call"
        );
        // Three frames: their calls are the scene's, already inside `draw_calls`
        // (44 includes them), so tier 2 adds none and is granted; the motes'
        // call would still be the 49th.
        let x = ExtrasDemand {
            tier: EXTRAS_MAX_TIER,
            frames: 3,
        };
        let c = allocate_with_extras(&g, &full_memory(), &x);
        assert_eq!(
            c.extras_tier, 2,
            "the ribbons replace the lines: no call of their own"
        );
        assert_eq!(c.extras_draw_calls, 0);
        assert_eq!(c.draw_calls, 48);
        // At the limit already: nothing paid for in calls; the layer reports no overrun it caused.
        let g = GraphDemand {
            draw_calls: 48,
            ..g
        };
        let c = allocate_with_extras(&g, &MemoryDemand::default(), &x);
        assert_eq!(
            (c.extras_tier, c.extras_draw_calls, c.draw_calls),
            (2, 0, 48)
        );
        assert!(!c.over_budget);
    }

    #[test]
    fn extras_drop_first_when_the_minimums_alone_overrun() {
        let g = GraphDemand {
            nodes: 60_000,
            edges: 0,
            hulls: 32,
            hull_tris: 2_938,
            faded: 0,
            draw_calls: 6,
            other_tris: 0,
        };
        let x = ExtrasDemand {
            tier: EXTRAS_MAX_TIER,
            frames: 3,
        };
        let plain = allocate(&g, &full_memory());
        let c = allocate_with_extras(&g, &full_memory(), &x);
        assert!(c.over_budget);
        assert_eq!(
            (c.extras_tris, c.extras_draw_calls),
            (0, 0),
            "no triangle bought"
        );
        assert_eq!(
            c.tris_total, plain.tris_total,
            "the minimums are what is reported"
        );
        assert_eq!(c.graph, plain.graph);
    }

    // ── splat account (ADR-2170 decision 7) ──

    /// Every field the graph, memory layers and extras own, i.e. everything but
    /// the splat fields and the draw-call total.
    fn without_splats(c: &FrameCaps) -> FrameCaps {
        FrameCaps {
            splat_volumes: 0,
            splat_tier: 0,
            splat_portal_tier: 0,
            splat_count: 0,
            splat_tris: 0,
            splat_draw_calls: 0,
            draw_calls: 0,
            ..*c
        }
    }

    fn splats(tier: usize, volumes: usize, entered: bool) -> SplatDemand {
        SplatDemand {
            tier,
            volumes,
            entered,
        }
    }

    #[test]
    fn splat_tiers_are_the_adr_counts_at_two_triangles_a_quad() {
        assert_eq!(SPLAT_TIER_COUNTS, [0, 25_000, 50_000, 100_000]);
        assert_eq!(SPLAT_TRIS_PER_SPLAT, 2);
        assert_eq!(SPLAT_MAX_VOLUMES, 2);
        assert_eq!(splat_count(0), 0);
        assert_eq!(splat_count(3), 100_000);
        assert_eq!(splat_count(9), 100_000, "tier clamps");
        // The reason for the separate account: tier 1 alone is half the whole limit.
        assert!(splat_count(1) * SPLAT_TRIS_PER_SPLAT > TRI_LIMIT / 2);
    }

    #[test]
    fn splat_grant_follows_tier_volume_and_entered_state() {
        let off = SplatGrant::default();
        assert_eq!(splat_grant(&splats(0, 2, true), 6), off, "tier 0 is off");
        assert_eq!(
            splat_grant(&splats(3, 0, true), 6),
            off,
            "no volume, no call"
        );
        // entered: the room draws the requested tier, one call
        let g = splat_grant(&splats(3, 1, true), 6);
        assert_eq!(
            (g.volumes, g.tier, g.count, g.tris, g.draw_calls),
            (1, 3, 100_000, 200_000, 1)
        );
        // not entered: held at the unentered ceiling however high the governor climbed
        let g = splat_grant(&splats(3, 1, false), 6);
        assert_eq!((g.tier, g.count), (SPLAT_UNENTERED_MAX_TIER, 25_000));
        // two volumes, one entered: the room at tier 3, the portal at the ceiling
        let g = splat_grant(&splats(3, 2, true), 6);
        assert_eq!(
            (g.volumes, g.tier, g.portal_tier, g.draw_calls),
            (2, 3, 1, 2)
        );
        assert_eq!(g.count, 100_000 + 25_000);
        assert_eq!(g.tris, 2 * g.count);
        // at most two volumes are ever charged
        assert_eq!(splat_grant(&splats(3, 7, true), 6).draw_calls, 2);
        // a tier-1 request is never raised
        assert_eq!(splat_grant(&splats(1, 2, true), 6).count, 50_000);
    }

    #[test]
    fn splat_draw_calls_count_against_the_limit() {
        let g = production();
        let m = MemoryDemand::default();
        let c = allocate_with_splats(&g, &m, &ExtrasDemand::default(), &splats(3, 2, true));
        let plain = allocate(&g, &m);
        assert_eq!(c.splat_draw_calls, 2);
        assert_eq!(
            c.draw_calls,
            plain.draw_calls + 2,
            "one call per volume, in the total"
        );
        assert!(c.draw_calls <= DRAW_CALL_LIMIT);
        // No room: a volume the limit has no call for is not granted.
        for room in 0..=2usize {
            let g = GraphDemand {
                draw_calls: DRAW_CALL_LIMIT - room,
                ..g
            };
            let c = allocate_with_splats(&g, &m, &ExtrasDemand::default(), &splats(3, 2, true));
            assert_eq!(c.splat_volumes, room, "room for {room} call(s)");
            assert_eq!(c.splat_draw_calls, room);
            assert!(c.draw_calls <= DRAW_CALL_LIMIT, "{room}: the limit holds");
            assert!(!c.over_budget, "{room}: splats never cause an overrun");
        }
        // A graph that already overruns the calls grants none and adds none.
        let g = GraphDemand {
            draw_calls: DRAW_CALL_LIMIT + 3,
            ..g
        };
        let c = allocate_with_splats(&g, &m, &ExtrasDemand::default(), &splats(3, 2, true));
        assert_eq!((c.splat_volumes, c.draw_calls), (0, DRAW_CALL_LIMIT + 3));
        assert!(c.over_budget, "the graph's own overrun is still reported");
    }

    #[test]
    fn splat_calls_are_reserved_before_the_extras() {
        // Two calls of room, tier 3 extras want one (the motes). Splats first:
        // both calls go to the splats and the motes tier is refused.
        let g = GraphDemand {
            draw_calls: DRAW_CALL_LIMIT - 2,
            ..production()
        };
        let m = MemoryDemand::default();
        let x = ExtrasDemand { tier: 3, frames: 3 };
        let without = allocate_with_extras(&g, &m, &x);
        assert_eq!(
            without.extras_tier, 3,
            "with nothing else asking, the motes fit"
        );
        let c = allocate_with_splats(&g, &m, &x, &splats(3, 2, true));
        assert_eq!(c.splat_draw_calls, 2, "the splats keep their calls");
        assert_eq!(c.extras_draw_calls, 0, "the extras cannot take them");
        assert_eq!(c.extras_tier, 2, "the ribbons need no call and stay");
        assert_eq!(c.draw_calls, DRAW_CALL_LIMIT);
        // With a spare call both fit.
        let g = GraphDemand {
            draw_calls: DRAW_CALL_LIMIT - 3,
            ..production()
        };
        let c = allocate_with_splats(&g, &m, &x, &splats(3, 2, true));
        assert_eq!(
            (c.splat_draw_calls, c.extras_draw_calls, c.extras_tier),
            (2, 1, 3)
        );
        assert_eq!(c.draw_calls, DRAW_CALL_LIMIT);
    }

    #[test]
    fn splat_triangles_are_reported_and_never_charged() {
        for (g, m) in [
            (production(), full_memory()),
            (production(), MemoryDemand::default()),
            (benchmark_production().0, benchmark_production().1),
        ] {
            let x = ExtrasDemand { tier: 4, frames: 3 };
            let plain = allocate_with_extras(&g, &m, &x);
            for tier in 0..=SPLAT_MAX_TIER {
                for volumes in 0..=3 {
                    for entered in [false, true] {
                        let c = allocate_with_splats(&g, &m, &x, &splats(tier, volumes, entered));
                        assert_eq!(
                            c.tris_total, plain.tris_total,
                            "tier {tier}: splat tris are not in the total"
                        );
                        assert!(c.tris_total <= TRI_LIMIT.max(plain.tris_total));
                        assert_eq!(
                            c.over_budget, plain.over_budget,
                            "tier {tier}: no splat-caused overrun"
                        );
                        assert_eq!(c.splat_tris, c.splat_count * SPLAT_TRIS_PER_SPLAT);
                        if c.splat_count > 0 {
                            assert!(
                                c.splat_tris > TRI_LIMIT / 2,
                                "reported on its own, outside TRI_LIMIT"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn graph_allocation_is_unchanged_by_any_splat_demand() {
        // The graph, cloud, route, burst and hull caps, and the extras the calls
        // leave room for, equal the no-splat allocation in every state.
        let m = full_memory();
        for g in [production(), benchmark_production().0] {
            for extras in 0..=EXTRAS_MAX_TIER {
                let x = ExtrasDemand {
                    tier: extras,
                    frames: 3,
                };
                let plain = allocate_with_extras(&g, &m, &x);
                for tier in 0..=SPLAT_MAX_TIER {
                    for volumes in 0..=2 {
                        for entered in [false, true] {
                            let c =
                                allocate_with_splats(&g, &m, &x, &splats(tier, volumes, entered));
                            assert_eq!(
                                without_splats(&c),
                                without_splats(&plain),
                                "extras {extras}, splats {tier}/{volumes}/{entered}"
                            );
                        }
                    }
                }
            }
        }
        // And allocate_with_extras is exactly the account with nothing asked.
        let g = production();
        let x = ExtrasDemand { tier: 3, frames: 3 };
        assert_eq!(
            allocate_with_extras(&g, &m, &x),
            allocate_with_splats(&g, &m, &x, &SplatDemand::default())
        );
        assert_eq!(
            allocate(&g, &m),
            allocate_with_splats(&g, &m, &ExtrasDemand::default(), &SplatDemand::default())
        );
    }

    #[test]
    fn only_an_entered_splat_volume_may_climb_past_the_unentered_ceiling() {
        let (g, m) = (production(), MemoryDemand::default());
        let x = ExtrasDemand::default();
        for tier in 0..=SPLAT_MAX_TIER {
            let out = allocate_with_splats(&g, &m, &x, &splats(tier, 1, false));
            assert!(
                out.splat_tier <= SPLAT_UNENTERED_MAX_TIER,
                "tier {tier} unentered"
            );
            let inn = allocate_with_splats(&g, &m, &x, &splats(tier, 1, true));
            assert_eq!(inn.splat_tier, tier, "tier {tier} entered");
        }
    }
}
