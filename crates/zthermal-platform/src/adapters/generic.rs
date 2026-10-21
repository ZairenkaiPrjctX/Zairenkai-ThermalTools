// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Capabilities every Linux/Android device can offer: kernel thermal sysfs, hwmon and the
//! Android framework diagnostics. Controls are never offered here.

use zthermal_core::capability::{Capability, CapabilityState, Privilege, ProbeRecord};
use zthermal_core::policy::validated_controls;
use zthermal_core::reading::DataStatus;

use super::{AdapterReport, Context};
use crate::device::SourceStatus;

/// A framework capability from what was actually observed. Without a probe in this run the
/// state stays `VALIDATION_REQUIRED`, which never implies that the command succeeded.
fn framework_capability(
    cap: Capability,
    android: bool,
    probe: Option<&SourceStatus>,
) -> Capability {
    let Some(p) = probe else {
        return if android {
            cap.with(
                CapabilityState::ValidationRequired,
                Some("not probed in this run (use --framework); needs the shell or root".into()),
            )
        } else {
            cap.with(
                CapabilityState::Unsupported,
                Some("not an Android system (no ro.build.version.sdk)".into()),
            )
        };
    };
    let (state, reason) = match p.status {
        DataStatus::Valid => (CapabilityState::AvailableReadOnly, None),
        DataStatus::Unsupported => (CapabilityState::Unsupported, p.detail.clone()),
        DataStatus::PermissionDenied => (CapabilityState::PermissionDenied, p.detail.clone()),
        DataStatus::ParseError => (
            CapabilityState::Error,
            Some(format!(
                "output not recognized: {}",
                p.detail.clone().unwrap_or_default()
            )),
        ),
        _ => (CapabilityState::TemporarilyUnavailable, p.detail.clone()),
    };
    let mut cap = cap
        .with(state, reason)
        .evidence(format!("probe: {}", p.source));
    cap.probe = Some(ProbeRecord {
        source: p.source.clone(),
        status: p.status,
        detail: p.detail.clone(),
    });
    cap
}

pub fn probe(ctx: &Context<'_>) -> AdapterReport {
    let mut r = AdapterReport::new("generic");
    r.applies = true;
    r.evidence.push("applies to every device".into());
    let inv = ctx.inventory;

    let readable = inv
        .zones
        .iter()
        .filter(|z| z.temp.status == DataStatus::Valid)
        .count();
    let zones = Capability::read(
        "sensors.thermal_zones",
        "read temperatures from /sys/class/thermal/thermal_zone*/temp",
        "Linux thermal sysfs",
        "list /sys/class/thermal",
        "read access to sysfs (granted to apps on most builds; SELinux may deny)",
        Privilege::Unprivileged,
    )
    .evidence(format!(
        "{} zones, {readable} with a valid reading",
        inv.zones.len()
    ));
    r.capabilities.push(match (inv.zones.len(), readable) {
        (0, _) => zones.with(
            CapabilityState::Unsupported,
            Some("no thermal zones".into()),
        ),
        (_, 0)
            if inv
                .zones
                .iter()
                .all(|z| z.temp.status == DataStatus::PermissionDenied) =>
        {
            zones.with(
                CapabilityState::PermissionDenied,
                Some("every zone denied the read".into()),
            )
        }
        (_, 0) => zones.with(
            CapabilityState::TemporarilyUnavailable,
            Some("no zone returned a valid reading".into()),
        ),
        _ => zones.with(CapabilityState::AvailableReadOnly, None),
    });

    let trips: usize = inv.zones.iter().map(|z| z.trips.len()).sum();
    let trip_cap = Capability::read(
        "sensors.trip_points",
        "read trip point types, temperatures and hysteresis (read only; never written)",
        "Linux thermal sysfs",
        "trip_point_*_{type,temp,hyst}",
        "read access to sysfs",
        Privilege::Unprivileged,
    )
    .evidence(format!("{trips} trip points"));
    r.capabilities.push(if trips > 0 {
        trip_cap.with(CapabilityState::AvailableReadOnly, None)
    } else {
        trip_cap.with(
            CapabilityState::Unsupported,
            Some("no trip points exposed".into()),
        )
    });

    let cdev = Capability::read(
        "cooling.state_read",
        "read cooling device type, cur_state and max_state",
        "Linux thermal sysfs",
        "list /sys/class/thermal/cooling_device*",
        "read access to sysfs",
        Privilege::Unprivileged,
    )
    .evidence(format!("{} cooling devices", inv.cooling_devices.len()));
    r.capabilities.push(if inv.cooling_devices.is_empty() {
        cdev.with(
            CapabilityState::Unsupported,
            Some("no cooling devices".into()),
        )
    } else {
        cdev.with(CapabilityState::AvailableReadOnly, None)
    });

    let hw_sensors: usize = inv.hwmon.iter().map(|h| h.sensors.len()).sum();
    let hw = Capability::read(
        "sensors.hwmon",
        "read hwmon temperature inputs",
        "Linux hwmon",
        "list /sys/class/hwmon/hwmon*/temp*_input",
        "read access to sysfs",
        Privilege::Unprivileged,
    )
    .evidence(format!(
        "{} hwmon devices, {hw_sensors} temperature inputs",
        inv.hwmon.len()
    ));
    r.capabilities.push(if hw_sensors > 0 {
        hw.with(CapabilityState::AvailableReadOnly, None)
    } else {
        hw.with(
            CapabilityState::Unsupported,
            Some("no hwmon temperature inputs".into()),
        )
    });

    let android = ctx.prop("ro.build.version.sdk").is_some();
    for (id, desc, discovery) in [
        (
            "framework.thermal_status",
            "Android thermal status, HAL temperatures and cooling devices",
            "dumpsys thermalservice",
        ),
        (
            "framework.battery_temperature",
            "battery temperature from BatteryService",
            "dumpsys battery",
        ),
    ] {
        let cap = Capability::read(
            id,
            desc,
            "Android 10+",
            discovery,
            "android.permission.DUMP (adb shell or root)",
            Privilege::Shell,
        );
        let probe = ctx.framework.map(|f| {
            if id == "framework.thermal_status" {
                &f.thermalservice
            } else {
                &f.battery
            }
        });
        r.capabilities
            .push(framework_capability(cap, android, probe));
    }

    let controls = validated_controls();
    let ctl = Capability {
        access: zthermal_core::capability::Access::Write,
        ..Capability::read(
            "control.thermal_policy",
            "apply a thermal policy through validated, reversible controls",
            "devices with a validated control descriptor",
            "validated control allowlist",
            "root, a validated descriptor matching this device, a hardware test record",
            Privilege::Root,
        )
    };
    r.capabilities.push(if controls.is_empty() {
        ctl.with(
            CapabilityState::Unsupported,
            Some("the validated control allowlist is empty in this release: no control has a hardware test record".into()),
        )
    } else {
        ctl.with(CapabilityState::ValidationRequired, Some("controls exist; device matching happens at plan time".into()))
    });
    r.notes.push(
        "controls are never derived from a writable node, a vendor name or root access".into(),
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::Fixture;
    use crate::adapters::FrameworkProbe;
    use crate::sysroot::testutil::TempDir;

    #[test]
    fn host_without_thermal_sysfs() {
        let t = TempDir::new("generic-empty");
        let fx = Fixture::new(t.path(), &[]);
        let r = probe(&fx.ctx());
        let state = |id: &str| r.capabilities.iter().find(|c| c.id == id).unwrap().state;
        assert_eq!(state("sensors.thermal_zones"), CapabilityState::Unsupported);
        assert_eq!(
            state("framework.thermal_status"),
            CapabilityState::Unsupported
        );
        assert_eq!(
            state("control.thermal_policy"),
            CapabilityState::Unsupported
        );
    }

    #[test]
    fn zones_are_read_only() {
        let t = TempDir::new("generic-zones");
        t.file("sys/class/thermal/thermal_zone0/type", "cpu-0-0\n");
        t.file("sys/class/thermal/thermal_zone0/temp", "41000\n");
        t.file(
            "sys/class/thermal/thermal_zone0/trip_point_0_type",
            "passive\n",
        );
        t.file(
            "sys/class/thermal/thermal_zone0/trip_point_0_temp",
            "95000\n",
        );
        let fx = Fixture::new(t.path(), &[("ro.build.version.sdk", "34")]);
        let r = probe(&fx.ctx());
        let cap = |id: &str| r.capabilities.iter().find(|c| c.id == id).unwrap().clone();
        assert_eq!(
            cap("sensors.thermal_zones").state,
            CapabilityState::AvailableReadOnly
        );
        assert_eq!(
            cap("sensors.trip_points").state,
            CapabilityState::AvailableReadOnly
        );
        assert_eq!(
            cap("framework.thermal_status").state,
            CapabilityState::ValidationRequired
        );
        assert!(r
            .capabilities
            .iter()
            .all(|c| c.state != CapabilityState::AvailableWritable && !c.hardware_tested));
    }

    fn framework_state(fw: Option<FrameworkProbe>, sdk: bool) -> (Capability, Capability) {
        let t = TempDir::new("generic-fw");
        let props: &[(&str, &str)] = if sdk {
            &[("ro.build.version.sdk", "34")]
        } else {
            &[]
        };
        let mut fx = Fixture::new(t.path(), props);
        fx.framework = fw;
        let r = probe(&fx.ctx());
        let get = |id: &str| r.capabilities.iter().find(|c| c.id == id).unwrap().clone();
        (
            get("framework.thermal_status"),
            get("framework.battery_temperature"),
        )
    }

    fn status(source: &str, status: DataStatus, detail: Option<&str>) -> SourceStatus {
        SourceStatus {
            source: source.into(),
            status,
            detail: detail.map(String::from),
        }
    }

    fn both(ts: SourceStatus, bat: SourceStatus) -> Option<FrameworkProbe> {
        Some(FrameworkProbe {
            thermalservice: ts,
            battery: bat,
        })
    }

    #[test]
    fn framework_capabilities_follow_the_probe() {
        let ok = status("dumpsys thermalservice", DataStatus::Valid, None);
        let (t, b) = framework_state(
            both(
                ok.clone(),
                status(
                    "dumpsys battery",
                    DataStatus::PermissionDenied,
                    Some("Permission Denial"),
                ),
            ),
            true,
        );
        assert_eq!(t.state, CapabilityState::AvailableReadOnly);
        assert_eq!(t.probe.as_ref().unwrap().status, DataStatus::Valid);
        assert_eq!(b.state, CapabilityState::PermissionDenied);
        let (t, _) = framework_state(
            both(
                status(
                    "dumpsys thermalservice",
                    DataStatus::Unsupported,
                    Some("dumpsys: not found"),
                ),
                ok.clone(),
            ),
            true,
        );
        assert_eq!(t.state, CapabilityState::Unsupported);
        let (t, _) = framework_state(
            both(
                status(
                    "dumpsys thermalservice",
                    DataStatus::ParseError,
                    Some("no recognizable section"),
                ),
                ok.clone(),
            ),
            true,
        );
        assert_eq!(t.state, CapabilityState::Error);
        assert!(t.reason.unwrap().contains("output not recognized"));
        let (t, _) = framework_state(
            both(
                status(
                    "dumpsys thermalservice",
                    DataStatus::Unavailable,
                    Some("timed out"),
                ),
                ok,
            ),
            true,
        );
        assert_eq!(t.state, CapabilityState::TemporarilyUnavailable);
    }

    #[test]
    fn unprobed_capabilities_say_so() {
        let (t, b) = framework_state(None, true);
        for c in [t, b] {
            assert_eq!(c.state, CapabilityState::ValidationRequired);
            assert!(c.probe.is_none());
            assert!(c.reason.unwrap().contains("not probed"));
        }
        let (t, _) = framework_state(None, false);
        assert_eq!(t.state, CapabilityState::Unsupported);
    }
}
