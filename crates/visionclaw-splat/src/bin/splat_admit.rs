//! `splat-admit`: the offline splat asset pipeline (ADR-2170 decisions 4 and 10).
//!
//! ```text
//! splat-admit fetch  <manifest> <asset> <src-dir>
//! splat-admit build  <manifest> <asset> <input> <work-dir> [--tool <cmd>] [--adaptive] [--all]
//! splat-admit admit  <manifest> <work-dir> <cache-dir> [--mode private-dev]
//! splat-admit verify <manifest> <cache-dir> [--mode private-dev]
//! ```
//!
//! `build` runs `@playcanvas/splat-transform` once per variant: decimate to a
//! scratch PLY (the decimation step must end in `.ply`), then convert to SPZ
//! v3. Decimation needs a WebGPU adapter, so `build` runs on a GPU build
//! host, never the HP while the headset is in use. It writes the produced
//! variants' splat counts, sizes and SHA-256 values back into the manifest.
//! `admit` copies verified variants into the content-addressed cache.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use visionclaw_splat::licence::CacheMode;
use visionclaw_splat::manifest::{Manifest, VariantName};
use visionclaw_splat::store::{admit_variant, verify_source, SplatStore};
use visionclaw_splat::{address, spz};

const USAGE: &str = "usage:
  splat-admit fetch  <manifest> <asset> <src-dir>
  splat-admit build  <manifest> <asset> <input> <work-dir> [--tool <cmd>] [--adaptive] [--all]
  splat-admit admit  <manifest> <work-dir> <cache-dir> [--mode private-dev]
  splat-admit verify <manifest> <cache-dir> [--mode private-dev]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("splat-admit: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Positional arguments and `--flag [value]` options.
struct Args {
    pos: Vec<String>,
    opts: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: &[String], valued: &[&str]) -> Self {
        let mut pos = Vec::new();
        let mut opts = Vec::new();
        let mut it = raw.iter();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                let v = valued.contains(&name).then(|| it.next().cloned()).flatten();
                opts.push((name.to_string(), v));
            } else {
                pos.push(a.clone());
            }
        }
        Self { pos, opts }
    }

    fn flag(&self, name: &str) -> bool {
        self.opts.iter().any(|(n, _)| n == name)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.opts
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.as_deref())
    }

    fn mode(&self) -> CacheMode {
        CacheMode::from_env_value(self.value("mode"))
    }
}

fn run(raw: &[String]) -> Result<(), String> {
    let (cmd, rest) = raw.split_first().ok_or(USAGE)?;
    let a = Args::parse(rest, &["tool", "mode"]);
    match (cmd.as_str(), a.pos.as_slice()) {
        ("fetch", [m, asset, dir]) => fetch(&load(m)?, asset, Path::new(dir)),
        ("build", [m, asset, input, work]) => build(
            Path::new(m),
            asset,
            Path::new(input),
            Path::new(work),
            a.value("tool").unwrap_or("splat-transform"),
            a.flag("adaptive"),
            a.flag("all"),
        ),
        ("admit", [m, work, cache]) => admit(&load(m)?, Path::new(work), Path::new(cache), a.mode()),
        ("verify", [m, cache]) => verify(load(m)?, Path::new(cache), a.mode()),
        _ => Err(USAGE.into()),
    }
}

fn load(path: &str) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    Manifest::from_json(&text).map_err(|e| format!("{path}: {e}"))
}

fn fetch(m: &Manifest, id: &str, dir: &Path) -> Result<(), String> {
    let asset = m.asset(id).ok_or(format!("no asset {id}"))?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for src in &asset.sources {
        let resp = ureq::get(&src.url)
            .call()
            .map_err(|e| format!("{}: {e}", src.url))?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut resp.into_reader(), &mut bytes)
            .map_err(|e| format!("{}: {e}", src.url))?;
        // A mismatch is a refusal, never a retry.
        verify_source(src, &bytes).map_err(|e| e.to_string())?;
        let out = dir.join(&src.name);
        std::fs::write(&out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("fetched {} ({} bytes, verified)", src.name, bytes.len());
    }
    Ok(())
}

fn run_tool(tool: &str, args: &[&str]) -> Result<(), String> {
    let mut parts = tool.split_whitespace();
    let prog = parts.next().ok_or("empty --tool")?;
    let status = Command::new(prog)
        .args(parts)
        .args(args)
        .status()
        .map_err(|e| format!("{prog}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{tool} {} failed: {status}", args.join(" ")))
    }
}

fn build(
    manifest_path: &Path,
    id: &str,
    input: &Path,
    work: &Path,
    tool: &str,
    adaptive: bool,
    all: bool,
) -> Result<(), String> {
    let mut m = load(&manifest_path.to_string_lossy())?;
    let out_dir = work.join(id);
    let scratch = work.join(format!("{id}.scratch"));
    for d in [&out_dir, &scratch] {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let input_s = input.to_string_lossy().to_string();
    let asset = m
        .assets
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or(format!("no asset {id}"))?;
    for v in asset.variants.iter_mut() {
        if v.sha256.is_some() && !all {
            continue;
        }
        let name = v.name.as_str();
        let ply = scratch.join(format!("{name}.ply"));
        let spz_path = out_dir.join(format!("{name}.spz"));
        let ply_s = ply.to_string_lossy().to_string();
        let spz_s = spz_path.to_string_lossy().to_string();
        let target = v.name.target_splats().to_string();
        let sh = v.sh_degree.to_string();
        let decimate = if adaptive { "--decimate-adaptive" } else { "--decimate" };
        let scratch_s = scratch.to_string_lossy().to_string();
        // Decimation must be the final action and write a PLY.
        run_tool(
            tool,
            &[
                "-w", "--scratch-dir", &scratch_s, &input_s, "-H", &sh, decimate, &target, &ply_s,
            ],
        )?;
        run_tool(tool, &["-w", &ply_s, "--spz-version", "3", &spz_s])?;
        let bytes = std::fs::read(&spz_path).map_err(|e| e.to_string())?;
        let header = spz::read_header(&bytes).map_err(|e| format!("{name}: {e}"))?;
        if header.sh_degree != v.sh_degree {
            return Err(format!(
                "{name}: produced SH {} but the manifest asks for {}",
                header.sh_degree, v.sh_degree
            ));
        }
        v.splats = header.num_splats;
        v.bytes = Some(bytes.len() as u64);
        v.sha256 = Some(address::sha256_hex(&bytes));
        println!(
            "built {id} {name}: {} splats, SH {}, {} bytes, {}",
            header.num_splats,
            header.sh_degree,
            bytes.len(),
            address::address_of_hex(v.sha256.as_deref().unwrap())
        );
        let _ = std::fs::remove_file(&ply);
    }
    m.validate().map_err(|e| e.to_string())?;
    std::fs::write(manifest_path, m.to_json()).map_err(|e| e.to_string())?;
    println!("manifest updated: {}", manifest_path.display());
    Ok(())
}

fn variant_file(work: &Path, asset: &str, name: VariantName) -> PathBuf {
    work.join(asset).join(format!("{}.spz", name.as_str()))
}

fn admit(m: &Manifest, work: &Path, cache: &Path, mode: CacheMode) -> Result<(), String> {
    let mut failed = 0;
    for asset in &m.assets {
        for v in asset.variants.iter().filter(|v| v.sha256.is_some()) {
            let path = variant_file(work, &asset.id, v.name);
            let Ok(bytes) = std::fs::read(&path) else {
                println!("skip {} {}: {} not present", asset.id, v.name.as_str(), path.display());
                continue;
            };
            match admit_variant(asset, v, &bytes, cache, mode) {
                Ok(addr) => println!("admitted {} {} as {addr}", asset.id, v.name.as_str()),
                Err(e) => {
                    failed += 1;
                    eprintln!("refused: {e}");
                }
            }
        }
    }
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} variant(s) refused"))
    }
}

fn verify(m: Manifest, cache: &Path, mode: CacheMode) -> Result<(), String> {
    let (store, refusals) = SplatStore::open(m, cache, mode);
    for a in &store.listing().assets {
        for v in &a.variants {
            println!("serving {} {} {} ({} bytes)", a.id, v.name.as_str(), v.address, v.bytes);
        }
    }
    for r in &refusals {
        eprintln!("refused: {r}");
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(format!("{} refusal(s)", refusals.len()))
    }
}
