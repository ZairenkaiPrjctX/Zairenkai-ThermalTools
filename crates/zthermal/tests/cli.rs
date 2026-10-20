//! End-to-end tests of the `zthermal` binary against the synthetic fixtures in
//! `tests/fixtures` (repository root). No test touches the host's /sys, /proc or /vendor:
//! every device command runs with `--root`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_zthermal"))
        .args(args)
        .current_dir(repo())
        .output()
        .expect("runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn device(name: &str) -> String {
    format!("tests/fixtures/devices/{name}")
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(repo().join("tests/golden").join(name)).expect("golden file")
}

fn json(args: &[&str]) -> serde_json::Value {
    let (code, out, err) = run(args);
    assert!(
        code == 0 || code == 3 || code == 5,
        "{args:?}: exit {code}: {err}"
    );
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("{args:?}: {e}: {out}"))
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(run(&[]).0, 2);
    assert_eq!(run(&["frobnicate"]).0, 2);
    assert_eq!(run(&["apply", "--mode", "turbo", "--dry-run"]).0, 2);
    assert_eq!(run(&["--help"]).0, 0);
}

#[test]
fn golden_text_outputs() {
    let (code, out, _) = run(&[
        "validate-config",
        "tests/fixtures/configs/qcom-invalid.conf",
    ]);
    assert_eq!((code, out), (3, golden("validate-qcom-invalid.txt")));
    let (code, out, _) = run(&[
        "diff-config",
        "tests/fixtures/configs/qcom-base.conf",
        "tests/fixtures/configs/qcom-weakened.conf",
    ]);
    assert_eq!((code, out), (3, golden("diff-qcom-weakened.txt")));
    let (code, out, _) = run(&["inspect-config", "tests/fixtures/configs/pixel-base.json"]);
    assert_eq!((code, out), (0, golden("inspect-pixel-base.txt")));
    let (code, out, _) = run(&[
        "sensors",
        "--framework",
        "--root",
        &device("synthetic-qcom"),
    ]);
    assert_eq!((code, out), (0, golden("sensors-synthetic-qcom.txt")));
}

#[test]
fn valid_configs_and_identical_diffs_pass() {
    assert_eq!(
        run(&["validate-config", "tests/fixtures/configs/qcom-base.conf"]).0,
        0
    );
    assert_eq!(
        run(&["validate-config", "tests/fixtures/configs/pixel-base.json"]).0,
        0
    );
    assert_eq!(
        run(&[
            "validate-config",
            "tests/fixtures/configs/pixel-invalid.json"
        ])
        .0,
        3
    );
    assert_eq!(
        run(&[
            "diff-config",
            "tests/fixtures/configs/qcom-base.conf",
            "tests/fixtures/configs/qcom-base.conf"
        ])
        .0,
        0
    );
    assert_eq!(
        run(&[
            "diff-config",
            "tests/fixtures/configs/qcom-base.conf",
            "tests/fixtures/configs/pixel-base.json"
        ])
        .0,
        1
    );
    let v = json(&[
        "inspect-config",
        "--json",
        "tests/fixtures/configs/binary.bin",
    ]);
    assert_eq!(v["detection"]["format"], "opaque");
}

#[test]
fn device_commands_on_fixtures() {
    let v = json(&["device-info", "--json", "--root", &device("synthetic-qcom")]);
    assert_eq!(v["identity"]["soc_vendor"]["value"], "qualcomm");
    let v = json(&["capabilities", "--json", "--root", &device("synthetic-mtk")]);
    let caps = v["capabilities"].as_array().unwrap();
    let mtk = caps
        .iter()
        .find(|c| c["id"] == "config.mediatek_thermal_policy")
        .unwrap();
    assert_eq!(mtk["state"], "DETECTED_UNVERIFIED");
    assert!(caps
        .iter()
        .all(|c| c["state"] != "AVAILABLE_WRITABLE" && c["hardware_tested"] == false));
    let v = json(&[
        "thermal-status",
        "--json",
        "--root",
        &device("synthetic-pixel"),
    ]);
    assert_eq!(v["summary"]["framework_status"], serde_json::Value::Null);
    assert!(v["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["status"] == "permission_denied"));
    let v = json(&[
        "cooling-devices",
        "--json",
        "--root",
        &device("synthetic-qcom"),
    ]);
    assert_eq!(v["cooling_devices"].as_array().unwrap().len(), 2);
    assert_eq!(run(&["sensors", "--root", &device("empty-host")]).0, 5);
}

#[test]
fn snapshot_is_redacted_by_default() {
    let v = json(&["snapshot", "--root", &device("synthetic-qcom")]);
    assert_eq!(v["redacted"], true);
    assert!(v["identity"]["build_fingerprint"]
        .as_str()
        .unwrap()
        .starts_with("redacted:sha256:"));
    let v = json(&[
        "snapshot",
        "--no-redact",
        "--root",
        &device("synthetic-qcom"),
    ]);
    assert_eq!(
        v["identity"]["build_fingerprint"],
        "synthetic/synthqcom/synthqcom:13/TEST.1/1:user/test-keys"
    );
    let (code, out, _) = run(&["report", "--root", &device("synthetic-qcom")]);
    assert_eq!(code, 0);
    assert!(out.contains("not sent anywhere"));
}

#[test]
fn apply_writes_nothing_and_rollback_is_a_no_op() {
    let root = device("synthetic-qcom");
    let (code, out, _) = run(&[
        "apply",
        "--mode",
        "performance",
        "--dry-run",
        "--root",
        &root,
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("writes: none"));
    let (code, out, _) = run(&["apply", "--mode", "gaming", "--root", &root]);
    assert_eq!(code, 4, "{out}");
    assert!(
        !repo().join(&root).join("data").exists(),
        "no journal may be created"
    );
    assert_eq!(run(&["rollback", "--root", &root]).0, 0);
}

#[test]
fn profiles_and_backup() {
    assert_eq!(run(&["profile-validate"]).0, 0);
    assert_eq!(run(&["profile-validate", "--profiles", "profiles"]).0, 0);
    assert_eq!(
        run(&["profile-validate", "tests/fixtures/configs/pixel-base.json"]).0,
        3
    );
    let dest = std::env::temp_dir().join(format!("zthermal-bk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    let d = dest.to_string_lossy().into_owned();
    let v = json(&[
        "backup-config",
        "--json",
        "--root",
        &device("synthetic-pixel"),
        "--dest",
        &d,
    ]);
    assert_eq!(v["manifest"]["entries"].as_array().unwrap().len(), 1);
    assert!(v["verified"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["ok"] == true));
    assert_eq!(
        run(&[
            "backup-config",
            "--root",
            &device("synthetic-pixel"),
            "--dest",
            &d
        ])
        .0,
        1,
        "never overwrites"
    );
    let _ = std::fs::remove_dir_all(&dest);
}
