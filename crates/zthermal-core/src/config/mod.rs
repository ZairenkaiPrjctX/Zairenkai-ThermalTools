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
    /// Text without a recognizable structure.
    UnknownText,
}

impl ConfigFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigFormat::QcomThermalEngine => "qcom_thermal_engine",
            ConfigFormat::PixelThermalInfoJson => "pixel_thermal_info_json",
            ConfigFormat::XiaomiThermaldDevices => "xiaomi_thermald_devices",
            ConfigFormat::Opaque => "opaque",
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

/// Text that has no line structure and the byte statistics of enciphered data. MediaTek
/// `/vendor/etc/.tp/*` files are printable ASCII of this kind.
fn looks_enciphered(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.len() < 256 {
        return None;
    }
    let ws =
        bytes.iter().filter(|b| **b == b' ' || **b == b'\t').count() as f64 / bytes.len() as f64;
    let h = opaque::entropy(bytes);
    (ws < 0.05 && h > 5.5).then(|| {
        format!(
            "printable text with entropy {h:.2} bits/byte and {:.1}% spaces",
            ws * 100.0
        )
    })
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
    let text = match std::str::from_utf8(data) {
        Ok(t) => t,
        Err(_) => unreachable!("assess() returns Some for invalid UTF-8"),
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
    if let Some(why) = looks_enciphered(text) {
        let mtk = if path.contains("/.tp/") {
            " (a MediaTek /.tp/ policy file)"
        } else {
            ""
        };
        return Detection {
            format: ConfigFormat::Opaque,
            reason: format!(
                "{why}{mtk}: enciphered by the vendor; no authorized decoder exists, so it is reported and never decoded"
            ),
            opaque: None,
        };
    }
    Detection {
        format: ConfigFormat::UnknownText,
        reason: "text without a recognized structure; not interpreted".into(),
        opaque: None,
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
        ConfigFormat::Opaque | ConfigFormat::UnknownText => {}
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
}
