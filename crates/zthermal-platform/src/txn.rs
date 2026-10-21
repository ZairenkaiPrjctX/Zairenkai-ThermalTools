// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Applying validated control writes as a transaction, with a journal and rollback.
//!
//! Order: validate everything → read every original value → persist the journal → write
//! one value → read it back → persist → next. Any failure rolls back what was written, in
//! reverse order, and each restore is verified. A restore that fails is recorded as
//! `rollback_failed` and reported; success is never claimed for it.
//!
//! The journal survives a crash: `rollback` restores every entry still marked `written`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use zthermal_core::policy::{
    conditions_match, denied_target, descriptor_problems, ControlDescriptor, DeviceFacts,
};

use crate::sysroot::SysRoot;

/// Node access used by transactions. [`SysRoot`] in production; tests inject failures.
pub trait Nodes {
    fn read(&self, path: &str) -> Result<String, String>;
    fn write(&self, path: &str, value: &str) -> Result<(), String>;
}

impl Nodes for SysRoot {
    fn read(&self, path: &str) -> Result<String, String> {
        self.read_attr(path).map_err(|e| e.detail)
    }

    fn write(&self, path: &str, value: &str) -> Result<(), String> {
        self.write_existing(path, value)
            .map_err(|e| format!("{path}: {e}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    Pending,
    Written,
    RolledBack,
    RollbackFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub control: String,
    pub target: String,
    pub original: String,
    pub value: String,
    pub state: EntryState,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub schema_version: u32,
    pub entries: Vec<JournalEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Applied,
    /// Refused before any write.
    Refused,
    /// A write failed; everything written was restored and verified.
    RolledBack,
    /// A write failed and at least one restore failed too: the device may be changed.
    RollbackFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub outcome: Outcome,
    pub problems: Vec<String>,
    pub journal: Journal,
}

pub struct Request<'a> {
    pub control: &'a ControlDescriptor,
    pub value: String,
}

fn save(path: &Path, j: &Journal) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("tmp");
    let text = serde_json::to_string_pretty(j).map_err(|e| e.to_string())?;
    fs::write(&tmp, text + "\n").map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load(path: &Path) -> Result<Journal, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: invalid journal: {e}", path.display()))
}

/// Everything that makes the request unsafe, checked before any read or write.
pub fn preflight(requests: &[Request<'_>], facts: &DeviceFacts) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for r in requests {
        let d = r.control;
        out.extend(
            descriptor_problems(d)
                .into_iter()
                .map(|p| format!("{}: {p}", d.id)),
        );
        if let Some(why) = denied_target(&d.target) {
            out.push(format!("{}: {why}", d.id));
        }
        if !conditions_match(&d.conditions, facts) {
            out.push(format!(
                "{}: device does not match the control's conditions",
                d.id
            ));
        }
        if !d.allowed.permits(&r.value) {
            out.push(format!(
                "{}: value {:?} is outside the validated values",
                d.id, r.value
            ));
        }
        if !d.hardware_tested {
            out.push(format!(
                "{}: not tested on hardware; only dry runs are allowed",
                d.id
            ));
        }
        if !seen.insert(d.target.clone()) {
            out.push(format!("{}: target written twice in one request", d.id));
        }
    }
    out
}

pub fn apply(
    root: &dyn Nodes,
    journal_path: &Path,
    requests: &[Request<'_>],
    facts: &DeviceFacts,
) -> Report {
    let refused = |problems: Vec<String>, journal: Journal| Report {
        outcome: Outcome::Refused,
        problems,
        journal,
    };
    let empty = Journal {
        schema_version: 1,
        entries: vec![],
    };
    if journal_path.exists() {
        if let Ok(j) = load(journal_path) {
            if j.entries
                .iter()
                .any(|e| e.state == EntryState::Written || e.state == EntryState::RollbackFailed)
            {
                return refused(
                    vec!["an earlier change has not been rolled back; run rollback first".into()],
                    j,
                );
            }
        }
    }
    let problems = preflight(requests, facts);
    if !problems.is_empty() {
        return refused(problems, empty);
    }
    let mut journal = empty;
    for r in requests {
        match root.read(&r.control.target) {
            Ok(original) => journal.entries.push(JournalEntry {
                control: r.control.id.clone(),
                target: r.control.target.clone(),
                original,
                value: r.value.clone(),
                state: EntryState::Pending,
                detail: None,
            }),
            Err(e) => {
                return refused(
                    vec![format!(
                        "{}: cannot read the original value: {e}",
                        r.control.id
                    )],
                    Journal {
                        schema_version: 1,
                        entries: vec![],
                    },
                )
            }
        }
    }
    if let Err(e) = save(journal_path, &journal) {
        return refused(
            vec![format!(
                "cannot write the journal before changing anything: {e}"
            )],
            journal,
        );
    }
    for i in 0..journal.entries.len() {
        let (target, value) = (
            journal.entries[i].target.clone(),
            journal.entries[i].value.clone(),
        );
        // Mark written before the write: if we crash mid-write, rollback still restores it.
        journal.entries[i].state = EntryState::Written;
        let failure = match save(journal_path, &journal) {
            Err(e) => Some(format!("journal: {e}")),
            Ok(()) => match root.write(&target, &value) {
                Err(e) => Some(format!("{target}: write failed: {e}")),
                Ok(()) => match root.read(&target) {
                    Ok(back) if back == value.trim() => None,
                    Ok(back) => Some(format!("{target}: read back {back:?}, expected {value:?}")),
                    Err(e) => Some(format!("{target}: read-back failed: {e}")),
                },
            },
        };
        if let Some(f) = failure {
            journal.entries[i].detail = Some(f.clone());
            let outcome = rollback_entries(root, &mut journal);
            let _ = save(journal_path, &journal);
            return Report {
                outcome,
                problems: vec![f],
                journal,
            };
        }
    }
    let _ = save(journal_path, &journal);
    Report {
        outcome: Outcome::Applied,
        problems: vec![],
        journal,
    }
}

fn rollback_entries(root: &dyn Nodes, journal: &mut Journal) -> Outcome {
    let mut ok = true;
    for e in journal.entries.iter_mut().rev() {
        if !matches!(e.state, EntryState::Written | EntryState::RollbackFailed) {
            continue;
        }
        let restored =
            root.write(&e.target, &e.original)
                .and_then(|()| match root.read(&e.target) {
                    Ok(back) if back == e.original => Ok(()),
                    Ok(back) => Err(format!(
                        "read back {back:?} after restoring {:?}",
                        e.original
                    )),
                    Err(err) => Err(err),
                });
        match restored {
            Ok(()) => e.state = EntryState::RolledBack,
            Err(why) => {
                ok = false;
                e.state = EntryState::RollbackFailed;
                e.detail = Some(format!("restore failed: {why}"));
            }
        }
    }
    if ok {
        Outcome::RolledBack
    } else {
        Outcome::RollbackFailed
    }
}

/// Restore everything a journal still records as written (after a crash or on request).
pub fn rollback(root: &dyn Nodes, journal_path: &Path) -> Result<Report, String> {
    let mut journal = load(journal_path)?;
    let outcome = rollback_entries(root, &mut journal);
    save(journal_path, &journal)?;
    let problems = journal
        .entries
        .iter()
        .filter_map(|e| e.detail.clone())
        .filter(|d| d.starts_with("restore failed"))
        .collect();
    Ok(Report {
        outcome,
        problems,
        journal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysroot::testutil::TempDir;
    use zthermal_core::identity::SocVendor;
    use zthermal_core::policy::{AllowedValues, Conditions};

    fn knob(id: &str, target: &str) -> ControlDescriptor {
        ControlDescriptor {
            id: id.into(),
            target: target.into(),
            description: "fixture-only knob".into(),
            conditions: Conditions {
                soc_vendor: Some(SocVendor::Qualcomm),
                ..Default::default()
            },
            allowed: AllowedValues::IntegerRange { min: 0, max: 2 },
            evidence: vec!["synthetic test fixture".into()],
            restore_original: true,
            verify_after_write: true,
            hardware_tested: true,
        }
    }

    fn qcom() -> DeviceFacts {
        DeviceFacts {
            soc_vendor: Some(SocVendor::Qualcomm),
            ..Default::default()
        }
    }

    #[test]
    fn applies_verifies_and_rolls_back_on_request() {
        let d = TempDir::new("txn");
        d.file("root/sys/kernel/a", "0\n");
        d.file("root/sys/kernel/b", "1\n");
        let root = SysRoot::at(d.path().join("root"));
        let j = d.path().join("state/journal.json");
        let (a, b) = (knob("t.a", "/sys/kernel/a"), knob("t.b", "/sys/kernel/b"));
        let r = apply(
            &root,
            &j,
            &[
                Request {
                    control: &a,
                    value: "2".into(),
                },
                Request {
                    control: &b,
                    value: "0".into(),
                },
            ],
            &qcom(),
        );
        assert_eq!(r.outcome, Outcome::Applied, "{:?}", r.problems);
        assert_eq!(root.read_attr("/sys/kernel/a").unwrap(), "2");
        // A second apply is refused until the first is rolled back.
        let again = apply(
            &root,
            &j,
            &[Request {
                control: &a,
                value: "1".into(),
            }],
            &qcom(),
        );
        assert_eq!(again.outcome, Outcome::Refused);
        let rb = rollback(&root, &j).unwrap();
        assert_eq!(rb.outcome, Outcome::RolledBack);
        assert_eq!(root.read_attr("/sys/kernel/a").unwrap(), "0");
        assert_eq!(root.read_attr("/sys/kernel/b").unwrap(), "1");
        assert!(load(&j)
            .unwrap()
            .entries
            .iter()
            .all(|e| e.state == EntryState::RolledBack));
    }

    #[test]
    fn unsafe_requests_are_refused_before_any_write() {
        let d = TempDir::new("txn-refuse");
        d.file("root/sys/kernel/a", "0\n");
        d.file(
            "root/sys/class/thermal/thermal_zone0/trip_point_0_temp",
            "95000\n",
        );
        let root = SysRoot::at(d.path().join("root"));
        let j = d.path().join("journal.json");
        let a = knob("t.a", "/sys/kernel/a");
        for (desc, value, facts) in [
            (a.clone(), "3".to_string(), qcom()), // out of range
            (a.clone(), "1".to_string(), DeviceFacts::default()), // wrong device
            (
                ControlDescriptor {
                    hardware_tested: false,
                    ..a.clone()
                },
                "1".into(),
                qcom(),
            ),
            (
                knob(
                    "t.trip",
                    "/sys/class/thermal/thermal_zone0/trip_point_0_temp",
                ),
                "1".into(),
                qcom(),
            ),
        ] {
            let r = apply(
                &root,
                &j,
                &[Request {
                    control: &desc,
                    value,
                }],
                &facts,
            );
            assert_eq!(r.outcome, Outcome::Refused);
            assert!(!r.problems.is_empty());
        }
        assert_eq!(root.read_attr("/sys/kernel/a").unwrap(), "0");
        assert_eq!(
            root.read_attr("/sys/class/thermal/thermal_zone0/trip_point_0_temp")
                .unwrap(),
            "95000"
        );
        assert!(
            !j.exists(),
            "nothing was journaled because nothing was attempted"
        );
        let dup = apply(
            &root,
            &j,
            &[
                Request {
                    control: &a,
                    value: "1".into(),
                },
                Request {
                    control: &a,
                    value: "2".into(),
                },
            ],
            &qcom(),
        );
        assert!(dup.problems.iter().any(|p| p.contains("twice")));
    }

    #[test]
    fn missing_original_aborts_without_writing() {
        let d = TempDir::new("txn-missing");
        d.file("root/sys/kernel/a", "0\n");
        let root = SysRoot::at(d.path().join("root"));
        let (a, gone) = (
            knob("t.a", "/sys/kernel/a"),
            knob("t.gone", "/sys/kernel/gone"),
        );
        let r = apply(
            &root,
            &d.path().join("j.json"),
            &[
                Request {
                    control: &a,
                    value: "1".into(),
                },
                Request {
                    control: &gone,
                    value: "1".into(),
                },
            ],
            &qcom(),
        );
        assert_eq!(r.outcome, Outcome::Refused);
        assert_eq!(root.read_attr("/sys/kernel/a").unwrap(), "0");
        assert!(
            !root.exists("/sys/kernel/gone"),
            "writes never create nodes"
        );
    }

    #[test]
    fn unreadable_original_refuses_before_writing() {
        let d = TempDir::new("txn-verify");
        d.file("root/sys/kernel/a", "0\n");
        d.file("root/sys/kernel/b", "0\n");
        let root = SysRoot::at(d.path().join("root"));
        // "b" is a directory-backed node that cannot be written: the write fails.
        std::fs::remove_file(d.path().join("root/sys/kernel/b")).unwrap();
        std::fs::create_dir(d.path().join("root/sys/kernel/b")).unwrap();
        std::fs::write(d.path().join("root/sys/kernel/b/x"), "").unwrap();
        let (a, b) = (knob("t.a", "/sys/kernel/a"), knob("t.b", "/sys/kernel/b"));
        // Reading a directory fails, so the original cannot be captured: refused, nothing written.
        let j = d.path().join("j.json");
        let r = apply(
            &root,
            &j,
            &[
                Request {
                    control: &a,
                    value: "2".into(),
                },
                Request {
                    control: &b,
                    value: "1".into(),
                },
            ],
            &qcom(),
        );
        assert_eq!(r.outcome, Outcome::Refused);
        assert_eq!(root.read_attr("/sys/kernel/a").unwrap(), "0");
    }

    /// Nodes in memory, with injected faults.
    struct FaultyNodes {
        values: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
        fail_write: Vec<String>,
        /// Writes "succeed" but the node keeps (or reports) a different value.
        ignore_write: Vec<String>,
        fail_restore: Vec<String>,
        restoring: std::cell::Cell<bool>,
    }

    impl FaultyNodes {
        fn new(pairs: &[(&str, &str)]) -> Self {
            Self {
                values: std::cell::RefCell::new(
                    pairs
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                ),
                fail_write: vec![],
                ignore_write: vec![],
                fail_restore: vec![],
                restoring: std::cell::Cell::new(false),
            }
        }
        fn get(&self, k: &str) -> String {
            self.values.borrow()[k].clone()
        }
    }

    impl Nodes for FaultyNodes {
        fn read(&self, path: &str) -> Result<String, String> {
            self.values
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| format!("{path}: missing"))
        }
        fn write(&self, path: &str, value: &str) -> Result<(), String> {
            let restoring = self.restoring.get();
            if self.fail_write.iter().any(|p| p == path) && !restoring {
                self.restoring.set(true); // the next writes are the rollback
                return Err(format!("{path}: EIO"));
            }
            if restoring && self.fail_restore.iter().any(|p| p == path) {
                return Err(format!("{path}: EBUSY"));
            }
            if self.ignore_write.iter().any(|p| p == path) && !restoring {
                self.restoring.set(true);
                return Ok(());
            }
            self.values.borrow_mut().insert(path.into(), value.into());
            Ok(())
        }
    }

    #[test]
    fn write_failure_mid_transaction_restores_previous_writes() {
        let d = TempDir::new("txn-mid");
        let mut nodes = FaultyNodes::new(&[("/sys/kernel/a", "0"), ("/sys/kernel/b", "1")]);
        nodes.fail_write = vec!["/sys/kernel/b".into()];
        let (a, b) = (knob("t.a", "/sys/kernel/a"), knob("t.b", "/sys/kernel/b"));
        let j = d.path().join("j.json");
        let r = apply(
            &nodes,
            &j,
            &[
                Request {
                    control: &a,
                    value: "2".into(),
                },
                Request {
                    control: &b,
                    value: "0".into(),
                },
            ],
            &qcom(),
        );
        assert_eq!(r.outcome, Outcome::RolledBack, "{r:?}");
        assert!(r.problems[0].contains("write failed"));
        assert_eq!(nodes.get("/sys/kernel/a"), "0", "earlier write restored");
        assert_eq!(nodes.get("/sys/kernel/b"), "1");
        assert!(r
            .journal
            .entries
            .iter()
            .all(|e| e.state == EntryState::RolledBack));
        assert_eq!(
            load(&j).unwrap(),
            r.journal,
            "journal on disk matches the report"
        );
    }

    #[test]
    fn read_back_mismatch_is_a_failure() {
        let d = TempDir::new("txn-readback");
        let mut nodes = FaultyNodes::new(&[("/sys/kernel/a", "0")]);
        nodes.ignore_write = vec!["/sys/kernel/a".into()];
        let a = knob("t.a", "/sys/kernel/a");
        let r = apply(
            &nodes,
            &d.path().join("j.json"),
            &[Request {
                control: &a,
                value: "2".into(),
            }],
            &qcom(),
        );
        assert_eq!(r.outcome, Outcome::RolledBack);
        assert!(
            r.problems[0].contains("read back \"0\", expected \"2\""),
            "{:?}",
            r.problems
        );
    }

    #[test]
    fn partial_rollback_failure_is_reported() {
        let d = TempDir::new("txn-partial");
        let mut nodes = FaultyNodes::new(&[
            ("/sys/kernel/a", "0"),
            ("/sys/kernel/b", "1"),
            ("/sys/kernel/c", "1"),
        ]);
        nodes.fail_write = vec!["/sys/kernel/c".into()];
        nodes.fail_restore = vec!["/sys/kernel/a".into()];
        let (a, b, c) = (
            knob("t.a", "/sys/kernel/a"),
            knob("t.b", "/sys/kernel/b"),
            knob("t.c", "/sys/kernel/c"),
        );
        let reqs = [
            Request {
                control: &a,
                value: "2".into(),
            },
            Request {
                control: &b,
                value: "2".into(),
            },
            Request {
                control: &c,
                value: "2".into(),
            },
        ];
        let r = apply(&nodes, &d.path().join("j.json"), &reqs, &qcom());
        assert_eq!(r.outcome, Outcome::RollbackFailed);
        assert_eq!(nodes.get("/sys/kernel/b"), "1", "b restored");
        assert_eq!(
            nodes.get("/sys/kernel/a"),
            "2",
            "a could not be restored and is reported"
        );
        let states: Vec<EntryState> = r.journal.entries.iter().map(|e| e.state).collect();
        assert_eq!(
            states,
            vec![
                EntryState::RollbackFailed,
                EntryState::RolledBack,
                EntryState::RolledBack
            ]
        );
    }

    #[test]
    fn failed_restore_is_reported_never_hidden() {
        let d = TempDir::new("txn-rbfail");
        d.file("root/sys/kernel/a", "5\n");
        let root = SysRoot::at(d.path().join("root"));
        let j = d.path().join("j.json");
        let journal = Journal {
            schema_version: 1,
            entries: vec![JournalEntry {
                control: "t.a".into(),
                target: "/sys/kernel/vanished".into(),
                original: "0".into(),
                value: "5".into(),
                state: EntryState::Written,
                detail: None,
            }],
        };
        save(&j, &journal).unwrap();
        let r = rollback(&root, &j).unwrap();
        assert_eq!(r.outcome, Outcome::RollbackFailed);
        assert!(r.problems[0].contains("restore failed"));
        assert_eq!(
            load(&j).unwrap().entries[0].state,
            EntryState::RollbackFailed
        );
        // A later apply is refused while the failed restore is unresolved.
        let a = knob("t.a", "/sys/kernel/a");
        assert_eq!(
            apply(
                &root,
                &j,
                &[Request {
                    control: &a,
                    value: "1".into()
                }],
                &qcom()
            )
            .outcome,
            Outcome::Refused
        );
    }
}
