//! Linux thermal sysfs discovery: thermal zones, cooling devices and hwmon sensors.
//!
//! Read-only. Units follow the kernel ABI: `thermal_zoneN/temp` and `trip_point_N_temp` are
//! millidegrees Celsius (Documentation/ABI/testing/sysfs-class-thermal), hwmon
//! `temp*_input` is millidegrees Celsius (Documentation/hwmon/sysfs-interface). A zone's
//! index is only a directory name: nothing is inferred from it.

use serde::{Deserialize, Serialize};
use zthermal_core::reading::{classify, Classification, DataStatus, TempUnit, Temperature};

use crate::sysroot::{NodeMode, ReadError, SysRoot};

pub const THERMAL_CLASS: &str = "/sys/class/thermal";
pub const HWMON_CLASS: &str = "/sys/class/hwmon";
/// Upper bound on entries examined per directory (a malformed sysfs cannot make us loop).
pub const MAX_ENTRIES: usize = 512;
const MAX_TRIPS: usize = 64;

/// A text attribute and how reading it went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attr {
    pub value: Option<String>,
    pub status: DataStatus,
    pub detail: Option<String>,
}

impl Attr {
    fn read(root: &SysRoot, path: &str) -> Self {
        match root.read_attr(path) {
            Ok(v) => Self {
                value: Some(v),
                status: DataStatus::Valid,
                detail: None,
            },
            Err(ReadError { status, detail }) => Self {
                value: None,
                status,
                detail: Some(detail),
            },
        }
    }

    fn int(&self) -> Option<i64> {
        self.value.as_deref().and_then(|v| v.parse().ok())
    }
}

fn read_temp(root: &SysRoot, path: &str) -> Temperature {
    match root.read_attr(path) {
        Ok(v) => Temperature::parse(&v, TempUnit::MilliCelsius),
        Err(e) => Temperature::missing(TempUnit::MilliCelsius, e.status, e.detail),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TripPoint {
    pub index: usize,
    pub kind: Attr,
    pub temp: Temperature,
    /// Hysteresis is a temperature difference, also in millidegrees.
    pub hyst: Option<Temperature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CdevBinding {
    /// The `cdevN` attribute directory in the zone.
    pub link: String,
    /// The cooling device it points to (`cooling_deviceM`), when the link resolves.
    pub cooling_device: Option<String>,
    pub trip_point: Option<i64>,
    pub weight: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThermalZone {
    pub path: String,
    pub zone_type: Attr,
    pub classification: Classification,
    pub temp: Temperature,
    pub temp_mode: Option<NodeMode>,
    pub mode: Attr,
    pub policy: Attr,
    pub available_policies: Attr,
    pub trips: Vec<TripPoint>,
    pub bindings: Vec<CdevBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoolingDevice {
    pub path: String,
    pub cdev_type: Attr,
    pub max_state: Attr,
    pub cur_state: Attr,
    pub cur_state_mode: Option<NodeMode>,
    /// `stats/total_trans`, when the kernel has CONFIG_THERMAL_STATISTICS.
    pub total_trans: Option<i64>,
    pub has_time_in_state: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HwmonSensor {
    pub input: String,
    pub label: Option<String>,
    pub classification: Classification,
    pub temp: Temperature,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hwmon {
    pub path: String,
    pub name: Attr,
    pub sensors: Vec<HwmonSensor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SysfsInventory {
    pub thermal_class_present: bool,
    pub zones: Vec<ThermalZone>,
    pub cooling_devices: Vec<CoolingDevice>,
    pub hwmon: Vec<Hwmon>,
    pub truncated: bool,
}

pub fn discover(root: &SysRoot) -> SysfsInventory {
    let entries = root.list_dir(THERMAL_CLASS);
    let truncated = entries.len() > MAX_ENTRIES;
    let entries: Vec<String> = entries.into_iter().take(MAX_ENTRIES).collect();
    let zones = entries
        .iter()
        .filter(|n| is_indexed(n, "thermal_zone"))
        .map(|n| zone(root, &format!("{THERMAL_CLASS}/{n}")))
        .collect();
    let cooling_devices = entries
        .iter()
        .filter(|n| is_indexed(n, "cooling_device"))
        .map(|n| cooling(root, &format!("{THERMAL_CLASS}/{n}")))
        .collect();
    let hwmon = root
        .list_dir(HWMON_CLASS)
        .into_iter()
        .take(MAX_ENTRIES)
        .filter(|n| is_indexed(n, "hwmon"))
        .map(|n| hwmon(root, &format!("{HWMON_CLASS}/{n}")))
        .collect();
    SysfsInventory {
        thermal_class_present: root.is_dir(THERMAL_CLASS),
        zones,
        cooling_devices,
        hwmon,
        truncated,
    }
}

fn is_indexed(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()))
}

fn zone(root: &SysRoot, dir: &str) -> ThermalZone {
    let zone_type = Attr::read(root, &format!("{dir}/type"));
    let classification = classify(zone_type.value.as_deref().unwrap_or(""));
    let mut trips = Vec::new();
    for i in 0..MAX_TRIPS {
        let temp_path = format!("{dir}/trip_point_{i}_temp");
        if !root.exists(&temp_path) {
            break;
        }
        let hyst_path = format!("{dir}/trip_point_{i}_hyst");
        trips.push(TripPoint {
            index: i,
            kind: Attr::read(root, &format!("{dir}/trip_point_{i}_type")),
            temp: read_temp(root, &temp_path),
            hyst: root.exists(&hyst_path).then(|| read_temp(root, &hyst_path)),
        });
    }
    let bindings = root
        .list_dir(dir)
        .into_iter()
        .filter(|n| is_indexed(n, "cdev"))
        .take(MAX_ENTRIES)
        .map(|n| {
            let link = format!("{dir}/{n}");
            CdevBinding {
                cooling_device: root.link_name(&link),
                trip_point: Attr::read(root, &format!("{dir}/{n}_trip_point")).int(),
                weight: Attr::read(root, &format!("{dir}/{n}_weight")).int(),
                link: n,
            }
        })
        .collect();
    ThermalZone {
        path: dir.to_string(),
        classification,
        temp: read_temp(root, &format!("{dir}/temp")),
        temp_mode: root.mode(&format!("{dir}/temp")),
        mode: Attr::read(root, &format!("{dir}/mode")),
        policy: Attr::read(root, &format!("{dir}/policy")),
        available_policies: Attr::read(root, &format!("{dir}/available_policies")),
        trips,
        bindings,
        zone_type,
    }
}

fn cooling(root: &SysRoot, dir: &str) -> CoolingDevice {
    CoolingDevice {
        path: dir.to_string(),
        cdev_type: Attr::read(root, &format!("{dir}/type")),
        max_state: Attr::read(root, &format!("{dir}/max_state")),
        cur_state: Attr::read(root, &format!("{dir}/cur_state")),
        cur_state_mode: root.mode(&format!("{dir}/cur_state")),
        total_trans: Attr::read(root, &format!("{dir}/stats/total_trans")).int(),
        has_time_in_state: root.exists(&format!("{dir}/stats/time_in_state_ms")),
    }
}

fn hwmon(root: &SysRoot, dir: &str) -> Hwmon {
    let sensors = root
        .list_dir(dir)
        .into_iter()
        .filter(|n| n.starts_with("temp") && n.ends_with("_input"))
        .take(MAX_ENTRIES)
        .map(|n| {
            let stem = n.trim_end_matches("_input");
            let label = Attr::read(root, &format!("{dir}/{stem}_label")).value;
            HwmonSensor {
                classification: classify(label.as_deref().unwrap_or("")),
                temp: read_temp(root, &format!("{dir}/{n}")),
                input: n,
                label,
            }
        })
        .collect();
    Hwmon {
        path: dir.to_string(),
        name: Attr::read(root, &format!("{dir}/name")),
        sensors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysroot::testutil::TempDir;
    use zthermal_core::reading::SensorCategory;

    fn fixture() -> TempDir {
        let d = TempDir::new("sysfs");
        let z = "sys/class/thermal/thermal_zone0";
        d.file(&format!("{z}/type"), "cpu-1-0-usr\n");
        d.file(&format!("{z}/temp"), "48300\n");
        d.file(&format!("{z}/mode"), "enabled\n");
        d.file(&format!("{z}/policy"), "step_wise\n");
        d.file(&format!("{z}/trip_point_0_temp"), "95000\n");
        d.file(&format!("{z}/trip_point_0_type"), "passive\n");
        d.file(&format!("{z}/trip_point_0_hyst"), "5000\n");
        d.file(&format!("{z}/trip_point_1_temp"), "115000\n");
        d.file(&format!("{z}/trip_point_1_type"), "critical\n");
        d.file(&format!("{z}/cdev0/type"), "thermal-cpufreq-0\n");
        d.file(&format!("{z}/cdev0_trip_point"), "0\n");
        d.file(&format!("{z}/cdev0_weight"), "1024\n");
        let z = "sys/class/thermal/thermal_zone10";
        d.file(&format!("{z}/type"), "battery\n");
        d.file(&format!("{z}/temp"), "\n"); // sensor offline
        let z = "sys/class/thermal/thermal_zone2";
        d.file(&format!("{z}/type"), "pm8350_tz\n");
        d.file(&format!("{z}/temp"), "garbage\n");
        d.file(
            "sys/class/thermal/cooling_device0/type",
            "thermal-cpufreq-0\n",
        );
        d.file("sys/class/thermal/cooling_device0/max_state", "17\n");
        d.file("sys/class/thermal/cooling_device0/cur_state", "0\n");
        d.file(
            "sys/class/thermal/cooling_device0/stats/total_trans",
            "12\n",
        );
        d.file(
            "sys/class/thermal/cooling_device0/stats/time_in_state_ms",
            "state0 100\n",
        );
        d.file("sys/class/thermal/thermal_zone_extra/type", "not a zone");
        d.file("sys/class/hwmon/hwmon0/name", "nvme\n");
        d.file("sys/class/hwmon/hwmon0/temp1_input", "38850\n");
        d.file("sys/class/hwmon/hwmon0/temp1_label", "Composite\n");
        d
    }

    #[test]
    fn discovers_zones_trips_bindings_and_cooling_devices() {
        let d = fixture();
        let inv = discover(&SysRoot::at(d.path()));
        assert!(inv.thermal_class_present);
        let paths: Vec<&str> = inv
            .zones
            .iter()
            .map(|z| z.path.rsplit('/').next().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec!["thermal_zone0", "thermal_zone2", "thermal_zone10"]
        ); // natural order, odd names skipped
        let cpu = &inv.zones[0];
        assert_eq!(cpu.temp.millicelsius, Some(48_300));
        assert_eq!(cpu.classification.category, SensorCategory::Cpu);
        assert_eq!(cpu.trips.len(), 2);
        assert_eq!(cpu.trips[1].kind.value.as_deref(), Some("critical"));
        assert_eq!(
            cpu.trips[0].hyst.as_ref().unwrap().millicelsius,
            Some(5_000)
        );
        assert_eq!(cpu.bindings[0].trip_point, Some(0));
        assert_eq!(cpu.bindings[0].weight, Some(1024));
        assert_eq!(cpu.policy.value.as_deref(), Some("step_wise"));
        assert_eq!(
            inv.cooling_devices[0].max_state.value.as_deref(),
            Some("17")
        );
        assert_eq!(inv.cooling_devices[0].total_trans, Some(12));
        assert!(inv.cooling_devices[0].has_time_in_state);
        assert_eq!(inv.hwmon[0].sensors[0].temp.millicelsius, Some(38_850));
        assert_eq!(inv.hwmon[0].sensors[0].label.as_deref(), Some("Composite"));
    }

    #[test]
    fn bad_and_missing_values_keep_their_status() {
        let d = fixture();
        let inv = discover(&SysRoot::at(d.path()));
        let pmic = &inv.zones[1];
        assert_eq!(
            (pmic.temp.status, pmic.temp.millicelsius),
            (DataStatus::ParseError, None)
        );
        assert_eq!(pmic.mode.status, DataStatus::Unsupported); // no mode attribute
        let batt = &inv.zones[2];
        assert_eq!(batt.temp.status, DataStatus::Unavailable);
        assert!(batt.trips.is_empty());
    }

    #[test]
    fn no_thermal_class_is_reported_not_faked() {
        let d = TempDir::new("sysfs-empty");
        let inv = discover(&SysRoot::at(d.path()));
        assert!(!inv.thermal_class_present);
        assert!(inv.zones.is_empty() && inv.cooling_devices.is_empty() && inv.hwmon.is_empty());
    }

    #[test]
    fn symlinked_bindings_resolve_to_cooling_devices() {
        let d = TempDir::new("sysfs-link");
        d.file("sys/devices/virtual/thermal/cooling_device3/type", "gpu\n");
        d.file("sys/class/thermal/thermal_zone0/type", "gpuss-0\n");
        d.file("sys/class/thermal/thermal_zone0/temp", "40000\n");
        std::os::unix::fs::symlink(
            d.path().join("sys/devices/virtual/thermal/cooling_device3"),
            d.path().join("sys/class/thermal/thermal_zone0/cdev1"),
        )
        .unwrap();
        let inv = discover(&SysRoot::at(d.path()));
        assert_eq!(
            inv.zones[0].bindings[0].cooling_device.as_deref(),
            Some("cooling_device3")
        );
    }
}
