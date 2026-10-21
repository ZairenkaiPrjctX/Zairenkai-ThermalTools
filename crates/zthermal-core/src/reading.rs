// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Readings: data-quality status, temperature normalization and sensor classification.
//!
//! Every value carries its raw text, its source and a [`DataStatus`]. Nothing here invents a
//! value: a reading that cannot be interpreted safely stays `None` with a reason.

use serde::{Deserialize, Serialize};

/// Quality of one reading or diagnostic record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataStatus {
    /// Read and interpreted with a known unit.
    Valid,
    /// The interface does not exist on this device or build.
    Unsupported,
    /// The interface exists but returned no value (e.g. sensor offline, EAGAIN, EIO).
    Unavailable,
    /// The caller lacks the permission to read it.
    PermissionDenied,
    /// A value was returned but could not be parsed.
    ParseError,
    /// The value is older than the caller's freshness limit.
    Stale,
    /// Parsed, but physically implausible or contradicted by another source.
    Inconsistent,
    /// Not determined.
    Unknown,
}

/// Unit of a raw temperature value, as defined by the interface that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TempUnit {
    /// Linux thermal and hwmon sysfs ABI: millidegree Celsius.
    MilliCelsius,
    /// `dumpsys battery` "temperature": tenths of a degree Celsius.
    DeciCelsius,
    /// Android framework `Temperature.getValue()` (dumpsys thermalservice): degrees Celsius.
    Celsius,
}

/// Physically plausible range for a device temperature sensor, in millidegrees Celsius.
/// Outside it a value is reported as `inconsistent` (kept raw, not converted or clamped):
/// -40 °C covers cold-soaked devices, 150 °C is above any silicon junction shutdown point.
pub const PLAUSIBLE_MIN_MC: i64 = -40_000;
pub const PLAUSIBLE_MAX_MC: i64 = 150_000;
/// A millidegree source reporting a non-zero magnitude this small (0.2 °C or less) most
/// likely reports whole degrees; such values are flagged, never silently multiplied.
pub const SUSPECT_DEGREES_MAX: i64 = 200;

/// A normalized temperature reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Temperature {
    /// The text exactly as read (trimmed), for diagnostics.
    pub raw: Option<String>,
    pub unit: TempUnit,
    /// Normalized value in millidegrees Celsius; `None` unless `status` is `valid` or
    /// `inconsistent` (an implausible but parseable value is kept for diagnosis).
    pub millicelsius: Option<i64>,
    pub status: DataStatus,
    /// Why the value is not `valid`.
    pub reason: Option<String>,
}

impl Temperature {
    pub fn missing(unit: TempUnit, status: DataStatus, reason: impl Into<String>) -> Self {
        Self {
            raw: None,
            unit,
            millicelsius: None,
            status,
            reason: Some(reason.into()),
        }
    }

    /// Normalize a raw text value in `unit`. A genuine `0` is a valid reading; an empty
    /// value is `unavailable`; text that is not a number is a `parse_error`.
    pub fn parse(raw: &str, unit: TempUnit) -> Self {
        let text = raw.trim();
        if text.is_empty() {
            return Self {
                raw: Some(String::new()),
                unit,
                millicelsius: None,
                status: DataStatus::Unavailable,
                reason: Some("empty value".into()),
            };
        }
        let mc = match unit {
            TempUnit::MilliCelsius => text.parse::<i64>().ok(),
            TempUnit::DeciCelsius => text.parse::<i64>().ok().and_then(|v| v.checked_mul(100)),
            TempUnit::Celsius => text
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && v.abs() < 1.0e9)
                .map(|v| (v * 1000.0).round() as i64),
        };
        let Some(mc) = mc else {
            return Self {
                raw: Some(text.to_string()),
                unit,
                millicelsius: None,
                status: DataStatus::ParseError,
                reason: Some(format!("not a number in {unit:?}")),
            };
        };
        let (status, reason) = if unit == TempUnit::MilliCelsius
            && mc != 0
            && mc.abs() <= SUSPECT_DEGREES_MAX
        {
            (
                DataStatus::Inconsistent,
                Some(format!(
                    "{mc} m°C is implausibly small; the driver may report whole degrees \
                     instead of the sysfs millidegree unit (value not converted)"
                )),
            )
        } else if (PLAUSIBLE_MIN_MC..=PLAUSIBLE_MAX_MC).contains(&mc) {
            (DataStatus::Valid, None)
        } else {
            (
                DataStatus::Inconsistent,
                Some(format!(
                    "{mc} m°C is outside the plausible range {PLAUSIBLE_MIN_MC}..={PLAUSIBLE_MAX_MC}; \
                     the sensor may be disabled or report a different unit"
                )),
            )
        };
        Self {
            raw: Some(text.to_string()),
            unit,
            millicelsius: Some(mc),
            status,
            reason,
        }
    }

    pub fn celsius(&self) -> Option<f64> {
        self.millicelsius.map(|m| m as f64 / 1000.0)
    }

    /// Mark the reading stale when it is older than `max_age_ms` at `now_ms`.
    pub fn check_freshness(&mut self, read_at_ms: u64, now_ms: u64, max_age_ms: u64) {
        if self.status == DataStatus::Valid && now_ms.saturating_sub(read_at_ms) > max_age_ms {
            self.status = DataStatus::Stale;
            self.reason = Some(format!(
                "read {} ms ago, limit {max_age_ms} ms",
                now_ms.saturating_sub(read_at_ms)
            ));
        }
    }
}

/// What a sensor measures, inferred from its name only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SensorCategory {
    Cpu,
    Gpu,
    Npu,
    Battery,
    Skin,
    Charger,
    Usb,
    Modem,
    Memory,
    Pmic,
    Camera,
    Display,
    Wifi,
    Ambient,
    Soc,
    Unknown,
}

/// Classification of a sensor name, with the rule that matched (so a report can show why).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    pub category: SensorCategory,
    /// The pattern that matched, or `None` for `unknown`.
    pub rule: Option<String>,
}

/// Name patterns, checked in order: more specific first (e.g. "usb" before "battery" for
/// "usb_therm", "skin"/"quiet" before "cpu"). Substring match on the lowercased name.
/// These are naming conventions seen in kernel and vendor sources, not guarantees: a
/// category is a hint for display, never a basis for a control decision.
const RULES: &[(&str, SensorCategory)] = &[
    ("usb", SensorCategory::Usb),
    ("charger", SensorCategory::Charger),
    ("chg", SensorCategory::Charger),
    ("battery", SensorCategory::Battery),
    ("batt", SensorCategory::Battery),
    ("bms", SensorCategory::Battery),
    ("skin", SensorCategory::Skin),
    ("quiet", SensorCategory::Skin),
    ("xo_therm", SensorCategory::Skin),
    ("case", SensorCategory::Skin),
    ("back_therm", SensorCategory::Skin),
    ("gpu", SensorCategory::Gpu),
    ("mali", SensorCategory::Gpu),
    ("g3d", SensorCategory::Gpu),
    ("npu", SensorCategory::Npu),
    ("tpu", SensorCategory::Npu),
    ("apu", SensorCategory::Npu),
    ("modem", SensorCategory::Modem),
    ("mdm", SensorCategory::Modem),
    ("pa_therm", SensorCategory::Modem),
    ("rfpa", SensorCategory::Modem),
    ("ddr", SensorCategory::Memory),
    ("dram", SensorCategory::Memory),
    ("pmic", SensorCategory::Pmic),
    ("pm8", SensorCategory::Pmic),
    ("pmk", SensorCategory::Pmic),
    ("camera", SensorCategory::Camera),
    ("cam_", SensorCategory::Camera),
    ("disp", SensorCategory::Display),
    ("lcd", SensorCategory::Display),
    ("wifi", SensorCategory::Wifi),
    ("wlan", SensorCategory::Wifi),
    ("ambient", SensorCategory::Ambient),
    ("cpu", SensorCategory::Cpu),
    ("cpuss", SensorCategory::Cpu),
    ("big", SensorCategory::Cpu),
    ("little", SensorCategory::Cpu),
    ("mid", SensorCategory::Cpu),
    ("soc", SensorCategory::Soc),
    ("tsens", SensorCategory::Soc),
];

pub fn classify(name: &str) -> Classification {
    let lower = name.to_ascii_lowercase();
    for (pat, cat) in RULES {
        if lower.contains(pat) {
            return Classification {
                category: *cat,
                rule: Some(format!("name contains \"{pat}\"")),
            };
        }
    }
    Classification {
        category: SensorCategory::Unknown,
        rule: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millicelsius_values_and_genuine_zero() {
        let t = Temperature::parse("42500\n", TempUnit::MilliCelsius);
        assert_eq!(
            (t.millicelsius, t.status),
            (Some(42_500), DataStatus::Valid)
        );
        assert_eq!(t.celsius(), Some(42.5));
        let z = Temperature::parse("0", TempUnit::MilliCelsius);
        assert_eq!((z.millicelsius, z.status), (Some(0), DataStatus::Valid)); // not "missing"
        let neg = Temperature::parse("-12000", TempUnit::MilliCelsius);
        assert_eq!(neg.status, DataStatus::Valid);
    }

    #[test]
    fn invalid_and_implausible_values_are_never_converted() {
        assert_eq!(
            Temperature::parse("", TempUnit::MilliCelsius).status,
            DataStatus::Unavailable
        );
        let bad = Temperature::parse("N/A", TempUnit::MilliCelsius);
        assert_eq!(
            (bad.status, bad.millicelsius),
            (DataStatus::ParseError, None)
        );
        assert_eq!(bad.raw.as_deref(), Some("N/A"));
        // A disabled sensor commonly reports -273000 or a huge sentinel.
        let off = Temperature::parse("-273000", TempUnit::MilliCelsius);
        assert_eq!(
            (off.status, off.millicelsius),
            (DataStatus::Inconsistent, Some(-273_000))
        );
        // A non-conforming driver reporting whole degrees: kept as-is, flagged, not "fixed".
        let deg = Temperature::parse("45", TempUnit::MilliCelsius);
        assert_eq!(
            (deg.status, deg.millicelsius),
            (DataStatus::Inconsistent, Some(45))
        );
        assert!(deg.reason.unwrap().contains("whole degrees"));
        let big = Temperature::parse("9223372036854775807", TempUnit::MilliCelsius);
        assert_eq!(big.status, DataStatus::Inconsistent);
        let overflow = Temperature::parse("99999999999999999999", TempUnit::MilliCelsius);
        assert_eq!(overflow.status, DataStatus::ParseError);
        let deci_overflow = Temperature::parse("92233720368547759", TempUnit::DeciCelsius);
        assert_eq!(deci_overflow.status, DataStatus::ParseError);
        assert_eq!(
            Temperature::parse("NaN", TempUnit::Celsius).status,
            DataStatus::ParseError
        );
        assert_eq!(
            Temperature::parse("inf", TempUnit::Celsius).status,
            DataStatus::ParseError
        );
    }

    #[test]
    fn other_units() {
        let b = Temperature::parse("312", TempUnit::DeciCelsius);
        assert_eq!(
            (b.millicelsius, b.status),
            (Some(31_200), DataStatus::Valid)
        );
        let f = Temperature::parse("36.25", TempUnit::Celsius);
        assert_eq!(f.millicelsius, Some(36_250));
    }

    #[test]
    fn stale_readings() {
        let mut t = Temperature::parse("40000", TempUnit::MilliCelsius);
        t.check_freshness(1_000, 2_000, 5_000);
        assert_eq!(t.status, DataStatus::Valid);
        t.check_freshness(1_000, 9_000, 5_000);
        assert_eq!(t.status, DataStatus::Stale);
        let mut bad = Temperature::parse("x", TempUnit::MilliCelsius);
        bad.check_freshness(0, 100_000, 1);
        assert_eq!(bad.status, DataStatus::ParseError); // stale only applies to valid values
    }

    #[test]
    fn classification_uses_names_never_indices() {
        assert_eq!(classify("usb_therm").category, SensorCategory::Usb);
        assert_eq!(classify("battery").category, SensorCategory::Battery);
        assert_eq!(classify("skin-therm").category, SensorCategory::Skin);
        assert_eq!(classify("quiet_therm").category, SensorCategory::Skin);
        assert_eq!(classify("cpu-1-0-usr").category, SensorCategory::Cpu);
        assert_eq!(classify("gpuss-0-usr").category, SensorCategory::Gpu);
        assert_eq!(classify("mtktsbattery").category, SensorCategory::Battery);
        assert_eq!(classify("mtktscpu").category, SensorCategory::Cpu);
        assert_eq!(classify("tsens_tz_sensor7").category, SensorCategory::Soc);
        assert_eq!(classify("thermal_zone12").category, SensorCategory::Unknown);
        assert_eq!(
            classify("BIG").rule.as_deref(),
            Some("name contains \"big\"")
        );
    }
}
