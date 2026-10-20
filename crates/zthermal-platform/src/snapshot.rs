//! One read-only snapshot of everything the toolkit can learn about a device, and its
//! redaction. A snapshot stays on the device or host unless the user moves it.

use serde::{Deserialize, Serialize};
use zthermal_core::capability::{Capability, ThrottlingSeverity};
use zthermal_core::config::sha256_hex;
use zthermal_core::identity::DeviceIdentity;
use zthermal_core::reading::DataStatus;

use crate::adapters::{probe_all, AdapterReport, Context};
use crate::android::{BatteryDump, ThermalServiceDump};
use crate::device::{self, SourceStatus};
use crate::exec::Runner;
use crate::sysfs::{discover, SysfsInventory};
use crate::sysroot::SysRoot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hottest {
    pub source: String,
    pub name: String,
    pub millicelsius: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub zones: usize,
    pub zones_valid: usize,
    pub zones_invalid: usize,
    pub cooling_devices: usize,
    pub hottest_zone: Option<Hottest>,
    pub framework_status: Option<ThrottlingSeverity>,
    pub battery_millicelsius: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub tool_version: String,
    pub generated_unix_ms: u64,
    /// `device` for the live system, `fixture` for a `--root` directory.
    pub root: String,
    pub redacted: bool,
    pub identity: DeviceIdentity,
    pub sources: Vec<SourceStatus>,
    pub summary: Summary,
    pub sysfs: SysfsInventory,
    pub thermalservice: Option<ThermalServiceDump>,
    pub battery: Option<BatteryDump>,
    pub adapters: Vec<AdapterReport>,
    pub capabilities: Vec<Capability>,
}

pub struct Options {
    /// Run `dumpsys` (needs the shell or root).
    pub framework: bool,
    pub now_ms: u64,
}

fn summarize(
    inv: &SysfsInventory,
    ts: Option<&ThermalServiceDump>,
    bat: Option<&BatteryDump>,
) -> Summary {
    let valid: Vec<_> = inv
        .zones
        .iter()
        .filter(|z| z.temp.status == DataStatus::Valid)
        .collect();
    let hottest = valid
        .iter()
        .filter_map(|z| z.temp.millicelsius.map(|m| (z, m)))
        .max_by_key(|(_, m)| *m)
        .map(|(z, m)| Hottest {
            source: z.path.clone(),
            name: z.zone_type.value.clone().unwrap_or_else(|| "?".into()),
            millicelsius: m,
        });
    Summary {
        zones: inv.zones.len(),
        zones_valid: valid.len(),
        zones_invalid: inv.zones.len() - valid.len(),
        cooling_devices: inv.cooling_devices.len(),
        hottest_zone: hottest,
        framework_status: ts.and_then(|t| t.status),
        battery_millicelsius: bat.and_then(|b| b.temperature.millicelsius),
    }
}

pub fn collect(root: &SysRoot, runner: &dyn Runner, opts: &Options) -> Snapshot {
    let (props, prop_status) = device::read_props(runner);
    let identity = device::identity(root, &props);
    let sysfs = discover(root);
    let mut sources = vec![prop_status];
    let (thermalservice, battery) = if opts.framework {
        let (t, ts) = device::thermalservice(runner);
        let (b, bs) = device::battery(runner);
        sources.extend([ts, bs]);
        (t, b)
    } else {
        sources.push(SourceStatus {
            source: "dumpsys".into(),
            status: DataStatus::Unknown,
            detail: Some("not requested (--framework)".into()),
        });
        (None, None)
    };
    let ctx = Context {
        root,
        props: &props,
        identity: &identity,
        inventory: &sysfs,
    };
    let adapters = probe_all(&ctx);
    let capabilities = adapters
        .iter()
        .flat_map(|a| a.capabilities.iter().cloned())
        .collect();
    Snapshot {
        schema_version: 1,
        tool_version: env!("CARGO_PKG_VERSION").into(),
        generated_unix_ms: opts.now_ms,
        root: if root.is_real() {
            "device".into()
        } else {
            "fixture".into()
        },
        redacted: false,
        summary: summarize(&sysfs, thermalservice.as_ref(), battery.as_ref()),
        identity,
        sources,
        sysfs,
        thermalservice,
        battery,
        adapters,
        capabilities,
    }
}

fn hide(v: &mut Option<String>) {
    if let Some(s) = v {
        *s = format!("redacted:sha256:{}", &sha256_hex(s.as_bytes())[..12]);
    }
}

impl Snapshot {
    /// Replaces build fingerprints with a short hash (still comparable between snapshots).
    /// Serial numbers and IMEIs are never read in the first place.
    pub fn redact(&mut self) {
        hide(&mut self.identity.build_fingerprint);
        hide(&mut self.identity.vendor_fingerprint);
        self.redacted = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::FakeRunner;
    use crate::sysroot::testutil::TempDir;

    fn fixture() -> TempDir {
        let t = TempDir::new("snap");
        t.file("sys/class/thermal/thermal_zone0/type", "cpu-0-0-usr\n");
        t.file("sys/class/thermal/thermal_zone0/temp", "41000\n");
        t.file("sys/class/thermal/thermal_zone1/type", "skin-therm\n");
        t.file("sys/class/thermal/thermal_zone1/temp", "-274000\n");
        t.file("proc/sys/kernel/osrelease", "5.10.0-test\n");
        t
    }

    #[test]
    fn collects_without_framework_and_redacts() {
        let t = fixture();
        let r = FakeRunner::default().with("getprop", "[ro.board.platform]: [taro]\n[ro.build.fingerprint]: [google/x/y:14/AB/1:user/release-keys]\n");
        let mut s = collect(
            &SysRoot::at(t.path()),
            &r,
            &Options {
                framework: false,
                now_ms: 5,
            },
        );
        assert_eq!(s.root, "fixture");
        assert_eq!((s.summary.zones, s.summary.zones_valid), (2, 1));
        assert_eq!(
            s.summary.hottest_zone.as_ref().unwrap().millicelsius,
            41_000
        );
        assert!(s.thermalservice.is_none());
        assert!(s
            .adapters
            .iter()
            .any(|a| a.adapter == "qualcomm" && a.applies));
        s.redact();
        assert!(s
            .identity
            .build_fingerprint
            .as_deref()
            .unwrap()
            .starts_with("redacted:sha256:"));
        assert_eq!(s.identity.kernel_release.as_deref(), Some("5.10.0-test"));
    }

    #[test]
    fn framework_failure_is_a_status_not_a_value() {
        let t = fixture();
        let s = collect(
            &SysRoot::at(t.path()),
            &FakeRunner::default(),
            &Options {
                framework: true,
                now_ms: 0,
            },
        );
        assert!(s.thermalservice.is_none() && s.battery.is_none());
        assert!(s
            .sources
            .iter()
            .filter(|x| x.source.starts_with("dumpsys"))
            .all(|x| x.status == DataStatus::Unsupported));
        assert_eq!(s.summary.battery_millicelsius, None);
    }
}
