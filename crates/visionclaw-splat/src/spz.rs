//! The SPZ v3 codec (Niantic `.spz`, <https://github.com/nianticlabs/spz>).
//!
//! v3 is one gzip stream holding a 16-byte header and the attribute blocks in
//! order: positions (3 × signed 24-bit fixed point per splat), alphas (u8),
//! colours (3 × u8), scales (3 × u8, log-encoded), rotations (u32
//! smallest-three) and SH (u8 per coefficient, channel-inner). The layout and
//! constants follow splat-transform 3.10.3's reader. Decoding is raw: no
//! coordinate conversion.
//!
//! Only v3 is served (ADR-2170 decision 4 as built): v2 has the older
//! rotation encoding and v4 is per-stream zstd. Produce variants with
//! `splat-transform --spz-version 3`.

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{Read, Write};

/// `'NGSP'` little-endian.
pub const MAGIC: u32 = 0x5053_474e;
/// The one served version.
pub const SERVED_VERSION: u32 = 3;
/// Header length for v1-3.
pub const HEADER_LEN: usize = 16;
/// Flag bit: trained with antialiasing.
pub const FLAG_ANTIALIASED: u8 = 0x1;
/// SPZ quantises SH-DC colour with this scale, not SH_C0.
pub const COLOR_SCALE: f32 = 0.15;
/// Total SH coefficients (all three channels) per splat, by degree.
pub const SH_COEFFS: [usize; 4] = [0, 9, 24, 45];
/// Hard cap on splats per file, so a hostile header cannot ask for a huge
/// allocation. Well above `desktop-hi`.
pub const MAX_SPLATS: u32 = 16_000_000;

/// The fields every version shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub num_splats: u32,
    pub sh_degree: u8,
    pub fractional_bits: u8,
    pub flags: u8,
}

/// Why a file is not a servable SPZ.
#[derive(Debug, thiserror::Error)]
pub enum SpzError {
    #[error("not gzip: SPZ v{0} files are refused, only v3 is served")]
    NotGzip(u32),
    #[error("not gzip and no NGSP header")]
    NotSpz,
    #[error("bad magic {0:#010x}")]
    Magic(u32),
    #[error("SPZ version {0} is refused, only v3 is served")]
    Version(u32),
    #[error("SH degree {0} is above 3")]
    ShDegree(u8),
    #[error("{0} splats is above the {MAX_SPLATS} cap")]
    TooMany(u32),
    #[error("payload is {got} bytes, expected {want}")]
    Truncated { got: usize, want: usize },
    #[error("gzip: {0}")]
    Io(#[from] std::io::Error),
}

/// Decoded splats, flat arrays ready to upload.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Splats {
    pub sh_degree: u8,
    pub antialiased: bool,
    /// xyz per splat.
    pub positions: Vec<f32>,
    /// Linear scales (already `exp`'d), xyz per splat.
    pub scales: Vec<f32>,
    /// Unit quaternions, xyzw per splat.
    pub rotations: Vec<f32>,
    /// Base colour in 0..1 (SH-DC evaluated) and opacity in 0..1, rgba per splat.
    pub colors: Vec<f32>,
    /// Higher-order SH, `SH_COEFFS[sh_degree]` per splat, channel-inner as stored, in -1..1.
    pub sh: Vec<f32>,
}

impl Splats {
    pub fn len(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

fn payload_len(h: &Header) -> usize {
    let n = h.num_splats as usize;
    n * (9 + 1 + 3 + 3 + 4 + SH_COEFFS[h.sh_degree as usize])
}

fn parse_header(b: &[u8]) -> Result<Header, SpzError> {
    if b.len() < HEADER_LEN {
        return Err(SpzError::Truncated {
            got: b.len(),
            want: HEADER_LEN,
        });
    }
    let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let magic = u32_at(0);
    if magic != MAGIC {
        return Err(SpzError::Magic(magic));
    }
    let h = Header {
        version: u32_at(4),
        num_splats: u32_at(8),
        sh_degree: b[12],
        fractional_bits: b[13],
        flags: b[14],
    };
    if h.version != SERVED_VERSION {
        return Err(SpzError::Version(h.version));
    }
    if h.sh_degree > 3 {
        return Err(SpzError::ShDegree(h.sh_degree));
    }
    if h.num_splats > MAX_SPLATS {
        return Err(SpzError::TooMany(h.num_splats));
    }
    Ok(h)
}

fn check_gzip(file: &[u8]) -> Result<(), SpzError> {
    if file.len() >= 2 && file[0] == 0x1f && file[1] == 0x8b {
        return Ok(());
    }
    // A plaintext NGSP header is v4 or later.
    if file.len() >= 8 && file[..4] == MAGIC.to_le_bytes() {
        return Err(SpzError::NotGzip(u32::from_le_bytes([
            file[4], file[5], file[6], file[7],
        ])));
    }
    Err(SpzError::NotSpz)
}

/// Read only the header, inflating no more than it needs. Admission uses
/// this to check a variant's splat count and SH degree against the manifest.
pub fn read_header(file: &[u8]) -> Result<Header, SpzError> {
    check_gzip(file)?;
    let mut head = [0u8; HEADER_LEN];
    let mut dec = GzDecoder::new(file);
    dec.read_exact(&mut head).map_err(|e| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => SpzError::Truncated {
            got: 0,
            want: HEADER_LEN,
        },
        _ => SpzError::Io(e),
    })?;
    parse_header(&head)
}

/// Decode a whole SPZ v3 file.
pub fn decode(file: &[u8]) -> Result<Splats, SpzError> {
    check_gzip(file)?;
    let mut raw = Vec::new();
    GzDecoder::new(file).read_to_end(&mut raw)?;
    let h = parse_header(&raw)?;
    let want = HEADER_LEN + payload_len(&h);
    if raw.len() < want {
        return Err(SpzError::Truncated {
            got: raw.len(),
            want,
        });
    }
    let n = h.num_splats as usize;
    let coeffs = SH_COEFFS[h.sh_degree as usize];
    let mut off = HEADER_LEN;
    let mut take = |len: usize| {
        let s = &raw[off..off + len];
        off += len;
        s
    };
    let pos = take(n * 9);
    let alpha = take(n);
    let color = take(n * 3);
    let scale = take(n * 3);
    let rot = take(n * 4);
    let sh = take(n * coeffs);

    let pscale = 1.0 / (1u32 << h.fractional_bits) as f32;
    let mut out = Splats {
        sh_degree: h.sh_degree,
        antialiased: h.flags & FLAG_ANTIALIASED != 0,
        positions: Vec::with_capacity(n * 3),
        scales: Vec::with_capacity(n * 3),
        rotations: Vec::with_capacity(n * 4),
        colors: Vec::with_capacity(n * 4),
        sh: Vec::with_capacity(n * coeffs),
    };
    for i in 0..n {
        for c in 0..3 {
            let o = i * 9 + c * 3;
            let mut v = pos[o] as i32 | (pos[o + 1] as i32) << 8 | (pos[o + 2] as i32) << 16;
            if v & 0x80_0000 != 0 {
                v |= !0xff_ffff; // sign-extend
            }
            out.positions.push(v as f32 * pscale);
        }
        for c in 0..3 {
            out.scales
                .push((scale[i * 3 + c] as f32 / 16.0 - 10.0).exp());
        }
        out.rotations
            .extend_from_slice(&unpack_rotation(u32::from_le_bytes([
                rot[i * 4],
                rot[i * 4 + 1],
                rot[i * 4 + 2],
                rot[i * 4 + 3],
            ])));
        for c in 0..3 {
            // byte -> SH-DC coefficient -> base colour, as the renderers do.
            let dc = (color[i * 3 + c] as f32 / 255.0 - 0.5) / COLOR_SCALE;
            out.colors.push((0.5 + SH_C0 * dc).clamp(0.0, 1.0));
        }
        out.colors.push(alpha[i] as f32 / 255.0);
        for c in 0..coeffs {
            out.sh.push((sh[i * coeffs + c] as f32 - 128.0) / 128.0);
        }
    }
    Ok(out)
}

/// The degree-0 SH basis constant.
pub const SH_C0: f32 = 0.282_094_8;

const ROT_MASK: u32 = (1 << 9) - 1;

/// Smallest-three: 2-bit largest index, then three 10-bit fields (9-bit
/// magnitude, sign bit) for the other components from the highest index down.
fn unpack_rotation(mut packed: u32) -> [f32; 4] {
    let mut q = [0.0f32; 4];
    let largest = (packed >> 30) as usize;
    let mut sum = 0.0;
    for j in (0..4).rev() {
        if j != largest {
            let mag = packed & ROT_MASK;
            let neg = (packed >> 9) & 1;
            packed >>= 10;
            let mut v = std::f32::consts::FRAC_1_SQRT_2 * mag as f32 / ROT_MASK as f32;
            if neg == 1 {
                v = -v;
            }
            q[j] = v;
            sum += v * v;
        }
    }
    q[largest] = (1.0 - sum).max(0.0).sqrt();
    q
}

fn pack_rotation(q: [f32; 4]) -> u32 {
    let norm = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    let mut q = q.map(|v| v / norm);
    let largest = (0..4)
        .max_by(|&a, &b| q[a].abs().total_cmp(&q[b].abs()))
        .unwrap();
    if q[largest] < 0.0 {
        q = q.map(|v| -v);
    }
    let mut packed = largest as u32;
    for (j, &v) in q.iter().enumerate() {
        if j != largest {
            let mag = ((v.abs() / std::f32::consts::FRAC_1_SQRT_2) * ROT_MASK as f32)
                .round()
                .min(ROT_MASK as f32) as u32;
            let neg = (v < 0.0) as u32;
            packed = (packed << 10) | (neg << 9) | mag;
        }
    }
    packed
}

/// Encode splats as SPZ v3. Used by tests on every tier to build fixtures
/// instead of committing assets.
pub fn encode_v3(s: &Splats, fractional_bits: u8) -> Vec<u8> {
    let n = s.len();
    let coeffs = SH_COEFFS[s.sh_degree as usize];
    let mut raw = Vec::with_capacity(HEADER_LEN + n * (20 + coeffs));
    raw.extend_from_slice(&MAGIC.to_le_bytes());
    raw.extend_from_slice(&SERVED_VERSION.to_le_bytes());
    raw.extend_from_slice(&(n as u32).to_le_bytes());
    raw.extend_from_slice(&[
        s.sh_degree,
        fractional_bits,
        if s.antialiased { FLAG_ANTIALIASED } else { 0 },
        0,
    ]);
    let fscale = (1u32 << fractional_bits) as f32;
    for p in &s.positions {
        let v = (p * fscale).round() as i32;
        raw.extend_from_slice(&v.to_le_bytes()[..3]);
    }
    for i in 0..n {
        raw.push((s.colors[i * 4 + 3] * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    for i in 0..n {
        for c in 0..3 {
            let dc = (s.colors[i * 4 + c] - 0.5) / SH_C0;
            raw.push(((dc * COLOR_SCALE + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    for v in &s.scales {
        raw.push(
            ((v.max(1e-12).ln() + 10.0) * 16.0)
                .round()
                .clamp(0.0, 255.0) as u8,
        );
    }
    for i in 0..n {
        let q = [
            s.rotations[i * 4],
            s.rotations[i * 4 + 1],
            s.rotations[i * 4 + 2],
            s.rotations[i * 4 + 3],
        ];
        raw.extend_from_slice(&pack_rotation(q).to_le_bytes());
    }
    for v in &s.sh {
        raw.push((v * 128.0 + 128.0).round().clamp(0.0, 255.0) as u8);
    }
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&raw).expect("in-memory gzip");
    enc.finish().expect("in-memory gzip")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(n: usize, sh_degree: u8) -> Splats {
        let coeffs = SH_COEFFS[sh_degree as usize];
        let mut s = Splats {
            sh_degree,
            ..Default::default()
        };
        for i in 0..n {
            let f = i as f32;
            s.positions
                .extend_from_slice(&[f * 0.25 - 3.0, -f * 0.125, f * 0.0625]);
            s.scales.extend_from_slice(&[0.01, 0.02, 0.04]);
            let q = [0.1 * f.sin(), 0.2, -0.3, 0.9];
            let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
            s.rotations.extend(q.iter().map(|v| v / n));
            s.colors.extend_from_slice(&[0.25, 0.5, 0.75, 0.6]);
            s.sh.extend((0..coeffs).map(|c| (c as f32 / coeffs as f32) - 0.5));
        }
        s
    }

    fn close(a: &[f32], b: &[f32], tol: f32) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
    }

    #[test]
    fn v3_round_trips_within_quantisation() {
        let s = fixture(40, 3);
        let back = decode(&encode_v3(&s, 12)).unwrap();
        assert_eq!(back.len(), 40);
        assert_eq!(back.sh_degree, 3);
        assert!(close(&back.positions, &s.positions, 1.0 / 4096.0));
        assert!(close(&back.colors, &s.colors, 0.01));
        assert!(close(&back.sh, &s.sh, 1.0 / 128.0));
        // log-encoded scale: 1/16 of a natural-log step.
        for (a, b) in back.scales.iter().zip(&s.scales) {
            assert!((a.ln() - b.ln()).abs() <= 1.0 / 32.0 + 1e-6);
        }
        // q and -q are the same rotation.
        for i in 0..40 {
            let dot: f32 = (0..4)
                .map(|c| back.rotations[i * 4 + c] * s.rotations[i * 4 + c])
                .sum();
            assert!(dot.abs() > 0.995, "splat {i}: |dot| {dot}");
        }
    }

    #[test]
    fn header_is_read_without_the_payload() {
        let h = read_header(&encode_v3(&fixture(7, 0), 12)).unwrap();
        assert_eq!(
            h,
            Header {
                version: 3,
                num_splats: 7,
                sh_degree: 0,
                fractional_bits: 12,
                flags: 0
            }
        );
    }

    #[test]
    fn v4_and_garbage_are_refused() {
        // A v4 file starts with a plaintext header.
        let mut v4 = MAGIC.to_le_bytes().to_vec();
        v4.extend_from_slice(&4u32.to_le_bytes());
        v4.extend_from_slice(&[0; 24]);
        assert!(matches!(read_header(&v4), Err(SpzError::NotGzip(4))));
        assert!(matches!(decode(b"PK\x03\x04"), Err(SpzError::NotSpz)));
    }

    #[test]
    fn a_v2_payload_inside_gzip_is_refused() {
        let mut raw = MAGIC.to_le_bytes().to_vec();
        raw.extend_from_slice(&2u32.to_le_bytes());
        raw.extend_from_slice(&[0; 8]);
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&raw).unwrap();
        assert!(matches!(
            read_header(&enc.finish().unwrap()),
            Err(SpzError::Version(2))
        ));
    }

    #[test]
    fn a_truncated_payload_is_refused() {
        let s = fixture(10, 0);
        let mut raw = Vec::new();
        GzDecoder::new(&encode_v3(&s, 12)[..])
            .read_to_end(&mut raw)
            .unwrap();
        raw.truncate(raw.len() - 5);
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&raw).unwrap();
        assert!(matches!(
            decode(&enc.finish().unwrap()),
            Err(SpzError::Truncated { .. })
        ));
    }

    #[test]
    fn a_hostile_splat_count_is_refused_before_allocating() {
        let mut raw = MAGIC.to_le_bytes().to_vec();
        raw.extend_from_slice(&3u32.to_le_bytes());
        raw.extend_from_slice(&u32::MAX.to_le_bytes());
        raw.extend_from_slice(&[0, 12, 0, 0]);
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&raw).unwrap();
        assert!(matches!(
            read_header(&enc.finish().unwrap()),
            Err(SpzError::TooMany(_))
        ));
    }

    #[test]
    fn rotation_packing_round_trips_each_largest_index() {
        for largest in 0..4 {
            let mut q = [0.1f32, -0.2, 0.3, -0.15];
            q[largest] = 0.9;
            let back = unpack_rotation(pack_rotation(q));
            let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
            let dot: f32 = (0..4).map(|c| back[c] * q[c] / n).sum();
            assert!(dot.abs() > 0.999, "largest {largest}: {dot}");
        }
    }
}
