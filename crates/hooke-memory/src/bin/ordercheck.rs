//! Diagnostic: label composition of the training order, by seed, along the epoch.
//!
//! Usage: `ordercheck <data root> <seed>...` or `ordercheck <data root> --dump` (selected rows). Rebuilds the exact row order `run train` uses
//! (same stratified pick, same per-seed shuffle) on the CPU and prints the share of True
//! answers in each tenth of the epoch and in the final 400 rows.

use anyhow::Result;
use hooke_memory::{data, store::EmbeddingStore};
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use std::path::PathBuf;

const TRAIN_SHARDS: &[i64] = &[0, 1, 2, 3, 7, 8];
const TRAIN_ROWS: usize = 25_438;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(&args[0]);
    let rows = data::load_split(&root, "factkg", "train")?;
    let store = EmbeddingStore::open(&root, "qwen3-embedding-4b", "factkg", "train")?;
    let (mut yes, mut no) = (Vec::new(), Vec::new());
    for (i, r) in rows.iter().enumerate() {
        if store.shard(r.collection_id).is_some_and(|s| TRAIN_SHARDS.contains(&s)) {
            if r.answer.trim() == "True" { yes.push(i) } else { no.push(i) }
        }
    }
    let half = TRAIN_ROWS / 2;
    let mut pick = StdRng::seed_from_u64(1000);
    yes.shuffle(&mut pick);
    no.shuffle(&mut pick);
    let mut base: Vec<usize> = yes[..half].iter().chain(&no[..half]).copied().collect();
    base.sort_unstable();
    if args.get(1).map(String::as_str) == Some("--dump") {
        for &i in &base { println!("{i}\t{}\t{}", rows[i].collection_id, rows[i].answer.trim()); }
        return Ok(());
    }
    for s in &args[1..] {
        let seed: u64 = s.parse()?;
        let mut order = base.clone();
        order.shuffle(&mut StdRng::seed_from_u64(1000 + seed));
        let t = |sl: &[usize]| sl.iter().filter(|&&i| rows[i].answer.trim() == "True").count() as f64 / sl.len() as f64;
        let tenths: Vec<String> = order.chunks(order.len().div_ceil(10)).map(|c| format!("{:.3}", t(c))).collect();
        println!("seed {seed}: True share by tenth {}; last 400 {:.3}; last 80 {:.3}", tenths.join(" "), t(&order[order.len() - 400..]), t(&order[order.len() - 80..]));
    }
    Ok(())
}
