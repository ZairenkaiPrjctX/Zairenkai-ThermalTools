# Testing

## Test categories

These are different claims and are never merged:

| Category | What it proves | Where |
|---|---|---|
| Host unit tests | Logic on synthetic inputs (parsers, policy, diff, transactions) | `cargo test` in each crate |
| Synthetic fixture tests | CLI behaviour against fake device trees under `--root` | `crates/zthermal/tests/cli.rs`, `tests/fixtures/` |
| Published-file checks | Parsers against real published configurations | `CONFIG_FORMATS.md` (run by hand; files not committed) |
| Emulator / controlled Android | Behaviour on Android userspace without vendor hardware | **not performed** |
| Physical-device tests | Behaviour on a real device and its vendor stack | **not performed**; procedure below |

A Linux host run is never a physical Android validation. Building Android binaries
(`scripts/build-android.sh`, CI `android` job) proves that they link, not that they run.

## Automated checks

| Check | Command |
|---|---|
| Unit and integration tests | `cargo test --workspace` |
| MSRV | `cargo +1.85.0 test --workspace` |
| Lint and format | `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check` |
| JSON schemas | `python3 scripts/check-schemas.py target/debug/zthermal` (needs `jsonschema`) |
| Shell scripts | `shellcheck scripts/*.sh` |
| Android binaries | `ANDROID_NDK_HOME=<ndk> scripts/build-android.sh` |

What they cover:
- **Core:**
  - normalization and plausibility;
  - identity conflicts and profile rules;
  - the policy deny-list and the adaptive state machine;
  - every Qualcomm and Pixel validation rule, including commented and malformed directives;
  - lossless round trips (CRLF, mixed endings, no final newline);
  - text classification: Base64, dense text, enciphered text, binary, empty, invalid UTF-8;
  - diff categories for hot and cold thresholds and hysteresis.
- **Platform:**
  - sysfs discovery and dumpsys parsing (Permission Denial, malformed output, BCL units);
  - vendor adapters, including confirmed versus candidate services;
  - framework capabilities for each probe outcome;
  - single-read Pixel inspection;
  - backups with tamper detection;
  - the transaction journal with injected failures;
  - `ProcessRunner` timeouts and no-shell arguments.
- **CLI:**
  - every command on `tests/fixtures/devices/*`, golden text outputs and exit codes;
  - redaction;
  - fixture-mode `doctor`;
  - `apply` refused in every mode.

Tests never read the host's `/sys`, `/proc` or `/vendor` (device commands use `--root`),
never need root, and none is skipped.

## Physical-device validation protocol

Status: **no device has been tested.** Nothing below has been performed yet.

### Target matrix

The aim is one device per row; a row is not "covered" until a record exists in
`docs/validation/`.

| # | Category | Example SoC family | Adapter exercised |
|---|---|---|---|
| 1 | Google Pixel (Tensor) | gs101 / gs201 / zuma | pixel |
| 2 | Qualcomm Snapdragon | sm8xxx / sm7xxx | qcom |
| 3 | MediaTek | mt6xxx | mediatek |
| 4 | Xiaomi family (any SoC) | — | xiaomi + SoC adapter |
| 5 | Samsung | Exynos or Snapdragon | generic (identity only for Exynos) |
| 6 | Another OEM family | any | generic |

### Rules

- Read-only only. No `apply` without `--dry-run`, except step 9, which must refuse.
- Do not stress the device, disable thermal services, change settings or properties, or
  remount partitions. Normal idle or light use only.
- Non-root first. Root, when used, is recorded and used only for the same read-only commands.
- Stop at the first sign of abnormal heat or behaviour, and record it.

### Setup

```sh
scripts/build-android.sh                          # host
adb push dist/zthermal-<version>-arm64-v8a /data/local/tmp/zthermal
adb shell chmod 755 /data/local/tmp/zthermal
adb shell getprop ro.product.device ro.soc.manufacturer ro.soc.model ro.board.platform \
  ro.build.version.release ro.build.version.sdk
adb shell uname -r
```

Record the build fingerprint only in redacted form (zthermal prints a short hash) unless
the device owner agrees to publish it.

### Steps, expected observations

| Step | Command (`adb shell /data/local/tmp/zthermal ...`) | Expected |
|---|---|---|
| 1 | `doctor` | exit 0; sources listed with their real status; `controls: 0 validated` |
| 2 | `device-info` | SoC vendor and evidence match the `getprop` values above |
| 3 | `sensors` | zones with plausible °C values (roughly 20–60 °C idle); invalid zones show a status, never a number |
| 4 | compare with `cat /sys/class/thermal/thermal_zone*/temp` | identical values (millidegrees / 1000), apart from sampling delay |
| 5 | `sensors --framework`, `thermal-status` | HAL temperatures, or `permission_denied` for the shell user, not a fake value; BCL entries in mV/mA/% |
| 6 | `capabilities --framework` | framework capabilities show `probe` results matching step 5 |
| 7 | `cooling-devices` | types and `cur_state/max_state` as in sysfs |
| 8 | `inspect-config <file>` for each file in `report` | expected format; Qualcomm round trip byte-exact; MediaTek/Xiaomi `opaque`; safety facts checked by hand |
| 9 | `apply --mode performance` | exit 4; a `snapshot` before and after shows no change |
| 10 | `snapshot -o /data/local/tmp/s1.json`, then `report` | valid JSON (`scripts/check-schemas.py` on the host), fingerprint redacted |
| 11 | repeat step 3 ten times, 5 s apart | stable output shape; readings change only with temperature |
| 12 | run `sensors` while disconnecting USB, then reconnect | the command either completes or fails cleanly; nothing persists on the device |

### Record

Copy `docs/validation/TEMPLATE.md` to `docs/validation/<oem>-<codename>-<date>.md` and fill
in every field, including failures. Only a filled record may change a row in
`COMPATIBILITY_MATRIX.md`, and only to "hardware tested (read-only)".
