// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Qualcomm `thermal-engine.conf`.
//!
//! No public specification exists. The syntax below is what 13 published files show (AOSP
//! hammerhead, angler, marlin, crosshatch, taimen, bonito, sunfish, coral; LineageOS
//! oneplus3, g3, klte, msm8916, msm8953; see `docs/CONFIG_FORMATS.md`):
//!
//! - `[NAME]` starts a section; lines before the first section are global (`sampling 5000`);
//! - each other line is `key value...`, separated by runs of spaces and tabs;
//! - `#` starts a full-line comment; a key may be a bare flag (`descending`, `disable`);
//! - concurrent actions in one column are joined with `+` (`cpu0+cpu4+gpu`).
//!
//! The parser is lossless: every line, including ones it cannot classify, is kept with its
//! own line ending, so serializing the model reproduces the file byte for byte.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Finding, RoundTrip, SafetyFact, Severity};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineKind {
    Blank,
    Comment,
    Section {
        name: String,
    },
    Entry {
        key: String,
        values: Vec<String>,
    },
    /// Kept verbatim and reported; never interpreted.
    Unparsed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub number: usize,
    pub raw: String,
    /// `"\n"`, `"\r\n"` or `""` (last line without a newline).
    pub ending: String,
    #[serde(flatten)]
    pub kind: LineKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    pub values: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub line: usize,
    pub entries: Vec<Entry>,
    /// A `#algo_type ...` comment line appears in the section.
    pub commented_algo: bool,
    /// The values of those comment lines (`#algo_type ss` gives `"ss"`), never used as active.
    pub commented_algo_values: Vec<String>,
}

impl Section {
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }
    pub fn has_flag(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub fn algo_type(&self) -> Option<&str> {
        self.get("algo_type")
            .and_then(|e| e.values.first())
            .map(String::as_str)
    }
    pub fn descending(&self) -> bool {
        self.has_flag("descending")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QcomConfig {
    pub lines: Vec<Line>,
}

/// algo_type values seen in published files.
pub const OBSERVED_ALGOS: &[&str] = &["monitor", "ss", "virtual"];

/// Keys seen in published files. Others are kept and reported as unreviewed.
pub const OBSERVED_KEYS: &[&str] = &[
    "algo_type",
    "sampling",
    "sensor",
    "sensors",
    "thresholds",
    "thresholds_clr",
    "actions",
    "action_info",
    "descending",
    "disable",
    "device",
    "set_point",
    "set_point_clr",
    "device_max_limit",
    "device_perf_floor",
    "time_constant",
    "trip_sensor",
    "list_cnt",
    "weights",
    "math",
    "override",
    "action_type",
    "limit",
    "c_mode",
    "temp_range",
    "max_temp",
    "offset",
];

/// Action names seen in published files.
pub const OBSERVED_ACTIONS: &[&str] = &[
    "cpu",
    "cpu0",
    "cpu1",
    "cpu2",
    "cpu3",
    "cpu4",
    "cpu5",
    "cpu6",
    "cpu7",
    "cluster0",
    "cluster1",
    "gpu",
    "lcd",
    "battery",
    "shutdown",
    "report",
    "modem",
    "modem_proc",
    "hotplug_1",
    "hotplug_2",
    "hotplug_3",
    "hotplug_4",
    "hotplug_5",
    "hotplug_6",
    "hotplug_7",
    "vdd_restriction",
    "override",
    "bus",
    "mnh",
    "ab",
    "fcc",
    "dc_icl",
];

fn classify(raw: &str) -> LineKind {
    let t = raw.trim();
    if t.is_empty() {
        return LineKind::Blank;
    }
    if t.starts_with('#') {
        return LineKind::Comment;
    }
    if let Some(inner) = t.strip_prefix('[') {
        return match inner.strip_suffix(']') {
            Some(name) if !name.trim().is_empty() && !name.contains(['[', ']']) => {
                LineKind::Section {
                    name: name.trim().to_string(),
                }
            }
            _ => LineKind::Unparsed,
        };
    }
    let mut words = t.split_whitespace();
    let key = words.next().unwrap_or_default();
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return LineKind::Unparsed;
    }
    LineKind::Entry {
        key: key.to_string(),
        values: words.map(str::to_string).collect(),
    }
}

fn int(v: &str) -> Option<i64> {
    v.parse().ok()
}

impl QcomConfig {
    pub fn parse(text: &str) -> QcomConfig {
        let mut lines = Vec::new();
        let mut rest = text;
        let mut number = 0;
        while !rest.is_empty() {
            number += 1;
            let (body, ending, next) = match rest.find('\n') {
                Some(i) if i > 0 && rest.as_bytes()[i - 1] == b'\r' => {
                    (&rest[..i - 1], "\r\n", &rest[i + 1..])
                }
                Some(i) => (&rest[..i], "\n", &rest[i + 1..]),
                None => (rest, "", ""),
            };
            lines.push(Line {
                number,
                raw: body.to_string(),
                ending: ending.to_string(),
                kind: classify(body),
            });
            rest = next;
        }
        QcomConfig { lines }
    }

    /// Exactly the text the model was parsed from.
    pub fn serialize(&self) -> String {
        self.lines
            .iter()
            .map(|l| format!("{}{}", l.raw, l.ending))
            .collect()
    }

    pub fn round_trip(&self, original: &str) -> RoundTrip {
        let out = self.serialize();
        RoundTrip {
            byte_exact: out == original,
            model_equal: QcomConfig::parse(&out) == *self,
        }
    }

    /// Entries before the first section.
    pub fn globals(&self) -> Vec<Entry> {
        self.lines
            .iter()
            .take_while(|l| !matches!(l.kind, LineKind::Section { .. }))
            .filter_map(|l| match &l.kind {
                LineKind::Entry { key, values } => Some(Entry {
                    key: key.clone(),
                    values: values.clone(),
                    line: l.number,
                }),
                _ => None,
            })
            .collect()
    }

    pub fn sections(&self) -> Vec<Section> {
        let mut out: Vec<Section> = Vec::new();
        for l in &self.lines {
            match &l.kind {
                LineKind::Section { name } => out.push(Section {
                    name: name.clone(),
                    line: l.number,
                    entries: vec![],
                    commented_algo: false,
                    commented_algo_values: vec![],
                }),
                LineKind::Comment => {
                    if let Some(s) = out.last_mut() {
                        let body = l.raw.trim().trim_start_matches('#').trim_start();
                        let mut words = body.split_whitespace();
                        if words.next() == Some("algo_type") {
                            s.commented_algo = true;
                            s.commented_algo_values
                                .push(words.collect::<Vec<_>>().join(" "));
                        }
                    }
                }
                LineKind::Entry { key, values } => {
                    if let Some(s) = out.last_mut() {
                        s.entries.push(Entry {
                            key: key.clone(),
                            values: values.clone(),
                            line: l.number,
                        });
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn summary(&self) -> serde_json::Value {
        let sections = self.sections();
        let mut algos: BTreeMap<String, usize> = BTreeMap::new();
        for s in &sections {
            *algos
                .entry(s.algo_type().unwrap_or("(none)").to_string())
                .or_default() += 1;
        }
        let count = |f: fn(&LineKind) -> bool| self.lines.iter().filter(|l| f(&l.kind)).count();
        json!({
            "lines": self.lines.len(),
            "sections": sections.len(),
            "algo_types": algos,
            "globals": self.globals().len(),
            "comments": count(|k| matches!(k, LineKind::Comment)),
            "unparsed_lines": count(|k| matches!(k, LineKind::Unparsed)),
            "crlf_lines": self.lines.iter().filter(|l| l.ending == "\r\n").count(),
            "units": "values are kept as written; their unit depends on the sensor and is not normalized",
        })
    }

    pub fn safety_facts(&self) -> Vec<SafetyFact> {
        let mut out = Vec::new();
        for s in self.sections() {
            let loc = format!("[{}]", s.name);
            let sensor = s
                .get("sensor")
                .and_then(|e| e.values.first())
                .cloned()
                .unwrap_or_else(|| "?".into());
            if let (Some(t), Some(a)) = (s.get("thresholds"), s.get("actions")) {
                for (i, col) in a.values.iter().enumerate() {
                    if col.split('+').any(|x| x == "shutdown") {
                        let at = t.values.get(i).map(String::as_str).unwrap_or("?");
                        out.push(SafetyFact {
                            location: loc.clone(),
                            description: format!(
                                "shutdown when {sensor} reaches {at} (column {})",
                                i + 1
                            ),
                        });
                    }
                }
            }
            if s.has_flag("disable") {
                out.push(SafetyFact {
                    location: loc.clone(),
                    description: "section carries the 'disable' flag".into(),
                });
            }
        }
        out
    }

    pub fn validate(&self) -> Vec<Finding> {
        let mut f = Vec::new();
        for l in &self.lines {
            if l.kind == LineKind::Unparsed {
                f.push(Finding::new(
                    Severity::Warning,
                    "unparsed_line",
                    Some(format!("line {}", l.number)),
                    format!("line kept verbatim but not understood: {:?}", l.raw.trim()),
                ));
            }
            if let LineKind::Entry { values, .. } = &l.kind {
                if values.iter().any(|v| v.starts_with('#')) {
                    f.push(Finding::new(
                        Severity::Warning,
                        "inline_hash",
                        Some(format!("line {}", l.number)),
                        "a value starts with '#'; whether the engine treats it as a comment is unverified",
                    ));
                }
            }
        }
        for g in self.globals() {
            if !["sampling", "c_mode", "debug"].contains(&g.key.as_str()) {
                f.push(Finding::new(
                    Severity::Info,
                    "unreviewed_global",
                    Some(format!("line {}", g.line)),
                    format!(
                        "global key {:?} is not in the reviewed vocabulary; kept",
                        g.key
                    ),
                ));
            }
        }
        let mut names = BTreeSet::new();
        for s in self.sections() {
            let loc = Some(format!("[{}] line {}", s.name, s.line));
            if !names.insert(s.name.clone()) {
                f.push(Finding::new(
                    Severity::Error,
                    "duplicate_section",
                    loc.clone(),
                    "section name appears more than once",
                ));
            }
            let mut keys = BTreeSet::new();
            for e in &s.entries {
                if !keys.insert(e.key.as_str()) {
                    f.push(Finding::new(
                        Severity::Warning,
                        "duplicate_key",
                        Some(format!("[{}] line {}", s.name, e.line)),
                        format!("key {:?} repeats in the section; which one the engine uses is unverified", e.key),
                    ));
                }
                if !OBSERVED_KEYS.contains(&e.key.as_str()) {
                    f.push(Finding::new(
                        Severity::Info,
                        "unreviewed_key",
                        Some(format!("[{}] line {}", s.name, e.line)),
                        format!("key {:?} is not in the reviewed vocabulary; kept", e.key),
                    ));
                }
            }
            if let Some(e) = s.get("algo_type") {
                if e.values.is_empty() {
                    f.push(Finding::new(
                        Severity::Error,
                        "algo_type_empty",
                        Some(format!("[{}] line {}", s.name, e.line)),
                        "algo_type has no value; the section cannot be validated",
                    ));
                } else if e.values.len() > 1 {
                    f.push(Finding::new(
                        Severity::Warning,
                        "algo_type_extra_values",
                        Some(format!("[{}] line {}", s.name, e.line)),
                        format!(
                            "algo_type has several values {:?}; only the first is checked",
                            e.values
                        ),
                    ));
                }
            }
            match s.algo_type() {
                None if s.get("algo_type").is_some() => {}
                None if s.commented_algo => {
                    validate_untyped(&s, &mut f);
                    f.push(Finding::new(
                        Severity::Info,
                        "algo_type_commented_out",
                        loc,
                        format!(
                            "algo_type is commented out ({:?}) and is not treated as active; \
                             algorithm-specific checks were skipped, only key types were checked. \
                             Whether the engine ignores such a section is unverified",
                            s.commented_algo_values
                        ),
                    ))
                }
                None => {
                    validate_untyped(&s, &mut f);
                    f.push(Finding::new(
                        Severity::Info,
                        "no_algo_type",
                        loc,
                        "section has no active algo_type; algorithm-specific checks were skipped, only key types were checked",
                    ))
                }
                Some("monitor") => validate_monitor(&s, &mut f),
                Some("ss") => validate_ss(&s, &mut f),
                Some("virtual") => validate_virtual(&s, &mut f),
                Some(other) => f.push(Finding::new(
                    Severity::Warning,
                    "unverified_algo_type",
                    loc,
                    format!("algo_type {other:?} was not seen in any reviewed file; its semantics are unverified"),
                )),
            }
        }
        f
    }
}

fn need(s: &Section, keys: &[&str], f: &mut Vec<Finding>) {
    for k in keys {
        if s.get(k).is_none() {
            f.push(Finding::new(
                Severity::Warning,
                "missing_key",
                Some(format!("[{}]", s.name)),
                format!("{} section without {k:?}", s.algo_type().unwrap_or("")),
            ));
        }
    }
}

fn ints(s: &Section, key: &str, f: &mut Vec<Finding>) -> Option<Vec<i64>> {
    let e = s.get(key)?;
    let parsed: Vec<Option<i64>> = e.values.iter().map(|v| int(v)).collect();
    if parsed.iter().any(Option::is_none) {
        f.push(Finding::new(
            Severity::Error,
            "not_integer",
            Some(format!("[{}] line {}", s.name, e.line)),
            format!("{key} must be integers: {:?}", e.values),
        ));
        return None;
    }
    Some(parsed.into_iter().flatten().collect())
}

/// Checks that hold for every observed algorithm: trigger and clear values are integers.
fn validate_untyped(s: &Section, f: &mut Vec<Finding>) {
    for key in ["thresholds", "thresholds_clr", "set_point", "set_point_clr"] {
        let _ = ints(s, key, f);
    }
}

fn validate_monitor(s: &Section, f: &mut Vec<Finding>) {
    need(
        s,
        &[
            "sensor",
            "thresholds",
            "thresholds_clr",
            "actions",
            "action_info",
        ],
        f,
    );
    let loc = Some(format!("[{}]", s.name));
    let counts: Vec<(&str, usize)> = ["thresholds", "thresholds_clr", "actions", "action_info"]
        .iter()
        .filter_map(|k| s.get(k).map(|e| (*k, e.values.len())))
        .collect();
    if counts.windows(2).any(|w| w[0].1 != w[1].1) {
        f.push(Finding::new(
            Severity::Error,
            "column_count",
            loc.clone(),
            format!("column counts differ: {counts:?}"),
        ));
    }
    if let (Some(a), Some(i)) = (s.get("actions"), s.get("action_info")) {
        for (n, (act, info)) in a.values.iter().zip(&i.values).enumerate() {
            if act.split('+').count() != info.split('+').count() {
                f.push(Finding::new(
                    Severity::Error,
                    "action_info_count",
                    loc.clone(),
                    format!(
                        "column {}: {act:?} has a different number of '+' parts than {info:?}",
                        n + 1
                    ),
                ));
            }
        }
        for act in a.values.iter().flat_map(|v| v.split('+')) {
            if !OBSERVED_ACTIONS.contains(&act) {
                f.push(Finding::new(
                    Severity::Info,
                    "unreviewed_action",
                    loc.clone(),
                    format!("action {act:?} was not seen in any reviewed file"),
                ));
            }
        }
    }
    let desc = s.descending();
    let t = ints(s, "thresholds", f);
    let c = ints(s, "thresholds_clr", f);
    if let Some(t) = &t {
        if t.windows(2)
            .any(|w| if desc { w[1] > w[0] } else { w[1] < w[0] })
        {
            f.push(Finding::new(
                Severity::Error,
                "threshold_order",
                loc.clone(),
                format!(
                    "thresholds are not {}: {t:?}",
                    if desc {
                        "descending (flag set)"
                    } else {
                        "ascending"
                    }
                ),
            ));
        }
    }
    if let (Some(t), Some(c)) = (t, c) {
        for (n, (thr, clr)) in t.iter().zip(&c).enumerate() {
            let inverted = if desc { clr <= thr } else { clr >= thr };
            if inverted {
                f.push(Finding::new(
                    Severity::Warning,
                    "clear_not_past_trigger",
                    loc.clone(),
                    format!(
                        "column {}: clear value {clr} is not {} the trigger {thr}; mitigation may never clear or may oscillate",
                        n + 1,
                        if desc { "above" } else { "below" }
                    ),
                ));
            }
        }
    }
}

fn validate_ss(s: &Section, f: &mut Vec<Finding>) {
    need(s, &["sensor", "device", "set_point", "set_point_clr"], f);
    let sp = ints(s, "set_point", f);
    let clr = ints(s, "set_point_clr", f);
    if let (Some(sp), Some(clr)) = (sp, clr) {
        if let (Some(sp), Some(clr)) = (sp.first(), clr.first()) {
            let inverted = if s.descending() { clr <= sp } else { clr >= sp };
            if inverted {
                f.push(Finding::new(
                    Severity::Warning,
                    "clear_not_past_trigger",
                    Some(format!("[{}]", s.name)),
                    format!("set_point_clr {clr} is not past set_point {sp}"),
                ));
            }
        }
    }
}

fn validate_virtual(s: &Section, f: &mut Vec<Finding>) {
    need(s, &["sensors"], f);
    let loc = Some(format!("[{}]", s.name));
    let n = s.get("sensors").map(|e| e.values.len()).unwrap_or(0);
    if let Some(cnt) = s.get("list_cnt").and_then(|e| e.values.first()) {
        if int(cnt) != Some(n as i64) {
            f.push(Finding::new(
                Severity::Error,
                "list_cnt",
                loc.clone(),
                format!("list_cnt {cnt} but {n} sensors listed"),
            ));
        }
    }
    if let Some(w) = s.get("weights") {
        if w.values.len() != n {
            f.push(Finding::new(
                Severity::Error,
                "weights_count",
                loc,
                format!("{} weights for {n} sensors", w.values.len()),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic file in the observed syntax (not copied from a vendor file).
    const SAMPLE: &str = "sampling 5000\r\n\
# comment\r\n\
\r\n\
[SKIN-MONITOR]\r\n\
algo_type\t\tmonitor\r\n\
sampling        1000\r\n\
sensor  skin-therm\r\n\
thresholds 45000 47000 57000\r\n\
thresholds_clr 43000 45000 56000\r\n\
actions cpu0+gpu cpu0+gpu shutdown\r\n\
action_info 900000+400000000 700000+250000000 1\r\n\
\r\n\
[VIRT]\r\n\
algo_type virtual\r\n\
sensors a b\r\n\
list_cnt 2\r\n\
weights 1 -1\r\n\
\r\n\
[CAL]\r\n\
some_key 1\r\n\
odd line ~\r\n\
[broken\r\n\
disable";

    #[test]
    fn lossless_round_trip() {
        let c = QcomConfig::parse(SAMPLE);
        assert_eq!(c.serialize(), SAMPLE);
        assert_eq!(
            c.round_trip(SAMPLE),
            RoundTrip {
                byte_exact: true,
                model_equal: true
            }
        );
        // Mixed endings and a missing final newline survive too.
        let mixed = "[A]\nalgo_type ss\r\nsensor x";
        assert_eq!(QcomConfig::parse(mixed).serialize(), mixed);
    }

    #[test]
    fn structure() {
        let c = QcomConfig::parse(SAMPLE);
        assert_eq!(c.globals().len(), 1);
        let s = c.sections();
        assert_eq!(
            s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["SKIN-MONITOR", "VIRT", "CAL"]
        );
        assert_eq!(s[0].algo_type(), Some("monitor"));
        assert_eq!(s[0].get("actions").unwrap().values[2], "shutdown");
        assert!(s[2].has_flag("disable"));
        let facts = c.safety_facts();
        assert!(facts[0]
            .description
            .contains("shutdown when skin-therm reaches 57000"));
        assert_eq!(c.summary()["unparsed_lines"], 1); // "odd line ~" is an entry; "[broken" is not
    }

    #[test]
    fn validation() {
        let codes = |t: &str| {
            QcomConfig::parse(t)
                .validate()
                .into_iter()
                .map(|f| f.code)
                .collect::<Vec<_>>()
        };
        let clean = codes(SAMPLE);
        assert!(clean.contains(&"unparsed_line".to_string()));
        assert!(clean.contains(&"no_algo_type".to_string()));
        assert!(!clean.contains(&"column_count".to_string()), "{clean:?}");

        let bad = "[M]\nalgo_type monitor\nsensor s\nthresholds 50000 40000\nthresholds_clr 51000 39000\nactions cpu0+gpu cpu0\naction_info 1 2\n[M]\nalgo_type pid\n[V]\nalgo_type virtual\nsensors a b c\nlist_cnt 2\nweights 1\n";
        let c = codes(bad);
        for want in [
            "threshold_order",
            "clear_not_past_trigger",
            "action_info_count",
            "duplicate_section",
            "unverified_algo_type",
            "list_cnt",
            "weights_count",
        ] {
            assert!(c.contains(&want.to_string()), "missing {want}: {c:?}");
        }
        let desc = "[B]\nalgo_type monitor\nsensor s\ndescending\nthresholds 20 10\nthresholds_clr 25 15\nactions battery battery\naction_info 1 2\n";
        assert!(!codes(desc)
            .iter()
            .any(|c| c == "threshold_order" || c == "clear_not_past_trigger"));
        assert!(
            codes("[A]\nalgo_type monitor\nthresholds 4x\n").contains(&"not_integer".to_string())
        );
    }

    fn findings(t: &str) -> Vec<Finding> {
        QcomConfig::parse(t).validate()
    }

    #[test]
    fn commented_directives_are_never_active() {
        // A commented algo_type with a monitor-looking body: no monitor checks run, the
        // comment is named in the diagnostic, and the file still round-trips exactly.
        let t = "[S]\n#algo_type monitor\nsensor s\nthresholds 50000 40000\nactions cpu0\naction_info 1 2\n";
        let c = QcomConfig::parse(t);
        assert_eq!(c.serialize(), t);
        let sec = &c.sections()[0];
        assert_eq!(sec.algo_type(), None);
        assert_eq!(sec.commented_algo_values, ["monitor"]);
        let f = c.validate();
        let codes: Vec<&str> = f.iter().map(|x| x.code.as_str()).collect();
        assert!(codes.contains(&"algo_type_commented_out"), "{codes:?}");
        assert!(
            !codes.contains(&"threshold_order") && !codes.contains(&"column_count"),
            "{codes:?}"
        );
        assert!(f.iter().any(|x| x.message.contains("[\"monitor\"]")));
    }

    #[test]
    fn whitespace_variants_of_comments_and_directives() {
        for t in [
            "[S]\n#algo_type ss\n",
            "[S]\n  #   algo_type\tss\n",
            "[S]\n#\talgo_type   ss  \n",
        ] {
            let c = QcomConfig::parse(t);
            assert_eq!(c.serialize(), t);
            assert_eq!(c.sections()[0].commented_algo_values, ["ss"], "{t:?}");
            assert_eq!(c.sections()[0].algo_type(), None);
        }
        // Not a commented algo_type: different key.
        assert!(!QcomConfig::parse("[S]\n#algo_types ss\n").sections()[0].commented_algo);
        // Active directives with tabs and runs of spaces are active.
        for t in ["[S]\nalgo_type\t\tss\n", "[S]\n   algo_type    ss\n"] {
            assert_eq!(QcomConfig::parse(t).sections()[0].algo_type(), Some("ss"));
        }
    }

    #[test]
    fn missing_empty_and_extra_algo_type() {
        let codes = |t: &str| findings(t).into_iter().map(|f| f.code).collect::<Vec<_>>();
        assert!(codes("[S]\nsensor s\n").contains(&"no_algo_type".to_string()));
        let c = codes("[S]\nalgo_type\nsensor s\n");
        assert!(
            c.contains(&"algo_type_empty".to_string()) && !c.contains(&"no_algo_type".to_string()),
            "{c:?}"
        );
        assert!(codes(
            "[S]\nalgo_type ss monitor\nsensor s\ndevice cpu0\nset_point 1\nset_point_clr 0\n"
        )
        .contains(&"algo_type_extra_values".to_string()));
        // Untyped sections still get key-type checks.
        assert!(codes("[S]\n#algo_type ss\nset_point 4x\n").contains(&"not_integer".to_string()));
        assert!(codes("[S]\nset_point abc\n").contains(&"not_integer".to_string()));
    }

    #[test]
    fn malformed_sections_are_kept_and_reported() {
        for t in [
            "[ ]\nalgo_type ss\n",
            "[A]x\nalgo_type ss\n",
            "[[A]]\nalgo_type ss\n",
            "[A\nalgo_type ss\n",
        ] {
            let c = QcomConfig::parse(t);
            assert_eq!(c.serialize(), t, "{t:?}");
            assert!(
                c.validate().iter().any(|f| f.code == "unparsed_line"),
                "{t:?}"
            );
            assert!(c.sections().is_empty(), "{t:?}");
        }
    }
}
