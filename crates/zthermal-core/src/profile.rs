// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Device and SoC profiles (`profiles/*.json`) and their validation.
//!
//! A profile records what is known about a device family and how it is known. It never
//! grants a control by itself: `controls` may only name descriptors in the validated
//! allowlist, and `hardware_tested` requires a hardware test record.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::identity::{soc_from_platform, SocVendor};
use crate::policy::validated_controls;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    OfficialDocumentation,
    PublicSource,
    VerifiedArtifact,
    HardwareTest,
    Inference,
    Assumption,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub kind: EvidenceKind,
    /// URL (pinned to a commit or tag where possible) or repository path.
    pub reference: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocInfo {
    pub vendor: SocVendor,
    pub model: Option<String>,
    pub board_platform: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRef {
    pub format: String,
    pub path: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Validation {
    pub hardware_tested: bool,
    pub last_reviewed: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub id: String,
    pub oem: String,
    pub brands: Vec<String>,
    pub family: String,
    /// `ro.product.device` values the profile applies to.
    pub devices: Vec<String>,
    pub soc: SocInfo,
    pub min_sdk: Option<u32>,
    pub config_files: Vec<ConfigRef>,
    /// Control descriptor ids from the validated allowlist. Empty in every profile today.
    pub controls: Vec<String>,
    pub evidence: Vec<EvidenceRef>,
    pub validation: Validation,
    pub limitations: Vec<String>,
}

pub const KNOWN_FORMATS: &[&str] = &["qcom_thermal_engine", "pixel_thermal_info_json"];

/// Everything wrong with one profile.
pub fn profile_problems(p: &Profile) -> Vec<String> {
    let mut out = Vec::new();
    let id_ok = !p.id.is_empty()
        && p.id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if p.schema_version != 1 {
        out.push(format!(
            "schema_version {} is not supported (expected 1)",
            p.schema_version
        ));
    }
    if !id_ok {
        out.push("id must be lowercase [a-z0-9-]".into());
    }
    if p.devices.is_empty() {
        out.push("a profile must name the devices (ro.product.device) it applies to".into());
    }
    if p.evidence.is_empty() {
        out.push("a profile needs at least one evidence reference".into());
    }
    if p.evidence
        .iter()
        .all(|e| matches!(e.kind, EvidenceKind::Inference | EvidenceKind::Assumption))
    {
        out.push("inference and assumptions alone cannot support a profile".into());
    }
    if p.validation.hardware_tested
        && !p
            .evidence
            .iter()
            .any(|e| e.kind == EvidenceKind::HardwareTest)
    {
        out.push("hardware_tested requires a hardware_test evidence record".into());
    }
    if let Some(bp) = &p.soc.board_platform {
        if let Some(v) = soc_from_platform(bp) {
            if v != p.soc.vendor {
                out.push(format!(
                    "soc.board_platform {bp:?} indicates {v:?}, not {:?}",
                    p.soc.vendor
                ));
            }
        }
    }
    let allow: BTreeSet<String> = validated_controls().into_iter().map(|c| c.id).collect();
    for c in &p.controls {
        if !allow.contains(c) {
            out.push(format!("control {c:?} is not in the validated allowlist"));
        }
    }
    for f in &p.config_files {
        if !KNOWN_FORMATS.contains(&f.format.as_str()) {
            out.push(format!("config format {:?} has no parser", f.format));
        }
        if !f.path.starts_with('/') || f.path.contains("..") {
            out.push(format!(
                "config path {:?} must be absolute without '..'",
                f.path
            ));
        }
    }
    for e in &p.evidence {
        if e.reference.trim().is_empty() {
            out.push("evidence reference is empty".into());
        }
    }
    out
}

/// Problems across a set of profiles (duplicates), plus each profile's own.
pub fn database_problems(profiles: &[(String, Profile)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut ids = BTreeSet::new();
    let mut devices = BTreeSet::new();
    for (file, p) in profiles {
        out.extend(
            profile_problems(p)
                .into_iter()
                .map(|m| format!("{file}: {m}")),
        );
        if !ids.insert(p.id.clone()) {
            out.push(format!("{file}: duplicate profile id {:?}", p.id));
        }
        for d in &p.devices {
            if !devices.insert(d.clone()) {
                out.push(format!(
                    "{file}: device {d:?} appears in more than one profile"
                ));
            }
        }
    }
    out
}

pub fn parse(text: &str) -> Result<Profile, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profile {
        parse(
            r#"{
              "schema_version": 1, "id": "example-device", "oem": "google", "brands": ["google"],
              "family": "Example", "devices": ["example"],
              "soc": {"vendor": "google_tensor", "model": null, "board_platform": "gs101"},
              "min_sdk": 31,
              "config_files": [{"format": "pixel_thermal_info_json", "path": "/vendor/etc/thermal_info_config.json", "evidence": "x"}],
              "controls": [],
              "evidence": [{"kind": "public_source", "reference": "https://example.invalid/x", "note": "n"}],
              "validation": {"hardware_tested": false, "last_reviewed": "2026-10-20"},
              "limitations": []
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn valid_profile() {
        assert_eq!(profile_problems(&sample()), Vec::<String>::new());
    }

    #[test]
    fn claims_must_be_backed() {
        let mut p = sample();
        p.validation.hardware_tested = true;
        p.controls = vec!["qcom.thermal_engine_profile".into()];
        p.soc.vendor = SocVendor::Qualcomm; // contradicts gs101
        p.config_files[0].format = "made_up".into();
        p.evidence = vec![EvidenceRef {
            kind: EvidenceKind::Assumption,
            reference: "x".into(),
            note: "".into(),
        }];
        let probs = profile_problems(&p);
        assert_eq!(probs.len(), 5, "{probs:?}");
    }

    #[test]
    fn unknown_fields_and_duplicates_are_rejected() {
        assert!(parse(r#"{"schema_version": 1, "surprise": true}"#).is_err());
        let p = sample();
        let probs = database_problems(&[("a.json".into(), p.clone()), ("b.json".into(), p)]);
        assert!(probs.iter().any(|m| m.contains("duplicate profile id")));
        assert!(probs.iter().any(|m| m.contains("more than one profile")));
    }
}
