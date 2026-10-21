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

### Setup (host)

```sh
scripts/build-android.sh                                   # builds dist/ with LICENSE, NOTICE
adb devices -l                                             # exactly one device, authorized
adb push dist/zthermal-0.2.0-arm64-v8a /data/local/tmp/zthermal
adb shell chmod 755 /data/local/tmp/zthermal
```

`/data/local/tmp` is fine for a manual test run by the shell user. It is never acceptable
for Core: Core refuses a binary there because the directory is group- or world-writable.

In the commands below, `Z=/data/local/tmp/zthermal`. Every command is read-only. Run them
as the shell user. Root is optional and must be recorded if used.

### 1. Identity evidence

```sh
adb shell getprop ro.product.manufacturer ro.product.brand ro.product.device ro.product.model
adb shell getprop ro.soc.manufacturer ro.soc.model ro.board.platform ro.hardware
adb shell getprop ro.build.version.release ro.build.version.sdk
adb shell uname -r
adb shell $Z device-info            # SoC/OEM conclusions must match the getprop values
adb shell $Z doctor                 # exit 0; "0 validated writable controls"
```

### 2. Before-state of everything thermal protection depends on

```sh
adb shell 'cat /sys/class/thermal/thermal_zone*/trip_point_*_temp /sys/class/thermal/thermal_zone*/trip_point_*_hyst \
  /sys/class/thermal/thermal_zone*/mode /sys/class/thermal/thermal_zone*/policy 2>/dev/null' | sha256sum > before-sysfs.sha
adb shell 'sha256sum /vendor/etc/thermal* /vendor/etc/.tp/* /odm/etc/thermal* 2>/dev/null' > before-configs.txt
adb shell 'getprop | grep -i thermal' > before-props.txt
```

### 3. Framework status, sensors and units

```sh
adb shell $Z thermal-status                       # hottest zone, framework severity, battery
adb shell $Z sensors                              # every zone: a valid °C value or a named status
adb shell 'for z in /sys/class/thermal/thermal_zone*; do echo "$z $(cat $z/type) $(cat $z/temp)"; done'
adb shell $Z sensors --framework                  # HAL temperatures; BCL types in mV/mA/%
adb shell dumpsys thermalservice | sed -n '/Current temperatures from HAL/,/^[A-Z]/p'
adb shell dumpsys battery | grep temperature      # tenths of °C
```

Compare each zthermal zone value with the raw millidegree value divided by 1000. Compare
HAL values with the `dumpsys` lines. Idle readings should be plausible (roughly 20–60 °C).
A zone that cannot be read must show a status (`unavailable`, `permission_denied`,
`inconsistent`), never a number.

### 4. Cooling devices and capabilities

```sh
adb shell $Z cooling-devices
adb shell 'for c in /sys/class/thermal/cooling_device*; do echo "$c $(cat $c/type) $(cat $c/cur_state)/$(cat $c/max_state)"; done'
adb shell $Z capabilities --framework --json > caps.json   # framework entries carry "probe"
```

### 5. Configuration files

```sh
adb shell $Z report | sed -n '/== Configuration files/,$p'
adb shell $Z inspect-config <each path listed above>
adb shell $Z backup-config --dest /data/local/tmp/zt-backup   # copies only; verifies SHA-256
adb pull /data/local/tmp/zt-backup
```

Expected:
- Qualcomm files: byte-exact round trip.
- Pixel files: model-equal round trip.
- MediaTek `.tp` and Xiaomi `thermal-*.conf`: `opaque`.
- Xiaomi `thermald-devices.conf`: `xiaomi_thermald_devices`.
- Check the safety facts by hand against the file.

### 6. Refused, unsupported and permission-limited paths

```sh
adb shell $Z apply --mode performance; echo "exit $?"           # must be 4
adb shell $Z apply --mode gaming --dry-run                      # "writes: none"
adb shell $Z rollback; echo "exit $?"                           # "no journal", exit 0
adb shell $Z inspect-config /nonexistent; echo "exit $?"        # exit 1, clear message
adb shell $Z backup-config --dest /data/local/tmp/zt-backup; echo "exit $?"   # exit 1: never overwrites
adb shell $Z sensors --root /data/local/tmp/empty; echo "exit $?"            # exit 5 (mkdir it first)
```

The shell user normally has `android.permission.DUMP`, so `Permission Denial` from
`dumpsys` is not reproducible from adb. It is covered by fixture tests
(`synthetic-pixel`). Record any zones or files the shell user cannot read: those are the
real permission limits.

### 7. Snapshot and report

```sh
adb shell $Z snapshot --framework -o /data/local/tmp/s1.json && adb pull /data/local/tmp/s1.json
adb shell $Z report --framework > report.txt
python3 scripts/check-schemas.py target/debug/zthermal --snapshot s1.json   # host
```

The fingerprint must be `redacted:sha256:…` unless `--no-redact` was given.

### 8. Repeated sampling and interruption

```sh
for i in 1 2 3 4 5 6 7 8 9 10; do adb shell $Z thermal-status; sleep 5; done
adb shell $Z sensors    # unplug USB while it runs, then reconnect and rerun
```

### 9. After-state: nothing changed

```sh
adb shell 'cat /sys/class/thermal/thermal_zone*/trip_point_*_temp /sys/class/thermal/thermal_zone*/trip_point_*_hyst \
  /sys/class/thermal/thermal_zone*/mode /sys/class/thermal/thermal_zone*/policy 2>/dev/null' | sha256sum > after-sysfs.sha
adb shell 'sha256sum /vendor/etc/thermal* /vendor/etc/.tp/* /odm/etc/thermal* 2>/dev/null' > after-configs.txt
adb shell 'getprop | grep -i thermal' > after-props.txt
diff before-sysfs.sha after-sysfs.sha && diff before-configs.txt after-configs.txt && echo UNCHANGED
diff before-props.txt after-props.txt     # init.svc states may change on their own; note any diff
adb shell rm -r /data/local/tmp/zthermal /data/local/tmp/zt-backup /data/local/tmp/s1.json
```

Cooling-device `cur_state` and temperatures change on their own and are not compared.

### Fixture-mode isolation (host)

Proves `--root` reads nothing from the host's thermal stack and runs no Android tool:

```sh
cargo build --release -p zthermal
for c in doctor "snapshot --framework" "report --framework" "capabilities --framework"; do
  strace -f -qq -e trace=openat,open,execve,stat,statx,newfstatat,readlink -o trace.txt \
    target/release/zthermal $c --root tests/fixtures/devices/synthetic-qcom > /dev/null
  grep -c execve trace.txt          # 1: zthermal itself
  grep -oE '"/[^"]*"' trace.txt | sort -u   # only the loader, libc/libm/libgcc, /proc/self/maps
done
```

### Record

Copy `docs/validation/TEMPLATE.md` to `docs/validation/<oem>-<codename>-<date>.md` and fill
in every field, including failures. Only a filled record may change a row in
`COMPATIBILITY_MATRIX.md`, and only to "hardware tested (read-only)".
