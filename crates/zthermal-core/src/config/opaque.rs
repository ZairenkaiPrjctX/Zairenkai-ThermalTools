//! Recognizing files this toolkit must not interpret: binary, encrypted or compressed data.
//!
//! Detection only. No decryption, decoding or key search is ever attempted.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpaqueInfo {
    pub size: usize,
    /// Shannon entropy of the byte distribution, bits per byte (8.0 = uniformly random).
    pub entropy_bits_per_byte: f64,
    /// Share of bytes that are not printable ASCII, tab, CR or LF.
    pub non_text_ratio: f64,
    pub magic: Option<String>,
    pub assessment: String,
}

fn magic(data: &[u8]) -> Option<&'static str> {
    const M: &[(&[u8], &str)] = &[
        (b"\x1f\x8b", "gzip"),
        (b"PK\x03\x04", "zip"),
        (b"\x7fELF", "ELF executable"),
        (b"\x28\xb5\x2f\xfd", "zstd"),
        (b"\xfd7zXZ\x00", "xz"),
        (b"BZh", "bzip2"),
        (b"\x04\x22\x4d\x18", "lz4"),
    ];
    M.iter().find(|(m, _)| data.starts_with(m)).map(|(_, n)| *n)
}

pub fn entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let n = data.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

/// `Some` when the data is not plain text. Text files (including UTF-8) return `None`.
pub fn assess(data: &[u8]) -> Option<OpaqueInfo> {
    if data.is_empty() {
        return None;
    }
    let non_text = data
        .iter()
        .filter(|&&b| {
            !(b == b'\t' || b == b'\n' || b == b'\r' || (0x20..0x7f).contains(&b) || b >= 0x80)
        })
        .count();
    let utf8 = std::str::from_utf8(data).is_ok();
    let ratio = non_text as f64 / data.len() as f64;
    let m = magic(data);
    if utf8 && ratio < 0.01 && m.is_none() {
        return None;
    }
    let h = entropy(data);
    let assessment = match m {
        Some(kind) => format!("{kind} data; not a plain-text configuration"),
        None if h > 7.5 && data.len() >= 256 => {
            "high-entropy binary (likely encrypted or compressed); no authorized decoder exists, \
             so it is reported and never decoded"
                .into()
        }
        None => "binary data in an unrecognized format".into(),
    };
    Some(OpaqueInfo {
        size: data.len(),
        entropy_bits_per_byte: (h * 1000.0).round() / 1000.0,
        non_text_ratio: (ratio * 1000.0).round() / 1000.0,
        magic: m.map(str::to_string),
        assessment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_not_opaque() {
        assert!(assess(b"[SKIN]\nalgo_type monitor\n").is_none());
        assert!(assess("thermal \u{00b0}C\n".as_bytes()).is_none());
        assert!(assess(b"").is_none());
    }

    #[test]
    fn binary_and_random_data_are_opaque() {
        // Deterministic pseudo-random bytes (xorshift) stand in for an encrypted file.
        let mut x: u32 = 0x1234_5678;
        let data: Vec<u8> = (0..4096)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x & 0xff) as u8
            })
            .collect();
        let o = assess(&data).unwrap();
        assert!(o.entropy_bits_per_byte > 7.9, "{o:?}");
        assert!(o.assessment.contains("never decoded"));
        let gz = assess(b"\x1f\x8b\x08\x00rest").unwrap();
        assert_eq!(gz.magic.as_deref(), Some("gzip"));
        assert!(assess(b"\x00\x01\x02 text with nulls").is_some());
    }
}
