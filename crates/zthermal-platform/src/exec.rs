//! Running Android tools (`getprop`, `dumpsys`) with an argument list, a timeout and an
//! output cap. Never through a shell; tests use [`FakeRunner`].

use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum ExecError {
    /// The program does not exist (e.g. `dumpsys` on a Linux host).
    NotFound(String),
    /// It ran and failed (non-zero exit), e.g. missing android.permission.DUMP.
    Failed(String),
    TimedOut(String),
    OutputTooLarge(String),
}

pub trait Runner {
    fn run(&self, argv: &[&str], timeout: Duration) -> Result<String, ExecError>;
}

pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, argv: &[&str], timeout: Duration) -> Result<String, ExecError> {
        let (prog, args) = argv
            .split_first()
            .ok_or_else(|| ExecError::Failed("empty command".into()))?;
        let mut child = Command::new(prog)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ExecError::NotFound(format!("{prog}: not found"))
                } else {
                    ExecError::Failed(format!("{prog}: {e}"))
                }
            })?;
        let mut stdout = child.stdout.take().expect("piped");
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let r = (&mut stdout)
                .take(MAX_OUTPUT_BYTES as u64 + 1)
                .read_to_end(&mut buf);
            (r, buf)
        });
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if start.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ExecError::TimedOut(format!(
                        "{}: no result after {timeout:?}",
                        argv.join(" ")
                    )));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => return Err(ExecError::Failed(format!("{prog}: {e}"))),
            }
        };
        let (_, buf) = reader
            .join()
            .map_err(|_| ExecError::Failed("reader thread panicked".into()))?;
        if buf.len() > MAX_OUTPUT_BYTES {
            return Err(ExecError::OutputTooLarge(format!(
                "{}: more than {MAX_OUTPUT_BYTES} bytes",
                argv.join(" ")
            )));
        }
        if !status.success() {
            return Err(ExecError::Failed(format!(
                "{}: exit {status}",
                argv.join(" ")
            )));
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// Canned outputs keyed by the space-joined argv. Unknown commands are "not found".
#[derive(Default)]
pub struct FakeRunner {
    pub outputs: BTreeMap<String, Result<String, ExecError>>,
}

impl FakeRunner {
    pub fn with(mut self, cmd: &str, out: &str) -> Self {
        self.outputs.insert(cmd.into(), Ok(out.into()));
        self
    }

    pub fn failing(mut self, cmd: &str, err: ExecError) -> Self {
        self.outputs.insert(cmd.into(), Err(err));
        self
    }
}

impl Runner for FakeRunner {
    fn run(&self, argv: &[&str], _timeout: Duration) -> Result<String, ExecError> {
        let key = argv.join(" ");
        self.outputs
            .get(&key)
            .cloned()
            .unwrap_or(Err(ExecError::NotFound(format!("{key}: not found"))))
    }
}

/// Canned outputs stored in a fixture directory: `argv` joined with `_` names the file,
/// `<dir>/getprop.txt` or `<dir>/dumpsys_thermalservice.txt`. A `.fail` file instead makes
/// the command fail with its content; a missing file means "not found".
pub struct FixtureRunner {
    pub dir: std::path::PathBuf,
}

impl Runner for FixtureRunner {
    fn run(&self, argv: &[&str], _timeout: Duration) -> Result<String, ExecError> {
        let name = argv.join("_");
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
            || name.contains("..")
        {
            return Err(ExecError::Failed(format!("{name:?}: not a fixture name")));
        }
        let out = self.dir.join(format!("{name}.txt"));
        let fail = self.dir.join(format!("{name}.fail"));
        if let Ok(msg) = std::fs::read_to_string(&fail) {
            return Err(ExecError::Failed(msg.trim().to_string()));
        }
        match std::fs::read(&out) {
            Ok(b) if b.len() > MAX_OUTPUT_BYTES => Err(ExecError::OutputTooLarge(name)),
            Ok(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
            Err(_) => Err(ExecError::NotFound(format!(
                "{}: not found (no fixture)",
                argv.join(" ")
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_runner_reports_missing_and_failing_programs() {
        let r = ProcessRunner;
        assert!(matches!(
            r.run(&["zthermal-no-such-program"], Duration::from_secs(1)),
            Err(ExecError::NotFound(_))
        ));
        assert!(matches!(
            r.run(&["false"], Duration::from_secs(5)),
            Err(ExecError::Failed(_))
        ));
        assert_eq!(
            r.run(&["echo", "a b"], Duration::from_secs(5)).unwrap(),
            "a b\n"
        );
        // Arguments are never interpreted by a shell.
        assert_eq!(
            r.run(&["echo", "$(id)"], Duration::from_secs(5)).unwrap(),
            "$(id)\n"
        );
        assert!(matches!(
            r.run(&["sleep", "5"], Duration::from_millis(100)),
            Err(ExecError::TimedOut(_))
        ));
    }

    #[test]
    fn fixture_runner() {
        let t = crate::sysroot::testutil::TempDir::new("fixture-runner");
        t.file("getprop.txt", "[a]: [b]\n");
        t.file("dumpsys_battery.fail", "Permission denied\n");
        let r = FixtureRunner {
            dir: t.path().to_path_buf(),
        };
        assert_eq!(
            r.run(&["getprop"], Duration::from_secs(1)).unwrap(),
            "[a]: [b]\n"
        );
        assert_eq!(
            r.run(&["dumpsys", "battery"], Duration::from_secs(1)),
            Err(ExecError::Failed("Permission denied".into()))
        );
        assert!(matches!(
            r.run(&["dumpsys", "thermalservice"], Duration::from_secs(1)),
            Err(ExecError::NotFound(_))
        ));
        assert!(matches!(
            r.run(&["../x"], Duration::from_secs(1)),
            Err(ExecError::Failed(_))
        ));
    }
}
