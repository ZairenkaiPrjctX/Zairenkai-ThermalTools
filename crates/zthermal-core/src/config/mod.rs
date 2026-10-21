//! Vendor thermal configuration files: format detection, parsing, validation, diff and
//! hashing.
//!
//! The toolkit inspects configuration files; it never writes one back to a device. A parser
//! keeps every line or key it does not understand, so its model always serializes to an
//! equivalent file, and an unknown directive is reported, never dropped.

pub mod diff;
pub mod opaque;
pub mod pixel;
pub mod qcom;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use opaque::OpaqueInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    /// Qualcomm `thermal-engine*.conf` (INI-like text).
    QcomThermalEngine,
    /// Pixel Thermal HAL `thermal_info_config*.json`.
    PixelThermalInfoJson,
    /// Xiaomi `thermald-devices.conf`: recognized, no semantic parser.
    XiaomiThermaldDevices,
    /// Binary, compressed, encrypted or enciphered data. Never decoded.
    Opaque,
    /// Text that looks encoded (Base64-like) or whose statistics are ambiguous. Never
    /// decoded; it may or may not be a configuration.
    SuspectedEncoded,
    /// Text without a recognizable structure, or an empty file.
    UnknownText,
}

impl ConfigFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigFormat::QcomThermalEngine => "qcom_thermal_engine",
            ConfigFormat::PixelThermalInfoJson => "pixel_thermal_info_json",
            ConfigFormat::XiaomiThermaldDevices => "xiaomi_thermald_devices",
            ConfigFormat::Opaque => "opaque",
            ConfigFormat::SuspectedEncoded => "suspected_encoded",
            ConfigFormat::UnknownText => "unknown_text",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    /// `line N`, `[SECTION]`, `Sensors[skin]` ...
    pub location: Option<String>,
}

impl Finding {
    pub fn new(
        severity: Severity,
        code: &str,
        location: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Finding {
            severity,
            code: code.to_string(),
            message: message.into(),
            location,
        }
    }
}

/// A statement about emergency or shutdown behaviour found in a file. Reported so that a
/// reviewer sees it, and used by the diff to flag changes that weaken it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyFact {
    pub location: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundTrip {
    /// `serialize(parse(file))` is byte-identical to the file.
    pub byte_exact: bool,
    /// `parse(serialize(parse(file)))` equals `parse(file)`.
    pub model_equal: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub format: ConfigFormat,
    pub reason: String,
    pub opaque: Option<OpaqueInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inspection {
    pub schema_version: u32,
    pub path: String,
    pub size: usize,
    pub sha256: String,
    pub detection: Detection,
    /// Format-specific counts (sections, sensors ...).
    pub summary: serde_json::Value,
    pub findings: Vec<Finding>,
    pub safety_facts: Vec<SafetyFact>,
    pub round_trip: Option<RoundTrip>,
}

impl Inspection {
    pub fn errors(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }
    pub fn warnings(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// How an unrecognized text file looks. Entropy is evidence, never proof: Base64 and dense
/// identifiers have high entropy too, so a text is called enciphered only when it also uses
/// a broad range of punctuation, as the MediaTek `/vendor/etc/.tp/*` files do.
#[derive(Debug, Clone, PartialEq)]
enum TextShape {
    /// Only Base64 / Base64url characters in lines without spaces.
    Base64Like,
    /// High entropy, almost no spaces, many distinct punctuation characters.
    Enciphered(String),
    /// High entropy and almost no spaces, but neither of the above.
    Ambiguous(String),
    Plain,
}

/// Distinct ASCII punctuation characters from which a text counts as "broad". Published
/// MediaTek `.tp` files use 24; Base64 uses at most 3, dense identifier lists about 12.
const ENCIPHERED_MIN_PUNCT: usize = 16;

fn text_shape(text: &str) -> TextShape {
    let bytes = text.as_bytes();
    let body: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| *b != b'\n' && *b != b'\r')
        .collect();
    if body.len() < 64 {
        return TextShape::Plain;
    }
    let b64 = body
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_'));
    if b64 {
        return TextShape::Base64Like;
    }
    if bytes.len() < 256 {
        return TextShape::Plain;
    }
    let ws =
        bytes.iter().filter(|b| **b == b' ' || **b == b'\t').count() as f64 / bytes.len() as f64;
    let h = opaque::entropy(bytes);
    if !(ws < 0.05 && h > 5.5) {
        return TextShape::Plain;
    }
    let punct = bytes
        .iter()
        .filter(|b| b.is_ascii_punctuation())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let stats = format!(
        "entropy {h:.2} bits/byte, {:.1}% spaces, {punct} distinct punctuation characters",
        ws * 100.0
    );
    if punct >= ENCIPHERED_MIN_PUNCT {
        TextShape::Enciphered(stats)
    } else {
        TextShape::Ambiguous(stats)
    }
}

/// Detects the format from the content. The path is used only to explain a result
/// (`/.tp/`), never to pick a parser.
pub fn detect(path: &str, data: &[u8]) -> Detection {
    if let Some(info) = opaque::assess(data) {
        return Detection {
            format: ConfigFormat::Opaque,
            reason: info.assessment.clone(),
            opaque: Some(info),
        };
    }
    let Ok(text) = std::str::from_utf8(data) else {
        // assess() already reports invalid UTF-8; this keeps detection total without a panic.
        return Detection {
            format: ConfigFormat::Opaque,
            reason: "not valid UTF-8; not interpreted".into(),
            opaque: None,
        };
    };
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('{')
        && (text.contains("\"Sensors\"") || text.contains("\"CoolingDevices\""))
    {
        return Detection {
            format: ConfigFormat::PixelThermalInfoJson,
            reason: "JSON object with Sensors/CoolingDevices keys".into(),
            opaque: None,
        };
    }
    let has_section = text.lines().any(|l| {
        let t = l.trim();
        t.starts_with('[') && t.ends_with(']') && t.len() > 2
    });
    // `algo_type` may be commented out in every section (LineageOS msm8953 does that), so
    // the other engine keys count as well.
    let engine_key = text.lines().any(|l| {
        let t = l.trim_start().trim_start_matches('#').trim_start();
        ["algo_type", "thresholds", "set_point"]
            .iter()
            .any(|k| t.split_whitespace().next() == Some(k))
    });
    if has_section && engine_key {
        return Detection {
            format: ConfigFormat::QcomThermalEngine,
            reason: "[SECTION] headers with thermal-engine keys (algo_type, thresholds, set_point)"
                .into(),
            opaque: None,
        };
    }
    let block_kv = text.lines().any(|l| l.trim() == "[")
        && text.lines().any(|l| l.trim().starts_with("cooling_name:"));
    if block_kv {
        return Detection {
            format: ConfigFormat::XiaomiThermaldDevices,
            reason: "'[' ... ']' blocks with cooling_name: lines; recognized, no semantic parser"
                .into(),
            opaque: None,
        };
    }
    if text.trim().is_empty() {
        return Detection {
            format: ConfigFormat::UnknownText,
            reason: if data.is_empty() {
                "empty file".into()
            } else {
                "only whitespace".into()
            },
            opaque: None,
        };
    }
    let mtk = if path.contains("/.tp/") {
        " (a MediaTek /.tp/ policy file)"
    } else {
        ""
    };
    match text_shape(text) {
        TextShape::Enciphered(stats) => Detection {
            format: ConfigFormat::Opaque,
            reason: format!(
                "printable text with {stats}{mtk}: enciphered by the vendor; no authorized decoder exists, so it is reported and never decoded"
            ),
            opaque: None,
        },
        TextShape::Base64Like => Detection {
            format: ConfigFormat::SuspectedEncoded,
            reason: "Base64-like text; it is not decoded, and whether it holds a configuration is unknown".into(),
            opaque: None,
        },
        TextShape::Ambiguous(stats) => Detection {
            format: ConfigFormat::SuspectedEncoded,
            reason: format!(
                "dense text with {stats}{mtk}: possibly encoded, possibly ordinary data; not interpreted"
            ),
            opaque: None,
        },
        TextShape::Plain => Detection {
            format: ConfigFormat::UnknownText,
            reason: "text without a recognized structure; not interpreted".into(),
            opaque: None,
        },
    }
}

/// Detects, parses and validates one file.
pub fn inspect(path: &str, data: &[u8]) -> Inspection {
    let detection = detect(path, data);
    let mut summary = serde_json::json!({});
    let mut findings = Vec::new();
    let mut safety_facts = Vec::new();
    let mut round_trip = None;
    let text = std::str::from_utf8(data).unwrap_or("");
    match detection.format {
        ConfigFormat::QcomThermalEngine => {
            let cfg = qcom::QcomConfig::parse(text);
            summary = cfg.summary();
            findings = cfg.validate();
            safety_facts = cfg.safety_facts();
            round_trip = Some(cfg.round_trip(text));
        }
        ConfigFormat::PixelThermalInfoJson => match pixel::PixelConfig::parse(text) {
            Ok(cfg) => {
                summary = cfg.summary();
                findings = cfg.validate();
                safety_facts = cfg.safety_facts();
                round_trip = Some(cfg.round_trip(text));
            }
            Err(e) => findings.push(Finding::new(Severity::Error, "json_parse", None, e)),
        },
        ConfigFormat::XiaomiThermaldDevices => findings.push(Finding::new(
            Severity::Info,
            "no_parser",
            None,
            "format recognized from its structure only; its semantics are not documented publicly",
        )),
        ConfigFormat::Opaque | ConfigFormat::SuspectedEncoded | ConfigFormat::UnknownText => {}
    }
    Inspection {
        schema_version: 1,
        path: path.to_string(),
        size: data.len(),
        sha256: sha256_hex(data),
        detection,
        summary,
        findings,
        safety_facts,
        round_trip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn detection_uses_content_not_names() {
        let q = b"[SKIN]\nalgo_type monitor\nsensor skin\n";
        assert_eq!(
            detect("/vendor/etc/thermal_info_config.json", q).format,
            ConfigFormat::QcomThermalEngine
        );
        let p = br#"{"Sensors": []}"#;
        assert_eq!(
            detect("/vendor/etc/thermal-engine.conf", p).format,
            ConfigFormat::PixelThermalInfoJson
        );
        assert_eq!(detect("x", b"hello\n").format, ConfigFormat::UnknownText);
        let x = b"#Device\n[\nname:gpu\ncooling_name:mtk-cl-gpu\n]\n";
        assert_eq!(detect("x", x).format, ConfigFormat::XiaomiThermaldDevices);
    }

    #[test]
    fn enciphered_printable_text_is_opaque() {
        // Deterministic printable noise without spaces, like the enciphered MediaTek files.
        let mut x: u32 = 0x9e37_79b9;
        let mut s = String::new();
        for i in 0..2000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            s.push(if i % 60 == 59 {
                '\n'
            } else {
                (b'!' + (x % 94) as u8) as char
            });
        }
        let d = detect("/vendor/etc/.tp/thermal.conf", s.as_bytes());
        assert_eq!(d.format, ConfigFormat::Opaque);
        assert!(
            d.reason.contains("MediaTek") && d.reason.contains("never decoded"),
            "{}",
            d.reason
        );
    }

    #[test]
    fn inspect_reports_hash_and_round_trip() {
        let i = inspect("t.conf", b"[A]\nalgo_type monitor\n");
        assert_eq!(i.sha256.len(), 64);
        assert_eq!(
            i.round_trip,
            Some(RoundTrip {
                byte_exact: true,
                model_equal: true
            })
        );
    }

    fn pseudo_random(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x & 0xff) as u8
            })
            .collect()
    }

    fn base64(data: &[u8]) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for (i, c) in data.chunks(3).enumerate() {
            let n = (c[0] as u32) << 16
                | (*c.get(1).unwrap_or(&0) as u32) << 8
                | *c.get(2).unwrap_or(&0) as u32;
            for k in 0..4 {
                out.push(if k > c.len() {
                    '='
                } else {
                    A[(n >> (18 - 6 * k) & 63) as usize] as char
                });
            }
            if i % 19 == 18 {
                out.push('\n');
            }
        }
        out
    }

    #[test]
    fn base64_is_suspected_encoded_not_enciphered() {
        let t = base64(&pseudo_random(3000, 7));
        let d = detect("/vendor/etc/x.conf", t.as_bytes());
        assert_eq!(d.format, ConfigFormat::SuspectedEncoded, "{}", d.reason);
        assert!(d.reason.contains("Base64-like") && d.reason.contains("not decoded"));
        // Base64url and CRLF line breaks too.
        let u = t.replace('+', "-").replace('/', "_").replace('\n', "\r\n");
        assert_eq!(
            detect("x", u.as_bytes()).format,
            ConfigFormat::SuspectedEncoded
        );
    }

    #[test]
    fn dense_ordinary_text_is_unknown_text() {
        // Long identifier lines without spaces: low punctuation diversity.
        let line =
            "vendor/lib64/libthermal_policy_client.so:vendor/lib64/libthermalclient.so|1234;\n";
        let t = line.repeat(80);
        assert_eq!(detect("x", t.as_bytes()).format, ConfigFormat::UnknownText);
        let hex: String = pseudo_random(2000, 3)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        // Hex is also in the Base64 alphabet: reported as encoded-looking, never decoded.
        assert_eq!(
            detect("x", hex.as_bytes()).format,
            ConfigFormat::SuspectedEncoded
        );
        assert_eq!(
            detect("x", b"plain words with spaces\n").format,
            ConfigFormat::UnknownText
        );
    }

    #[test]
    fn ambiguous_high_entropy_text() {
        // High entropy, no spaces, but only a few punctuation characters and not Base64.
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789#%&";
        let t: String = pseudo_random(4000, 11)
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if i % 70 == 69 {
                    '\n'
                } else {
                    alphabet[*b as usize % alphabet.len()] as char
                }
            })
            .collect();
        let d = detect("x", t.as_bytes());
        assert_eq!(d.format, ConfigFormat::SuspectedEncoded, "{}", d.reason);
        assert!(d.reason.contains("possibly encoded"));
    }

    #[test]
    fn empty_binary_and_invalid_encodings() {
        let e = detect("x", b"");
        assert_eq!(
            (e.format, e.reason.as_str()),
            (ConfigFormat::UnknownText, "empty file")
        );
        assert_eq!(detect("x", b"  \n\t\n").format, ConfigFormat::UnknownText);
        assert_eq!(
            detect("x", &pseudo_random(4096, 5)).format,
            ConfigFormat::Opaque
        );
        let latin1 = b"temp\xe9rature max 45\n".repeat(20);
        let d = detect("x", &latin1);
        assert_eq!(d.format, ConfigFormat::Opaque);
        assert!(d.reason.contains("not valid UTF-8"), "{}", d.reason);
    }

    #[test]
    fn supported_text_is_still_recognized() {
        let q = "# c\n[SKIN]\nalgo_type monitor\nsensor skin\nthresholds 45000\nthresholds_clr 43000\nactions cpu0\naction_info 1\n";
        assert_eq!(
            detect("x", q.as_bytes()).format,
            ConfigFormat::QcomThermalEngine
        );
        let i = inspect("x", q.as_bytes());
        assert_eq!(
            i.round_trip,
            Some(RoundTrip {
                byte_exact: true,
                model_equal: true
            })
        );
        // Unsupported content is never rewritten: inspection reports, it does not normalize.
        let b = base64(&pseudo_random(300, 1));
        let i = inspect("x", b.as_bytes());
        assert!(i.round_trip.is_none() && i.findings.is_empty());
        assert_eq!(i.sha256, sha256_hex(b.as_bytes()));
    }
}
