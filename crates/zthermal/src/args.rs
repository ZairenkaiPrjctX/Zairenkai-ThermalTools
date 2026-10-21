// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Command-line parsing (standard library only).

use std::path::PathBuf;

pub const USAGE: &str = "usage: zthermal <command> [options]

commands:
  doctor                       check what this environment allows
  device-info                  device identity (SoC, OEM) with evidence
  capabilities                 what can be read or controlled here, and why
  sensors                      thermal zones and hwmon temperatures
  thermal-status               summary: hottest zone, framework status, battery
  cooling-devices              cooling device states
  inspect-config <file>        detect, parse and summarize a vendor configuration
  validate-config <file>       validate a vendor configuration (exit 3 on errors)
  diff-config <old> <new>      semantic diff (exit 3 when a change weakens protection)
  backup-config --dest <dir>   copy every discovered vendor configuration, with SHA-256
  profile-list                 list device profiles
  profile-validate [<file>..]  validate profiles (exit 3 on problems)
  snapshot                     everything above as one JSON document
  report                       snapshot rendered as text (or --format json)
  apply --mode <m> --dry-run   plan a policy mode (diagnostic, balanced, power_saver,
                               performance, gaming, adaptive); nothing is written
  rollback                     restore values recorded in the transaction journal

options:
  --root <dir>       read a fixture directory instead of the live system; commands come
                     from <dir>/_commands/<argv joined by _>.txt
  --json             JSON output (commands other than snapshot default to text)
  --format <f>       json or text (report)
  --framework        also run dumpsys thermalservice / battery (needs adb shell or root)
  --no-redact        keep build fingerprints in snapshot and report
  -o, --output <f>   write the output to a file
  --profiles <dir>   profile directory (default: profiles built into the binary)
  --journal <file>   transaction journal (default /data/local/tmp/zthermal/journal.json)
  --dest <dir>       backup directory (must not exist or be empty)
  -h, --help, --version";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub command: String,
    pub positional: Vec<String>,
    pub root: Option<PathBuf>,
    pub json: bool,
    pub framework: bool,
    pub redact: bool,
    pub output: Option<PathBuf>,
    pub profiles: Option<PathBuf>,
    pub journal: Option<PathBuf>,
    pub dest: Option<PathBuf>,
    pub mode: Option<String>,
    pub dry_run: bool,
}

pub const COMMANDS: &[&str] = &[
    "doctor",
    "device-info",
    "capabilities",
    "sensors",
    "thermal-status",
    "cooling-devices",
    "inspect-config",
    "validate-config",
    "diff-config",
    "backup-config",
    "profile-list",
    "profile-validate",
    "snapshot",
    "report",
    "apply",
    "rollback",
    "help",
    "version",
];

pub fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        command: String::new(),
        positional: vec![],
        root: None,
        json: false,
        framework: false,
        redact: true,
        output: None,
        profiles: None,
        journal: None,
        dest: None,
        mode: None,
        dry_run: false,
    };
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => a.command = "help".into(),
            "--version" => a.command = "version".into(),
            "--root" => a.root = Some(value("--root")?.into()),
            "--json" => a.json = true,
            "--format" => match value("--format")?.as_str() {
                "json" => a.json = true,
                "text" => a.json = false,
                f => return Err(format!("unknown format {f:?}")),
            },
            "--framework" => a.framework = true,
            "--no-redact" => a.redact = false,
            "-o" | "--output" => a.output = Some(value("--output")?.into()),
            "--profiles" => a.profiles = Some(value("--profiles")?.into()),
            "--journal" => a.journal = Some(value("--journal")?.into()),
            "--dest" => a.dest = Some(value("--dest")?.into()),
            "--mode" => a.mode = Some(value("--mode")?),
            "--dry-run" => a.dry_run = true,
            s if s.starts_with('-') => return Err(format!("unknown option {s:?}")),
            s if a.command.is_empty() => a.command = s.to_string(),
            s => a.positional.push(s.to_string()),
        }
    }
    if a.command.is_empty() {
        return Err("no command".into());
    }
    if !COMMANDS.contains(&a.command.as_str()) {
        return Err(format!("unknown command {:?}", a.command));
    }
    let want = match a.command.as_str() {
        "inspect-config" | "validate-config" => Some(1),
        "diff-config" => Some(2),
        "profile-validate" => None,
        _ => Some(0),
    };
    if let Some(n) = want {
        if a.positional.len() != n {
            return Err(format!(
                "{} takes {n} argument(s), got {}",
                a.command,
                a.positional.len()
            ));
        }
    }
    if a.command == "apply" && a.mode.is_none() {
        return Err("apply needs --mode".into());
    }
    if a.command == "backup-config" && a.dest.is_none() {
        return Err("backup-config needs --dest".into());
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Args, String> {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn parses_commands_and_options() {
        let a = p("sensors --root /tmp/x --json").unwrap();
        assert_eq!(
            (a.command.as_str(), a.json, a.root.as_deref()),
            ("sensors", true, Some(std::path::Path::new("/tmp/x")))
        );
        assert_eq!(p("diff-config a b").unwrap().positional, ["a", "b"]);
        assert!(p("apply --mode gaming --dry-run").unwrap().dry_run);
        assert!(!p("report --no-redact").unwrap().redact);
    }

    #[test]
    fn rejects_bad_usage() {
        assert!(p("").is_err());
        assert!(p("frobnicate").is_err());
        assert!(p("sensors --wat").is_err());
        assert!(p("inspect-config").is_err());
        assert!(p("diff-config a").is_err());
        assert!(p("apply --dry-run").is_err());
        assert!(p("report --format xml").is_err());
        assert!(p("sensors --root").is_err());
        assert!(p("backup-config").is_err());
    }
}
