//! Committed golden outputs of today's kernels.
//!
//! One text file per scenario under `goldens/`, one `[fixture ...]` section
//! per fixture, one line per node: `index fx fy fz`. Values are written with
//! Rust's shortest round-trip `f32` formatting, so parsing a line gives back
//! the exact bits that were recorded.
//!
//! Comparison bound, per component: `|Δ| <= 1e-5 * |golden| + 1e-6 * M`,
//! where `M` is the largest absolute component in that section. Today's
//! deterministic kernels reproduce their goldens bit for bit on the build
//! that wrote them (the bitwise count is printed); the bound exists so a
//! rework that only reassociates arithmetic or changes fast-math intrinsics
//! (ulp-level, about 1e-7 relative) still passes, while any change to a force
//! law (1e-3 relative or more on these fixtures) fails. The float-atomic
//! semantic kernels vary run to run by a few ulp of the accumulated sum,
//! which sits inside the same bound (see the determinism test).
//!
//! Regenerate with `VISIONCLAW_REGEN_GOLDENS=1` (see the README). A fixture
//! whose fingerprint differs from the recorded one fails with a message to
//! regenerate deliberately, never silently.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::fixtures::Fixture;
use crate::harness::Forces;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/force_kernel_baseline/goldens")
}

fn regenerating() -> bool {
    std::env::var("VISIONCLAW_REGEN_GOLDENS").is_ok_and(|v| v == "1")
}

/// One scenario's golden file, filled fixture by fixture.
pub struct Golden {
    scenario: String,
    device: String,
    sections: Vec<(String, u64, Forces)>,
}

impl Golden {
    pub fn new(scenario: &str, device: &str) -> Self {
        Golden {
            scenario: scenario.to_string(),
            device: device.to_string(),
            sections: Vec::new(),
        }
    }

    /// Record one fixture's forces for this scenario.
    pub fn add(&mut self, fx: &Fixture, forces: &Forces) {
        self.sections
            .push((fx.name.to_string(), fx.fingerprint(), forces.clone()));
    }

    /// Write the file (when regenerating) or compare against it.
    pub fn finish(self) {
        let path = dir().join(format!("{}.golden", self.scenario));
        if regenerating() {
            std::fs::create_dir_all(dir()).expect("create goldens dir");
            std::fs::write(&path, self.render()).expect("write golden");
            println!("  golden {}: regenerated", self.scenario);
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "golden {} missing ({e}). Regenerate deliberately with VISIONCLAW_REGEN_GOLDENS=1 \
                 (see tests/force_kernel_baseline/README.md).",
                path.display()
            )
        });
        let recorded = parse(&text);
        assert_eq!(
            recorded.len(),
            self.sections.len(),
            "golden {}: section count changed",
            self.scenario
        );
        for ((name, fp, forces), (rname, rfp, rforces)) in self.sections.iter().zip(&recorded) {
            assert_eq!(name, rname, "golden {}: fixture order", self.scenario);
            assert_eq!(
                fp, rfp,
                "golden {}/{}: the fixture's input fingerprint changed ({:016x} recorded, {:016x} \
                 now). The fixture was edited; regenerate the goldens deliberately.",
                self.scenario, name, rfp, fp
            );
            compare(&format!("{}/{}", self.scenario, name), forces, rforces);
        }
    }

    fn render(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# VisionClaw force-kernel golden: scenario {}",
            self.scenario
        );
        let _ = writeln!(
            s,
            "# Written by tests/force_kernel_baseline; regenerate with VISIONCLAW_REGEN_GOLDENS=1 \
             (README in that directory)."
        );
        let _ = writeln!(
            s,
            "# unified_ptx_tag={} semantic_ptx_tag={} cuda_arch={} device={}",
            option_env!("VISIONCLAW_UNIFIED_PTX_TAG").unwrap_or("unknown"),
            option_env!("SEMANTIC_FORCES_PTX_TAG").unwrap_or("unknown"),
            std::env::var("CUDA_ARCH").unwrap_or_else(|_| "unset".into()),
            self.device
        );
        let _ = writeln!(s, "# line format: node fx fy fz (shortest round-trip f32)");
        for (name, fp, forces) in &self.sections {
            let _ = writeln!(
                s,
                "[fixture {name} fingerprint={fp:016x} n={}]",
                forces.len()
            );
            for (i, f) in forces.iter().enumerate() {
                let _ = writeln!(s, "{i} {} {} {}", f[0], f[1], f[2]);
            }
        }
        s
    }
}

fn parse(text: &str) -> Vec<(String, u64, Forces)> {
    let mut out: Vec<(String, u64, Forces)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(head) = line.strip_prefix("[fixture ") {
            let mut parts = head.trim_end_matches(']').split_whitespace();
            let name = parts.next().expect("fixture name").to_string();
            let fp = parts
                .next()
                .and_then(|p| p.strip_prefix("fingerprint="))
                .and_then(|h| u64::from_str_radix(h, 16).ok())
                .expect("fingerprint");
            out.push((name, fp, Vec::new()));
            continue;
        }
        let mut it = line.split_whitespace().skip(1);
        let mut v = [0f32; 3];
        for c in &mut v {
            *c = it
                .next()
                .and_then(|t| t.parse().ok())
                .unwrap_or_else(|| panic!("malformed golden line: {line}"));
        }
        out.last_mut().expect("line before any section").2.push(v);
    }
    out
}

fn compare(label: &str, now: &Forces, golden: &Forces) {
    assert_eq!(now.len(), golden.len(), "{label}: node count");
    let m = golden
        .iter()
        .flat_map(|f| f.iter())
        .filter(|v| v.is_finite())
        .fold(0f32, |a, v| a.max(v.abs())) as f64;
    let (mut bitwise, mut worst) = (0usize, (0f64, 0usize, 0usize));
    for (i, (a, g)) in now.iter().zip(golden).enumerate() {
        for k in 0..3 {
            if a[k].to_bits() == g[k].to_bits() || (a[k] == 0.0 && g[k] == 0.0) {
                bitwise += 1;
                continue;
            }
            assert!(
                !(g[k].is_nan() ^ a[k].is_nan()),
                "{label}: node {i} axis {k}: NaN mismatch (now {}, golden {})",
                a[k],
                g[k]
            );
            let tol = 1e-5 * (g[k] as f64).abs() + 1e-6 * m;
            let ratio = (a[k] as f64 - g[k] as f64).abs() / tol.max(f64::MIN_POSITIVE);
            if ratio > worst.0 {
                worst = (ratio, i, k);
            }
        }
    }
    let (ratio, i, k) = worst;
    assert!(
        ratio <= 1.0,
        "{label}: node {i} axis {k} departs from the golden: now {} vs golden {} (bound \
         1e-5*|g| + 1e-6*{m:.3e})",
        now[i][k],
        golden[i][k]
    );
    println!(
        "  golden {label}: {bitwise}/{} components bitwise identical, worst |Δ|/bound {ratio:.3}",
        now.len() * 3
    );
}
