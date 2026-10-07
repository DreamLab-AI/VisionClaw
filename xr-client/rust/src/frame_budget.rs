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
//! 1. **Minimums**, in priority order: the graph's far tiers (an impostor quad
//!    per node, a ribbon quad per edge, labelled nodes on the full mesh), the
//!    route at one sample per hop, a cloud floor that still reads as a cloud,
//!    and a minimum near field of gem nodes.
//! 2. **Growth** up to each layer's demand, in the same priority: the route to
//!    full curve detail, the cloud to its sprite cap, then the graph's near
//!    detail (hulls, then gem nodes, then cylinder edges; the reverse of the
//!    order they give way).
//!
//! Near-tier costs are charged as the increment over the far tier that the
//! node or edge would otherwise use. If the minimums alone exceed the budget
//! the minimums are still returned, with `over_budget` set, so the benchmark
//! reports it rather than a layer silently vanishing.

use crate::hulls::{DEFAULT_MAX_HULLS, MAX_TRIS_PER_HULL};
use crate::lod::{
    CYLINDER_TRIS_PER_EDGE, DEFAULT_NEAR_CAP, DEFAULT_NEAR_EDGE_CAP, GEM_TRIS_PER_NODE, IMPOSTOR_TRIS_PER_NODE,
    RIBBON_TRIS_PER_EDGE,
};
use crate::memory_cloud::{DEFAULT_SPRITE_CAP, TRIANGLES_PER_SPRITE};
use crate::memory_route::{ring_cap_for_budget, route_triangles_at, ROUTE_RING_CAP};

/// Frame triangle budget.
pub const TRI_BUDGET: usize = 100_000;
/// Frame draw-call budget.
pub const DRAW_CALL_BUDGET: usize = 50;
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
    let far = g.nodes * IMPOSTOR_TRIS_PER_NODE + g.edges * RIBBON_TRIS_PER_EDGE + faded * GEM_STEP;

    let route_min = m.route.map_or(0, |r| route_triangles_at(r.rows, r.sidecar, 0));
    let cloud_want = m.cloud_rows.min(DEFAULT_SPRITE_CAP);
    let cloud_min = cloud_want.min(CLOUD_MIN_SPRITES);
    let gem_candidates = g.nodes - faded;
    let gem_max = gem_candidates.min(DEFAULT_NEAR_CAP);
    let gem_min = gem_max.min(GEM_MIN);

    let minimums = far + route_min + cloud_min * TRIANGLES_PER_SPRITE + gem_min * GEM_STEP;
    let over_tris = minimums > TRI_BUDGET;
    let mut rem = TRI_BUDGET.saturating_sub(minimums);

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

    let spent = TRI_BUDGET.saturating_sub(rem);
    let tris_total = if over_tris { minimums + hull_cost } else { spent };
    let draw_calls = g.draw_calls
        + if cloud_sprites > 0 { CLOUD_DRAW_CALLS } else { 0 }
        + if m.route.is_some() { ROUTE_DRAW_CALLS } else { 0 };

    FrameCaps {
        graph: GraphCaps { gem_nodes, cylinder_edges, max_hulls },
        cloud_sprites,
        route_tris,
        route_ring_cap,
        tris_total,
        draw_calls,
        over_budget: over_tris || draw_calls > DRAW_CALL_BUDGET,
    }
}

// ── Godot adapter ──

#[cfg(not(test))]
use godot::prelude::*;

/// Godot face of [`allocate`]: `FrameBudget.new().allocate(...)`.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct FrameBudget {
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl FrameBudget {
    /// Caps for one frame. `route_rows` 0 = no route shown; `cloud_rows` 0 =
    /// cloud hidden. Returns gem_nodes, cylinder_edges, max_hulls,
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
        graph_draw_calls: i64,
        cloud_rows: i64,
        route_rows: i64,
        route_sidecar: i64,
    ) -> Dictionary {
        let u = |v: i64| v.max(0) as usize;
        let g = GraphDemand {
            nodes: u(nodes),
            edges: u(edges),
            hulls: u(hulls),
            hull_tris: u(hull_tris),
            faded: u(faded),
            draw_calls: u(graph_draw_calls),
        };
        let route = (route_rows >= 2).then(|| RouteShape { rows: u(route_rows), sidecar: u(route_sidecar) });
        let c = allocate(&g, &MemoryDemand { cloud_rows: u(cloud_rows), route });
        let mut d = Dictionary::new();
        d.set("gem_nodes", c.graph.gem_nodes as i64);
        d.set("cylinder_edges", c.graph.cylinder_edges as i64);
        d.set("max_hulls", c.graph.max_hulls as i64);
        d.set("cloud_sprites", c.cloud_sprites as i64);
        d.set("route_tris", c.route_tris as i64);
        d.set("route_ring_cap", c.route_ring_cap as i64);
        d.set("tris_total", c.tris_total as i64);
        d.set("draw_calls", c.draw_calls as i64);
        d.set("over_budget", c.over_budget);
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_route::{route_triangles_for, MAX_PATH, MAX_SIDECAR};

    /// Production graph as xr-graph measures it: 13 164 nodes, 20 000 edges,
    /// 32 hulls (2 938 triangles), 6 draw calls.
    fn production() -> GraphDemand {
        GraphDemand { nodes: 13_164, edges: 20_000, hulls: 32, hull_tris: 2_938, faded: 0, draw_calls: 6 }
    }

    fn full_memory() -> MemoryDemand {
        MemoryDemand { cloud_rows: 20_000, route: Some(RouteShape { rows: MAX_PATH, sidecar: MAX_SIDECAR }) }
    }

    /// Recompute what the caps cost, independently of `allocate`'s bookkeeping.
    fn cost(g: &GraphDemand, m: &MemoryDemand, c: &FrameCaps) -> usize {
        let faded = g.faded.min(g.nodes);
        let gem = c.graph.gem_nodes.min(g.nodes - faded);
        let cyl = c.graph.cylinder_edges.min(g.edges);
        let hull = if c.graph.max_hulls >= g.hulls { g.hull_tris } else { c.graph.max_hulls * MAX_TRIS_PER_HULL };
        let route = m.route.map_or(0, |r| route_triangles_at(r.rows, r.sidecar, c.route_ring_cap));
        (faded + gem) * GEM_TRIS_PER_NODE
            + (g.nodes - faded - gem) * IMPOSTOR_TRIS_PER_NODE
            + cyl * CYLINDER_TRIS_PER_EDGE
            + (g.edges - cyl) * RIBBON_TRIS_PER_EDGE
            + hull
            + c.cloud_sprites * TRIANGLES_PER_SPRITE
            + route
    }

    #[test]
    fn production_scene_with_full_cloud_and_longest_route_fits() {
        let (g, m) = (production(), full_memory());
        let c = allocate(&g, &m);
        assert!(!c.over_budget);
        assert_eq!(cost(&g, &m, &c), c.tris_total, "bookkeeping matches an independent recount");
        assert!(c.tris_total <= TRI_BUDGET, "{}", c.tris_total);
        assert!(c.draw_calls <= DRAW_CALL_BUDGET);
        assert_eq!(c.draw_calls, 6 + CLOUD_DRAW_CALLS + ROUTE_DRAW_CALLS);
        // priority: route and cloud whole, hulls whole, near detail takes the rest
        assert_eq!(c.route_tris, route_triangles_for(MAX_PATH, MAX_SIDECAR));
        assert_eq!(c.cloud_sprites, DEFAULT_SPRITE_CAP);
        assert_eq!(c.graph.max_hulls, 32);
        assert!(c.graph.gem_nodes >= GEM_MIN && c.graph.gem_nodes < DEFAULT_NEAR_CAP, "{:?}", c.graph);
        assert!(TRI_BUDGET - c.tris_total < CYLINDER_STEP.max(GEM_STEP), "budget used up, not padded");
    }

    #[test]
    fn small_graph_without_memory_layers_gets_the_defaults() {
        let g = GraphDemand { nodes: 1_000, edges: 1_500, hulls: 12, hull_tris: 1_100, faded: 0, draw_calls: 6 };
        let c = allocate(&g, &MemoryDemand::default());
        assert_eq!(c.graph, GraphCaps { gem_nodes: DEFAULT_NEAR_CAP, cylinder_edges: DEFAULT_NEAR_EDGE_CAP, max_hulls: 12 });
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
            let g = GraphDemand { nodes, edges: nodes * 3 / 2, hulls: 32, hull_tris: 2_938, faded: 0, draw_calls: 6 };
            let c = allocate(&g, &m);
            // A higher-priority layer below its demand could not afford its next
            // step from what the lower layers took (leftovers trickle down).
            if c.graph.gem_nodes < DEFAULT_NEAR_CAP {
                assert!(c.graph.cylinder_edges * CYLINDER_STEP < GEM_STEP, "{nodes}: cylinders give way before gems");
            }
            if c.graph.max_hulls < 32 {
                assert_eq!(c.graph.gem_nodes, GEM_MIN, "{nodes}: gems at minimum before hulls give");
                assert!(c.graph.cylinder_edges * CYLINDER_STEP < MAX_TRIS_PER_HULL, "{nodes}");
            }
            if c.cloud_sprites < DEFAULT_SPRITE_CAP {
                assert_eq!((c.graph.max_hulls, c.graph.gem_nodes, c.graph.cylinder_edges), (0, GEM_MIN, 0), "{nodes}: graph detail gone before the cloud shrinks");
            }
            if c.route_tris < route_full {
                assert_eq!(c.cloud_sprites, CLOUD_MIN_SPRITES, "{nodes}: cloud at floor before the route thins");
            }
            if !c.over_budget {
                assert!(c.tris_total <= TRI_BUDGET, "{nodes}: {}", c.tris_total);
                assert_eq!(cost(&g, &m, &c), c.tris_total, "{nodes}");
            }
            if let Some(p) = prev {
                assert!(c.cloud_sprites <= p.cloud_sprites && c.route_tris <= p.route_tris, "{nodes}: monotone");
            }
            prev = Some(c);
        }
        assert!(prev.unwrap().over_budget, "the sweep reaches an impossible graph");
    }

    #[test]
    fn a_thinner_route_is_used_before_it_overruns() {
        // a 13-row route wants 10 samples per hop; with little room it drops detail
        let g = GraphDemand { nodes: 21_625, edges: 21_625, hulls: 0, hull_tris: 0, faded: 0, draw_calls: 6 };
        let m = MemoryDemand { cloud_rows: 20_000, route: Some(RouteShape { rows: 13, sidecar: 5 }) };
        let c = allocate(&g, &m);
        assert!(!c.over_budget);
        assert!(c.route_ring_cap < ROUTE_RING_CAP && c.route_ring_cap >= 13, "{}", c.route_ring_cap);
        assert_eq!(c.graph.gem_nodes, GEM_MIN);
        // the next detail step up would not have fitted in what trickled down
        let next = (c.route_ring_cap + 1..=ROUTE_RING_CAP)
            .map(|cap| route_triangles_at(13, 5, cap))
            .find(|&t| t > c.route_tris)
            .unwrap();
        let trickled = (c.cloud_sprites - CLOUD_MIN_SPRITES) * TRIANGLES_PER_SPRITE
            + c.graph.cylinder_edges * CYLINDER_STEP
            + (TRI_BUDGET - c.tris_total);
        assert!(next - c.route_tris > trickled, "next step {} vs {trickled}", next - c.route_tris);
        assert!(c.tris_total <= TRI_BUDGET);
        assert_eq!(cost(&g, &m, &c), c.tris_total);
    }

    #[test]
    fn impossible_graph_reports_over_budget_with_minimums() {
        let g = GraphDemand { nodes: 60_000, edges: 0, hulls: 32, hull_tris: 2_938, faded: 0, draw_calls: 6 };
        let c = allocate(&g, &full_memory());
        assert!(c.over_budget);
        assert_eq!(c.graph, GraphCaps { gem_nodes: GEM_MIN, cylinder_edges: 0, max_hulls: 0 });
        assert_eq!(c.cloud_sprites, CLOUD_MIN_SPRITES);
        assert_eq!(c.route_tris, route_triangles_at(MAX_PATH, MAX_SIDECAR, 0));
        assert!(c.tris_total > TRI_BUDGET);
    }

    #[test]
    fn faded_nodes_are_charged_the_full_mesh() {
        let base = production();
        let f = GraphDemand { faded: 10, ..base };
        let m = full_memory();
        let (a, b) = (allocate(&base, &m), allocate(&f, &m));
        assert_eq!(cost(&f, &m, &b), b.tris_total);
        assert!(b.tris_total <= TRI_BUDGET);
        assert_eq!(a.graph.gem_nodes - b.graph.gem_nodes, 10, "each faded node displaces one gem");
    }

    #[test]
    fn hidden_cloud_and_no_route_cost_nothing() {
        let c = allocate(&production(), &MemoryDemand::default());
        assert_eq!((c.cloud_sprites, c.route_tris), (0, 0));
        assert_eq!(c.draw_calls, 6);
        let small = allocate(&production(), &MemoryDemand { cloud_rows: 500, route: None });
        assert_eq!(small.cloud_sprites, 500, "a small snapshot draws in full");
        assert_eq!(small.draw_calls, 7);
    }

    #[test]
    fn draw_call_overrun_is_reported() {
        let g = GraphDemand { draw_calls: 48, ..production() };
        assert!(allocate(&g, &full_memory()).over_budget);
        assert!(!allocate(&g, &MemoryDemand::default()).over_budget);
    }
}
