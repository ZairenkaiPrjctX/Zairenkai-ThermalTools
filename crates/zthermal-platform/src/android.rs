//! Android framework diagnostics: `dumpsys thermalservice` and `dumpsys battery`.
//!
//! Both need the shell or root (android.permission.DUMP); an ordinary app cannot run them,
//! and that is reported as a capability limit, not hidden. Parsing is tolerant: unknown lines
//! are ignored, and a section that is absent is reported as absent.
//!
//! Formats (AOSP frameworks/base): ThermalManagerService.dump prints "Thermal Status: N" and
//! sections such as "Current temperatures from HAL:" with one `Temperature{...}` per line;
//! `Temperature.toString()` prints `mValue`, `mType` (TemperatureType), `mName` and `mStatus`
//! (ThrottlingSeverity). `mValue` is degrees Celsius except for the BCL types, which carry
//! millivolts (BCL_VOLTAGE), milliamps (BCL_CURRENT) or percent (BCL_PERCENTAGE) and are
//! never reported as temperatures. BatteryService.dump prints "  temperature: N" in tenths
//! of a degree Celsius.

use serde::{Deserialize, Serialize};
use zthermal_core::capability::ThrottlingSeverity;
use zthermal_core::reading::{classify, Classification, DataStatus, TempUnit, Temperature};

/// android.os.Temperature TYPE_* constants (and HAL TemperatureType).
pub fn temperature_type_name(code: i64) -> &'static str {
    match code {
        -1 => "UNKNOWN",
        0 => "CPU",
        1 => "GPU",
        2 => "BATTERY",
        3 => "SKIN",
        4 => "USB_PORT",
        5 => "POWER_AMPLIFIER",
        6 => "BCL_VOLTAGE",
        7 => "BCL_CURRENT",
        8 => "BCL_PERCENTAGE",
        9 => "NPU",
        10 => "TPU",
        11 => "DISPLAY",
        12 => "MODEM",
        13 => "SOC",
        14 => "WIFI",
        15 => "CAMERA",
        16 => "FLASHLIGHT",
        17 => "SPEAKER",
        18 => "AMBIENT",
        19 => "POGO",
        _ => "UNRECOGNIZED",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HalTemperature {
    pub name: String,
    pub type_code: Option<i64>,
    pub type_name: String,
    pub classification: Classification,
    /// Degrees Celsius as reported by the HAL, normalized to millidegrees. Unsupported for
    /// BCL types, whose value is in `bcl`.
    pub value: Temperature,
    /// Value and unit of a BCL_VOLTAGE / BCL_CURRENT / BCL_PERCENTAGE entry.
    pub bcl: Option<BclValue>,
    /// The HAL's own severity for this sensor (framework assessment, not a temperature).
    pub severity: Option<ThrottlingSeverity>,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BclValue {
    pub raw: String,
    pub unit: String,
}

/// Unit of a BCL TemperatureType (AIDL Temperature.aidl), `None` for temperature types.
pub fn bcl_unit(type_code: i64) -> Option<&'static str> {
    match type_code {
        6 => Some("mV"),
        7 => Some("mA"),
        8 => Some("%"),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HalCoolingDevice {
    pub name: String,
    pub type_code: Option<i64>,
    pub value: Option<i64>,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThermalServiceDump {
    pub status: Option<ThrottlingSeverity>,
    pub status_raw: Option<String>,
    pub status_override: Option<bool>,
    pub hal_ready: Option<bool>,
    /// The line under "HAL connection:", e.g. "ThermalHAL AIDL 2  connected: yes".
    pub hal_connection: Option<String>,
    /// "Current temperatures from HAL:" (falls back to "Cached temperatures:").
    pub temperatures: Vec<HalTemperature>,
    pub temperatures_section: Option<String>,
    pub cooling_devices: Vec<HalCoolingDevice>,
    pub unparsed_entries: usize,
}

/// Fields of `Name{k=v, k=v}`. Values may contain spaces but not ", m".
fn fields(entry: &str) -> Option<Vec<(String, String)>> {
    let open = entry.find('{')?;
    let body = entry[open + 1..].strip_suffix('}')?;
    Some(
        body.split(", ")
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect(),
    )
}

fn field<'a>(f: &'a [(String, String)], key: &str) -> Option<&'a str> {
    f.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn temperature(entry: &str) -> Option<HalTemperature> {
    let f = fields(entry)?;
    let name = field(&f, "mName")?.to_string();
    let type_code = field(&f, "mType").and_then(|t| t.parse().ok());
    let raw_value = field(&f, "mValue");
    let bcl = type_code.and_then(bcl_unit).map(|unit| BclValue {
        raw: raw_value.unwrap_or("").into(),
        unit: unit.into(),
    });
    let value = match (raw_value, &bcl) {
        (_, Some(b)) => Temperature::missing(
            TempUnit::Celsius,
            DataStatus::Unsupported,
            format!("BCL entry: the value is in {}, not a temperature", b.unit),
        ),
        (Some(v), None) => Temperature::parse(v, TempUnit::Celsius),
        (None, None) => {
            Temperature::missing(TempUnit::Celsius, DataStatus::ParseError, "no mValue")
        }
    };
    Some(HalTemperature {
        classification: classify(&name),
        type_name: type_code
            .map(temperature_type_name)
            .unwrap_or("UNRECOGNIZED")
            .to_string(),
        severity: field(&f, "mStatus")
            .and_then(|s| s.parse().ok())
            .and_then(ThrottlingSeverity::from_code),
        type_code,
        value,
        bcl,
        name,
        raw: entry.to_string(),
    })
}

fn cooling(entry: &str) -> Option<HalCoolingDevice> {
    let f = fields(entry)?;
    Some(HalCoolingDevice {
        name: field(&f, "mName")?.to_string(),
        type_code: field(&f, "mType").and_then(|t| t.parse().ok()),
        value: field(&f, "mValue").and_then(|v| v.parse().ok()),
        raw: entry.to_string(),
    })
}

pub fn parse_thermalservice(text: &str) -> ThermalServiceDump {
    let mut d = ThermalServiceDump {
        status: None,
        status_raw: None,
        status_override: None,
        hal_ready: None,
        hal_connection: None,
        temperatures: vec![],
        temperatures_section: None,
        cooling_devices: vec![],
        unparsed_entries: 0,
    };
    let mut section = String::new();
    let mut cached = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("Thermal Status:") {
            d.status_raw = Some(v.trim().to_string());
            d.status = v
                .trim()
                .parse()
                .ok()
                .and_then(ThrottlingSeverity::from_code);
        } else if let Some(v) = t.strip_prefix("IsStatusOverride:") {
            d.status_override = v.trim().parse().ok();
        } else if let Some(v) = t.strip_prefix("HAL Ready:") {
            d.hal_ready = v.trim().parse().ok();
        } else if t.ends_with(':') && !t.contains('{') {
            section = t.trim_end_matches(':').to_string();
        } else if section == "HAL connection" && !t.is_empty() && d.hal_connection.is_none() {
            d.hal_connection = Some(t.to_string());
        } else if t.starts_with("Temperature{") {
            match temperature(t) {
                Some(temp) if section == "Current temperatures from HAL" => {
                    d.temperatures.push(temp)
                }
                Some(temp) if section == "Cached temperatures" => cached.push(temp),
                Some(_) => {}
                None => d.unparsed_entries += 1,
            }
        } else if t.starts_with("CoolingDevice{") && section.starts_with("Current cooling devices")
        {
            match cooling(t) {
                Some(c) => d.cooling_devices.push(c),
                None => d.unparsed_entries += 1,
            }
        }
    }
    if !d.temperatures.is_empty() {
        d.temperatures_section = Some("Current temperatures from HAL".into());
    } else if !cached.is_empty() {
        d.temperatures = cached;
        d.temperatures_section = Some("Cached temperatures".into());
    }
    d
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatteryDump {
    pub present: Option<bool>,
    pub temperature: Temperature,
    pub status_code: Option<i64>,
    pub health_code: Option<i64>,
    pub ac_powered: Option<bool>,
    pub usb_powered: Option<bool>,
    pub wireless_powered: Option<bool>,
}

pub fn parse_battery(text: &str) -> BatteryDump {
    let get = |key: &str| {
        text.lines().find_map(|l| {
            l.trim()
                .strip_prefix(key)
                .and_then(|r| r.strip_prefix(':'))
                .map(|v| v.trim().to_string())
        })
    };
    BatteryDump {
        present: get("present").and_then(|v| v.parse().ok()),
        temperature: match get("temperature") {
            Some(v) => Temperature::parse(&v, TempUnit::DeciCelsius),
            None => Temperature::missing(
                TempUnit::DeciCelsius,
                DataStatus::Unsupported,
                "no temperature line",
            ),
        },
        status_code: get("status").and_then(|v| v.parse().ok()),
        health_code: get("health").and_then(|v| v.parse().ok()),
        ac_powered: get("AC powered").and_then(|v| v.parse().ok()),
        usb_powered: get("USB powered").and_then(|v| v.parse().ok()),
        wireless_powered: get("Wireless powered").and_then(|v| v.parse().ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic text in the AOSP dump format (not captured from a device).
    const THERMAL: &str = "IsStatusOverride: false
ThermalEventListeners:
\tcallbacks: 2
Thermal Status: 1
Cached temperatures:
\tTemperature{mValue=30.0, mType=3, mName=skin, mStatus=0}
HAL Ready: true
HAL connection:
\tThermalHAL AIDL 2  connected: yes
Current temperatures from HAL:
\tTemperature{mValue=41.5, mType=0, mName=cpu-1-0, mStatus=0}
\tTemperature{mValue=33.25, mType=3, mName=quiet_therm, mStatus=1}
\tTemperature{mValue=NaN, mType=2, mName=battery, mStatus=0}
\tTemperature{mValue=3412.0, mType=6, mName=vbat, mStatus=0}
\tTemperature{garbage
Current cooling devices from HAL:
\tCoolingDevice{mValue=0, mType=2, mName=cpu-cluster0}
";

    #[test]
    fn thermalservice() {
        let d = parse_thermalservice(THERMAL);
        assert_eq!(d.status, Some(ThrottlingSeverity::Light));
        assert_eq!((d.status_override, d.hal_ready), (Some(false), Some(true)));
        assert_eq!(
            d.hal_connection.as_deref(),
            Some("ThermalHAL AIDL 2  connected: yes")
        );
        assert_eq!(
            d.temperatures_section.as_deref(),
            Some("Current temperatures from HAL")
        );
        assert_eq!(d.temperatures.len(), 4);
        let vbat = &d.temperatures[3];
        assert_eq!(vbat.value.millicelsius, None); // 3412 mV is not 3412 degrees
        assert_eq!(vbat.value.status, DataStatus::Unsupported);
        assert_eq!(
            vbat.bcl.as_ref().map(|b| (b.raw.as_str(), b.unit.as_str())),
            Some(("3412.0", "mV"))
        );
        assert_eq!(d.temperatures[0].value.millicelsius, Some(41_500));
        assert_eq!(d.temperatures[0].type_name, "CPU");
        assert_eq!(d.temperatures[1].severity, Some(ThrottlingSeverity::Light));
        assert_eq!(d.temperatures[2].value.status, DataStatus::ParseError); // NaN is not a reading
        assert_eq!(d.unparsed_entries, 1);
        assert_eq!(d.cooling_devices[0].name, "cpu-cluster0");
    }

    #[test]
    fn cached_section_is_a_fallback_and_absence_is_reported() {
        let d = parse_thermalservice("Thermal Status: 0\nCached temperatures:\n\tTemperature{mValue=30.0, mType=3, mName=skin, mStatus=0}\n");
        assert_eq!(
            d.temperatures_section.as_deref(),
            Some("Cached temperatures")
        );
        let d = parse_thermalservice("Permission Denial: can't dump thermalservice from pid=1\n");
        assert_eq!((d.status, d.temperatures.len()), (None, 0));
        assert!(d.temperatures_section.is_none());
        assert_eq!(parse_thermalservice("Thermal Status: 42").status, None);
    }

    #[test]
    fn battery() {
        let b = parse_battery("Current Battery Service state:\n  AC powered: false\n  USB powered: true\n  status: 2\n  health: 2\n  present: true\n  temperature: 312\n");
        assert_eq!(b.temperature.millicelsius, Some(31_200));
        assert_eq!(
            (b.present, b.usb_powered, b.status_code),
            (Some(true), Some(true), Some(2))
        );
        let none = parse_battery("");
        assert_eq!(none.temperature.status, DataStatus::Unsupported);
    }
}
