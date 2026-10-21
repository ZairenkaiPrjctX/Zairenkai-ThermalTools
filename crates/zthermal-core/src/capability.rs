// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Capability model: what the toolkit can do on this device, and how sure it is.
//!
//! Recognizing a manufacturer, identifying a SoC, detecting an implementation, parsing a
//! format, validating a control and testing on hardware are different facts. A capability
//! records each separately; nothing here is ever promoted to "writable" by inference.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityState {
    AvailableReadOnly,
    /// Only for controls in the validated allowlist whose preconditions all hold.
    AvailableWritable,
    Unsupported,
    PermissionDenied,
    /// Present on the device, but its semantics are not verified (e.g. an unparsed format).
    DetectedUnverified,
    /// Would be usable after a validation step that has not happened.
    ValidationRequired,
    TemporarilyUnavailable,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
}

/// Privilege the operation needs (not the privilege the caller has).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Privilege {
    /// Any process (an ordinary app can read most of /sys/class/thermal).
    Unprivileged,
    /// adb shell (`dumpsys` needs android.permission.DUMP).
    Shell,
    Root,
}

/// How far a capability has been validated. Each level is a different claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Validation {
    /// Behaviour checked only against synthetic fixtures or mocks.
    FixturesOnly,
    /// Checked against public source code or published configuration files.
    PublicSource,
    /// Checked on a physical device (with a recorded test report).
    HardwareTested,
    /// Not validated at all.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// Stable identifier, e.g. `sensors.sysfs_thermal_zones`.
    pub id: String,
    pub description: String,
    /// Which devices or platforms this applies to (e.g. "any Linux thermal sysfs").
    pub scope: String,
    pub discovery: String,
    pub requires: String,
    pub privilege: Privilege,
    pub access: Access,
    pub state: CapabilityState,
    pub validation: Validation,
    pub evidence: Vec<String>,
    /// Why the capability is not available, when it is not.
    pub reason: Option<String>,
    /// True only with a recorded physical-device test. Always false in this release.
    pub hardware_tested: bool,
    /// The runtime probe behind `state`, when one ran in this invocation. `None` means the
    /// state comes from static detection only.
    pub probe: Option<ProbeRecord>,
}

/// What one runtime probe (for example `dumpsys thermalservice`) returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeRecord {
    pub source: String,
    pub status: crate::reading::DataStatus,
    pub detail: Option<String>,
}

impl Capability {
    pub fn read(
        id: &str,
        description: &str,
        scope: &str,
        discovery: &str,
        requires: &str,
        privilege: Privilege,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            scope: scope.into(),
            discovery: discovery.into(),
            requires: requires.into(),
            privilege,
            access: Access::Read,
            state: CapabilityState::Unsupported,
            validation: Validation::FixturesOnly,
            evidence: vec![],
            reason: None,
            hardware_tested: false,
            probe: None,
        }
    }

    pub fn with(mut self, state: CapabilityState, reason: Option<String>) -> Self {
        self.state = state;
        self.reason = reason;
        self
    }

    pub fn evidence(mut self, e: impl Into<String>) -> Self {
        self.evidence.push(e.into());
        self
    }

    pub fn validated(mut self, v: Validation) -> Self {
        self.validation = v;
        self
    }
}

/// Android `ThrottlingSeverity` (android.os.Temperature THROTTLING_* constants and
/// android.hardware.thermal ThrottlingSeverity), as reported by the framework. A severity
/// is the framework's assessment, never a temperature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ThrottlingSeverity {
    None,
    Light,
    Moderate,
    Severe,
    Critical,
    Emergency,
    Shutdown,
}

impl ThrottlingSeverity {
    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            0 => Self::None,
            1 => Self::Light,
            2 => Self::Moderate,
            3 => Self::Severe,
            4 => Self::Critical,
            5 => Self::Emergency,
            6 => Self::Shutdown,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_serialize_as_documented() {
        let s = serde_json::to_string(&CapabilityState::AvailableReadOnly).unwrap();
        assert_eq!(s, "\"AVAILABLE_READ_ONLY\"");
        let s = serde_json::to_string(&CapabilityState::DetectedUnverified).unwrap();
        assert_eq!(s, "\"DETECTED_UNVERIFIED\"");
    }

    #[test]
    fn builder_defaults_are_conservative() {
        let c = Capability::read("x", "d", "s", "m", "r", Privilege::Unprivileged);
        assert_eq!(c.state, CapabilityState::Unsupported);
        assert_eq!(c.access, Access::Read);
        assert!(!c.hardware_tested);
        assert_eq!(c.validation, Validation::FixturesOnly);
    }

    #[test]
    fn severities() {
        assert_eq!(
            ThrottlingSeverity::from_code(3),
            Some(ThrottlingSeverity::Severe)
        );
        assert_eq!(ThrottlingSeverity::from_code(7), None);
        assert_eq!(ThrottlingSeverity::from_code(-1), None);
        assert!(ThrottlingSeverity::Critical > ThrottlingSeverity::Moderate);
    }
}
