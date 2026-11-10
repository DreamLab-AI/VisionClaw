//! Static H1 runner (`PREREGISTRATION.md`, deviation D4): every arm over seeds 0–4 on one
//! pair-level 80/10/10 split per seed, metrics M1–M5 from final positions, the space-geo-supply
//! slice, the six gate verdicts on seed means, and the two D4 controls.
//!
//! Usage: `h1 --corpus <knowledge dir> --corpus-label <text> --out <dir> [--threads N]
//! [--subsample N] [--seeds 0,1,2]`. `--subsample` keeps a seeded random node subset and its
//! induced triples, for mechanics checks only.

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use relational_field::corpus::{self, Triple};
use relational_field::embed::{self, Pos};
use relational_field::metrics::{self, TypeClassifier};
use relational_field::split::{pair, split, Split};
use relational_field::{controls, sim};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

/// The D1 space-geo-supply slice.
const SGS: [&str; 4] = [
    "space-science-and-systems",
    "earth-observation-and-geospatial-sensing",
    "supply-chains-and-logistics",
    "supply-chain",
];
const ARMS: [&str; 5] = ["b0", "b1", "b2", "rf", "rf_sim"];
/// Rewiring attempts per training triple (D4).
const REWIRE_ATTEMPTS: usize = 10;

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|x| x == name)
        .and_then(|i| a.get(i + 1).cloned())
}

/// The graph as the runner needs it, possibly subsampled.
struct Data {
    n: usize,
    domain: Vec<Option<u16>>,
    domain_names: Vec<String>,
    rel_names: Vec<String>,
    triples: Vec<Triple>,
    in_slice: Vec<bool>,
}

fn subsample(g: corpus::Graph, keep: Option<usize>) -> Data {
    let n_all = g.titles.len();
    let ids: Vec<u32> = match keep {
        Some(k) if k < n_all => {
            let mut v: Vec<u32> = (0..n_all as u32).collect();
            v.shuffle(&mut StdRng::seed_from_u64(0x5ab5));
            v.truncate(k);
            v.sort_unstable();
            v
        }
        _ => (0..n_all as u32).collect(),
    };
    let remap: HashMap<u32, u32> = ids
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new as u32))
        .collect();
    let triples = g
        .triples
        .iter()
        .filter_map(|t| {
            Some(Triple {
                head: *remap.get(&t.head)?,
                rel: t.rel,
                tail: *remap.get(&t.tail)?,
            })
        })
        .collect();
    let domain: Vec<Option<u16>> = ids.iter().map(|&i| g.domain[i as usize]).collect();
    let slice_ids: HashSet<u16> = g
        .domain_names
        .iter()
        .enumerate()
        .filter(|(_, d)| SGS.contains(&d.as_str()))
        .map(|(i, _)| i as u16)
        .collect();
    let in_slice = domain
        .iter()
        .map(|d| d.is_some_and(|d| slice_ids.contains(&d)))
        .collect();
    Data {
        n: ids.len(),
        domain,
        domain_names: g.domain_names,
        rel_names: g.rel_names,
        triples,
        in_slice,
    }
}

/// Everything fixed per seed before any arm runs: the split and the shared negatives.
struct SeedData {
    seed: u64,
    sp: Split,
    m1_neg: Vec<(u32, u32)>,
    m2_neg: Vec<(u32, u32)>,
    m2_fallbacks: usize,
    /// Indices into `sp.test_pairs` / the negatives for pairs with both endpoints in the slice.
    slice_pairs: Vec<usize>,
    slice_test: Vec<Triple>,
    domain_nodes: Vec<u32>,
    slice_nodes: Vec<u32>,
    nb: Vec<HashSet<u32>>,
}

fn prepare(d: &Data, seed: u64) -> SeedData {
    let sp = split(&d.triples, seed);
    let mut rng = StdRng::seed_from_u64(seed ^ 0x4831_5354);
    let n = d.n as u32;
    let non_edge = |a: u32, b: u32| a != b && !sp.all_pairs.contains(&pair(a, b));
    let m1_neg: Vec<(u32, u32)> = sp
        .test_pairs
        .iter()
        .map(|_| loop {
            let (a, b) = (rng.gen_range(0..n), rng.gen_range(0..n));
            if non_edge(a, b) {
                return (a, b);
            }
        })
        .collect();

    // M2 source: head of the pair's first test triple in (rel, head, tail) order.
    let mut first: HashMap<(u32, u32), Triple> = HashMap::new();
    let mut sorted = sp.test.clone();
    sorted.sort_by_key(|t| (t.rel, t.head, t.tail));
    for t in sorted {
        first.entry(pair(t.head, t.tail)).or_insert(t);
    }
    let mut members: HashMap<u16, Vec<u32>> = HashMap::new();
    for (i, dm) in d.domain.iter().enumerate() {
        if let Some(dm) = dm {
            members.entry(*dm).or_default().push(i as u32);
        }
    }
    let mut m2_fallbacks = 0;
    let m2_neg: Vec<(u32, u32)> = sp
        .test_pairs
        .iter()
        .map(|p| {
            let src = first[p].head;
            if let Some(pool) = d.domain[src as usize].and_then(|dm| members.get(&dm)) {
                for _ in 0..10_000 {
                    let x = pool[rng.gen_range(0..pool.len())];
                    if non_edge(src, x) {
                        return (src, x);
                    }
                }
            }
            m2_fallbacks += 1;
            loop {
                let x = rng.gen_range(0..n);
                if non_edge(src, x) {
                    return (src, x);
                }
            }
        })
        .collect();

    let slice_pairs = (0..sp.test_pairs.len())
        .filter(|&k| {
            d.in_slice[sp.test_pairs[k].0 as usize] && d.in_slice[sp.test_pairs[k].1 as usize]
        })
        .collect();
    let slice_test = sp
        .test
        .iter()
        .copied()
        .filter(|t| d.in_slice[t.head as usize] && d.in_slice[t.tail as usize])
        .collect();
    let domain_nodes = (0..n).filter(|&i| d.domain[i as usize].is_some()).collect();
    let slice_nodes = (0..n).filter(|&i| d.in_slice[i as usize]).collect();
    let nb = metrics::neighbours(d.n, &sp.train);
    SeedData {
        seed,
        sp,
        m1_neg,
        m2_neg,
        m2_fallbacks,
        slice_pairs,
        slice_test,
        domain_nodes,
        slice_nodes,
        nb,
    }
}

#[derive(Clone, Debug, Serialize)]
struct SliceMetrics {
    m1: f64,
    m2: f64,
    m3: f64,
    m4: f64,
}

#[derive(Clone, Debug, Serialize)]
struct ArmMetrics {
    m1: f64,
    m2: f64,
    m3: f64,
    m4: f64,
    m5: f64,
    /// Median training-triple length the M3 features are normalised by.
    median_edge: f64,
    slice: SliceMetrics,
}

fn pick<T: Copy>(v: &[T], idx: &[usize]) -> Vec<T> {
    idx.iter().map(|&k| v[k]).collect()
}

/// Score one set of final positions. M3's classifier is always fitted on the true training labels.
fn evaluate(d: &Data, s: &SeedData, p: &Pos) -> ArmMetrics {
    let tp = &s.sp.test_pairs;
    let clf = TypeClassifier::fit(p, &s.sp.train, d.rel_names.len());
    let nn = metrics::knn(p, metrics::K);
    let (sp_pos, sp_m1, sp_m2) = (
        pick(tp, &s.slice_pairs),
        pick(&s.m1_neg, &s.slice_pairs),
        pick(&s.m2_neg, &s.slice_pairs),
    );
    ArmMetrics {
        m1: metrics::link_auc(p, tp, &s.m1_neg),
        m2: metrics::link_auc(p, tp, &s.m2_neg),
        m3: clf.accuracy(p, &s.sp.test),
        m4: metrics::domain_knn_accuracy(&nn, &d.domain, &s.domain_nodes),
        m5: metrics::neighbour_recall(&nn, &s.nb),
        median_edge: clf.scale(),
        slice: SliceMetrics {
            m1: metrics::link_auc(p, &sp_pos, &sp_m1),
            m2: metrics::link_auc(p, &sp_pos, &sp_m2),
            m3: clf.accuracy(p, &s.slice_test),
            m4: metrics::domain_knn_accuracy(&nn, &d.domain, &s.slice_nodes),
        },
    }
}

/// Per-relation learned parameters with their training support, raw and relative to RF's own
/// median training-edge length.
fn rf_params(d: &Data, train: &[Triple], f: &embed::Field, median_edge: f64) -> Vec<Value> {
    let mut count = vec![0usize; d.rel_names.len()];
    train.iter().for_each(|t| count[t.rel as usize] += 1);
    f.rels
        .iter()
        .enumerate()
        .filter(|(r, _)| count[*r] > 0)
        .map(|(r, p)| {
            json!({
                "relation": d.rel_names[r], "train_triples": count[r],
                "kappa": p.kappa(), "rest": p.rest(), "mu": p.mu(), "h": p.h, "b": p.b,
                "rest_over_median": f64::from(p.rest()) / median_edge, "h_over_median": f64::from(p.h) / median_edge,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
enum Job {
    B0,
    B1,
    B2,
    RfAndSim,
    PermRfAndSim,
    RewireB0,
    RewireRf,
}

const JOBS: [Job; 7] = [
    Job::B0,
    Job::B1,
    Job::B2,
    Job::RfAndSim,
    Job::PermRfAndSim,
    Job::RewireB0,
    Job::RewireRf,
];

/// Run one job for one seed; returns `(name, payload)` entries.
fn run_job(d: &Data, s: &SeedData, job: Job) -> Vec<(String, Value)> {
    let t0 = Instant::now();
    let (n, nr, seed) = (d.n, d.rel_names.len(), s.seed);
    let (train, val) = (&s.sp.train, &s.sp.val);
    let secs = |t: Instant| t.elapsed().as_secs_f64();
    let rewired = || controls::rewire(train, &s.sp.all_pairs, REWIRE_ATTEMPTS, seed ^ 0x2e_3172);
    let mut out = Vec::new();
    match job {
        Job::B0 => {
            let p = sim::simulate(
                &sim::uniform_springs(n, train, &sim::LIVE),
                &sim::LIVE,
                seed,
            );
            out.push((
                "b0".into(),
                json!({"metrics": evaluate(d, s, &p), "seconds": secs(t0)}),
            ));
        }
        Job::B1 => {
            let f = embed::train_field(embed::B1, n, nr, train, val, seed);
            out.push(("b1".into(), json!({"metrics": evaluate(d, s, &f.pos), "best_epoch": f.best_epoch, "seconds": secs(t0)})));
        }
        Job::B2 => {
            let (p, best) = embed::train_transe(n, nr, train, val, seed);
            out.push((
                "b2".into(),
                json!({"metrics": evaluate(d, s, &p), "best_epoch": best, "seconds": secs(t0)}),
            ));
        }
        Job::RfAndSim => {
            let f = embed::train_field(embed::RF, n, nr, train, val, seed);
            let m = evaluate(d, s, &f.pos);
            let params = rf_params(d, train, &f, m.median_edge);
            out.push(("rf".into(), json!({"metrics": m, "best_epoch": f.best_epoch, "parameters": params, "seconds": secs(t0)})));
            let t1 = Instant::now();
            let (adj, mapping) = sim::learned_springs(n, train, &f.rels, &d.rel_names, &sim::LIVE);
            let p = sim::simulate(&adj, &sim::LIVE, seed);
            out.push((
                "rf_sim".into(),
                json!({"metrics": evaluate(d, s, &p), "mapping": mapping, "seconds": secs(t1)}),
            ));
        }
        Job::PermRfAndSim => {
            let (tr, va) = (
                controls::permute_labels(train, seed ^ 0x9e_0001),
                controls::permute_labels(val, seed ^ 0x9e_0002),
            );
            let f = embed::train_field(embed::RF, n, nr, &tr, &va, seed);
            let m3 = TypeClassifier::fit(&f.pos, train, nr).accuracy(&f.pos, &s.sp.test);
            out.push((
                "perm_rf".into(),
                json!({"m3": m3, "best_epoch": f.best_epoch, "seconds": secs(t0)}),
            ));
            let t1 = Instant::now();
            let (adj, _) = sim::learned_springs(n, &tr, &f.rels, &d.rel_names, &sim::LIVE);
            let p = sim::simulate(&adj, &sim::LIVE, seed);
            let m3 = TypeClassifier::fit(&p, train, nr).accuracy(&p, &s.sp.test);
            out.push(("perm_rf_sim".into(), json!({"m3": m3, "seconds": secs(t1)})));
        }
        Job::RewireB0 => {
            let rw = rewired();
            let p = sim::simulate(
                &sim::uniform_springs(n, &rw.triples, &sim::LIVE),
                &sim::LIVE,
                seed,
            );
            let m2 = metrics::link_auc(&p, &s.sp.test_pairs, &s.m2_neg);
            out.push(("rewire_b0".into(), json!({"m2": m2, "attempts": rw.attempts, "accepted": rw.accepted, "seconds": secs(t0)})));
        }
        Job::RewireRf => {
            let rw = rewired();
            let f = embed::train_field(embed::RF, n, nr, &rw.triples, val, seed);
            let m2 = metrics::link_auc(&f.pos, &s.sp.test_pairs, &s.m2_neg);
            out.push(("rewire_rf".into(), json!({"m2": m2, "best_epoch": f.best_epoch, "attempts": rw.attempts, "accepted": rw.accepted, "seconds": secs(t0)})));
        }
    }
    eprintln!("seed {seed} {job:?} done in {:.1}s", secs(t0));
    out
}

fn ms(v: &[f64]) -> Value {
    let (m, s) = metrics::mean_sd(v);
    json!({"mean": m, "sd": s, "per_seed": v})
}

fn main() -> std::io::Result<()> {
    let corpus_dir = PathBuf::from(arg("--corpus").expect("--corpus <knowledge dir> is required"));
    let label = arg("--corpus-label").unwrap_or_else(|| "unlabelled".into());
    let out = PathBuf::from(arg("--out").expect("--out <dir> is required"));
    let threads: usize =
        arg("--threads").map_or(40, |v| v.parse().expect("--threads takes an integer"));
    let keep: Option<usize> =
        arg("--subsample").map(|v| v.parse().expect("--subsample takes an integer"));
    let seeds: Vec<u64> = arg("--seeds").map_or(vec![0, 1, 2, 3, 4], |v| {
        v.split(',')
            .map(|x| x.parse().expect("--seeds takes integers"))
            .collect()
    });
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .expect("rayon pool is built once");
    std::fs::create_dir_all(&out)?;
    let started = Instant::now();

    let g = corpus::load(&corpus_dir)?;
    let stats = g.stats.clone();
    let d = subsample(g, keep);
    let seed_data: Vec<SeedData> = seeds.par_iter().map(|&s| prepare(&d, s)).collect();
    let extraction = json!({
        "corpus": label, "load_stats": stats, "subsample": keep, "nodes": d.n, "triples": d.triples.len(),
        "relation_types": d.rel_names.len(), "domains": d.domain_names.len(),
        "slice_nodes": d.in_slice.iter().filter(|x| **x).count(),
        "per_seed": seed_data.iter().map(|s| json!({
            "seed": s.seed, "train": s.sp.train.len(), "val": s.sp.val.len(), "test": s.sp.test.len(),
            "test_pairs": s.sp.test_pairs.len(), "slice_test_pairs": s.slice_pairs.len(), "slice_test_triples": s.slice_test.len(),
            "m2_fallbacks": s.m2_fallbacks,
            "m3_chance": metrics::chance(&s.sp.train, &s.sp.test, d.rel_names.len()),
            "m3_chance_slice": metrics::chance(&s.sp.train, &s.slice_test, d.rel_names.len()),
        })).collect::<Vec<_>>(),
    });
    std::fs::write(
        out.join("extraction.json"),
        serde_json::to_string_pretty(&extraction)?,
    )?;
    eprintln!("{}", serde_json::to_string_pretty(&extraction)?);

    let jobs: Vec<(usize, Job)> = (0..seed_data.len())
        .flat_map(|k| JOBS.iter().map(move |&j| (k, j)))
        .collect();
    let results: Vec<(usize, Vec<(String, Value)>)> = jobs
        .par_iter()
        .map(|&(k, j)| (k, run_job(&d, &seed_data[k], j)))
        .collect();
    let mut per_seed: Vec<BTreeMap<String, Value>> = vec![BTreeMap::new(); seed_data.len()];
    for (k, entries) in results {
        per_seed[k].extend(entries);
    }
    for (k, s) in seed_data.iter().enumerate() {
        let mut v = json!(per_seed[k]);
        v["seed"] = json!(s.seed);
        v["m3_chance"] = extraction["per_seed"][k]["m3_chance"].clone();
        std::fs::write(
            out.join(format!("seed{}.json", s.seed)),
            serde_json::to_string_pretty(&v)?,
        )?;
    }

    // Seed means and sds.
    let col = |arm: &str, path: &[&str]| -> Vec<f64> {
        per_seed
            .iter()
            .map(|m| {
                let mut v = &m[arm];
                for p in path {
                    v = &v[*p];
                }
                v.as_f64()
                    .unwrap_or_else(|| panic!("missing {arm}/{path:?}"))
            })
            .collect()
    };
    let mean = |arm: &str, path: &[&str]| metrics::mean_sd(&col(arm, path)).0;
    let mut table = serde_json::Map::new();
    for arm in ARMS {
        let mut row = serde_json::Map::new();
        for m in ["m1", "m2", "m3", "m4", "m5"] {
            row.insert(m.into(), ms(&col(arm, &["metrics", m])));
        }
        for m in ["m1", "m2", "m3", "m4"] {
            row.insert(
                format!("slice_{m}"),
                ms(&col(arm, &["metrics", "slice", m])),
            );
        }
        table.insert(arm.into(), Value::Object(row));
    }
    let chance: Vec<f64> = (0..seed_data.len())
        .map(|k| {
            extraction["per_seed"][k]["m3_chance"]
                .as_f64()
                .expect("chance")
        })
        .collect();
    let chance_slice: Vec<f64> = (0..seed_data.len())
        .map(|k| {
            extraction["per_seed"][k]["m3_chance_slice"]
                .as_f64()
                .expect("chance")
        })
        .collect();

    let m3 = |a| mean(a, &["metrics", "m3"]);
    let gate = |name: &str, value: f64, threshold: f64, rule: &str| json!({"gate": name, "value": value, "threshold": threshold, "rule": rule, "pass": value >= threshold});
    let gates = vec![
        gate("1. RF − B0, M3", m3("rf") - m3("b0"), 0.15, "≥ +0.15"),
        gate(
            "2. RF − B0, M2",
            mean("rf", &["metrics", "m2"]) - mean("b0", &["metrics", "m2"]),
            0.05,
            "≥ +0.05",
        ),
        gate("3. RF − B1, M3", m3("rf") - m3("b1"), 0.10, "≥ +0.10"),
        gate("4. RF − B2, M3", m3("rf") - m3("b2"), -0.02, "≥ −0.02"),
        gate(
            "5. RF-sim − B0, M3",
            m3("rf_sim") - m3("b0"),
            0.08,
            "≥ +0.08",
        ),
        gate(
            "6. RF M5 / B0 M5",
            mean("rf", &["metrics", "m5"]) / mean("b0", &["metrics", "m5"]),
            0.8,
            "≥ 0.8",
        ),
    ];
    let passed: Vec<String> = gates
        .iter()
        .filter(|g| g["pass"] == json!(true))
        .map(|g| g["gate"].as_str().expect("name").to_string())
        .collect();
    let verdict = if passed.len() == gates.len() {
        "supported".to_string()
    } else {
        format!(
            "not supported as stated ({} of 6 gates passed)",
            passed.len()
        )
    };

    // Per-relation RF parameters across seeds.
    let mut params: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for m in &per_seed {
        for p in m["rf"]["parameters"].as_array().expect("rf parameters") {
            params
                .entry(p["relation"].as_str().expect("relation").to_string())
                .or_default()
                .push(p);
        }
    }
    let param_summary: Vec<Value> = params
        .iter()
        .map(|(r, ps)| {
            let f = |k: &str| ms(&ps.iter().map(|p| p[k].as_f64().expect("number")).collect::<Vec<_>>());
            json!({"relation": r, "seeds": ps.len(), "train_triples": f("train_triples")["mean"], "kappa": f("kappa"), "rest_over_median": f("rest_over_median"),
                   "mu": f("mu"), "h_over_median": f("h_over_median"), "b": f("b")})
        })
        .collect();

    let rw_b0 = col("rewire_b0", &["m2"]);
    let rw_rf = col("rewire_rf", &["m2"]);
    let summary = json!({
        "corpus": label, "subsample": keep, "seeds": seeds, "threads": threads,
        "wall_clock_seconds": started.elapsed().as_secs_f64(),
        "table": table,
        "m3_chance": ms(&chance), "m3_chance_slice": ms(&chance_slice),
        "gates": gates, "gates_passed": passed, "verdict": verdict,
        "controls": {
            "label_permutation_m3": {"perm_rf": ms(&col("perm_rf", &["m3"])), "perm_rf_sim": ms(&col("perm_rf_sim", &["m3"])), "chance": ms(&chance)},
            "rewiring_m2": {"b0": ms(&rw_b0), "rf": ms(&rw_rf),
                            "rf_minus_b0": ms(&rw_rf.iter().zip(&rw_b0).map(|(a, b)| a - b).collect::<Vec<_>>()),
                            "accepted_swaps": ms(&col("rewire_rf", &["accepted"])), "attempts": ms(&col("rewire_rf", &["attempts"]))},
        },
        "best_epoch": {"rf": col("rf", &["best_epoch"]), "b1": col("b1", &["best_epoch"]), "b2": col("b2", &["best_epoch"])},
        "rf_parameters": param_summary,
    });
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
