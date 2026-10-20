//! Semantic diff of two configuration files of the same format.
//!
//! Changes are reported per section/key (Qualcomm) or per sensor/cooling device key
//! (Pixel). A change that removes or weakens mitigation is marked `safety` so a reviewer
//! cannot miss it: a removed section or sensor, a raised trigger, a removed `shutdown`
//! action, an added `disable`, or a severity level that disappears.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::pixel::{hal_float, PixelConfig, SEVERITIES};
use super::qcom::{QcomConfig, Section};
use super::{detect, sha256_hex, ConfigFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub location: String,
    pub kind: ChangeKind,
    pub old: Option<String>,
    pub new: Option<String>,
    /// Why this change weakens thermal protection, if it does.
    pub safety: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffReport {
    pub schema_version: u32,
    pub format: ConfigFormat,
    pub old_sha256: String,
    pub new_sha256: String,
    pub identical: bool,
    pub changes: Vec<Change>,
    pub safety_relevant: usize,
    pub note: Option<String>,
}

fn change(
    location: String,
    kind: ChangeKind,
    old: Option<String>,
    new: Option<String>,
    safety: Option<String>,
) -> Change {
    Change {
        location,
        kind,
        old,
        new,
        safety,
    }
}

/// Diffs two files. Formats must match; opaque or unknown files are compared by hash only.
pub fn diff(old_path: &str, old: &[u8], new_path: &str, new: &[u8]) -> Result<DiffReport, String> {
    let (fo, fnew) = (detect(old_path, old).format, detect(new_path, new).format);
    if fo != fnew {
        return Err(format!(
            "formats differ: {} vs {}",
            fo.as_str(),
            fnew.as_str()
        ));
    }
    let (ot, nt) = (
        std::str::from_utf8(old).unwrap_or(""),
        std::str::from_utf8(new).unwrap_or(""),
    );
    let mut note = None;
    let changes = match fo {
        ConfigFormat::QcomThermalEngine => {
            qcom_diff(&QcomConfig::parse(ot), &QcomConfig::parse(nt))
        }
        ConfigFormat::PixelThermalInfoJson => {
            pixel_diff(&PixelConfig::parse(ot)?, &PixelConfig::parse(nt)?)
        }
        _ => {
            note = Some("no semantic parser for this format; compared by SHA-256 only".into());
            if old == new {
                vec![]
            } else {
                vec![change(
                    "(file)".into(),
                    ChangeKind::Changed,
                    Some(sha256_hex(old)),
                    Some(sha256_hex(new)),
                    None,
                )]
            }
        }
    };
    Ok(DiffReport {
        schema_version: 1,
        format: fo,
        old_sha256: sha256_hex(old),
        new_sha256: sha256_hex(new),
        identical: old == new,
        safety_relevant: changes.iter().filter(|c| c.safety.is_some()).count(),
        changes,
        note,
    })
}

fn qcom_entries(s: &[(String, Vec<String>)]) -> BTreeMap<String, String> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for (k, v) in s {
        let n = seen.entry(k).or_default();
        *n += 1;
        let key = if *n == 1 {
            k.clone()
        } else {
            format!("{k}#{n}")
        };
        out.insert(key, v.join(" "));
    }
    out
}

fn has_shutdown(s: &Section) -> bool {
    s.get("actions").is_some_and(|a| {
        a.values
            .iter()
            .any(|v| v.split('+').any(|x| x == "shutdown"))
    })
}

fn raised(old: &str, new: &str, descending: bool) -> bool {
    let p = |s: &str| {
        s.split_whitespace()
            .map(|x| x.parse::<i64>().ok())
            .collect::<Vec<_>>()
    };
    p(old).iter().zip(p(new)).any(|(o, n)| match (o, n) {
        (Some(o), Some(n)) => {
            if descending {
                n < *o
            } else {
                n > *o
            }
        }
        _ => false,
    })
}

fn qcom_safety(key: &str, old: Option<&str>, new: Option<&str>, sec: &Section) -> Option<String> {
    let trigger_keys = ["thresholds", "thresholds_clr", "set_point", "set_point_clr"];
    match (old, new) {
        (None, Some(_)) if key == "disable" => Some("adds the 'disable' flag to a section".into()),
        (Some(_), None) if trigger_keys.contains(&key) || key == "actions" || key == "sensor" => {
            Some(format!("removes {key} from a mitigation section"))
        }
        (Some(o), Some(n)) if trigger_keys.contains(&key) && raised(o, n, sec.descending()) => {
            Some(format!("moves a {key} value later (less protective)"))
        }
        (Some(o), Some(n))
            if key == "actions" && o.contains("shutdown") && !n.contains("shutdown") =>
        {
            Some("removes a shutdown action".into())
        }
        (Some(_), Some(_)) if key == "algo_type" || key == "descending" => {
            Some(format!("changes {key}"))
        }
        _ => None,
    }
}

pub fn qcom_diff(old: &QcomConfig, new: &QcomConfig) -> Vec<Change> {
    let mut out = Vec::new();
    let pairs = |s: &[super::qcom::Entry]| {
        s.iter()
            .map(|e| (e.key.clone(), e.values.clone()))
            .collect::<Vec<_>>()
    };
    let (og, ng) = (
        qcom_entries(&pairs(&old.globals())),
        qcom_entries(&pairs(&new.globals())),
    );
    for k in og.keys().chain(ng.keys().filter(|k| !og.contains_key(*k))) {
        let (o, n) = (og.get(k), ng.get(k));
        if o != n {
            let kind = if o.is_none() {
                ChangeKind::Added
            } else if n.is_none() {
                ChangeKind::Removed
            } else {
                ChangeKind::Changed
            };
            out.push(change(
                format!("(global).{k}"),
                kind,
                o.cloned(),
                n.cloned(),
                None,
            ));
        }
    }
    let os: Vec<Section> = old.sections();
    let ns: Vec<Section> = new.sections();
    for s in &os {
        let Some(t) = ns.iter().find(|x| x.name == s.name) else {
            let safety = s.algo_type().is_some().then(|| {
                format!(
                    "removes a {} mitigation section",
                    s.algo_type().unwrap_or("")
                )
            });
            let safety = if has_shutdown(s) {
                Some("removes a section with a shutdown action".into())
            } else {
                safety
            };
            out.push(change(
                format!("[{}]", s.name),
                ChangeKind::Removed,
                Some(format!("{} keys", s.entries.len())),
                None,
                safety,
            ));
            continue;
        };
        let (oe, ne) = (
            qcom_entries(&pairs(&s.entries)),
            qcom_entries(&pairs(&t.entries)),
        );
        for k in oe.keys().chain(ne.keys().filter(|k| !oe.contains_key(*k))) {
            let (o, n) = (oe.get(k), ne.get(k));
            if o != n {
                let kind = if o.is_none() {
                    ChangeKind::Added
                } else if n.is_none() {
                    ChangeKind::Removed
                } else {
                    ChangeKind::Changed
                };
                let base = k.split('#').next().unwrap_or(k);
                let safety = qcom_safety(base, o.map(String::as_str), n.map(String::as_str), s);
                out.push(change(
                    format!("[{}].{k}", s.name),
                    kind,
                    o.cloned(),
                    n.cloned(),
                    safety,
                ));
            }
        }
    }
    for t in ns.iter().filter(|t| !os.iter().any(|s| s.name == t.name)) {
        out.push(change(
            format!("[{}]", t.name),
            ChangeKind::Added,
            None,
            Some(format!("{} keys", t.entries.len())),
            None,
        ));
    }
    out
}

fn by_name(list: Vec<&Map<String, Value>>) -> Vec<(String, &Map<String, Value>)> {
    list.into_iter()
        .map(|m| {
            (
                m.get("Name")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string(),
                m,
            )
        })
        .collect()
}

fn hot_safety(old: &Value, new: &Value) -> Option<String> {
    let arr = |v: &Value| {
        v.as_array().map(|a| {
            a.iter()
                .map(|x| hal_float(x).unwrap_or(f32::NAN))
                .collect::<Vec<_>>()
        })
    };
    let (o, n) = (arr(old)?, arr(new)?);
    if o.len() != 7 {
        return None;
    }
    if n.len() != 7 {
        return Some("removes the hot threshold levels".into());
    }
    for i in 0..7 {
        if !o[i].is_nan() && n[i].is_nan() {
            return Some(format!("removes the {} level", SEVERITIES[i]));
        }
        if !o[i].is_nan() && n[i] > o[i] {
            return Some(format!(
                "raises the {} threshold from {} to {}",
                SEVERITIES[i], o[i], n[i]
            ));
        }
    }
    None
}

fn pixel_entity_diff(
    kind: &str,
    old: Vec<&Map<String, Value>>,
    new: Vec<&Map<String, Value>>,
    out: &mut Vec<Change>,
) {
    let (o, n) = (by_name(old), by_name(new));
    for (name, om) in &o {
        let loc = format!("{kind}[{name}]");
        let Some((_, nm)) = n.iter().find(|(x, _)| x == name) else {
            out.push(change(
                loc,
                ChangeKind::Removed,
                Some("present".into()),
                None,
                Some(format!("removes a {kind} entry")),
            ));
            continue;
        };
        for k in om.keys().chain(nm.keys().filter(|k| !om.contains_key(*k))) {
            let (ov, nv) = (om.get(k), nm.get(k));
            if ov == nv {
                continue;
            }
            let ck = match (ov, nv) {
                (None, _) => ChangeKind::Added,
                (_, None) => ChangeKind::Removed,
                _ => ChangeKind::Changed,
            };
            let safety = match (k.as_str(), ov, nv) {
                ("HotThreshold", Some(o), Some(n)) => hot_safety(o, n),
                ("HotThreshold", Some(_), None) => Some("removes all hot thresholds".into()),
                ("isDisabled", _, Some(Value::Bool(true))) => {
                    Some(format!("disables the {kind} entry"))
                }
                ("BindedCdevInfo", Some(_), _) => {
                    Some("changes which cooling devices this sensor throttles".into())
                }
                ("Type", Some(_), Some(_)) => Some("changes the sensor type".into()),
                _ => None,
            };
            out.push(change(
                format!("{loc}.{k}"),
                ck,
                ov.map(Value::to_string),
                nv.map(Value::to_string),
                safety,
            ));
        }
    }
    for (name, _) in n.iter().filter(|(x, _)| !o.iter().any(|(y, _)| y == x)) {
        out.push(change(
            format!("{kind}[{name}]"),
            ChangeKind::Added,
            None,
            Some("present".into()),
            None,
        ));
    }
}

pub fn pixel_diff(old: &PixelConfig, new: &PixelConfig) -> Vec<Change> {
    let mut out = Vec::new();
    pixel_entity_diff("Sensors", old.sensors(), new.sensors(), &mut out);
    pixel_entity_diff(
        "CoolingDevices",
        old.cooling_devices(),
        new.cooling_devices(),
        &mut out,
    );
    for k in ["Include", "PowerRails", "Stats", "Comment"] {
        let (o, n) = (old.root.get(k), new.root.get(k));
        if o != n {
            let kind = if o.is_none() {
                ChangeKind::Added
            } else if n.is_none() {
                ChangeKind::Removed
            } else {
                ChangeKind::Changed
            };
            let short = |v: Option<&Value>| v.map(|v| format!("{} bytes", v.to_string().len()));
            out.push(change(
                k.into(),
                kind,
                short(o),
                short(n),
                (k == "Include").then(|| "changes the included files".into()),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qcom_changes_and_safety() {
        let a = "sampling 5000\n[M]\nalgo_type monitor\nsensor s\nthresholds 40000 50000\nthresholds_clr 38000 48000\nactions cpu0 shutdown\naction_info 1 1\n[GONE]\nalgo_type ss\n";
        let b = "sampling 2000\n[M]\nalgo_type monitor\nsensor s\nthresholds 40000 60000\nthresholds_clr 38000 48000\nactions cpu0 report\naction_info 1 1\ndisable\n[NEW]\nalgo_type ss\n";
        let r = diff("a", a.as_bytes(), "b", b.as_bytes()).unwrap();
        let loc = |l: &str| {
            r.changes
                .iter()
                .find(|c| c.location == l)
                .unwrap_or_else(|| panic!("{l}: {:?}", r.changes))
        };
        assert_eq!(loc("(global).sampling").kind, ChangeKind::Changed);
        assert!(loc("[M].thresholds")
            .safety
            .as_deref()
            .unwrap()
            .contains("later"));
        assert_eq!(
            loc("[M].actions").safety.as_deref(),
            Some("removes a shutdown action")
        );
        assert!(loc("[M].disable").safety.is_some());
        assert!(loc("[GONE]").safety.is_some());
        assert_eq!(loc("[NEW]").kind, ChangeKind::Added);
        assert_eq!(r.safety_relevant, 4);
        // Lowering a trigger is more protective and is not flagged.
        let c = a.replace("50000", "45000");
        let r = diff("a", a.as_bytes(), "c", c.as_bytes()).unwrap();
        assert_eq!((r.changes.len(), r.safety_relevant), (1, 0));
        assert!(
            diff("a", a.as_bytes(), "a", a.as_bytes())
                .unwrap()
                .identical
        );
    }

    #[test]
    fn pixel_changes_and_safety() {
        let a = r#"{"Sensors": [{"Name": "skin", "Type": "SKIN", "HotThreshold": ["NaN", 39, 43, 45, 47, 52, 55]},
                    {"Name": "gone", "Type": "CPU"}], "CoolingDevices": [{"Name": "fan", "Type": "FAN"}]}"#;
        let b = r#"{"Sensors": [{"Name": "skin", "Type": "SKIN", "HotThreshold": ["NaN", 39, 43, 45, 47, 52, 60], "Multiplier": 1}],
                    "CoolingDevices": [{"Name": "fan", "Type": "FAN", "isDisabled": true}]}"#;
        let r = diff("a", a.as_bytes(), "b", b.as_bytes()).unwrap();
        let loc = |l: &str| r.changes.iter().find(|c| c.location == l).unwrap();
        assert!(loc("Sensors[skin].HotThreshold")
            .safety
            .as_deref()
            .unwrap()
            .contains("SHUTDOWN"));
        assert_eq!(loc("Sensors[skin].Multiplier").kind, ChangeKind::Added);
        assert!(loc("Sensors[gone]").safety.is_some());
        assert!(loc("CoolingDevices[fan].isDisabled").safety.is_some());
        assert_eq!(r.safety_relevant, 3);
    }

    #[test]
    fn mismatched_and_opaque() {
        assert!(diff("a", b"[A]\nalgo_type ss\n", "b", br#"{"Sensors": []}"#).is_err());
        let r = diff("a", b"\x00\x01", "b", b"\x00\x02").unwrap();
        assert_eq!(r.format, ConfigFormat::Opaque);
        assert_eq!(r.changes.len(), 1);
        assert!(r.note.is_some());
    }
}
