// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Pixel Thermal HAL `thermal_info_config*.json`.
//!
//! The checks mirror the HAL's own parser (`hardware/google/pixel` `thermal/utils/
//! thermal_info.cpp` at 8612656e216075f79a1efa3bb9be6b6943d6e8cc): a file this module calls
//! an error is one that parser rejects. Keys it does not know are kept and reported.
//!
//! JSON is read strictly: a duplicated key is an error here, because a last-wins reader
//! would silently drop the first value. Files with `//` comments are rejected as not strict
//! JSON; whether the HAL's jsoncpp configuration accepts comments is unverified.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{json, Map, Number, Value};

use super::{Finding, RoundTrip, SafetyFact, Severity};

/// AIDL TemperatureType names (hardware/interfaces thermal/aidl).
pub const TEMPERATURE_TYPES: &[&str] = &[
    "UNKNOWN",
    "CPU",
    "GPU",
    "BATTERY",
    "SKIN",
    "USB_PORT",
    "POWER_AMPLIFIER",
    "BCL_VOLTAGE",
    "BCL_CURRENT",
    "BCL_PERCENTAGE",
    "NPU",
    "TPU",
    "DISPLAY",
    "MODEM",
    "SOC",
    "WIFI",
    "CAMERA",
    "FLASHLIGHT",
    "SPEAKER",
    "AMBIENT",
    "POGO",
];

/// AIDL CoolingType names.
pub const COOLING_TYPES: &[&str] = &[
    "FAN",
    "BATTERY",
    "CPU",
    "GPU",
    "MODEM",
    "NPU",
    "COMPONENT",
    "TPU",
    "POWER_AMPLIFIER",
    "DISPLAY",
    "SPEAKER",
    "WIFI",
    "CAMERA",
    "FLASHLIGHT",
    "USB_PORT",
];

/// ThrottlingSeverity names, the index of each threshold array entry.
pub const SEVERITIES: [&str; 7] = [
    "NONE",
    "LIGHT",
    "MODERATE",
    "SEVERE",
    "CRITICAL",
    "EMERGENCY",
    "SHUTDOWN",
];

pub const FORMULAS: &[&str] = &[
    "COUNT_THRESHOLD",
    "WEIGHTED_AVG",
    "MAXIMUM",
    "MINIMUM",
    "USE_ML_MODEL",
    "USE_LINEAR_MODEL",
    "PREVIOUSLY_PREDICTED",
];

pub const TOP_LEVEL_KEYS: &[&str] = &[
    "Include",
    "Sensors",
    "CoolingDevices",
    "PowerRails",
    "Stats",
    "Comment",
];

/// Sensor keys the parser reads, plus `Version`, which published files carry and this
/// parser revision ignores.
pub const SENSOR_KEYS: &[&str] = &[
    "Name",
    "isDisabled",
    "Type",
    "Monitor",
    "SendCallback",
    "SendPowerHint",
    "Hidden",
    "LogLevel",
    "HotThreshold",
    "HotHysteresis",
    "ColdThreshold",
    "ColdHysteresis",
    "VrThreshold",
    "SeverityReference",
    "OffsetThresholds",
    "OffsetValues",
    "TempPath",
    "Multiplier",
    "PollingDelay",
    "PassiveDelay",
    "TimeResolution",
    "StepRatio",
    "VirtualSensor",
    "Combination",
    "CombinationType",
    "Coefficient",
    "CoefficientType",
    "Formula",
    "Offset",
    "BackupSensor",
    "TriggerSensor",
    "ModelPath",
    "PreviousSampleCount",
    "OutputLabelCount",
    "PredictHotSpotCount",
    "ValidateInput",
    "SupportUnderSampling",
    "PredictorInfo",
    "PIDInfo",
    "BindedCdevInfo",
    "Profile",
    "ExcludedPowerInfo",
    "Version",
];

pub const COOLING_DEVICE_KEYS: &[&str] = &[
    "Name",
    "isDisabled",
    "Type",
    "ReadPath",
    "WritePath",
    "State2Power",
    "PowerRail",
    "ScalingAvailableFrequenciesPath",
];

/// A JSON value read with duplicate-key detection.
struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Strict;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Strict, E> {
        Ok(Strict(Value::Bool(v)))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Strict, E> {
        Ok(Strict(Value::Number(v.into())))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Strict, E> {
        Ok(Strict(Value::Number(v.into())))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Strict, E> {
        Number::from_f64(v)
            .map(|n| Strict(Value::Number(n)))
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E>(self, v: &str) -> Result<Strict, E> {
        Ok(Strict(Value::String(v.to_string())))
    }
    fn visit_string<E>(self, v: String) -> Result<Strict, E> {
        Ok(Strict(Value::String(v)))
    }
    fn visit_unit<E>(self) -> Result<Strict, E> {
        Ok(Strict(Value::Null))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Strict, A::Error> {
        let mut out = Vec::new();
        while let Some(Strict(v)) = seq.next_element()? {
            out.push(v);
        }
        Ok(Strict(Value::Array(out)))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Strict, A::Error> {
        let mut out = Map::new();
        while let Some(k) = map.next_key::<String>()? {
            if out.contains_key(&k) {
                return Err(de::Error::custom(format!("duplicate key {k:?}")));
            }
            let Strict(v) = map.next_value()?;
            out.insert(k, v);
        }
        Ok(Strict(Value::Object(out)))
    }
}

pub fn parse_strict(text: &str) -> Result<Value, String> {
    let mut de = serde_json::Deserializer::from_str(text.trim_start_matches('\u{feff}'));
    let Strict(v) = Strict::deserialize(&mut de).map_err(|e| format!("not strict JSON: {e}"))?;
    de.end()
        .map_err(|e| format!("trailing data after JSON: {e}"))?;
    Ok(v)
}

/// The HAL's `getFloatFromValue`: numbers as float, strings through `std::stof`.
/// `Err` means the HAL would throw or reject the value.
pub fn hal_float(v: &Value) -> Result<f32, String> {
    match v {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN) as f32),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Null => Ok(0.0),
        Value::String(s) => {
            let t = s.trim();
            match t.to_ascii_lowercase().as_str() {
                "nan" | "+nan" | "-nan" => Ok(f32::NAN),
                "inf" | "+inf" | "infinity" | "+infinity" => Ok(f32::INFINITY),
                "-inf" | "-infinity" => Ok(f32::NEG_INFINITY),
                _ => t
                    .parse::<f32>()
                    .map_err(|_| format!("{s:?} is not a number std::stof accepts as a whole")),
            }
        }
        _ => Err(format!("{v} is not a number")),
    }
}

fn arr_f32(v: Option<&Value>) -> Result<Option<Vec<f32>>, String> {
    match v {
        None => Ok(None),
        Some(Value::Array(a)) => a
            .iter()
            .map(hal_float)
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(other) => Err(format!(
            "{other} is not an array (the HAL reads size 0 and ignores it)"
        )),
    }
}

fn truthy(o: &Map<String, Value>, key: &str) -> bool {
    o.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn len(o: &Map<String, Value>, key: &str) -> usize {
    match o.get(key) {
        Some(Value::Array(a)) => a.len(),
        Some(Value::Object(m)) => m.len(),
        _ => 0,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PixelConfig {
    pub root: Value,
}

impl PixelConfig {
    pub fn parse(text: &str) -> Result<PixelConfig, String> {
        let root = parse_strict(text)?;
        if !root.is_object() {
            return Err("top level is not a JSON object".into());
        }
        Ok(PixelConfig { root })
    }

    pub fn serialize(&self) -> String {
        let mut s = serde_json::to_string_pretty(&self.root).expect("Value serializes");
        s.push('\n');
        s
    }

    /// The JSON model is equal after a round trip; whitespace is not preserved.
    pub fn round_trip(&self, original: &str) -> RoundTrip {
        let out = self.serialize();
        RoundTrip {
            byte_exact: out == original,
            model_equal: PixelConfig::parse(&out).ok().as_ref() == Some(self),
        }
    }

    fn list(&self, key: &str) -> Vec<&Map<String, Value>> {
        self.root
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_object).collect())
            .unwrap_or_default()
    }

    pub fn sensors(&self) -> Vec<&Map<String, Value>> {
        self.list("Sensors")
    }

    pub fn cooling_devices(&self) -> Vec<&Map<String, Value>> {
        self.list("CoolingDevices")
    }

    pub fn includes(&self) -> Vec<String> {
        self.root
            .get("Include")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn summary(&self) -> Value {
        let sensors = self.sensors();
        json!({
            "top_level_keys": self.root.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
            "sensors": sensors.len(),
            "virtual_sensors": sensors.iter().filter(|s| truthy(s, "VirtualSensor")).count(),
            "sensors_with_hot_thresholds": sensors.iter().filter(|s| len(s, "HotThreshold") > 0).count(),
            "cooling_devices": self.cooling_devices().len(),
            "power_rails": self.list("PowerRails").len(),
            "includes": self.includes(),
            "has_stats": self.root.get("Stats").is_some(),
            "units": "temperatures in degrees Celsius except BCL types (mV, mA, %)",
        })
    }

    pub fn safety_facts(&self) -> Vec<SafetyFact> {
        let mut out = Vec::new();
        for s in self.sensors() {
            let name = s.get("Name").and_then(Value::as_str).unwrap_or("?");
            if let Ok(Some(hot)) = arr_f32(s.get("HotThreshold")) {
                for (i, t) in hot.iter().enumerate().skip(4) {
                    if hot.len() == 7 && !t.is_nan() {
                        out.push(SafetyFact {
                            location: format!("Sensors[{name}].HotThreshold[{i}]"),
                            description: format!("{} severity at {t}", SEVERITIES[i]),
                        });
                    }
                }
            }
        }
        out
    }

    pub fn validate(&self) -> Vec<Finding> {
        let mut f = Vec::new();
        let root = self.root.as_object().expect("checked in parse");
        for k in root.keys() {
            if !TOP_LEVEL_KEYS.contains(&k.as_str()) {
                f.push(Finding::new(
                    Severity::Info,
                    "unknown_key",
                    None,
                    format!("top-level key {k:?} is not read by the HAL; kept"),
                ));
            }
        }
        if !self.includes().is_empty() {
            f.push(Finding::new(
                Severity::Info,
                "include",
                None,
                format!(
                    "includes {:?} are merged by the HAL (the main file wins); inspect them too",
                    self.includes()
                ),
            ));
        }
        let cdev_names = self.validate_cooling_devices(&mut f);
        let mut names = BTreeSet::new();
        match root.get("Sensors") {
            None => {}
            Some(Value::Array(a)) => {
                for (i, s) in a.iter().enumerate() {
                    match s.as_object() {
                        Some(o) => validate_sensor(i, o, &mut names, &cdev_names, &mut f),
                        None => f.push(Finding::new(
                            Severity::Error,
                            "not_object",
                            Some(format!("Sensors[{i}]")),
                            "sensor is not an object",
                        )),
                    }
                }
            }
            Some(_) => f.push(Finding::new(
                Severity::Error,
                "not_array",
                None,
                "Sensors is not an array",
            )),
        }
        f
    }

    fn validate_cooling_devices(&self, f: &mut Vec<Finding>) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for (i, c) in self.cooling_devices().into_iter().enumerate() {
            let name = c.get("Name").and_then(Value::as_str).unwrap_or("");
            let loc = Some(format!(
                "CoolingDevices[{}]",
                if name.is_empty() {
                    i.to_string()
                } else {
                    name.to_string()
                }
            ));
            if name.is_empty() {
                f.push(Finding::new(
                    Severity::Error,
                    "missing_name",
                    loc,
                    "cooling device without Name",
                ));
                continue;
            }
            if truthy(c, "isDisabled") {
                f.push(Finding::new(
                    Severity::Info,
                    "disabled",
                    loc,
                    "isDisabled: the HAL skips it",
                ));
                continue;
            }
            if !names.insert(name.to_string()) {
                f.push(Finding::new(
                    Severity::Error,
                    "duplicate_name",
                    loc.clone(),
                    "duplicate cooling device Name",
                ));
            }
            let ty = c.get("Type").and_then(Value::as_str).unwrap_or("");
            if !COOLING_TYPES.contains(&ty) {
                f.push(Finding::new(
                    Severity::Error,
                    "invalid_type",
                    loc.clone(),
                    format!("Type {ty:?} is not a CoolingType"),
                ));
            }
            for k in c.keys() {
                if !COOLING_DEVICE_KEYS.contains(&k.as_str()) {
                    f.push(Finding::new(
                        Severity::Info,
                        "unknown_key",
                        loc.clone(),
                        format!("key {k:?} is not read; kept"),
                    ));
                }
            }
        }
        names
    }
}

fn validate_sensor(
    i: usize,
    s: &Map<String, Value>,
    names: &mut BTreeSet<String>,
    cdevs: &BTreeSet<String>,
    f: &mut Vec<Finding>,
) {
    let name = s.get("Name").and_then(Value::as_str).unwrap_or("");
    let loc = Some(format!(
        "Sensors[{}]",
        if name.is_empty() {
            i.to_string()
        } else {
            name.to_string()
        }
    ));
    let err = |f: &mut Vec<Finding>, code: &str, m: String| {
        f.push(Finding::new(Severity::Error, code, loc.clone(), m))
    };
    if name.is_empty() {
        return err(f, "missing_name", "sensor without Name".into());
    }
    if truthy(s, "isDisabled") {
        f.push(Finding::new(
            Severity::Info,
            "disabled",
            loc,
            "isDisabled: the HAL skips it",
        ));
        return;
    }
    if !names.insert(name.to_string()) {
        err(f, "duplicate_name", "duplicate sensor Name".into());
    }
    let ty = s.get("Type").and_then(Value::as_str).unwrap_or("");
    if !TEMPERATURE_TYPES.contains(&ty) {
        err(
            f,
            "invalid_type",
            format!("Type {ty:?} is not a TemperatureType"),
        );
    }
    for k in s.keys() {
        if !SENSOR_KEYS.contains(&k.as_str()) {
            f.push(Finding::new(
                Severity::Info,
                "unknown_key",
                loc.clone(),
                format!("key {k:?} is not read; kept"),
            ));
        }
    }
    let monitor = match s.get("Monitor") {
        Some(Value::Bool(b)) => *b,
        _ => truthy(s, "SendCallback"),
    };
    if truthy(s, "Hidden") && monitor {
        err(
            f,
            "hidden_and_monitor",
            "Hidden and Monitor/SendCallback cannot both be true".into(),
        );
    }

    let mut arrays = [
        vec![f32::NAN; 7],
        vec![0.0; 7],
        vec![f32::NAN; 7],
        vec![0.0; 7],
    ];
    for (slot, key) in [
        "HotThreshold",
        "HotHysteresis",
        "ColdThreshold",
        "ColdHysteresis",
    ]
    .iter()
    .enumerate()
    {
        match arr_f32(s.get(*key)) {
            Ok(None) => {}
            Ok(Some(v)) if v.is_empty() => {}
            Ok(Some(v)) if v.len() != 7 => err(
                f,
                "threshold_count",
                format!("{key} has {} entries, not 0 or 7", v.len()),
            ),
            Ok(Some(v)) => {
                if key.ends_with("Hysteresis") && v.iter().any(|x| x.is_nan()) {
                    err(f, "nan_hysteresis", format!("{key} contains NaN"));
                }
                arrays[slot] = v;
            }
            Err(e) if s.get(*key).is_some_and(|v| !v.is_array()) => f.push(Finding::new(
                Severity::Warning,
                "not_array",
                loc.clone(),
                format!("{key}: {e}"),
            )),
            Err(e) => err(f, "not_numeric", format!("{key}: {e}")),
        }
    }
    let [hot, hot_h, cold, cold_h] = &arrays;
    // The HAL starts its running minimum at FLT_MIN, so 0 and negative hot thresholds fail.
    let mut min = f32::MIN_POSITIVE;
    for (j, t) in hot.iter().enumerate().filter(|(_, t)| !t.is_nan()) {
        if *t < min {
            err(
                f,
                "hot_order",
                format!("HotThreshold[{j}] {t} < {min} (must be non-decreasing and > 0)"),
            );
        }
        min = *t;
    }
    let mut max = f32::MAX;
    for (j, t) in cold.iter().enumerate().filter(|(_, t)| !t.is_nan()) {
        if *t > max {
            err(
                f,
                "cold_order",
                format!("ColdThreshold[{j}] {t} > {max} (must be non-increasing)"),
            );
        }
        max = *t;
    }
    for j in 0..6 {
        if let Some(k) = (j + 1..7).find(|k| !hot[*k].is_nan()) {
            if !hot[j].is_nan() && hot[j] > hot[k] - hot_h[k] {
                err(
                    f,
                    "hot_overlap",
                    format!(
                        "HotThreshold[{j}] {} overlaps [{k}] {} - hysteresis {}",
                        hot[j], hot[k], hot_h[k]
                    ),
                );
            }
        }
        if let Some(k) = (j + 1..7).find(|k| !cold[*k].is_nan()) {
            if !cold[j].is_nan() && cold[j] < cold[k] + cold_h[k] {
                err(
                    f,
                    "cold_overlap",
                    format!(
                        "ColdThreshold[{j}] {} overlaps [{k}] {} + hysteresis {}",
                        cold[j], cold[k], cold_h[k]
                    ),
                );
            }
        }
    }

    if let Some(r) = s.get("StepRatio") {
        match hal_float(r) {
            Ok(r) if (0.0..=1.0).contains(&r) => {}
            _ => err(f, "step_ratio", format!("StepRatio {r} must be 0..1")),
        }
        if !s.contains_key("PassiveDelay") {
            err(
                f,
                "step_ratio_delay",
                "StepRatio needs an explicit PassiveDelay".into(),
            );
        }
    }

    if truthy(s, "VirtualSensor") {
        let formula = s.get("Formula").and_then(Value::as_str).unwrap_or("");
        if !FORMULAS.contains(&formula) {
            err(
                f,
                "invalid_formula",
                format!("Formula {formula:?} is not one the HAL knows"),
            );
        }
        let n = len(s, "Combination");
        let model = ["USE_ML_MODEL", "USE_LINEAR_MODEL", "PREVIOUSLY_PREDICTED"].contains(&formula);
        let coef = len(s, "Coefficient");
        if coef == 0 && !["USE_ML_MODEL", "PREVIOUSLY_PREDICTED"].contains(&formula) {
            err(
                f,
                "missing_coefficient",
                "virtual sensor without Coefficient".into(),
            );
        } else if coef != n && !model {
            err(
                f,
                "coefficient_count",
                format!("{coef} coefficients for {n} combined inputs"),
            );
        }
        let c = len(s, "CombinationType");
        if c != 0 && c != n {
            err(
                f,
                "combination_type_count",
                format!("{c} CombinationType entries for {n} inputs"),
            );
        }
        let ct = len(s, "CoefficientType");
        if ct != 0 && ct != coef {
            err(
                f,
                "coefficient_type_count",
                format!("{ct} CoefficientType entries for {coef} coefficients"),
            );
        }
    }

    if let Some(Value::Array(binds)) = s.get("BindedCdevInfo") {
        for b in binds.iter().filter_map(Value::as_object) {
            let cdev = b.get("CdevRequest").and_then(Value::as_str).unwrap_or("?");
            if b.contains_key("CdevCeiling") && b.contains_key("CdevCeilingFrequency") {
                err(
                    f,
                    "ceiling_conflict",
                    format!("{cdev}: CdevCeiling and CdevCeilingFrequency are exclusive"),
                );
            }
            if b.contains_key("LimitInfo") && b.contains_key("LimitInfoFrequency") {
                err(
                    f,
                    "limit_conflict",
                    format!("{cdev}: LimitInfo and LimitInfoFrequency are exclusive"),
                );
            }
            if !cdevs.contains(cdev) {
                f.push(Finding::new(
                    Severity::Info,
                    "cdev_not_in_file",
                    loc.clone(),
                    format!("cooling device {cdev:?} is not defined in this file (it may come from an Include)"),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic config in the HAL's format (not copied from a device).
    const GOOD: &str = r#"{
      "Sensors": [
        {"Name": "skin", "Type": "SKIN", "HotThreshold": ["NaN", 39.0, 43.0, 45.0, 46.5, 52.0, 55.0],
         "HotHysteresis": [0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], "Multiplier": 0.001, "Monitor": true,
         "PassiveDelay": 7000, "StepRatio": 0.5,
         "BindedCdevInfo": [{"CdevRequest": "fan", "CdevCeiling": [0,1,2,3,4,5,6]}]},
        {"Name": "virt", "Type": "SKIN", "VirtualSensor": true, "Formula": "WEIGHTED_AVG",
         "Combination": ["a", "b"], "Coefficient": [0.5, "0.5"], "Version": "1.0"},
        {"Name": "off", "Type": "NOT_A_TYPE", "isDisabled": true}
      ],
      "CoolingDevices": [{"Name": "fan", "Type": "FAN"}],
      "Comment": "x"
    }"#;

    fn codes(t: &str) -> Vec<String> {
        PixelConfig::parse(t)
            .unwrap()
            .validate()
            .into_iter()
            .filter(|f| f.severity >= Severity::Warning)
            .map(|f| f.code)
            .collect()
    }

    #[test]
    fn valid_file_and_round_trip() {
        assert_eq!(codes(GOOD), Vec::<String>::new());
        let c = PixelConfig::parse(GOOD).unwrap();
        let rt = c.round_trip(GOOD);
        assert!(rt.model_equal && !rt.byte_exact);
        assert_eq!(c.summary()["sensors"], 3);
        let facts = c.safety_facts();
        assert_eq!(facts.len(), 3);
        assert_eq!(facts[2].description, "SHUTDOWN severity at 55");
        // Key order is preserved.
        assert!(
            c.serialize().find("\"Sensors\"").unwrap()
                < c.serialize().find("\"CoolingDevices\"").unwrap()
        );
    }

    #[test]
    fn strictness() {
        assert!(PixelConfig::parse(r#"{"Sensors": [], "Sensors": []}"#)
            .unwrap_err()
            .contains("duplicate key"));
        assert!(PixelConfig::parse("{\n// comment\n\"Sensors\": []}").is_err());
        assert!(PixelConfig::parse("[]").is_err());
        assert!(hal_float(&json!("NaN")).unwrap().is_nan());
        assert_eq!(hal_float(&json!(" 38.8")).unwrap(), 38.8);
        assert!(hal_float(&json!("hot")).is_err());
    }

    #[test]
    fn hal_rejections() {
        let one = |sensor: &str| codes(&format!(r#"{{"Sensors": [{sensor}]}}"#));
        assert_eq!(
            one(r#"{"Name": "a", "Type": "SKIN", "HotThreshold": [1, 2]}"#),
            ["threshold_count"]
        );
        assert_eq!(
            one(
                r#"{"Name": "a", "Type": "SKIN", "HotThreshold": ["NaN", 50, 40, "NaN", "NaN", "NaN", "NaN"]}"#
            ),
            ["hot_order", "hot_overlap"]
        ); // the HAL stops at the first; all are reported here
        assert_eq!(
            one(
                r#"{"Name": "a", "Type": "SKIN", "HotThreshold": [0, 50, "NaN", "NaN", "NaN", "NaN", "NaN"]}"#
            ),
            ["hot_order"]
        );
        assert_eq!(
            one(
                r#"{"Name": "a", "Type": "SKIN", "HotThreshold": ["NaN", 40, 41, "NaN", "NaN", "NaN", "NaN"], "HotHysteresis": [0, 0, 2, 0, 0, 0, 0]}"#
            ),
            ["hot_overlap"]
        );
        assert_eq!(
            one(
                r#"{"Name": "a", "Type": "SKIN", "ColdThreshold": [10, 20, "NaN", "NaN", "NaN", "NaN", "NaN"]}"#
            ),
            ["cold_order", "cold_overlap"]
        );
        assert_eq!(
            one(r#"{"Name": "a", "Type": "SKIN", "HotHysteresis": [0, "NaN", 0, 0, 0, 0, 0]}"#),
            ["nan_hysteresis"]
        );
        assert_eq!(one(r#"{"Name": "a", "Type": "skin"}"#), ["invalid_type"]);
        assert_eq!(
            one(r#"{"Name": "a", "Type": "SKIN", "Hidden": true, "SendCallback": true}"#),
            ["hidden_and_monitor"]
        );
        assert_eq!(
            one(r#"{"Name": "a", "Type": "SKIN", "StepRatio": 2, "PassiveDelay": 1}"#),
            ["step_ratio"]
        );
        assert_eq!(
            one(r#"{"Name": "a", "Type": "SKIN", "StepRatio": 0.5}"#),
            ["step_ratio_delay"]
        );
        assert_eq!(
            one(
                r#"{"Name": "a", "Type": "SKIN", "VirtualSensor": true, "Formula": "MAXIMUM", "Combination": ["x", "y"], "Coefficient": [1]}"#
            ),
            ["coefficient_count"]
        );
        assert_eq!(
            one(
                r#"{"Name": "a", "Type": "SKIN", "BindedCdevInfo": [{"CdevRequest": "c", "LimitInfo": [], "LimitInfoFrequency": []}]}"#
            ),
            ["limit_conflict"]
        );
        assert_eq!(
            codes(r#"{"Sensors": [{"Name": "a", "Type": "SKIN"}, {"Name": "a", "Type": "CPU"}]}"#),
            ["duplicate_name"]
        );
        assert_eq!(
            codes(r#"{"CoolingDevices": [{"Name": "c", "Type": "HEATER"}]}"#),
            ["invalid_type"]
        );
    }
}
