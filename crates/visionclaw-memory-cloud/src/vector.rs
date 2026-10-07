//! Vector codecs: ruvector text literals, L2 normalisation and the
//! little-endian f32 blob served by `GET /api/memory-cloud/vectors`.
//!
//! The ruvector extension's text form is a bracketed, comma-separated list
//! (`[0.1,-0.2,0.3]`). Reading the column as `embedding::text` and passing
//! query vectors as `($1::text)::ruvector` keeps the Postgres client free of
//! any custom binary type support.

use thiserror::Error;

/// A vector that could not be parsed or normalised.
#[derive(Debug, Error, PartialEq)]
pub enum VectorError {
    /// The literal is not wrapped in `[` … `]`.
    #[error("ruvector literal must be wrapped in [ ]")]
    NotBracketed,
    /// An element is not a finite number.
    #[error("ruvector literal element {index} is not a finite number")]
    BadElement {
        /// Zero-based position of the offending element.
        index: usize,
    },
    /// The vector has zero (or non-finite) length and cannot be normalised.
    #[error("vector has zero norm")]
    ZeroNorm,
}

/// Parse a ruvector text literal such as `[0.5,-1,2e-3]`.
///
/// ```
/// use visionclaw_memory_cloud::vector::parse_ruvector_literal;
/// assert_eq!(parse_ruvector_literal("[0.5, -1,2e-3]").unwrap(), vec![0.5, -1.0, 0.002]);
/// assert_eq!(parse_ruvector_literal("[]").unwrap(), Vec::<f32>::new());
/// ```
pub fn parse_ruvector_literal(literal: &str) -> Result<Vec<f32>, VectorError> {
    let inner = literal
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or(VectorError::NotBracketed)?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .enumerate()
        .map(|(index, part)| {
            part.trim()
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or(VectorError::BadElement { index })
        })
        .collect()
}

/// Format a vector as a ruvector text literal. Uses Rust's shortest
/// round-trip float formatting, so parsing the result yields identical bits.
///
/// ```
/// use visionclaw_memory_cloud::vector::{format_ruvector_literal, parse_ruvector_literal};
/// let v = vec![0.1_f32, -2.5, 1e-7];
/// assert_eq!(parse_ruvector_literal(&format_ruvector_literal(&v)).unwrap(), v);
/// ```
pub fn format_ruvector_literal(values: &[f32]) -> String {
    let mut out = String::with_capacity(values.len() * 12 + 2);
    out.push('[');
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&v.to_string());
    }
    out.push(']');
    out
}

/// Scale `values` in place to unit Euclidean length (accumulating in f64).
///
/// ```
/// use visionclaw_memory_cloud::vector::l2_normalise;
/// let mut v = [3.0_f32, 4.0];
/// l2_normalise(&mut v).unwrap();
/// assert_eq!(v, [0.6, 0.8]);
/// ```
pub fn l2_normalise(values: &mut [f32]) -> Result<(), VectorError> {
    let norm = values
        .iter()
        .map(|&v| f64::from(v) * f64::from(v))
        .sum::<f64>()
        .sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return Err(VectorError::ZeroNorm);
    }
    for v in values.iter_mut() {
        *v = (f64::from(*v) / norm) as f32;
    }
    Ok(())
}

/// Dot product, accumulated in f64. For unit vectors this is the cosine
/// similarity.
pub fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| f64::from(x) * f64::from(y))
        .sum()
}

/// Encode row-major vectors as the wire blob: little-endian IEEE-754 f32,
/// no header (`count * dim * 4` bytes).
///
/// ```
/// use visionclaw_memory_cloud::vector::{decode_vectors_blob, encode_vectors_blob};
/// let blob = encode_vectors_blob(&[1.0, -0.5]);
/// assert_eq!(blob, vec![0, 0, 0x80, 0x3f, 0, 0, 0, 0xbf]);
/// assert_eq!(decode_vectors_blob(&blob).unwrap(), vec![1.0, -0.5]);
/// ```
pub fn encode_vectors_blob(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Decode a wire blob back to floats; `None` when its length is not a
/// multiple of four bytes.
pub fn decode_vectors_blob(blob: &[u8]) -> Option<Vec<f32>> {
    let (words, rest) = blob.as_chunks::<4>();
    if !rest.is_empty() {
        return None;
    }
    Some(words.iter().map(|w| f32::from_le_bytes(*w)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_malformed_literals() {
        assert_eq!(
            parse_ruvector_literal("1,2"),
            Err(VectorError::NotBracketed)
        );
        assert_eq!(
            parse_ruvector_literal("[1,,2]"),
            Err(VectorError::BadElement { index: 1 })
        );
        assert_eq!(
            parse_ruvector_literal("[1,NaN]"),
            Err(VectorError::BadElement { index: 1 })
        );
        assert_eq!(
            parse_ruvector_literal("[inf]"),
            Err(VectorError::BadElement { index: 0 })
        );
    }

    #[test]
    fn parse_accepts_sidecar_output() {
        let v = parse_ruvector_literal("[-0.11883841,0.04829862,-0.00254809]").unwrap();
        assert_eq!(v, vec![-0.11883841, 0.04829862, -0.00254809]);
    }

    #[test]
    fn format_round_trips_exactly() {
        let v: Vec<f32> = (0..384).map(|i| ((i as f32) * 0.731).sin() / 7.0).collect();
        let back = parse_ruvector_literal(&format_ruvector_literal(&v)).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn normalise_yields_unit_length_and_rejects_zero() {
        let mut v: Vec<f32> = (1..=384).map(|i| i as f32).collect();
        l2_normalise(&mut v).unwrap();
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
        let mut z = [0.0_f32; 4];
        assert_eq!(l2_normalise(&mut z), Err(VectorError::ZeroNorm));
    }

    #[test]
    fn blob_is_little_endian_and_rejects_ragged_input() {
        let blob = encode_vectors_blob(&[f32::from_bits(0x0102_0304)]);
        assert_eq!(blob, vec![4, 3, 2, 1]);
        assert_eq!(decode_vectors_blob(&[0, 1, 2]), None);
    }
}
