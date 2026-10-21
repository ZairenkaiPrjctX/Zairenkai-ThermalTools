// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Policy modes, writable-control descriptors and the adaptive decision state machine.
//!
//! Three rules hold everywhere:
//! 1. A write is possible only through a [`ControlDescriptor`] in [`VALIDATED_CONTROLS`]
//!    that passes [`descriptor_problems`] and whose conditions match the device.
//! 2. Thermal protection is never a target: trip points, zone `mode`, `emul_temp`, zone
//!    policies, cooling-device states and CPU/GPU frequency limits are refused by path,
//!    whatever a descriptor says ([`denied_target`]).
//! 3. Missing or invalid readings are never treated as "cool": the adaptive policy holds
//!    or backs off, and explains why.
//!
//! No control has been validated on hardware, so [`VALIDATED_CONTROLS`] is empty and every
//! mode other than `diagnostic` produces a plan with no actions and an explanation.

use serde::{Deserialize, Serialize};

use crate::capability::ThrottlingSeverity;
use crate::identity::SocVendor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    /// Inspect only; never changes anything.
    Diagnostic,
    /// Leave the system's own behaviour unchanged.
    Balanced,
    PowerSaver,
    /// Only verified controls; the system's thermal mitigation stays in charge.
    Performance,
    /// Like `performance`: never disables protection or raises thresholds.
    Gaming,
    /// Picks one of the above from observed severity, with hysteresis.
    Adaptive,
}

impl PolicyMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "diagnostic" => Self::Diagnostic,
            "balanced" => Self::Balanced,
            "power_saver" | "power-saver" => Self::PowerSaver,
            "performance" => Self::Performance,
            "gaming" => Self::Gaming,
            "adaptive" => Self::Adaptive,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AllowedValues {
    IntegerRange { min: i64, max: i64 },
    OneOf { values: Vec<String> },
}

impl AllowedValues {
    pub fn permits(&self, value: &str) -> bool {
        match self {
            Self::IntegerRange { min, max } => value
                .trim()
                .parse::<i64>()
                .is_ok_and(|v| (*min..=*max).contains(&v)),
            Self::OneOf { values } => values.iter().any(|v| v == value),
        }
    }
}

/// Device conditions a descriptor applies to. Every listed condition must match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Conditions {
    pub soc_vendor: Option<SocVendor>,
    /// Exact `ro.board.platform` values; empty means "not restricted by platform".
    pub board_platforms: Vec<String>,
    /// Exact `ro.product.device` values; empty means "not restricted by device".
    pub devices: Vec<String>,
}

/// A writable control. Every field is mandatory evidence; see `docs/SAFETY.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlDescriptor {
    pub id: String,
    /// Exact absolute path; no globs, no `..`.
    pub target: String,
    pub description: String,
    pub conditions: Conditions,
    pub allowed: AllowedValues,
    /// Sources showing the control's semantics (documentation, source code, test report).
    pub evidence: Vec<String>,
    /// The original value is read before the first write and restored on rollback.
    pub restore_original: bool,
    /// The value is read back after writing and must equal what was written.
    pub verify_after_write: bool,
    /// Set only with a recorded physical-device test; real writes require it.
    pub hardware_tested: bool,
}

/// The validated allowlist. Empty: no writable control has been validated on hardware.
/// Adding one requires the evidence in `docs/SAFETY.md` and a reviewed change.
pub const VALIDATED_CONTROLS: &[fn() -> ControlDescriptor] = &[];

pub fn validated_controls() -> Vec<ControlDescriptor> {
    VALIDATED_CONTROLS.iter().map(|f| f()).collect()
}

/// Why a target path can never be written, whatever a descriptor claims.
pub fn denied_target(path: &str) -> Option<&'static str> {
    if !path.starts_with('/')
        || path.contains("/../")
        || path.ends_with("/..")
        || path.contains('*')
    {
        return Some("target must be an exact absolute path without globs or '..'");
    }
    let p = path.to_ascii_lowercase();
    let leaf = p.rsplit('/').next().unwrap_or("");
    let in_thermal = p.contains("/thermal_zone")
        || p.contains("/class/thermal/")
        || p.contains("/virtual/thermal/");
    if in_thermal
        && (leaf.starts_with("trip_point_")
            || leaf == "mode"
            || leaf == "emul_temp"
            || leaf == "policy"
            || leaf == "sustainable_power"
            || leaf.starts_with("k_")
            || leaf == "integral_cutoff"
            || leaf == "slope"
            || leaf == "offset")
    {
        return Some("thermal zone trip points, mode, emulation and governor parameters are thermal protection");
    }
    if p.contains("/cooling_device") && (leaf == "cur_state" || leaf == "max_state") {
        return Some("forcing a cooling-device state overrides the thermal governor");
    }
    if leaf.starts_with("scaling_")
        || leaf.ends_with("_freq")
        || leaf == "max_freq"
        || leaf == "min_freq"
        || p.contains("/devfreq/")
        || p.contains("/kgsl/") && leaf.contains("pwrlevel")
    {
        return Some("CPU/GPU frequency limits are not forced by this toolkit (cpufreq floors belong to Zairenkai-FAS)");
    }
    if p.starts_with("/vendor/")
        || p.starts_with("/odm/")
        || p.starts_with("/system/")
        || p.starts_with("/product/")
        || p.starts_with("/system_ext/")
    {
        return Some("vendor and system configuration files are never rewritten in place");
    }
    if p.contains("thermal")
        && (p.contains("shutdown") || p.contains("critical") || p.contains("emergency"))
    {
        return Some("emergency and shutdown thermal settings are never modified");
    }
    if !(p.starts_with("/sys/") || p.starts_with("/proc/sys/")) {
        return Some("only sysfs and /proc/sys nodes can be controls");
    }
    None
}

/// Everything wrong with a descriptor. An empty list is required before any use.
pub fn descriptor_problems(d: &ControlDescriptor) -> Vec<String> {
    let mut out = Vec::new();
    if d.id.is_empty()
        || !d
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
    {
        out.push("id must be lowercase [a-z0-9._]".to_string());
    }
    if let Some(why) = denied_target(&d.target) {
        out.push(format!("target {}: {why}", d.target));
    }
    match &d.allowed {
        AllowedValues::IntegerRange { min, max } if min > max => {
            out.push("empty value range".into())
        }
        AllowedValues::OneOf { values }
            if values.is_empty() || values.iter().any(|v| v.is_empty() || v.contains('\n')) =>
        {
            out.push("allowed values must be non-empty single-line strings".into())
        }
        _ => {}
    }
    if d.evidence.is_empty() {
        out.push("no evidence for the control's semantics".into());
    }
    if !d.restore_original {
        out.push("the original value must be captured and restorable".into());
    }
    if !d.verify_after_write {
        out.push("writes must be verified by read-back".into());
    }
    if d.conditions == Conditions::default() {
        out.push("a control must be scoped to a SoC vendor, platform or device".into());
    }
    out
}

/// Device facts a descriptor's conditions are matched against.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceFacts {
    pub soc_vendor: Option<SocVendor>,
    pub board_platform: Option<String>,
    pub device: Option<String>,
}

pub fn conditions_match(c: &Conditions, f: &DeviceFacts) -> bool {
    c.soc_vendor.is_none_or(|v| f.soc_vendor == Some(v))
        && (c.board_platforms.is_empty()
            || f.board_platform
                .as_ref()
                .is_some_and(|p| c.board_platforms.contains(p)))
        && (c.devices.is_empty() || f.device.as_ref().is_some_and(|d| c.devices.contains(d)))
}

/// What a severity sample says, after validity checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    Severity(ThrottlingSeverity),
    /// No trustworthy reading (unavailable, stale, permission denied, parse error).
    Missing,
}

/// Adaptive policy parameters. Defaults: escalate after 3 consecutive samples at or above
/// `moderate`, relax after 5 consecutive samples at or below `light`, and keep any mode for
/// at least 60 s. These are policy pacing values, not temperature thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveConfig {
    pub escalate_at: ThrottlingSeverity,
    pub relax_at_or_below: ThrottlingSeverity,
    pub escalate_after: u32,
    pub relax_after: u32,
    pub min_dwell_ms: u64,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            escalate_at: ThrottlingSeverity::Moderate,
            relax_at_or_below: ThrottlingSeverity::Light,
            escalate_after: 3,
            relax_after: 5,
            min_dwell_ms: 60_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub mode: PolicyMode,
    pub changed: bool,
    pub explanation: String,
}

/// Adaptive state machine: chooses `balanced` or `power_saver` from framework severity.
/// It never chooses `performance` or `gaming` on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adaptive {
    cfg: AdaptiveConfig,
    mode: PolicyMode,
    since_ms: u64,
    hot_streak: u32,
    cool_streak: u32,
}

impl Adaptive {
    pub fn new(cfg: AdaptiveConfig, now_ms: u64) -> Self {
        Self {
            cfg,
            mode: PolicyMode::Balanced,
            since_ms: now_ms,
            hot_streak: 0,
            cool_streak: 0,
        }
    }

    pub fn mode(&self) -> PolicyMode {
        self.mode
    }

    pub fn observe(&mut self, obs: Observation, now_ms: u64) -> Decision {
        let dwell_ok = now_ms.saturating_sub(self.since_ms) >= self.cfg.min_dwell_ms;
        let explanation;
        let target = match obs {
            Observation::Missing => {
                // Missing data resets the relax streak: absence of a reading is not "cool".
                self.cool_streak = 0;
                explanation =
                    "no trustworthy severity reading; holding the current mode (a missing \
                               reading is never treated as a cool device)"
                        .to_string();
                self.mode
            }
            Observation::Severity(s) if s >= self.cfg.escalate_at => {
                self.hot_streak += 1;
                self.cool_streak = 0;
                if self.hot_streak >= self.cfg.escalate_after {
                    explanation =
                        format!("severity {s:?} for {} consecutive samples", self.hot_streak);
                    PolicyMode::PowerSaver
                } else {
                    explanation = format!(
                        "severity {s:?}, {}/{} samples before acting",
                        self.hot_streak, self.cfg.escalate_after
                    );
                    self.mode
                }
            }
            Observation::Severity(s) if s <= self.cfg.relax_at_or_below => {
                self.cool_streak += 1;
                self.hot_streak = 0;
                if self.cool_streak >= self.cfg.relax_after {
                    explanation = format!(
                        "severity {s:?} for {} consecutive samples",
                        self.cool_streak
                    );
                    PolicyMode::Balanced
                } else {
                    explanation = format!(
                        "severity {s:?}, {}/{} samples before relaxing",
                        self.cool_streak, self.cfg.relax_after
                    );
                    self.mode
                }
            }
            Observation::Severity(s) => {
                // Between the clear and escalate levels (hysteresis band): hold.
                self.hot_streak = 0;
                self.cool_streak = 0;
                explanation = format!("severity {s:?} inside the hysteresis band; holding");
                self.mode
            }
        };
        if target != self.mode {
            // Escalating to save power is always allowed; relaxing waits for the dwell time.
            if target == PolicyMode::PowerSaver || dwell_ok {
                self.mode = target;
                self.since_ms = now_ms;
                return Decision {
                    mode: target,
                    changed: true,
                    explanation,
                };
            }
            return Decision {
                mode: self.mode,
                changed: false,
                explanation: format!(
                    "{explanation}; waiting for the {} ms minimum dwell time",
                    self.cfg.min_dwell_ms
                ),
            };
        }
        Decision {
            mode: self.mode,
            changed: false,
            explanation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedWrite {
    pub control: String,
    pub target: String,
    pub value: String,
}

/// A policy plan. `writes` lists exactly what an apply would do; it is empty unless a
/// validated control applies to this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub schema_version: u32,
    pub mode: PolicyMode,
    pub dry_run: bool,
    pub writes: Vec<PlannedWrite>,
    pub notes: Vec<String>,
}

/// Build the plan for `mode` from the controls that apply to `facts`. Mode-to-value mapping
/// lives in each control's evidence; with no validated controls nothing is planned.
pub fn plan(
    mode: PolicyMode,
    facts: &DeviceFacts,
    controls: &[ControlDescriptor],
    dry_run: bool,
) -> Plan {
    let mut notes = Vec::new();
    if mode == PolicyMode::Diagnostic {
        notes.push("diagnostic mode never changes the device".to_string());
        return Plan {
            schema_version: 1,
            mode,
            dry_run,
            writes: vec![],
            notes,
        };
    }
    let usable: Vec<&ControlDescriptor> = controls
        .iter()
        .filter(|c| descriptor_problems(c).is_empty() && conditions_match(&c.conditions, facts))
        .collect();
    if usable.is_empty() {
        notes.push(
            "no validated writable control applies to this device; nothing would change. The \
             system's own thermal management stays fully in charge"
                .to_string(),
        );
    }
    if mode == PolicyMode::Balanced {
        notes.push("balanced leaves the system's behaviour unchanged".to_string());
    }
    if matches!(mode, PolicyMode::Performance | PolicyMode::Gaming) {
        notes.push(
            "performance and gaming never disable throttling, raise thresholds or force frequencies"
                .to_string(),
        );
    }
    Plan {
        schema_version: 1,
        mode,
        dry_run,
        writes: vec![],
        notes,
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A descriptor for tests only: a harmless sysfs node in a fixture root.
    pub fn test_descriptor() -> ControlDescriptor {
        ControlDescriptor {
            id: "test.example_knob".into(),
            target: "/sys/kernel/zthermal_test/knob".into(),
            description: "fixture-only knob".into(),
            conditions: Conditions {
                soc_vendor: Some(SocVendor::Qualcomm),
                ..Default::default()
            },
            allowed: AllowedValues::IntegerRange { min: 0, max: 2 },
            evidence: vec!["synthetic test fixture".into()],
            restore_original: true,
            verify_after_write: true,
            hardware_tested: false,
        }
    }

    #[test]
    fn production_allowlist_is_empty() {
        assert!(validated_controls().is_empty());
    }

    #[test]
    fn thermal_protection_is_never_a_target() {
        for p in [
            "/sys/class/thermal/thermal_zone0/trip_point_0_temp",
            "/sys/class/thermal/thermal_zone3/mode",
            "/sys/class/thermal/thermal_zone3/emul_temp",
            "/sys/class/thermal/thermal_zone3/policy",
            "/sys/devices/virtual/thermal/thermal_zone1/trip_point_2_hyst",
            "/sys/class/thermal/thermal_zone0/k_po",
            "/sys/class/thermal/cooling_device4/cur_state",
            "/sys/devices/system/cpu/cpufreq/policy0/scaling_max_freq",
            "/sys/devices/system/cpu/cpufreq/policy0/scaling_min_freq",
            "/sys/class/kgsl/kgsl-3d0/max_pwrlevel",
            "/sys/class/devfreq/soc:qcom,gpubw/max_freq",
            "/vendor/etc/thermal-engine.conf",
            "/odm/etc/thermal.conf",
            "/sys/module/foo_thermal/parameters/shutdown_temp",
            "relative/path",
            "/sys/../etc/passwd",
            "/sys/class/thermal/thermal_zone*/temp",
            "/data/local/tmp/x",
        ] {
            assert!(denied_target(p).is_some(), "{p} must be refused");
        }
        assert_eq!(denied_target("/sys/kernel/zthermal_test/knob"), None);
    }

    #[test]
    fn descriptors_need_complete_evidence() {
        assert!(descriptor_problems(&test_descriptor()).is_empty());
        let mut d = test_descriptor();
        d.target = "/sys/class/thermal/thermal_zone0/trip_point_0_temp".into();
        d.evidence.clear();
        d.verify_after_write = false;
        d.restore_original = false;
        d.conditions = Conditions::default();
        d.allowed = AllowedValues::IntegerRange { min: 3, max: 1 };
        assert_eq!(descriptor_problems(&d).len(), 6);
        let mut d = test_descriptor();
        d.allowed = AllowedValues::OneOf {
            values: vec!["a\nb".into()],
        };
        assert_eq!(descriptor_problems(&d).len(), 1);
    }

    #[test]
    fn allowed_values() {
        let r = AllowedValues::IntegerRange { min: 0, max: 2 };
        assert!(
            r.permits("1")
                && r.permits(" 2 ")
                && !r.permits("3")
                && !r.permits("-1")
                && !r.permits("x")
        );
        assert!(!r.permits("99999999999999999999"));
        let s = AllowedValues::OneOf {
            values: vec!["on".into(), "off".into()],
        };
        assert!(s.permits("on") && !s.permits("ON") && !s.permits("on "));
    }

    #[test]
    fn conditions() {
        let c = test_descriptor().conditions;
        let q = DeviceFacts {
            soc_vendor: Some(SocVendor::Qualcomm),
            ..Default::default()
        };
        let m = DeviceFacts {
            soc_vendor: Some(SocVendor::Mediatek),
            ..Default::default()
        };
        assert!(conditions_match(&c, &q));
        assert!(!conditions_match(&c, &m));
        assert!(!conditions_match(&c, &DeviceFacts::default())); // unknown SoC never matches
        let c = Conditions {
            board_platforms: vec!["taro".into()],
            ..Default::default()
        };
        assert!(!conditions_match(&c, &q));
    }

    #[test]
    fn plans_without_validated_controls_change_nothing() {
        for mode in [
            PolicyMode::Balanced,
            PolicyMode::PowerSaver,
            PolicyMode::Performance,
            PolicyMode::Gaming,
            PolicyMode::Adaptive,
        ] {
            let p = plan(mode, &DeviceFacts::default(), &validated_controls(), true);
            assert!(p.writes.is_empty(), "{mode:?}");
            assert!(
                p.notes.iter().any(|n| n.contains("nothing would change")),
                "{mode:?}"
            );
        }
        let p = plan(PolicyMode::Gaming, &DeviceFacts::default(), &[], true);
        assert!(p
            .notes
            .iter()
            .any(|n| n.contains("never disable throttling")));
        let p = plan(
            PolicyMode::Diagnostic,
            &DeviceFacts::default(),
            &[test_descriptor()],
            false,
        );
        assert!(p.writes.is_empty());
    }

    #[test]
    fn mode_names() {
        assert_eq!(
            PolicyMode::parse("power-saver"),
            Some(PolicyMode::PowerSaver)
        );
        assert_eq!(PolicyMode::parse("turbo"), None);
    }

    #[test]
    fn adaptive_escalates_with_debounce_and_relaxes_with_hysteresis() {
        use ThrottlingSeverity as S;
        let cfg = AdaptiveConfig::default();
        let mut a = Adaptive::new(cfg, 0);
        assert!(!a.observe(Observation::Severity(S::Moderate), 1_000).changed);
        assert!(!a.observe(Observation::Severity(S::Severe), 2_000).changed);
        let d = a.observe(Observation::Severity(S::Moderate), 3_000);
        assert!(d.changed && d.mode == PolicyMode::PowerSaver, "{d:?}");
        // Cool samples, but the minimum dwell time has not elapsed.
        for t in 4..9 {
            a.observe(Observation::Severity(S::None), t * 1_000);
        }
        assert_eq!(a.mode(), PolicyMode::PowerSaver);
        let d = a.observe(Observation::Severity(S::None), 70_000);
        assert!(d.changed && d.mode == PolicyMode::Balanced, "{d:?}");
    }

    #[test]
    fn missing_readings_never_count_as_cool() {
        use ThrottlingSeverity as S;
        let mut a = Adaptive::new(
            AdaptiveConfig {
                min_dwell_ms: 0,
                ..Default::default()
            },
            0,
        );
        for t in 0..3 {
            a.observe(Observation::Severity(S::Severe), t);
        }
        assert_eq!(a.mode(), PolicyMode::PowerSaver);
        for t in 3..20 {
            let d = a.observe(Observation::Missing, t);
            assert!(!d.changed);
            assert!(d.explanation.contains("never treated as a cool device"));
        }
        assert_eq!(a.mode(), PolicyMode::PowerSaver);
        // Cool streak interrupted by a missing reading starts over.
        for t in 20..24 {
            a.observe(Observation::Severity(S::None), t);
        }
        a.observe(Observation::Missing, 24);
        a.observe(Observation::Severity(S::None), 25);
        assert_eq!(a.mode(), PolicyMode::PowerSaver);
    }

    #[test]
    fn hysteresis_band_holds_and_adaptive_never_picks_performance() {
        use ThrottlingSeverity as S;
        let mut a = Adaptive::new(
            AdaptiveConfig {
                min_dwell_ms: 0,
                ..Default::default()
            },
            0,
        );
        for t in 0..100 {
            let s = [S::None, S::Light, S::Moderate, S::Severe, S::Critical][(t % 5) as usize];
            let d = a.observe(Observation::Severity(s), t);
            assert!(matches!(
                d.mode,
                PolicyMode::Balanced | PolicyMode::PowerSaver
            ));
        }
        // Light sits in the relax band in the default config; with relax_at_or_below = None
        // it is inside the hysteresis band and holds.
        let mut a = Adaptive::new(
            AdaptiveConfig {
                relax_at_or_below: S::None,
                min_dwell_ms: 0,
                ..Default::default()
            },
            0,
        );
        let d = a.observe(Observation::Severity(S::Light), 1);
        assert!(d.explanation.contains("hysteresis band"));
    }
}
