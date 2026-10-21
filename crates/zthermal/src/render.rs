// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Text renderings. JSON output is the serde form of the same data.

use std::fmt::Write as _;
use std::path::Path;

use zthermal_core::config::diff::DiffReport;
use zthermal_core::config::{Inspection, Severity};
use zthermal_core::identity::DeviceIdentity;
use zthermal_core::policy::Plan;
use zthermal_core::profile::Profile;
use zthermal_core::reading::{DataStatus, Temperature};
use zthermal_platform::backup::BackupManifest;
use zthermal_platform::snapshot::Snapshot;
use zthermal_platform::txn::Report;

fn opt(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("-")
}

/// A temperature as text: the value when valid, otherwise its status and why.
pub fn temp(t: &Temperature) -> String {
    match (t.status, t.celsius()) {
        (DataStatus::Valid, Some(c)) => format!("{c:.1} °C"),
        (status, _) => {
            let raw = t
                .raw
                .as_deref()
                .map(|r| format!(" raw={r}"))
                .unwrap_or_default();
            format!(
                "{}{raw}",
                serde_json::to_value(status)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default()
            )
        }
    }
}

fn status_name<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "?".into())
}

pub fn identity(id: &DeviceIdentity, profile: Option<&Profile>) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "manufacturer   {}", opt(&id.manufacturer));
    let _ = writeln!(s, "brand          {}", opt(&id.brand));
    let _ = writeln!(
        s,
        "oem family     {:?} (confidence {:?})",
        id.oem_family.value, id.oem_family.confidence
    );
    let _ = writeln!(s, "device         {}", opt(&id.device));
    let _ = writeln!(s, "model          {}", opt(&id.model));
    let _ = writeln!(
        s,
        "soc vendor     {:?} (confidence {:?})",
        id.soc_vendor.value, id.soc_vendor.confidence
    );
    for e in &id.soc_vendor.evidence {
        let _ = writeln!(s, "  evidence     {} = {}", e.source, e.value);
    }
    for e in &id.soc_vendor.conflicts {
        let _ = writeln!(
            s,
            "  conflict     {} = {} (indicates {})",
            e.source, e.value, e.indicates
        );
    }
    let _ = writeln!(s, "soc model      {}", opt(&id.soc_model));
    let _ = writeln!(s, "board platform {}", opt(&id.board_platform));
    let _ = writeln!(
        s,
        "android        {} (sdk {})",
        opt(&id.android_release),
        id.sdk.map(|v| v.to_string()).unwrap_or("-".into())
    );
    let _ = writeln!(s, "kernel         {}", opt(&id.kernel_release));
    let _ = writeln!(s, "fingerprint    {}", opt(&id.build_fingerprint));
    let _ = writeln!(
        s,
        "profile        {}",
        profile
            .map(|p| format!(
                "{} (evidence: public source; hardware tested: {})",
                p.id, p.validation.hardware_tested
            ))
            .unwrap_or("none".into())
    );
    if !id.missing.is_empty() {
        let _ = writeln!(s, "missing props  {}", id.missing.len());
    }
    s
}

pub fn capabilities(snap: &Snapshot) -> String {
    let mut s = String::new();
    for a in &snap.adapters {
        let _ = writeln!(
            s,
            "adapter {}: {}",
            a.adapter,
            if a.applies { "applies" } else { "no evidence" }
        );
        for e in &a.evidence {
            let _ = writeln!(s, "  evidence: {e}");
        }
        for (name, state) in &a.services {
            let _ = writeln!(s, "  service: {name} = {state}");
        }
        for (name, state) in &a.candidate_services {
            let _ = writeln!(s, "  candidate service (unconfirmed): {name} = {state}");
        }
        for c in &a.capabilities {
            let _ = writeln!(
                s,
                "  {:<34} {:<22} needs {:?}; validation {:?}",
                c.id,
                status_name(&c.state),
                c.privilege,
                c.validation
            );
            if let Some(r) = &c.reason {
                let _ = writeln!(s, "      {r}");
            }
            if let Some(p) = &c.probe {
                let _ = writeln!(
                    s,
                    "      probed: {} -> {}",
                    p.source,
                    status_name(&p.status)
                );
            }
        }
        for n in &a.notes {
            let _ = writeln!(s, "  note: {n}");
        }
    }
    s
}

pub fn sensors(snap: &Snapshot) -> String {
    let mut s = String::new();
    if !snap.sysfs.thermal_class_present {
        let _ = writeln!(s, "/sys/class/thermal: absent");
    }
    for z in &snap.sysfs.zones {
        let name = z.path.rsplit('/').next().unwrap_or("");
        let _ = writeln!(
            s,
            "{name:<16} {:<28} {:<9} {:<26} trips {}",
            opt(&z.zone_type.value),
            status_name(&z.classification.category),
            temp(&z.temp),
            z.trips.len()
        );
    }
    for h in &snap.sysfs.hwmon {
        for x in &h.sensors {
            let _ = writeln!(
                s,
                "{:<16} {:<28} {:<9} {}",
                opt(&h.name.value),
                x.label.as_deref().unwrap_or(&x.input),
                status_name(&x.classification.category),
                temp(&x.temp)
            );
        }
    }
    if let Some(t) = &snap.thermalservice {
        let _ = writeln!(
            s,
            "framework ({}):",
            t.temperatures_section
                .as_deref()
                .unwrap_or("no temperatures")
        );
        for x in &t.temperatures {
            let v = match &x.bcl {
                Some(b) => format!("{} {}", b.raw, b.unit),
                None => temp(&x.value),
            };
            let _ = writeln!(s, "  {:<28} {:<16} {v}", x.name, x.type_name);
        }
    }
    s
}

pub fn status(snap: &Snapshot) -> String {
    let m = &snap.summary;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "zones          {} ({} valid, {} without a valid reading)",
        m.zones, m.zones_valid, m.zones_invalid
    );
    match &m.hottest_zone {
        Some(h) => {
            let _ = writeln!(
                s,
                "hottest zone   {} {:.1} °C ({})",
                h.name,
                h.millicelsius as f64 / 1000.0,
                h.source
            );
        }
        None => {
            let _ = writeln!(s, "hottest zone   none with a valid reading");
        }
    }
    let _ = writeln!(
        s,
        "framework      {}",
        m.framework_status
            .map(|v| status_name(&v))
            .unwrap_or("not available".into())
    );
    let _ = writeln!(
        s,
        "battery        {}",
        m.battery_millicelsius
            .map(|v| format!("{:.1} °C", v as f64 / 1000.0))
            .unwrap_or("not available".into())
    );
    for src in &snap.sources {
        if src.status != DataStatus::Valid {
            let _ = writeln!(
                s,
                "source         {}: {}{}",
                src.source,
                status_name(&src.status),
                src.detail
                    .as_ref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            );
        }
    }
    s
}

pub fn cooling(snap: &Snapshot) -> String {
    let mut s = String::new();
    for c in &snap.sysfs.cooling_devices {
        let _ = writeln!(
            s,
            "{:<20} {:<28} state {}/{}",
            c.path.rsplit('/').next().unwrap_or(""),
            opt(&c.cdev_type.value),
            opt(&c.cur_state.value),
            opt(&c.max_state.value)
        );
    }
    if s.is_empty() {
        s.push_str("no cooling devices\n");
    }
    s
}

pub fn inspection(i: &Inspection, only_problems: bool) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "file     {}", i.path);
    let _ = writeln!(
        s,
        "format   {} ({})",
        i.detection.format.as_str(),
        i.detection.reason
    );
    let _ = writeln!(s, "size     {} bytes", i.size);
    let _ = writeln!(s, "sha256   {}", i.sha256);
    if let Some(rt) = &i.round_trip {
        let _ = writeln!(
            s,
            "round trip: model equal {}, byte exact {}",
            rt.model_equal, rt.byte_exact
        );
    }
    if !only_problems {
        if let Some(o) = i.summary.as_object() {
            for (k, v) in o {
                let _ = writeln!(s, "  {k}: {v}");
            }
        }
        for f in &i.safety_facts {
            let _ = writeln!(s, "safety   {}: {}", f.location, f.description);
        }
    }
    for f in i
        .findings
        .iter()
        .filter(|f| !only_problems || f.severity >= Severity::Warning)
    {
        let _ = writeln!(
            s,
            "{:<8} {} {}: {}",
            status_name(&f.severity),
            f.location.as_deref().unwrap_or("-"),
            f.code,
            f.message
        );
    }
    let _ = writeln!(s, "{} errors, {} warnings", i.errors(), i.warnings());
    s
}

pub fn diff(r: &DiffReport) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "format {}  old {}  new {}",
        r.format.as_str(),
        &r.old_sha256[..12],
        &r.new_sha256[..12]
    );
    if let Some(n) = &r.note {
        let _ = writeln!(s, "note: {n}");
    }
    for c in &r.changes {
        let _ = writeln!(
            s,
            "{:<8} {}: {} -> {}",
            status_name(&c.kind),
            c.location,
            c.old.as_deref().unwrap_or("-"),
            c.new.as_deref().unwrap_or("-")
        );
        if let Some(why) = &c.safety {
            let _ = writeln!(s, "         SAFETY: {why}");
        } else if let (Some(cat), Some(effect)) = (&c.category, &c.effect) {
            let _ = writeln!(s, "         {}: {effect}", status_name(cat));
        }
    }
    let _ = writeln!(
        s,
        "{} changes, {} weaken thermal protection",
        r.changes.len(),
        r.safety_relevant
    );
    s
}

pub fn backup(m: &BackupManifest, dest: &Path) -> String {
    let mut s = String::new();
    for e in &m.entries {
        let _ = writeln!(
            s,
            "saved   {} ({} bytes, sha256 {})",
            e.source,
            e.size,
            &e.sha256[..16]
        );
    }
    for k in &m.skipped {
        let _ = writeln!(s, "skipped {}: {}", k.source, k.detail);
    }
    let _ = writeln!(s, "manifest {}", dest.join("manifest.json").display());
    s
}

pub fn profiles(ps: &[(String, Result<Profile, String>)]) -> String {
    let mut s = String::new();
    for (n, p) in ps {
        match p {
            Ok(p) => {
                let _ = writeln!(
                    s,
                    "{:<20} devices {:<20} soc {:?} {}  hardware tested: {}",
                    p.id,
                    p.devices.join(","),
                    p.soc.vendor,
                    p.soc.model.as_deref().unwrap_or(""),
                    p.validation.hardware_tested
                );
            }
            Err(e) => {
                let _ = writeln!(s, "{n}: invalid: {e}");
            }
        }
    }
    s
}

pub fn profile_problems(n: usize, problems: &[String]) -> String {
    let mut s = format!("{n} profiles parsed\n");
    for p in problems {
        let _ = writeln!(s, "problem: {p}");
    }
    if problems.is_empty() {
        s.push_str("no problems\n");
    }
    s
}

pub fn doctor(checks: &[(&str, &str, String)]) -> String {
    checks
        .iter()
        .map(|(c, l, d)| format!("[{l:<4}] {c:<14} {d}\n"))
        .collect()
}

pub fn plan(p: &Plan) -> String {
    let mut s = format!("mode {}  dry run {}\n", status_name(&p.mode), p.dry_run);
    for w in &p.writes {
        let _ = writeln!(s, "write {} = {} ({})", w.target, w.value, w.control);
    }
    if p.writes.is_empty() {
        s.push_str("writes: none\n");
    }
    for n in &p.notes {
        let _ = writeln!(s, "note: {n}");
    }
    s
}

pub fn txn_report(r: &Report) -> String {
    let mut s = format!("outcome {}\n", status_name(&r.outcome));
    for p in &r.problems {
        let _ = writeln!(s, "problem: {p}");
    }
    for e in &r.journal.entries {
        let _ = writeln!(
            s,
            "{} {} original={} value={} state={}",
            e.control,
            e.target,
            e.original,
            e.value,
            status_name(&e.state)
        );
    }
    s
}

pub fn report(snap: &Snapshot, profile: Option<&Profile>) -> String {
    let mut s = format!(
        "Zairenkai ThermalTools report (schema {}, tool {}, {}, redacted {})\n\n",
        snap.schema_version, snap.tool_version, snap.root, snap.redacted
    );
    s += "== Device\n";
    s += &identity(&snap.identity, profile);
    s += "\n== Status\n";
    s += &status(snap);
    s += "\n== Sensors\n";
    s += &sensors(snap);
    s += "\n== Cooling devices\n";
    s += &cooling(snap);
    s += "\n== Capabilities\n";
    s += &capabilities(snap);
    s += "\n== Configuration files\n";
    for a in snap.adapters.iter().filter(|a| a.applies) {
        for f in &a.config_files {
            match &f.inspection {
                Some(i) => {
                    let _ = writeln!(
                        s,
                        "{} [{}] {} errors, {} warnings, {} safety facts",
                        f.path,
                        i.detection.format.as_str(),
                        i.errors(),
                        i.warnings(),
                        i.safety_facts.len()
                    );
                }
                None => {
                    let _ = writeln!(s, "{} {}", f.path, status_name(&f.status));
                }
            }
        }
    }
    s += "\nThis report was generated locally and was not sent anywhere.\n";
    s
}
