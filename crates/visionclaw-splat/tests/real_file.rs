//! Cross-implementation check against a real splat-transform SPZ v3 file.
//!
//! Assets are never committed, so this runs only when `SPLAT_REAL_SPZ`
//! names a file (`splat-transform in.spz --spz-version 3 out.spz`), and
//! `SPLAT_REAL_SPLATS` optionally gives the count splat-transform reported.

use visionclaw_splat::spz;

#[test]
fn a_splat_transform_v3_file_decodes() {
    let Ok(path) = std::env::var("SPLAT_REAL_SPZ") else {
        eprintln!("SPLAT_REAL_SPZ unset: skipped");
        return;
    };
    let bytes = std::fs::read(&path).expect("read SPLAT_REAL_SPZ");
    let header = spz::read_header(&bytes).expect("header");
    let s = spz::decode(&bytes).expect("decode");
    assert_eq!(s.len() as u32, header.num_splats);
    if let Ok(n) = std::env::var("SPLAT_REAL_SPLATS") {
        assert_eq!(s.len(), n.parse::<usize>().unwrap());
    }
    // Every rotation is a unit quaternion and every value is finite.
    for q in s.rotations.chunks_exact(4) {
        let n: f32 = q.iter().map(|v| v * v).sum();
        assert!((n - 1.0).abs() < 0.01, "|q|^2 = {n}");
    }
    assert!(s.positions.iter().chain(&s.scales).chain(&s.colors).all(|v| v.is_finite()));
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in s.positions.chunks_exact(3) {
        for c in 0..3 {
            lo[c] = lo[c].min(p[c]);
            hi[c] = hi[c].max(p[c]);
        }
    }
    eprintln!("{} splats, SH {}, bounds {lo:?}..{hi:?}", s.len(), s.sh_degree);
}
