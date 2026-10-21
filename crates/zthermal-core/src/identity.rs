// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Normalized device identity from several independent evidence sources.
//!
//! Any single property may be missing, modified or misleading (custom ROMs rewrite
//! `ro.product.*`; `ro.hardware` is often just "qcom" or a board name). Every conclusion
//! records which properties supported it, and disagreements are reported, not resolved
//! silently. Identity never implies a capability: write access is decided elsewhere.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Properties read for identification. The serial number, IMEI and other per-unit
/// identifiers are never read.
pub const IDENTITY_PROPS: &[&str] = &[
    "ro.product.manufacturer",
    "ro.product.brand",
    "ro.product.vendor.manufacturer",
    "ro.product.vendor.brand",
    "ro.product.device",
    "ro.product.vendor.device",
    "ro.product.model",
    "ro.product.name",
    "ro.soc.manufacturer",
    "ro.soc.model",
    "ro.board.platform",
    "ro.hardware",
    "ro.hardware.chipname",
    "ro.build.version.release",
    "ro.build.version.sdk",
    "ro.build.fingerprint",
    "ro.vendor.build.fingerprint",
    "ro.build.type",
    "ro.vndk.version",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SocVendor {
    Qualcomm,
    Mediatek,
    GoogleTensor,
    SamsungExynos,
    Unisoc,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Two or more independent sources agree.
    High,
    /// One source, no contradiction.
    Medium,
    /// Sources disagree; the value shown is the strongest source's.
    Low,
    /// Nothing usable.
    None,
}

/// One piece of evidence: which property said what, and what it was taken to mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub source: String,
    pub value: String,
    pub indicates: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conclusion<T> {
    pub value: T,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    /// Sources that contradict `value`.
    pub conflicts: Vec<Evidence>,
}

/// OEM families. A brand (e.g. "POCO") is reported separately: same family never means
/// same thermal implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum OemFamily {
    Xiaomi,
    Samsung,
    Vivo,
    OnePlus,
    Oppo,
    Google,
    Nothing,
    Transsion,
    Motorola,
    Other,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub schema_version: u32,
    pub manufacturer: Option<String>,
    pub brand: Option<String>,
    pub oem_family: Conclusion<OemFamily>,
    pub device: Option<String>,
    pub model: Option<String>,
    pub soc_vendor: Conclusion<SocVendor>,
    /// SoC model as reported (e.g. "SM8450", "MT6895"), or the board platform name.
    pub soc_model: Option<String>,
    pub board_platform: Option<String>,
    pub android_release: Option<String>,
    pub sdk: Option<u32>,
    pub build_fingerprint: Option<String>,
    pub vendor_fingerprint: Option<String>,
    pub build_type: Option<String>,
    pub kernel_release: Option<String>,
    pub arch: Option<String>,
    /// Properties that could not be read.
    pub missing: Vec<String>,
}

fn prop<'a>(props: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    props.get(key).map(|s| s.trim()).filter(|s| !s.is_empty())
}

/// OEM family from a manufacturer or brand string.
pub fn oem_family_of(name: &str) -> OemFamily {
    match name.trim().to_ascii_lowercase().as_str() {
        "xiaomi" | "redmi" | "poco" | "blackshark" => OemFamily::Xiaomi,
        "samsung" => OemFamily::Samsung,
        "vivo" | "iqoo" => OemFamily::Vivo,
        "oneplus" => OemFamily::OnePlus,
        "oppo" | "realme" | "oplus" => OemFamily::Oppo,
        "google" => OemFamily::Google,
        "nothing" => OemFamily::Nothing,
        "infinix" | "tecno" | "itel" | "infinix mobility limited" => OemFamily::Transsion,
        "motorola" | "lenovo" => OemFamily::Motorola,
        "" => OemFamily::Unknown,
        _ => OemFamily::Other,
    }
}

/// SoC vendor implied by `ro.soc.manufacturer` (Android 12+).
fn soc_from_manufacturer(v: &str) -> Option<SocVendor> {
    match v.to_ascii_lowercase().as_str() {
        "qti" | "qualcomm" | "qualcomm technologies, inc" | "qualcomm technologies, inc." => {
            Some(SocVendor::Qualcomm)
        }
        "mediatek" | "mtk" => Some(SocVendor::Mediatek),
        "google" => Some(SocVendor::GoogleTensor),
        "samsung" | "samsung electronics" => Some(SocVendor::SamsungExynos),
        "unisoc" | "spreadtrum" => Some(SocVendor::Unisoc),
        _ => None,
    }
}

/// SoC vendor implied by a platform or chip name (`ro.board.platform`, `ro.soc.model`,
/// `ro.hardware`, `ro.hardware.chipname`). Prefix and code-name rules only.
pub fn soc_from_platform(v: &str) -> Option<SocVendor> {
    let p = v.trim().to_ascii_lowercase();
    let digits_after = |prefix: &str| {
        p.strip_prefix(prefix)
            .is_some_and(|r| r.chars().next().is_some_and(|c| c.is_ascii_digit()))
    };
    if digits_after("mt") {
        return Some(SocVendor::Mediatek);
    }
    if digits_after("msm") || digits_after("sdm") || digits_after("sm") || digits_after("apq") {
        return Some(SocVendor::Qualcomm);
    }
    if digits_after("exynos") || digits_after("s5e") || digits_after("universal") {
        return Some(SocVendor::SamsungExynos);
    }
    if digits_after("ums") || digits_after("sc9") || digits_after("ud7") {
        return Some(SocVendor::Unisoc);
    }
    // Qualcomm board platform code names (kernel/AOSP device trees) and Tensor code names.
    const QCOM: &[&str] = &[
        "qcom",
        "trinket",
        "bengal",
        "holi",
        "lito",
        "kona",
        "lahaina",
        "taro",
        "kalama",
        "pineapple",
        "sun",
        "parrot",
        "crow",
        "blair",
        "atoll",
        "sdmmagpie",
        "khaje",
        "monaco",
        "niobe",
        "volcano",
        "cliffs",
    ];
    const TENSOR: &[&str] = &["gs101", "gs201", "zuma", "zumapro", "laguna", "tensor"];
    if QCOM.contains(&p.as_str()) {
        return Some(SocVendor::Qualcomm);
    }
    if TENSOR.contains(&p.as_str()) {
        return Some(SocVendor::GoogleTensor);
    }
    None
}

fn conclude<T: Copy + PartialEq>(evidence: Vec<(Evidence, T)>, unknown: T) -> Conclusion<T> {
    let Some((_, first)) = evidence.first() else {
        return Conclusion {
            value: unknown,
            confidence: Confidence::None,
            evidence: vec![],
            conflicts: vec![],
        };
    };
    let first = *first;
    let (agree, disagree): (Vec<_>, Vec<_>) = evidence.into_iter().partition(|(_, v)| *v == first);
    let confidence = match (agree.len(), disagree.is_empty()) {
        (_, false) => Confidence::Low,
        (1, true) => Confidence::Medium,
        _ => Confidence::High,
    };
    Conclusion {
        value: first,
        confidence,
        evidence: agree.into_iter().map(|(e, _)| e).collect(),
        conflicts: disagree.into_iter().map(|(e, _)| e).collect(),
    }
}

impl DeviceIdentity {
    /// Build an identity from properties (missing keys allowed) plus kernel facts.
    pub fn from_props(
        props: &BTreeMap<String, String>,
        kernel_release: Option<String>,
        arch: Option<String>,
    ) -> Self {
        // SoC: ro.soc.manufacturer is the most direct source (Android 12+), then names.
        let mut soc = Vec::new();
        if let Some(v) = prop(props, "ro.soc.manufacturer") {
            if let Some(s) = soc_from_manufacturer(v) {
                soc.push((ev("ro.soc.manufacturer", v, s), s));
            }
        }
        for key in [
            "ro.soc.model",
            "ro.board.platform",
            "ro.hardware.chipname",
            "ro.hardware",
        ] {
            if let Some(v) = prop(props, key) {
                if let Some(s) = soc_from_platform(v) {
                    soc.push((ev(key, v, s), s));
                }
            }
        }
        let mut oem = Vec::new();
        for key in [
            "ro.product.manufacturer",
            "ro.product.vendor.manufacturer",
            "ro.product.brand",
            "ro.product.vendor.brand",
        ] {
            if let Some(v) = prop(props, key) {
                let f = oem_family_of(v);
                if f != OemFamily::Unknown {
                    oem.push((
                        Evidence {
                            source: key.into(),
                            value: v.into(),
                            indicates: format!("{f:?}"),
                        },
                        f,
                    ));
                }
            }
        }
        let missing = IDENTITY_PROPS
            .iter()
            .filter(|k| prop(props, k).is_none())
            .map(|k| k.to_string())
            .collect();
        let own = |k: &str| prop(props, k).map(str::to_string);
        Self {
            schema_version: 1,
            manufacturer: own("ro.product.manufacturer")
                .or_else(|| own("ro.product.vendor.manufacturer")),
            brand: own("ro.product.brand").or_else(|| own("ro.product.vendor.brand")),
            oem_family: conclude(oem, OemFamily::Unknown),
            device: own("ro.product.device").or_else(|| own("ro.product.vendor.device")),
            model: own("ro.product.model"),
            soc_vendor: conclude(soc, SocVendor::Unknown),
            soc_model: own("ro.soc.model").or_else(|| own("ro.hardware.chipname")),
            board_platform: own("ro.board.platform"),
            android_release: own("ro.build.version.release"),
            sdk: prop(props, "ro.build.version.sdk").and_then(|s| s.parse().ok()),
            build_fingerprint: own("ro.build.fingerprint"),
            vendor_fingerprint: own("ro.vendor.build.fingerprint"),
            build_type: own("ro.build.type"),
            kernel_release,
            arch,
            missing,
        }
    }
}

fn ev(source: &str, value: &str, s: SocVendor) -> Evidence {
    Evidence {
        source: source.into(),
        value: value.into(),
        indicates: format!("{s:?}"),
    }
}

/// Parse `getprop` output (`[key]: [value]` per line). Malformed lines are skipped.
pub fn parse_getprop(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix('[')?;
            let (k, rest) = rest.split_once("]: [")?;
            let v = rest.strip_suffix(']')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn getprop_parsing() {
        let p = parse_getprop("[ro.product.brand]: [POCO]\n[ro.empty]: []\ngarbage\n[a]: [x]y]\n");
        assert_eq!(p.get("ro.product.brand").map(String::as_str), Some("POCO"));
        assert_eq!(p.get("ro.empty").map(String::as_str), Some(""));
        assert_eq!(p.get("a").map(String::as_str), Some("x]y"));
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn agreeing_sources_give_high_confidence() {
        let id = DeviceIdentity::from_props(
            &props(&[
                ("ro.product.manufacturer", "Xiaomi"),
                ("ro.product.brand", "POCO"),
                ("ro.soc.manufacturer", "QTI"),
                ("ro.soc.model", "SM8450"),
                ("ro.board.platform", "taro"),
                ("ro.build.version.sdk", "34"),
            ]),
            Some("5.10.198".into()),
            Some("aarch64".into()),
        );
        assert_eq!(id.soc_vendor.value, SocVendor::Qualcomm);
        assert_eq!(id.soc_vendor.confidence, Confidence::High);
        assert_eq!(id.soc_vendor.evidence.len(), 3);
        assert_eq!(id.oem_family.value, OemFamily::Xiaomi);
        assert_eq!(id.brand.as_deref(), Some("POCO")); // brand kept separately
        assert_eq!(id.sdk, Some(34));
    }

    #[test]
    fn conflicting_properties_are_reported_not_hidden() {
        let id = DeviceIdentity::from_props(
            &props(&[
                ("ro.soc.manufacturer", "Mediatek"),
                ("ro.board.platform", "kona"),
            ]),
            None,
            None,
        );
        assert_eq!(id.soc_vendor.value, SocVendor::Mediatek);
        assert_eq!(id.soc_vendor.confidence, Confidence::Low);
        assert_eq!(id.soc_vendor.conflicts[0].source, "ro.board.platform");
    }

    #[test]
    fn missing_properties_give_unknown_not_a_guess() {
        let id = DeviceIdentity::from_props(&BTreeMap::new(), None, None);
        assert_eq!(id.soc_vendor.value, SocVendor::Unknown);
        assert_eq!(id.soc_vendor.confidence, Confidence::None);
        assert_eq!(id.oem_family.value, OemFamily::Unknown);
        assert_eq!(id.missing.len(), IDENTITY_PROPS.len());
        // A bare "qcom" hardware value is one weak source, not high confidence.
        let id = DeviceIdentity::from_props(&props(&[("ro.hardware", "qcom")]), None, None);
        assert_eq!(
            (id.soc_vendor.value, id.soc_vendor.confidence),
            (SocVendor::Qualcomm, Confidence::Medium)
        );
    }

    #[test]
    fn platform_names() {
        for (p, v) in [
            ("mt6789", SocVendor::Mediatek),
            ("MT6895", SocVendor::Mediatek),
            ("sdm845", SocVendor::Qualcomm),
            ("sm8650", SocVendor::Qualcomm),
            ("lahaina", SocVendor::Qualcomm),
            ("gs201", SocVendor::GoogleTensor),
            ("zuma", SocVendor::GoogleTensor),
            ("exynos2100", SocVendor::SamsungExynos),
            ("s5e9925", SocVendor::SamsungExynos),
        ] {
            assert_eq!(soc_from_platform(p), Some(v), "{p}");
        }
        for p in ["mtk", "smart", "msm", "unknown", "", "taro2"] {
            assert_eq!(soc_from_platform(p), None, "{p}");
        }
    }

    #[test]
    fn oem_families_keep_brands_distinct() {
        assert_eq!(oem_family_of("Redmi"), OemFamily::Xiaomi);
        assert_eq!(oem_family_of("iQOO"), OemFamily::Vivo);
        assert_eq!(oem_family_of("realme"), OemFamily::Oppo);
        assert_eq!(oem_family_of("TECNO"), OemFamily::Transsion);
        assert_eq!(oem_family_of("Fairphone"), OemFamily::Other);
        assert_eq!(oem_family_of(""), OemFamily::Unknown);
    }
}
