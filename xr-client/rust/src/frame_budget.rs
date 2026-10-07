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
    /// Draw calls all layers spend.
    pub draw_calls: usize,
    /// The minimums alone exceed a budget.
    pub over_budget: bool,
}

/// Divide the frame budget between the layers (see the module docs).
pub fn allocate(g: &GraphDemand, m: &MemoryDemand) -> FrameCaps {
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

    // gem nodes, then cylinder edges
    let gem_nodes = gem_max.min(gem_min + rem / GEM_STEP);
    rem -= (gem_nodes - gem_min) * GEM_STEP;
    let cylinder_edges = g.edges.min(DEFAULT_NEAR_EDGE_CAP).min(rem / CYLINDER_STEP);
    rem -= cylinder_edges * CYLINDER_STEP;

    let spent = TRI_LIMIT.saturating_sub(rem);
    let tris_total = if over_tris {
        minimums + hull_cost
    } else {
        spent
    };
    let draw_calls = g.draw_calls
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
        over_budget: over_tris || draw_calls > DRAW_CALL_LIMIT,
    }
}

// ── Godot adapter ──

#[cfg(not(test))]
use godot::prelude::*;

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
        let c = allocate(
            &g,
            &MemoryDemand {
                cloud_rows: u(cloud_rows),
                route,
                burst_slots: u(burst_slots),
            },
        );
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
        d.set("over_budget", c.over_budget);
        d
    }

    /// [slots, triangles per slot] the allocator assumes for the burst pool.
    #[func]
    fn burst_pool_spec(&self) -> PackedInt32Array {
        PackedInt32Array::from(&[BURST_POOL_SLOTS as i32, BURST_TRIS_PER_SLOT as i32][..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_route::{route_triangles_for, MAX_PATH, MAX_SIDECAR};

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
}
