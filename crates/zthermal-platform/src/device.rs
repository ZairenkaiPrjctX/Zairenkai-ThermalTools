//! Device facts: Android properties, kernel release, framework dumps.
//!
//! Every source can be missing. A failure is recorded with its reason and the matching
//! data status; nothing is substituted for a value that could not be read.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zthermal_core::identity::{parse_getprop, DeviceIdentity};
use zthermal_core::reading::DataStatus;

use crate::android::{parse_battery, parse_thermalservice, BatteryDump, ThermalServiceDump};
use crate::exec::{ExecError, Runner};
use crate::sysroot::SysRoot;

pub const PROP_TIMEOUT: Duration = Duration::from_secs(5);
pub const DUMPSYS_TIMEOUT: Duration = Duration::from_secs(10);

/// How one external source went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub source: String,
    pub status: DataStatus,
    pub detail: Option<String>,
}

impl SourceStatus {
    pub fn ok(source: &str) -> Self {
        Self {
            source: source.into(),
            status: DataStatus::Valid,
            detail: None,
        }
    }

    pub fn from_exec(source: &str, e: &ExecError) -> Self {
        let (status, detail) = match e {
            ExecError::NotFound(d) => (DataStatus::Unsupported, d),
            ExecError::Failed(d) => (DataStatus::PermissionDenied, d),
            ExecError::TimedOut(d) | ExecError::OutputTooLarge(d) => (DataStatus::Unavailable, d),
        };
        Self {
            source: source.into(),
            status,
            detail: Some(detail.clone()),
        }
    }
}

/// `getprop` output, or an empty map with the reason.
pub fn read_props(runner: &dyn Runner) -> (BTreeMap<String, String>, SourceStatus) {
    match runner.run(&["getprop"], PROP_TIMEOUT) {
        Ok(out) => (parse_getprop(&out), SourceStatus::ok("getprop")),
        Err(e) => (BTreeMap::new(), SourceStatus::from_exec("getprop", &e)),
    }
}

pub fn kernel_release(root: &SysRoot) -> Option<String> {
    root.read_attr("/proc/sys/kernel/osrelease")
        .ok()
        .filter(|s| !s.is_empty())
}

pub fn identity(root: &SysRoot, props: &BTreeMap<String, String>) -> DeviceIdentity {
    DeviceIdentity::from_props(
        props,
        kernel_release(root),
        Some(std::env::consts::ARCH.to_string()),
    )
}

/// `dumpsys thermalservice` (needs android.permission.DUMP: adb shell or root).
pub fn thermalservice(runner: &dyn Runner) -> (Option<ThermalServiceDump>, SourceStatus) {
    let src = "dumpsys thermalservice";
    match runner.run(&["dumpsys", "thermalservice"], DUMPSYS_TIMEOUT) {
        Ok(out) if out.contains("Permission Denial") => (
            None,
            SourceStatus {
                source: src.into(),
                status: DataStatus::PermissionDenied,
                detail: Some("Permission Denial".into()),
            },
        ),
        Ok(out) => {
            let d = parse_thermalservice(&out);
            let status = if d.status_raw.is_none() && d.temperatures.is_empty() {
                SourceStatus {
                    source: src.into(),
                    status: DataStatus::ParseError,
                    detail: Some("no recognizable section".into()),
                }
            } else {
                SourceStatus::ok(src)
            };
            (Some(d), status)
        }
        Err(e) => (None, SourceStatus::from_exec(src, &e)),
    }
}

/// `dumpsys battery`.
pub fn battery(runner: &dyn Runner) -> (Option<BatteryDump>, SourceStatus) {
    let src = "dumpsys battery";
    match runner.run(&["dumpsys", "battery"], DUMPSYS_TIMEOUT) {
        Ok(out) if out.contains("Permission Denial") => (
            None,
            SourceStatus {
                source: src.into(),
                status: DataStatus::PermissionDenied,
                detail: Some("Permission Denial".into()),
            },
        ),
        Ok(out) => {
            let b = parse_battery(&out);
            if out.contains("Current Battery Service state") || b.temperature.raw.is_some() {
                (Some(b), SourceStatus::ok(src))
            } else {
                (
                    Some(b),
                    SourceStatus {
                        source: src.into(),
                        status: DataStatus::ParseError,
                        detail: Some("no BatteryService state in the output".into()),
                    },
                )
            }
        }
        Err(e) => (None, SourceStatus::from_exec(src, &e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::FakeRunner;

    #[test]
    fn missing_tools_are_reported_not_faked() {
        let r = FakeRunner::default();
        let (props, st) = read_props(&r);
        assert!(props.is_empty());
        assert_eq!(st.status, DataStatus::Unsupported);
        let (d, st) = thermalservice(&r);
        assert!(d.is_none());
        assert_eq!(st.status, DataStatus::Unsupported);
    }

    #[test]
    fn permission_denial_is_recognized() {
        let r = FakeRunner::default()
            .with(
                "dumpsys thermalservice",
                "Permission Denial: can't dump thermalservice from pid=1, uid=10000\n",
            )
            .with(
                "dumpsys battery",
                "Current Battery Service state:\n  temperature: 305\n",
            )
            .with("getprop", "[ro.product.brand]: [google]\n");
        assert_eq!(thermalservice(&r).1.status, DataStatus::PermissionDenied);
        assert_eq!(
            battery(&r).0.unwrap().temperature.millicelsius,
            Some(30_500)
        );
        assert_eq!(
            read_props(&r).0.get("ro.product.brand").map(String::as_str),
            Some("google")
        );
        let r =
            FakeRunner::default().failing("dumpsys battery", ExecError::Failed("exit 1".into()));
        assert_eq!(battery(&r).1.status, DataStatus::PermissionDenied);
    }

    #[test]
    fn malformed_framework_output_is_a_parse_error() {
        let r = FakeRunner::default()
            .with(
                "dumpsys thermalservice",
                "Can't find service: thermalservice\n",
            )
            .with("dumpsys battery", "garbage\n");
        assert_eq!(thermalservice(&r).1.status, DataStatus::ParseError);
        assert_eq!(battery(&r).1.status, DataStatus::ParseError);
    }
}
