//! D3 pilot (`PREREGISTRATION-D3-PILOT.md`): forecast typed links that appeared between the
//! pilot cut and the H1 February cut, between pages that existed at the pilot cut. Reads no
//! corpus state after the February cut, so no H1 held-out link is ever a target or a negative.
//!
//! Usage: `pilot_d3 --t0 <pages dir> --t1 <pages dir> --out <dir> [--stats-only]`

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use relational_field::corpus::Triple;
use relational_field::embed::{self, Pos, RelParams};
use relational_field::property::{self, Snapshot};
use relational_field::sim;
use relational_field::split::{pair, split};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;

const SEEDS: [u64; 5] = [0, 1, 2, 3, 4];
const ARMS: [&str; 4] = ["b0", "rf", "rf_sim", "b1"];

struct Case {
    head: u32,
    rel: u16,
    tail: u32,
}

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned())
}

fn dist(p: &Pos, i: u32, j: u32) -> f32 {
    let (a, b) = (p[i as usize], p[j as usize]);
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// RF's own typed score, `b − κ(d − L)² − μ(Δz − h)²` (embed.rs).
fn rf_score(p: &Pos, r: &RelParams, i: u32, j: u32) -> f32 {
    let d = dist(p, i, j).max(1e-6);
    let dz = p[j as usize][2] - p[i as usize][2];
    r.b - r.kappa() * (d - r.rest()).powi(2) - r.mu() * (dz - r.h).powi(2)
}

/// Natural log of C(n, k).
fn ln_choose(n: u64, k: u64) -> f64 {
    let lf = |m: u64| (1..=m).map(|x| (x as f64).ln()).sum::<f64>();
    lf(n) - lf(k) - lf(n - k)
}

/// Exact two-sided McNemar p-value from discordant counts.
fn mcnemar(gains: u64, losses: u64) -> f64 {
    let n = gains + losses;
    if n == 0 {
        return 1.0;
    }
    let k = gains.min(losses);
    let tail: f64 = (0..=k).map(|i| (ln_choose(n, i) - n as f64 * 2f64.ln()).exp()).sum();
    (2.0 * tail).min(1.0)
}

fn main() -> std::io::Result<()> {
    let t0 = PathBuf::from(arg("--t0").expect("--t0"));
    let t1 = PathBuf::from(arg("--t1").expect("--t1"));
    let out = PathBuf::from(arg("--out").expect("--out"));
    let stats_only = std::env::args().any(|a| a == "--stats-only");
    std::fs::create_dir_all(&out)?;

    let (s0, s1): (Snapshot, Snapshot) = (property::load(&t0)?, property::load(&t1)?);
    let node: HashMap<String, u32> = s0.stems.iter().enumerate().map(|(i, s)| (s.clone(), i as u32)).collect();
    let n = node.len();
    let mut rel_names: Vec<&'static str> = s0.triples.iter().chain(&s1.triples).map(|t| t.1).collect::<HashSet<_>>().into_iter().collect();
    rel_names.sort_unstable();
    let rel: HashMap<&'static str, u16> = rel_names.iter().enumerate().map(|(i, r)| (*r, i as u16)).collect();
    let train_all = property::to_ids(&s0.triples, &node, &rel);
    let later = property::to_ids(&s1.triples, &node, &rel);

    let t0_pairs: HashSet<(u32, u32)> = train_all.iter().map(|t| pair(t.head, t.tail)).collect();
    let mut nbr: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    for t in train_all.iter().chain(&later) {
        nbr[t.head as usize].insert(t.tail);
        nbr[t.tail as usize].insert(t.head);
    }
    // One case per new pair: the first of its triples in (pair, rel, head) order.
    let mut by_pair: BTreeMap<(u32, u32), Triple> = BTreeMap::new();
    let mut later_sorted = later.clone();
    later_sorted.sort_by_key(|t| (pair(t.head, t.tail), t.rel, t.head));
    for t in later_sorted {
        let p = pair(t.head, t.tail);
        if !t0_pairs.contains(&p) {
            by_pair.entry(p).or_insert(t);
        }
    }
    let cases: Vec<Case> = by_pair.values().map(|t| Case { head: t.head, rel: t.rel, tail: t.tail }).collect();
    let mut cases_per_rel: BTreeMap<&str, usize> = BTreeMap::new();
    cases.iter().for_each(|c| *cases_per_rel.entry(rel_names[c.rel as usize]).or_default() += 1);

    let extraction = json!({
        "t0": {"dir": t0, "stats": s0.stats}, "t1": {"dir": t1, "stats": s1.stats},
        "nodes": n, "train_triples": train_all.len(), "t1_triples_between_t0_pages": later.len(),
        "cases": cases.len(), "cases_per_relation": cases_per_rel,
    });
    std::fs::write(out.join("extraction.json"), serde_json::to_string_pretty(&extraction)?)?;
    println!("{}", serde_json::to_string_pretty(&extraction)?);
    if stats_only {
        return Ok(());
    }

    // Type constraint: a negative tail is drawn from nodes seen as a tail of the same relation.
    let mut tails: Vec<Vec<u32>> = vec![Vec::new(); rel_names.len()];
    for t in &train_all {
        tails[t.rel as usize].push(t.tail);
    }
    for v in &mut tails {
        v.sort_unstable();
        v.dedup();
    }

    // margins[seed][arm][case] = score(positive) − score(negative); a win is margin > 0.
    let mut margins: Vec<Vec<Vec<f32>>> = Vec::new();
    for &seed in &SEEDS {
        let sp = split(&train_all, seed);
        let mut train = sp.train.clone();
        train.extend_from_slice(&sp.test); // the pilot needs no static test split
        let val = sp.val;

        let mut rng = StdRng::seed_from_u64(seed ^ 0xD3);
        let negs: Vec<u32> = cases
            .iter()
            .map(|c| {
                let ok = |x: u32| x != c.head && x != c.tail && !nbr[c.head as usize].contains(&x);
                let pool = &tails[c.rel as usize];
                for _ in 0..200 {
                    if pool.is_empty() {
                        break;
                    }
                    let x = pool[rng.gen_range(0..pool.len())];
                    if ok(x) {
                        return x;
                    }
                }
                loop {
                    let x = rng.gen_range(0..n as u32);
                    if ok(x) {
                        return x;
                    }
                }
            })
            .collect();

        let b0 = sim::simulate(&sim::uniform_springs(n, &train, &sim::LIVE), &sim::LIVE, seed);
        let rf = embed::train_field(embed::RF, n, rel_names.len(), &train, &val, seed);
        let names: Vec<String> = rel_names.iter().map(|s| s.to_string()).collect();
        let (adj, mapping) = sim::learned_springs(n, &train, &rf.rels, &names, &sim::LIVE);
        let rf_sim = sim::simulate(&adj, &sim::LIVE, seed);
        let law: HashMap<String, (f32, f32, f32, f32)> =
            mapping.per_relation.iter().map(|(nm, k, l, aw, dz)| (nm.clone(), (*k, *l, *aw, *dz))).collect();
        let b1 = embed::train_field(embed::B1, n, rel_names.len(), &train, &val, seed);

        let sim_score = |c: &Case, j: u32| -> f32 {
            let d = dist(&rf_sim, c.head, j);
            match law.get(rel_names[c.rel as usize]) {
                Some(&(k, l, aw, dz)) => {
                    let z = rf_sim[j as usize][2] - rf_sim[c.head as usize][2];
                    -k * (d - l).powi(2) - aw * (z - dz).powi(2)
                }
                None => -d, // relation absent from training: no learned law to apply
            }
        };
        let per_arm: Vec<Vec<f32>> = vec![
            cases.iter().zip(&negs).map(|(c, &x)| dist(&b0, c.head, x) - dist(&b0, c.head, c.tail)).collect(),
            cases.iter().zip(&negs).map(|(c, &x)| {
                let r = &rf.rels[c.rel as usize];
                rf_score(&rf.pos, r, c.head, c.tail) - rf_score(&rf.pos, r, c.head, x)
            }).collect(),
            cases.iter().zip(&negs).map(|(c, &x)| sim_score(c, c.tail) - sim_score(c, x)).collect(),
            cases.iter().zip(&negs).map(|(c, &x)| dist(&b1.pos, c.head, x) - dist(&b1.pos, c.head, c.tail)).collect(),
        ];
        eprintln!("seed {seed}: rf best epoch {}, b1 best epoch {}", rf.best_epoch, b1.best_epoch);
        margins.push(per_arm);

        let mut f = std::fs::File::create(out.join(format!("cases-seed{seed}.jsonl")))?;
        for (i, c) in cases.iter().enumerate() {
            let mut row = json!({"seed": seed, "case": i, "head": s0.stems[c.head as usize], "rel": rel_names[c.rel as usize],
                "tail": s0.stems[c.tail as usize], "negative": s0.stems[negs[i] as usize]});
            for (a, arm) in ARMS.iter().enumerate() {
                row[*arm] = json!(margins.last().unwrap()[a][i]);
            }
            writeln!(f, "{row}")?;
        }
    }

    // Per-seed AUC (ties count half), and the case-level majority outcome over seeds.
    let win = |m: f32| if m > 0.0 { 1.0 } else if m == 0.0 { 0.5 } else { 0.0 };
    let auc: BTreeMap<&str, Vec<f64>> = ARMS.iter().enumerate().map(|(a, arm)| {
        (*arm, margins.iter().map(|s| s[a].iter().map(|&m| win(m)).sum::<f64>() / cases.len() as f64).collect())
    }).collect();
    let majority = |a: usize, i: usize| margins.iter().filter(|s| s[a][i] > 0.0).count() * 2 > SEEDS.len();
    let paired = |a: usize, b: usize| {
        let (mut g, mut l) = (0u64, 0u64);
        for i in 0..cases.len() {
            match (majority(a, i), majority(b, i)) {
                (true, false) => g += 1,
                (false, true) => l += 1,
                _ => {}
            }
        }
        json!({"gains": g, "losses": l, "exact_mcnemar_p": mcnemar(g, l)})
    };
    let mean = |v: &Vec<f64>| v.iter().sum::<f64>() / v.len() as f64;
    let delta = mean(&auc["rf"]) - mean(&auc["b0"]);
    let rf_vs_b0 = paired(1, 0);

    let mut per_rel: BTreeMap<&str, serde_json::Value> = BTreeMap::new();
    let mut regressions = Vec::new();
    for (r, name) in rel_names.iter().enumerate() {
        let idx: Vec<usize> = (0..cases.len()).filter(|&i| cases[i].rel as usize == r).collect();
        if idx.is_empty() {
            continue;
        }
        let rate = |a: usize| idx.iter().filter(|&&i| majority(a, i)).count() as f64 / idx.len() as f64;
        let d = rate(1) - rate(0);
        if idx.len() >= 30 && d < -0.05 {
            regressions.push(*name);
        }
        per_rel.insert(name, json!({"cases": idx.len(), "b0": rate(0), "rf": rate(1), "rf_sim": rate(2), "b1": rate(3), "rf_minus_b0": d}));
    }
    let p = rf_vs_b0["exact_mcnemar_p"].as_f64().unwrap();
    let favours_rf = rf_vs_b0["gains"].as_u64() > rf_vs_b0["losses"].as_u64();
    let proceed = delta >= 0.05 && p < 0.01 && favours_rf && regressions.is_empty();

    let summary = json!({
        "record": "PREREGISTRATION-D3-PILOT.md", "cases": cases.len(), "seeds": SEEDS,
        "auc_per_seed": auc, "auc_mean": auc.iter().map(|(k, v)| (*k, mean(v))).collect::<BTreeMap<_, _>>(),
        "rf_minus_b0": delta, "rf_vs_b0_majority": rf_vs_b0, "rf_sim_vs_b0_majority": paired(2, 0),
        "rf_sim_minus_b0": mean(&auc["rf_sim"]) - mean(&auc["b0"]),
        "per_relation": per_rel, "relation_regressions": regressions,
        "keep_rule": {"delta_ge_0_05": delta >= 0.05, "p_lt_0_01": p < 0.01, "favours_rf": favours_rf, "no_regression": regressions.is_empty()},
        "verdict": if proceed { "PROCEED" } else { "NOT SUPPORTED" },
    });
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&summary)?)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
