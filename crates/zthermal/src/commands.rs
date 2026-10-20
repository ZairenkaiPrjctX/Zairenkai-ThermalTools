//! Command implementations. Every command reads; only `apply` (without `--dry-run`) and
//! `rollback` can write, and both go through the transaction layer's safety checks.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};
use zthermal_core::config::diff::diff;
use zthermal_core::config::{inspect, Severity};
use zthermal_core::policy::{plan, validated_controls, DeviceFacts, PolicyMode};
use zthermal_core::profile::{self, Profile};
use zthermal_platform::backup;
use zthermal_platform::exec::{FixtureRunner, ProcessRunner, Runner};
use zthermal_platform::snapshot::{collect, Options, Snapshot};
use zthermal_platform::sysroot::SysRoot;
use zthermal_platform::txn;

use crate::args::{Args, USAGE};
use crate::render;

pub const EXIT_OK: u8 = 0;
pub const EXIT_ERROR: u8 = 1;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_INVALID: u8 = 3;
pub const EXIT_REFUSED: u8 = 4;
pub const EXIT_UNSUPPORTED: u8 = 5;

pub const DEFAULT_JOURNAL: &str = "/data/local/tmp/zthermal/journal.json";
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

/// Profiles shipped in the binary. Each is validated by `profile-validate` and in tests.
pub const EMBEDDED_PROFILES: &[(&str, &str)] = &[(
    "google-shusky.json",
    include_str!("../../../profiles/google-shusky.json"),
)];

struct Env {
    root: SysRoot,
    runner: Box<dyn Runner>,
}

fn env(a: &Args) -> Env {
    match &a.root {
        Some(dir) => Env {
            root: SysRoot::at(dir),
            runner: Box::new(FixtureRunner {
                dir: dir.join("_commands"),
            }),
        },
        None => Env {
            root: SysRoot::real(),
            runner: Box::new(ProcessRunner),
        },
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn snapshot(a: &Args, e: &Env, framework: bool) -> Snapshot {
    let mut s = collect(
        &e.root,
        e.runner.as_ref(),
        &Options {
            framework,
            now_ms: now_ms(),
        },
    );
    if a.redact {
        s.redact();
    }
    s
}

/// Prints JSON or text, to stdout or `--output`.
fn emit(a: &Args, value: &impl Serialize, text: impl FnOnce() -> String) -> Result<(), String> {
    let out = if a.json {
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())? + "\n"
    } else {
        text()
    };
    match &a.output {
        Some(p) => std::fs::write(p, out).map_err(|e| format!("{}: {e}", p.display())),
        None => {
            print!("{out}");
            Ok(())
        }
    }
}

fn read_input(path: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut buf = Vec::new();
    f.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{path}: {e}"))?;
    if buf.len() as u64 > MAX_INPUT_BYTES {
        return Err(format!(
            "{path}: larger than {MAX_INPUT_BYTES} bytes; refused"
        ));
    }
    Ok(buf)
}

/// A profile source name and its parse result.
pub type LoadedProfile = (String, Result<Profile, String>);

pub fn load_profiles(dir: Option<&Path>, files: &[String]) -> Result<Vec<LoadedProfile>, String> {
    let texts: Vec<(String, String)> = if !files.is_empty() {
        files
            .iter()
            .map(|f| read_input(f).map(|b| (f.clone(), String::from_utf8_lossy(&b).into_owned())))
            .collect::<Result<_, _>>()?
    } else if let Some(d) = dir {
        let mut names: Vec<PathBuf> = std::fs::read_dir(d)
            .map_err(|e| format!("{}: {e}", d.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        names.sort();
        names
            .iter()
            .map(|p| {
                read_input(&p.to_string_lossy()).map(|b| {
                    (
                        p.display().to_string(),
                        String::from_utf8_lossy(&b).into_owned(),
                    )
                })
            })
            .collect::<Result<_, _>>()?
    } else {
        EMBEDDED_PROFILES
            .iter()
            .map(|(n, t)| (format!("embedded:{n}"), t.to_string()))
            .collect()
    };
    Ok(texts
        .into_iter()
        .map(|(n, t)| (n, profile::parse(&t)))
        .collect())
}

fn matching_profile(a: &Args, s: &Snapshot) -> Option<Profile> {
    let device = s.identity.device.as_deref()?;
    load_profiles(a.profiles.as_deref(), &[])
        .ok()?
        .into_iter()
        .filter_map(|(_, p)| p.ok())
        .find(|p| p.devices.iter().any(|d| d == device) && profile::profile_problems(p).is_empty())
}

fn euid() -> Option<u32> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|v| v.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
}

fn journal_path(a: &Args, e: &Env) -> PathBuf {
    a.journal
        .clone()
        .unwrap_or_else(|| e.root.path(DEFAULT_JOURNAL))
}

pub fn run(a: &Args) -> u8 {
    match dispatch(a) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("zthermal: {e}");
            EXIT_ERROR
        }
    }
}

fn dispatch(a: &Args) -> Result<u8, String> {
    let e = env(a);
    match a.command.as_str() {
        "help" => {
            println!("{USAGE}");
            Ok(EXIT_OK)
        }
        "version" => {
            println!("zthermal {}", env!("CARGO_PKG_VERSION"));
            Ok(EXIT_OK)
        }
        "doctor" => doctor(a, &e),
        "device-info" => {
            let s = snapshot(a, &e, false);
            let prof = matching_profile(a, &s);
            let v =
                json!({"identity": s.identity, "matched_profile": prof.as_ref().map(|p| &p.id)});
            emit(a, &v, || render::identity(&s.identity, prof.as_ref()))?;
            Ok(EXIT_OK)
        }
        "capabilities" => {
            let s = snapshot(a, &e, false);
            emit(
                a,
                &json!({"schema_version": 1, "capabilities": s.capabilities, "adapters": s.adapters.iter().map(|r| json!({"adapter": r.adapter, "applies": r.applies, "evidence": r.evidence, "notes": r.notes})).collect::<Vec<_>>()}),
                || render::capabilities(&s),
            )?;
            Ok(EXIT_OK)
        }
        "sensors" => {
            let s = snapshot(a, &e, a.framework);
            emit(
                a,
                &json!({"zones": s.sysfs.zones, "hwmon": s.sysfs.hwmon, "framework": s.thermalservice.as_ref().map(|t| &t.temperatures), "sources": s.sources}),
                || render::sensors(&s),
            )?;
            Ok(
                if s.sysfs.zones.is_empty()
                    && s.sysfs.hwmon.is_empty()
                    && s.thermalservice.is_none()
                {
                    EXIT_UNSUPPORTED
                } else {
                    EXIT_OK
                },
            )
        }
        "thermal-status" => {
            let s = snapshot(a, &e, true);
            emit(
                a,
                &json!({"summary": s.summary, "sources": s.sources, "framework": s.thermalservice}),
                || render::status(&s),
            )?;
            Ok(
                if s.summary.zones_valid == 0 && s.summary.framework_status.is_none() {
                    EXIT_UNSUPPORTED
                } else {
                    EXIT_OK
                },
            )
        }
        "cooling-devices" => {
            let s = snapshot(a, &e, a.framework);
            emit(
                a,
                &json!({"cooling_devices": s.sysfs.cooling_devices, "framework": s.thermalservice.as_ref().map(|t| &t.cooling_devices)}),
                || render::cooling(&s),
            )?;
            Ok(if s.sysfs.cooling_devices.is_empty() {
                EXIT_UNSUPPORTED
            } else {
                EXIT_OK
            })
        }
        "inspect-config" | "validate-config" => {
            let path = &a.positional[0];
            let data = read_input(path)?;
            let i = inspect(path, &data);
            let only_problems = a.command == "validate-config";
            emit(a, &i, || render::inspection(&i, only_problems))?;
            Ok(
                if only_problems && i.findings.iter().any(|f| f.severity == Severity::Error) {
                    EXIT_INVALID
                } else {
                    EXIT_OK
                },
            )
        }
        "diff-config" => {
            let (o, n) = (&a.positional[0], &a.positional[1]);
            let r = diff(o, &read_input(o)?, n, &read_input(n)?)?;
            emit(a, &r, || render::diff(&r))?;
            Ok(if r.safety_relevant > 0 {
                EXIT_INVALID
            } else {
                EXIT_OK
            })
        }
        "backup-config" => {
            let s = snapshot(a, &e, false);
            let files: Vec<String> = s
                .adapters
                .iter()
                .flat_map(|r| {
                    r.config_files
                        .iter()
                        .filter(|f| f.inspection.is_some())
                        .map(|f| f.path.clone())
                })
                .collect();
            let dest = a.dest.as_ref().expect("checked by args");
            let m = backup::create(&e.root, &files, dest, now_ms())?;
            let (_, verified) = backup::verify(dest)?;
            emit(a, &json!({"manifest": m, "verified": verified}), || {
                render::backup(&m, dest)
            })?;
            Ok(if verified.iter().all(|v| v.ok) {
                EXIT_OK
            } else {
                EXIT_ERROR
            })
        }
        "profile-list" => {
            let ps = load_profiles(a.profiles.as_deref(), &[])?;
            let v: Vec<Value> = ps
                .iter()
                .map(|(n, p)| match p {
                    Ok(p) => json!({"source": n, "id": p.id, "devices": p.devices, "soc": p.soc, "hardware_tested": p.validation.hardware_tested}),
                    Err(e) => json!({"source": n, "error": e}),
                })
                .collect();
            emit(a, &v, || render::profiles(&ps))?;
            Ok(EXIT_OK)
        }
        "profile-validate" => {
            let ps = load_profiles(a.profiles.as_deref(), &a.positional)?;
            let mut problems: Vec<String> = ps
                .iter()
                .filter_map(|(n, p)| p.as_ref().err().map(|e| format!("{n}: {e}")))
                .collect();
            let ok: Vec<(String, Profile)> = ps
                .into_iter()
                .filter_map(|(n, p)| p.ok().map(|p| (n, p)))
                .collect();
            problems.extend(profile::database_problems(&ok));
            emit(
                a,
                &json!({"profiles": ok.len(), "problems": problems}),
                || render::profile_problems(ok.len(), &problems),
            )?;
            Ok(if problems.is_empty() {
                EXIT_OK
            } else {
                EXIT_INVALID
            })
        }
        "snapshot" => {
            let s = snapshot(a, &e, a.framework);
            let a2 = Args {
                json: true,
                ..a.clone()
            };
            emit(&a2, &s, String::new)?;
            Ok(EXIT_OK)
        }
        "report" => {
            let s = snapshot(a, &e, a.framework);
            let prof = matching_profile(a, &s);
            emit(a, &json!({"snapshot": s, "matched_profile": prof}), || {
                render::report(&s, prof.as_ref())
            })?;
            Ok(EXIT_OK)
        }
        "apply" => apply(a, &e),
        "rollback" => rollback(a, &e),
        other => Err(format!("unhandled command {other}")),
    }
}

fn doctor(a: &Args, e: &Env) -> Result<u8, String> {
    let s = snapshot(a, e, a.framework);
    let mut checks: Vec<(&str, &str, String)> = Vec::new();
    checks.push((
        "binary",
        "info",
        format!(
            "zthermal {} for {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::ARCH
        ),
    ));
    checks.push((
        "root",
        "info",
        if e.root.is_real() {
            "live system".into()
        } else {
            format!("fixture {}", e.root.path("/").display())
        },
    ));
    checks.push(match euid() {
        Some(0) => (
            "privilege",
            "info",
            "running as root; reads only unless a validated control applies".into(),
        ),
        Some(u) => (
            "privilege",
            "info",
            format!("running as uid {u}; root-only sources are reported as permission denied"),
        ),
        None => (
            "privilege",
            "warn",
            "uid unknown (/proc/self/status unreadable)".into(),
        ),
    });
    for src in &s.sources {
        let level = match src.status {
            zthermal_core::reading::DataStatus::Valid => "ok",
            zthermal_core::reading::DataStatus::Unknown => "info",
            _ => "warn",
        };
        checks.push((
            "source",
            level,
            format!(
                "{}: {:?}{}",
                src.source,
                src.status,
                src.detail
                    .as_ref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            ),
        ));
    }
    checks.push(if s.sysfs.thermal_class_present {
        (
            "thermal_sysfs",
            "ok",
            format!(
                "{} zones ({} valid), {} cooling devices",
                s.summary.zones, s.summary.zones_valid, s.summary.cooling_devices
            ),
        )
    } else {
        (
            "thermal_sysfs",
            "warn",
            "/sys/class/thermal is absent".into(),
        )
    });
    match e.root.read_attr("/sys/fs/selinux/enforce") {
        Ok(v) => checks.push((
            "selinux",
            "info",
            if v == "1" {
                "enforcing".into()
            } else {
                format!("enforce = {v}")
            },
        )),
        Err(_) => checks.push(("selinux", "info", "status not readable".into())),
    }
    let profiles = load_profiles(a.profiles.as_deref(), &[])?;
    let bad = profiles
        .iter()
        .filter(|(_, p)| {
            p.as_ref()
                .map(|p| !profile::profile_problems(p).is_empty())
                .unwrap_or(true)
        })
        .count();
    checks.push(if bad == 0 {
        (
            "profiles",
            "ok",
            format!("{} profiles valid", profiles.len()),
        )
    } else {
        ("profiles", "fail", format!("{bad} invalid profiles"))
    });
    checks.push((
        "controls",
        "info",
        format!(
            "{} validated writable controls in this release",
            validated_controls().len()
        ),
    ));
    let jp = journal_path(a, e);
    if let Ok(j) = txn::load(&jp) {
        let pending = j
            .entries
            .iter()
            .filter(|x| {
                matches!(
                    x.state,
                    txn::EntryState::Written | txn::EntryState::RollbackFailed
                )
            })
            .count();
        if pending > 0 {
            checks.push((
                "journal",
                "fail",
                format!(
                    "{pending} changes in {} are not rolled back; run rollback",
                    jp.display()
                ),
            ));
        }
    }
    let v: Vec<Value> = checks
        .iter()
        .map(|(c, l, d)| json!({"check": c, "level": l, "detail": d}))
        .collect();
    emit(a, &v, || render::doctor(&checks))?;
    Ok(if checks.iter().any(|(_, l, _)| *l == "fail") {
        EXIT_ERROR
    } else {
        EXIT_OK
    })
}

fn apply(a: &Args, e: &Env) -> Result<u8, String> {
    let mode = a.mode.as_deref().unwrap_or_default();
    let Some(mode) = PolicyMode::parse(mode) else {
        eprintln!("zthermal: unknown mode {mode:?}");
        return Ok(EXIT_USAGE);
    };
    let s = snapshot(a, e, false);
    let facts = DeviceFacts {
        soc_vendor: Some(s.identity.soc_vendor.value),
        board_platform: s.identity.board_platform.clone(),
        device: s.identity.device.clone(),
    };
    let controls = validated_controls();
    let p = plan(mode, &facts, &controls, a.dry_run);
    if a.dry_run {
        emit(a, &p, || render::plan(&p))?;
        return Ok(EXIT_OK);
    }
    if p.writes.is_empty() {
        emit(a, &json!({"outcome": "refused", "plan": p}), || {
            format!("{}refused: nothing validated to apply on this device; the system's thermal management is unchanged\n", render::plan(&p))
        })?;
        return Ok(EXIT_REFUSED);
    }
    if e.root.is_real() && euid() != Some(0) {
        eprintln!("zthermal: applying needs root; nothing was written");
        return Ok(EXIT_REFUSED);
    }
    let requests: Vec<txn::Request<'_>> = p
        .writes
        .iter()
        .filter_map(|w| {
            controls
                .iter()
                .find(|c| c.id == w.control)
                .map(|c| txn::Request {
                    control: c,
                    value: w.value.clone(),
                })
        })
        .collect();
    let r = txn::apply(&e.root, &journal_path(a, e), &requests, &facts);
    emit(a, &r, || render::txn_report(&r))?;
    Ok(match r.outcome {
        txn::Outcome::Applied => EXIT_OK,
        txn::Outcome::Refused => EXIT_REFUSED,
        txn::Outcome::RolledBack | txn::Outcome::RollbackFailed => EXIT_ERROR,
    })
}

fn rollback(a: &Args, e: &Env) -> Result<u8, String> {
    let jp = journal_path(a, e);
    if !jp.exists() {
        emit(
            a,
            &json!({"outcome": "nothing_to_roll_back", "journal": jp}),
            || format!("no journal at {}; nothing to roll back\n", jp.display()),
        )?;
        return Ok(EXIT_OK);
    }
    let r = txn::rollback(&e.root, &jp)?;
    emit(a, &r, || render::txn_report(&r))?;
    Ok(match r.outcome {
        txn::Outcome::RollbackFailed => EXIT_ERROR,
        _ => EXIT_OK,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_profiles_are_valid() {
        let ps = load_profiles(None, &[]).unwrap();
        assert!(!ps.is_empty());
        let ok: Vec<(String, Profile)> = ps
            .into_iter()
            .map(|(n, p)| (n, p.expect("parses")))
            .collect();
        assert_eq!(profile::database_problems(&ok), Vec::<String>::new());
        assert!(ok
            .iter()
            .all(|(_, p)| !p.validation.hardware_tested && p.controls.is_empty()));
    }
}
