# Zairenkai ThermalTools

Read-only thermal diagnostics and vendor thermal configuration analysis for Android and
Linux, with a policy planner that refuses every change it cannot prove safe.

`zthermal` reads the kernel thermal sysfs, hwmon, Android properties and (with the shell or
root) `dumpsys thermalservice` / `dumpsys battery`. It recognizes Qualcomm, MediaTek,
Pixel/Tensor and Xiaomi thermal components, and it parses, validates and diffs Qualcomm
`thermal-engine.conf` and Pixel `thermal_info_config.json`. It never decodes encrypted
vendor files and never writes vendor configuration.

**Status:** built and tested on a Linux host against synthetic fixtures and published
configuration files. It has **not run on an Android device**
([docs/COMPATIBILITY_MATRIX.md](docs/COMPATIBILITY_MATRIX.md)). The validated control
allowlist is empty, so `apply` only plans: nothing is written in this release.

## Commands

| Command | What it does | Privilege |
|---|---|---|
| `doctor` | what this environment allows (sources, SELinux, profiles, journal) | none |
| `device-info` | SoC and OEM identity with the evidence and conflicts behind it | none |
| `capabilities` | each capability's state, required privilege and validation level; `--framework` runs and records the dumpsys probes | none / shell |
| `sensors` | thermal zones, trip points, hwmon; `--framework` adds HAL temperatures | none / shell |
| `thermal-status` | hottest valid zone, framework status, battery temperature | shell for framework |
| `cooling-devices` | cooling device states | none |
| `inspect-config <f>` | detect format, parse, summarize, list safety facts, hash | none |
| `validate-config <f>` | findings only; exit 3 on errors | none |
| `diff-config <a> <b>` | semantic diff; exit 3 when a change weakens protection | none |
| `backup-config --dest <d>` | copy every discovered vendor config with a SHA-256 manifest | none |
| `profile-list`, `profile-validate` | evidence-backed device profiles | none |
| `snapshot` | everything as one JSON document (redacted by default) | none / shell |
| `report` | the snapshot as text (`--format json` for JSON) | none / shell |
| `apply --mode <m> --dry-run` | plan a policy mode; never writes | none |
| `rollback` | restore values recorded in the transaction journal | root |

Modes: `diagnostic`, `balanced`, `power_saver`, `performance`, `gaming`, `adaptive`.

Exit codes: `0` success, `1` runtime error, `2` usage error, `3` validation problems found,
`4` refused by the safety policy, `5` not available on this device.

`--root <dir>` reads a fixture directory instead of the live system, so every command can be
tried on a development host: `zthermal report --framework --root tests/fixtures/devices/synthetic-qcom`.

## Build and test

```sh
cargo build --workspace
cargo test --workspace                       # no root, no device needed
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
ANDROID_NDK_HOME=<ndk> scripts/build-android.sh   # dist/zthermal-<version>-<abi>, SHA256SUMS
python3 scripts/check-schemas.py target/debug/zthermal   # needs jsonschema
```

On a device (not yet tested): `adb push dist/zthermal-0.2.0-arm64-v8a /data/local/tmp/zthermal`,
then `adb shell /data/local/tmp/zthermal report --framework`.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Safety model](docs/SAFETY.md)
- [Configuration formats](docs/CONFIG_FORMATS.md) and what was verified against real files
- [Compatibility matrix](docs/COMPATIBILITY_MATRIX.md)
- [Integration contract](docs/INTEGRATION.md) for Core, the app and the SDK
- [Testing](docs/TESTING.md), including the hardware-validation protocol
- [Licensing and provenance](docs/LICENSING.md)

Zairenkai-Core reads `zthermal snapshot` through a read-only adapter
([docs/INTEGRATION.md](docs/INTEGRATION.md)).

## Licence

Apache License 2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Vendor configuration files
are not part of the repository: those used for verification are referenced by URL and
commit ([docs/CONFIG_FORMATS.md](docs/CONFIG_FORMATS.md),
[docs/LICENSING.md](docs/LICENSING.md)).
