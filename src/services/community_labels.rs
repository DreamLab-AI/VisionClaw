//! ADR-2206 — community labels, cores and stable identities.
//!
//! Pure, actor-free functions over a caller's view of the graph. The handler
//! (`GET /api/analytics/communities/labels`) passes the view
//! `graph_access::caller_view` produced, so every count, term and exemplar here
//! is computed only over nodes the caller may see (ADR-050/ADR-2003): the IDF
//! background corpus, `size`, `coreSize` and `hasHull` included.

use std::collections::{HashMap, HashSet};

/// Thresholds, defaulted to the values measured offline on the live layout
/// (ADR-2206 Context). Tests shrink them so tiny corpora still exercise a rule.
#[derive(Debug, Clone)]
pub struct LabelParams {
    /// Spatial neighbours consulted by the core rule.
    pub k: usize,
    /// A member is core when at least this share of its neighbours share its community.
    pub core_share: f32,
    /// A community has a hull only when its core has MORE than this many members.
    pub hull_min_core: usize,
    /// A term must occur at least this often across all communities to be scored.
    pub min_corpus_count: u32,
    /// ... and at least this often inside the community.
    pub min_community_count: u32,
    /// Terms in a label.
    pub top_terms: usize,
    /// Exemplar nodes per community.
    pub exemplars: usize,
    /// `crossDomain` when the top domain's share is below this.
    pub cross_domain_below: f32,
    /// Largest member count for which the O(n²) spatial kNN runs; above it the
    /// topology rule is used.
    pub max_spatial_nodes: usize,
}

impl Default for LabelParams {
    fn default() -> Self {
        Self {
            k: 15,
            core_share: 0.5,
            hull_min_core: 5,
            min_corpus_count: 5,
            min_community_count: 3,
            top_terms: 3,
            exemplars: 3,
            cross_domain_below: 0.5,
            max_spatial_nodes: 20_000,
        }
    }
}

/// One node of the caller's view, as the label service reads it.
#[derive(Debug, Clone, Default)]
pub struct MemberInput {
    /// Graph node id (masked, as in `node_analytics`).
    pub id: u32,
    /// Community id of the run the client holds (0 = unassigned).
    pub community: u32,
    pub label: String,
    pub definition: String,
    /// `domain` metadata, falling back to `source_domain`.
    pub domain: Option<String>,
    /// An ontology class (not a page, agent or domain root): the only nodes that
    /// feed terms, domain mix and exemplars.
    pub is_content: bool,
    /// A domain root: never an exemplar.
    pub is_domain_root: bool,
    /// Current layout position; the spatial core rule needs every member to have one.
    pub pos: Option<[f32; 3]>,
}

/// An edge of the view.
#[derive(Debug, Clone, Copy)]
pub struct EdgeInput {
    pub a: u32,
    pub b: u32,
    /// `provenance == asserted`; only asserted edges rank exemplars.
    pub asserted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DomainShare {
    pub domain: String,
    pub share: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Exemplar {
    pub id: u32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommunityLabel {
    pub community_id: u32,
    /// Filled by the handler from [`StableRegistry`]; 0 here.
    pub stable_id: u64,
    pub size: usize,
    pub core_size: usize,
    pub core_ids: Vec<u32>,
    pub label: String,
    pub terms: Vec<String>,
    pub domain_mix: Vec<DomainShare>,
    pub cross_domain: bool,
    pub exemplars: Vec<Exemplar>,
    pub has_hull: bool,
}

/// Light English stemmer: merges plural and singular only ("agents" -> "agent").
pub fn stem(word: &str) -> String {
    let w = word.to_lowercase();
    if w.len() > 4 && w.ends_with("ies") {
        return format!("{}y", &w[..w.len() - 3]);
    }
    if w.len() > 4 && (w.ends_with("sses") || w.ends_with("shes") || w.ends_with("ches") || w.ends_with("xes")) {
        return w[..w.len() - 2].to_string();
    }
    if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") && !w.ends_with("is") {
        return w[..w.len() - 1].to_string();
    }
    w
}

const STOPWORDS: &str = "a about above after again all also an and any are as at be because been before being between both but by can could did do does during each either etc for from further had has have having he her here his how however i if in into is it its itself may more most much must no nor not of off on once only or other our out over own per same should so some such than that the their them then there these they this those through to too under until up upon use used uses using very via was we were what when where which while who whom why will with within without would you your often particularly typically including include includes enabling enable enables across among based set sets one two new well towards toward rather like many make makes made provide provides providing allows allow allowing refers refer term terms known called concept approach approaches system systems different various key";

fn is_stop(w: &str) -> bool {
    STOPWORDS.split(' ').any(|s| s == w)
}

/// Lower-case alphanumeric tokens (`[a-z][a-z0-9-]+`), stop words and tokens of
/// two characters or fewer removed.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let t = cur.trim_matches('-').to_string();
        if t.len() > 2 && t.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && !is_stop(&t) {
            out.push(t);
        }
        cur.clear();
    };
    for ch in text.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() || (ch == '-' && !cur.is_empty()) {
            cur.push(ch);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Unigrams then bigrams of one text, as (stemmed key, surface form).
fn terms_of(text: &str) -> Vec<(String, String)> {
    let toks = tokens(text);
    let mut out: Vec<(String, String)> = toks.iter().map(|t| (stem(t), t.clone())).collect();
    for w in toks.windows(2) {
        out.push((format!("{} {}", stem(&w[0]), stem(&w[1])), format!("{} {}", w[0], w[1])));
    }
    out
}

fn title_case(term: &str) -> String {
    term.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}


/// Members of each community (id > 0) whose neighbourhood is at least
/// `core_share` the same community. Spatial kNN when every member has a finite,
/// non-degenerate position and the view is small enough; otherwise topology.
pub fn core_members(
    members: &[MemberInput],
    edges: &[EdgeInput],
    p: &LabelParams,
) -> HashMap<u32, Vec<u32>> {
    let n = members.len();
    let comm: Vec<u32> = members.iter().map(|m| m.community).collect();
    let neighbours: Vec<Vec<usize>> = match spatial_neighbours(members, p) {
        Some(nn) => nn,
        None => topology_neighbours(members, edges),
    };
    let mut core: HashMap<u32, Vec<u32>> = HashMap::new();
    for i in 0..n {
        let c = comm[i];
        if c == 0 {
            continue;
        }
        let nb = &neighbours[i];
        let same = nb.iter().filter(|&&j| comm[j] == c).count();
        if !nb.is_empty() && same as f32 >= p.core_share * nb.len() as f32 {
            core.entry(c).or_default().push(members[i].id);
        }
    }
    core
}

/// k nearest members by position, or `None` when positions cannot be trusted
/// (a member without one, non-finite, every point coincident, or too many nodes).
fn spatial_neighbours(members: &[MemberInput], p: &LabelParams) -> Option<Vec<Vec<usize>>> {
    let n = members.len();
    if n < 2 || n > p.max_spatial_nodes || p.k == 0 {
        return None;
    }
    let mut pts: Vec<[f32; 3]> = Vec::with_capacity(n);
    for m in members {
        let q = m.pos?;
        if !q.iter().all(|v| v.is_finite()) {
            return None;
        }
        pts.push(q);
    }
    if pts.iter().all(|q| *q == pts[0]) {
        return None;
    }
    let k = p.k.min(n - 1);
    let mut out = Vec::with_capacity(n);
    let mut dist: Vec<(f32, usize)> = Vec::with_capacity(n);
    for i in 0..n {
        dist.clear();
        for j in 0..n {
            if i != j {
                let d = (pts[i][0] - pts[j][0]).powi(2) + (pts[i][1] - pts[j][1]).powi(2) + (pts[i][2] - pts[j][2]).powi(2);
                dist.push((d, j));
            }
        }
        dist.select_nth_unstable_by(k - 1, |a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        out.push(dist[..k].iter().map(|&(_, j)| j).collect());
    }
    Some(out)
}

/// Undirected, de-duplicated graph neighbours (the fallback core rule's neighbourhood).
fn topology_neighbours(members: &[MemberInput], edges: &[EdgeInput]) -> Vec<Vec<usize>> {
    let index: HashMap<u32, usize> = members.iter().enumerate().map(|(i, m)| (m.id, i)).collect();
    let mut sets: Vec<HashSet<usize>> = vec![HashSet::new(); members.len()];
    for e in edges {
        if let (Some(&a), Some(&b)) = (index.get(&e.a), index.get(&e.b)) {
            if a != b {
                sets[a].insert(b);
                sets[b].insert(a);
            }
        }
    }
    sets.into_iter().map(|s| s.into_iter().collect()).collect()
}

/// Labels for every community (id > 0) present in `members`, ascending by id.
pub fn build_labels(
    members: &[MemberInput],
    edges: &[EdgeInput],
    p: &LabelParams,
) -> Vec<CommunityLabel> {
    let core = core_members(members, edges, p);
    let by_id: HashMap<u32, &MemberInput> = members.iter().map(|m| (m.id, m)).collect();

    // Per-community term counts over the view, plus the corpus-wide totals (IDF background).
    let mut per: HashMap<u32, HashMap<String, u32>> = HashMap::new();
    let mut all: HashMap<String, u32> = HashMap::new();
    let mut surface: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let mut size: HashMap<u32, usize> = HashMap::new();
    let mut domains: HashMap<u32, HashMap<String, u32>> = HashMap::new();
    for m in members.iter().filter(|m| m.community > 0) {
        *size.entry(m.community).or_default() += 1;
        if !m.is_content || m.is_domain_root {
            continue;
        }
        if let Some(d) = m.domain.as_ref().filter(|d| !d.is_empty()) {
            *domains.entry(m.community).or_default().entry(d.clone()).or_default() += 1;
        }
        let counts = per.entry(m.community).or_default();
        for (key, surf) in terms_of(&format!("{}. {}", m.label, m.definition)) {
            *counts.entry(key.clone()).or_default() += 1;
            *all.entry(key.clone()).or_default() += 1;
            *surface.entry(key).or_default().entry(surf).or_default() += 1;
        }
    }
    let mean_total = if per.is_empty() {
        1.0
    } else {
        per.values().map(|c| c.values().sum::<u32>() as f64).sum::<f64>() / per.len() as f64
    };

    // Asserted in-community degree between content members, for exemplars.
    let mut degree: HashMap<u32, u32> = HashMap::new();
    for e in edges.iter().filter(|e| e.asserted && e.a != e.b) {
        if let (Some(a), Some(b)) = (by_id.get(&e.a), by_id.get(&e.b)) {
            if a.community > 0 && a.community == b.community && a.is_content && b.is_content {
                *degree.entry(a.id).or_default() += 1;
                *degree.entry(b.id).or_default() += 1;
            }
        }
    }

    let mut ids: Vec<u32> = size.keys().copied().collect();
    ids.sort_unstable();
    ids.into_iter()
        .map(|c| {
            let counts = per.get(&c);
            let total = counts.map(|m| m.values().sum::<u32>()).unwrap_or(0).max(1) as f64;
            let mut scored: Vec<(String, f64)> = counts
                .into_iter()
                .flatten()
                .filter(|(k, &f)| f >= p.min_community_count && all[*k] >= p.min_corpus_count)
                .map(|(k, &f)| (k.clone(), (f as f64 / total) * (1.0 + mean_total / all[k] as f64).ln()))
                .collect();
            scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

            // Compact on stems: a term that is a sub- or super-set of a chosen one is skipped.
            let mut chosen: Vec<String> = Vec::new();
            for (key, _) in &scored {
                let kw: HashSet<&str> = key.split(' ').collect();
                let overlaps = chosen.iter().any(|o| {
                    let ow: HashSet<&str> = o.split(' ').collect();
                    kw.is_subset(&ow) || ow.is_subset(&kw)
                });
                if !overlaps {
                    chosen.push(key.clone());
                    if chosen.len() == p.top_terms {
                        break;
                    }
                }
            }
            let terms: Vec<String> = chosen
                .iter()
                .map(|k| {
                    let forms = &surface[k];
                    forms
                        .iter()
                        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                        .map(|(f, _)| f.clone())
                        .unwrap_or_else(|| k.clone())
                })
                .collect();
            let label = terms.iter().map(|t| title_case(t)).collect::<Vec<_>>().join(" \u{b7} ");

            let mut mix: Vec<(String, u32)> = domains.get(&c).map(|d| d.iter().map(|(k, v)| (k.clone(), *v)).collect()).unwrap_or_default();
            mix.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let dom_total: u32 = mix.iter().map(|(_, v)| v).sum();
            let domain_mix: Vec<DomainShare> = mix
                .iter()
                .take(3)
                .map(|(d, v)| DomainShare { domain: d.clone(), share: *v as f32 / dom_total.max(1) as f32 })
                .collect();
            let cross_domain = domain_mix.first().is_some_and(|d| d.share < p.cross_domain_below);

            let mut ex: Vec<(u32, u32)> = members
                .iter()
                .filter(|m| m.community == c && m.is_content && !m.is_domain_root)
                .filter_map(|m| degree.get(&m.id).map(|d| (m.id, *d)))
                .collect();
            ex.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let exemplars = ex
                .into_iter()
                .take(p.exemplars)
                .map(|(id, _)| Exemplar { id, label: by_id[&id].label.clone() })
                .collect();

            let mut core_ids = core.get(&c).cloned().unwrap_or_default();
            core_ids.sort_unstable();
            CommunityLabel {
                community_id: c,
                stable_id: 0,
                size: size[&c],
                core_size: core_ids.len(),
                has_hull: core_ids.len() > p.hull_min_core,
                core_ids,
                label,
                terms,
                domain_mix,
                cross_domain,
                exemplars,
            }
        })
        .collect()
}

/// Minimum Jaccard overlap for a community to keep its predecessor's identity.
const JACCARD_MIN: f64 = 0.5;

/// Keeps a stable identity for each community across detection runs.
#[derive(Debug, Default)]
pub struct StableRegistry {
    generation: u64,
    next_stable: u64,
    prev: Vec<(u64, HashSet<u32>)>,
    by_community: HashMap<u32, u64>,
}

/// The process-wide registry: written once per detection run by the clustering
/// actor (after the modularity gate), read by the labels endpoint.
pub fn registry() -> &'static std::sync::Mutex<StableRegistry> {
    static R: std::sync::OnceLock<std::sync::Mutex<StableRegistry>> = std::sync::OnceLock::new();
    R.get_or_init(|| std::sync::Mutex::new(StableRegistry::new()))
}

impl StableRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    /// Record a finished detection run (`(node id, community id)` after the
    /// modularity gate; community 0 is ignored) and return its generation.
    pub fn observe(&mut self, assignments: &[(u32, u32)]) -> u64 {
        let mut now: HashMap<u32, HashSet<u32>> = HashMap::new();
        for &(node, c) in assignments {
            if c > 0 {
                now.entry(c).or_default().insert(node);
            }
        }
        // Every (old, new) pair with Jaccard >= 0.5, best first; each side is claimed once.
        let mut pairs: Vec<(f64, usize, u32)> = Vec::new();
        for (&c, members) in &now {
            for (pi, (_, old)) in self.prev.iter().enumerate() {
                let inter = members.intersection(old).count();
                if inter == 0 {
                    continue;
                }
                let j = inter as f64 / (members.len() + old.len() - inter) as f64;
                if j >= JACCARD_MIN {
                    pairs.push((j, pi, c));
                }
            }
        }
        pairs.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut by_community: HashMap<u32, u64> = HashMap::new();
        let mut used_old: HashSet<usize> = HashSet::new();
        for (_, pi, c) in pairs {
            if by_community.contains_key(&c) || used_old.contains(&pi) {
                continue;
            }
            used_old.insert(pi);
            by_community.insert(c, self.prev[pi].0);
        }
        let mut ids: Vec<u32> = now.keys().copied().collect();
        ids.sort_unstable();
        for c in ids {
            if !by_community.contains_key(&c) {
                self.next_stable += 1;
                by_community.insert(c, self.next_stable);
            }
        }
        self.prev = now.into_iter().map(|(c, m)| (by_community[&c], m)).collect();
        self.by_community = by_community;
        self.generation += 1;
        self.generation
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// The run's `community_id -> stableId` map, copied so a caller can release the
    /// registry lock before doing any real work.
    pub fn stable_ids(&self) -> HashMap<u32, u64> {
        self.by_community.clone()
    }
    /// Stable id of the run's community `community_id`.
    pub fn stable_id(&self, community_id: u32) -> Option<u64> {
        self.by_community.get(&community_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(id: u32, community: u32, label: &str, def: &str, domain: &str) -> MemberInput {
        MemberInput {
            id,
            community,
            label: label.into(),
            definition: def.into(),
            domain: Some(domain.into()),
            is_content: true,
            ..Default::default()
        }
    }

    /// Thresholds small enough for a hand-sized corpus.
    fn tiny() -> LabelParams {
        LabelParams {
            k: 3,
            hull_min_core: 2,
            min_corpus_count: 2,
            min_community_count: 2,
            ..Default::default()
        }
    }

    fn corpus() -> Vec<MemberInput> {
        vec![
            // community 1: agents (singular and plural must merge)
            content(1, 1, "Agent", "an agent acts", "ai"),
            content(2, 1, "Agents", "agents coordinate agent teams", "ai"),
            content(3, 1, "Agent Teams", "agent teams share goals", "ai"),
            // community 2: rendering
            content(4, 2, "Shader", "a shader renders pixels", "graphics"),
            content(5, 2, "Shaders", "shaders render pixels fast", "graphics"),
            content(6, 2, "Pixel Shader", "pixel shader stage", "graphics"),
        ]
    }

    #[test]
    fn stemmer_merges_plural_and_singular() {
        assert_eq!(stem("agents"), stem("agent"));
        assert_eq!(stem("agent"), "agent");
        assert_eq!(stem("policies"), stem("policy"));
        assert_eq!(stem("class"), "class", "-ss is not a plural");
        assert_eq!(stem("analysis"), "analysis", "-is is not a plural");
    }

    #[test]
    fn ctfidf_label_merges_agent_and_agents_and_picks_the_distinctive_term() {
        let labels = build_labels(&corpus(), &[], &tiny());
        let c1 = labels.iter().find(|l| l.community_id == 1).expect("community 1");
        assert_eq!(c1.terms[0].to_lowercase(), "agent", "terms: {:?}", c1.terms);
        let agentish = c1.terms.iter().filter(|t| t.to_lowercase().contains("agent")).count();
        assert!(agentish <= 2, "agent/agents must not fill every slot: {:?}", c1.terms);
        let c2 = labels.iter().find(|l| l.community_id == 2).unwrap();
        assert!(c2.terms.iter().any(|t| t.to_lowercase().contains("shader")), "{:?}", c2.terms);
        assert!(c2.label.to_lowercase().contains("shader"));
    }

    #[test]
    fn display_term_is_the_most_frequent_surface_form() {
        let labels = build_labels(&corpus(), &[], &tiny());
        let c1 = labels.iter().find(|l| l.community_id == 1).unwrap();
        // "agent" x6 vs "agents" x2 in the corpus above.
        assert_eq!(c1.terms[0], "agent");
    }

    #[test]
    fn domain_mix_reports_shares_and_flags_cross_domain() {
        let mut m = corpus();
        m[1].domain = Some("graphics".into()); // community 1: ai 2/3, graphics 1/3
        m[2].domain = Some("graphics".into()); // ai 1/3
        let labels = build_labels(&m, &[], &tiny());
        let c1 = labels.iter().find(|l| l.community_id == 1).unwrap();
        assert_eq!(c1.domain_mix[0].domain, "graphics");
        assert!((c1.domain_mix[0].share - 2.0 / 3.0).abs() < 1e-3);
        assert!(!c1.cross_domain);
        let mut m = corpus();
        m.push(content(7, 1, "Sensor", "sensor data", "robotics"));
        m.push(content(8, 1, "Planner", "planner steps", "robotics"));
        m.push(content(9, 1, "Servo", "servo motor", "robotics"));
        m.push(content(10, 1, "Link", "network link", "networks"));
        let labels = build_labels(&m, &[], &tiny());
        let c1 = labels.iter().find(|l| l.community_id == 1).unwrap();
        assert_eq!(c1.domain_mix.len(), 3);
        assert!(c1.cross_domain, "ai is 3 of 7 (< 0.5): {:?}", c1.domain_mix);
    }

    #[test]
    fn exemplars_rank_by_asserted_degree_and_skip_roots_and_non_content() {
        let mut m = corpus();
        m[0].is_domain_root = true;
        m.push(MemberInput {
            id: 10,
            community: 1,
            label: "Some Page".into(),
            is_content: false,
            ..Default::default()
        });
        let e = |a, b, asserted| EdgeInput { a, b, asserted };
        let edges = [
            e(1, 2, true),
            e(1, 3, true),
            e(1, 10, true),
            e(10, 2, true),
            e(10, 3, true),
            e(2, 3, true),
            e(3, 2, false), // inferred: does not count
            e(4, 5, true),  // other community
            e(2, 5, true),  // cross-community edge: does not count
        ];
        let labels = build_labels(&m, &edges, &tiny());
        let c1 = labels.iter().find(|l| l.community_id == 1).unwrap();
        let ids: Vec<u32> = c1.exemplars.iter().map(|x| x.id).collect();
        assert!(!ids.contains(&1), "domain root excluded: {ids:?}");
        assert!(!ids.contains(&10), "page node excluded: {ids:?}");
        assert_eq!(ids, vec![2, 3], "ranked by asserted in-community degree");
        assert_eq!(c1.exemplars[0].label, "Agents");
    }

    #[test]
    fn topology_core_is_members_with_half_their_neighbours_in_the_community() {
        // 1-2-3 triangle in community 1; 4 in community 1 hangs off 5,6 (community 2).
        let m = vec![
            content(1, 1, "a", "", "d"),
            content(2, 1, "b", "", "d"),
            content(3, 1, "c", "", "d"),
            content(4, 1, "bridge", "", "d"),
            content(5, 2, "e", "", "d"),
            content(6, 2, "f", "", "d"),
        ];
        let e = |a, b| EdgeInput { a, b, asserted: true };
        let edges = [e(1, 2), e(2, 3), e(1, 3), e(4, 5), e(4, 6), e(4, 1), e(5, 6)];
        let core = core_members(&m, &edges, &tiny());
        let mut c1 = core[&1].clone();
        c1.sort_unstable();
        assert_eq!(c1, vec![1, 2, 3], "4 has 1/3 same-community neighbours");
        let mut c2 = core[&2].clone();
        c2.sort_unstable();
        assert_eq!(c2, vec![5, 6]);
    }

    #[test]
    fn spatial_core_uses_positions_when_every_member_has_one() {
        // Two clusters on a line; node 3 (community 1) sits inside cluster B.
        let at = |id, c, x: f32| MemberInput {
            id,
            community: c,
            is_content: true,
            pos: Some([x, 0.0, 0.0]),
            ..Default::default()
        };
        let m = vec![
            at(1, 1, 0.0),
            at(2, 1, 1.0),
            at(3, 1, 101.0), // strays among community 2
            at(4, 2, 100.0),
            at(5, 2, 102.0),
            at(6, 2, 103.0),
        ];
        let p = LabelParams { k: 2, ..tiny() };
        let core = core_members(&m, &[], &p);
        let mut c1 = core[&1].clone();
        c1.sort_unstable();
        assert_eq!(c1, vec![1, 2], "positions, not topology (no edges), decide");
        assert!(!core[&1].contains(&3));
    }

    #[test]
    fn hull_needs_a_core_larger_than_the_threshold() {
        let at = |id, c, x: f32| MemberInput {
            id,
            community: c,
            is_content: true,
            pos: Some([x, 0.0, 0.0]),
            ..Default::default()
        };
        let m = vec![
            at(1, 1, 0.0),
            at(2, 1, 1.0),
            at(3, 1, 2.0),
            at(4, 2, 500.0),
            at(5, 2, 900.0),
        ];
        let p = LabelParams { k: 2, hull_min_core: 2, ..tiny() };
        let labels = build_labels(&m, &[], &p);
        assert!(labels.iter().find(|l| l.community_id == 1).unwrap().has_hull);
        let c2 = labels.iter().find(|l| l.community_id == 2).unwrap();
        assert!(!c2.has_hull);
        assert_eq!(c2.size, 2);
    }

    #[test]
    fn a_hidden_term_never_reaches_a_visible_label_or_shifts_idf() {
        // The view handed in is what caller_view produced: the private node is absent.
        // Building with it present proves the term would otherwise top the ranking.
        let mut with_private = corpus();
        for i in 20..26 {
            with_private.push(content(i, 1, "Zebrafish", "zebrafish zebrafish", "ai"));
        }
        let leaked = build_labels(&with_private, &[], &tiny());
        assert!(leaked[0].terms[0].to_lowercase().contains("zebrafish"), "{:?}", leaked[0].terms);
        let public_only = build_labels(&corpus(), &[], &tiny());
        for l in &public_only {
            assert!(!l.label.to_lowercase().contains("zebrafish"));
            assert!(l.terms.iter().all(|t| !t.to_lowercase().contains("zebrafish")));
            assert!(l.exemplars.iter().all(|x| x.id < 20));
        }
        assert_eq!(public_only[0].size, 3, "size counts the view only");
    }

    #[test]
    fn stable_ids_survive_a_renumbered_run_and_split_gets_a_new_one() {
        let mut r = StableRegistry::new();
        let run1: Vec<(u32, u32)> = (0..10).map(|n| (n, 1)).chain((10..20).map(|n| (n, 2))).collect();
        assert_eq!(r.observe(&run1), 1);
        let (a, b) = (r.stable_id(1).unwrap(), r.stable_id(2).unwrap());
        assert_ne!(a, b);
        // Renumbered: old 1 -> 7, old 2 -> 3, one member moved, a new community 9 appears.
        let mut run2: Vec<(u32, u32)> = (0..10).map(|n| (n, 7)).chain((10..19).map(|n| (n, 3))).collect();
        run2[0] = (0, 3);
        run2.push((19, 9));
        run2.extend((30..34).map(|n| (n, 9)));
        assert_eq!(r.observe(&run2), 2);
        assert_eq!(r.stable_id(7), Some(a));
        assert_eq!(r.stable_id(3), Some(b));
        let fresh = r.stable_id(9).unwrap();
        assert!(fresh != a && fresh != b, "no overlap >= 0.5 -> new identity");
        // Wire id 0 (unassigned) never gets an identity.
        assert_eq!(r.stable_id(0), None);
    }

    #[test]
    fn two_new_communities_do_not_both_claim_one_old_identity() {
        let mut r = StableRegistry::new();
        let run1: Vec<(u32, u32)> = (0..10).map(|n| (n, 1)).collect();
        r.observe(&run1);
        let a = r.stable_id(1).unwrap();
        // Community 1 splits 6/4: only the 6-share (J=0.6) keeps the identity.
        let run2: Vec<(u32, u32)> = (0..6).map(|n| (n, 5)).chain((6..10).map(|n| (n, 6))).collect();
        r.observe(&run2);
        assert_eq!(r.stable_id(5), Some(a));
        assert_ne!(r.stable_id(6), Some(a));
    }
}
