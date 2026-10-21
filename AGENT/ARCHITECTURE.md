# Architecture: Zairenkai-ThermalTools

> Owned by this repository. Document only what inspected code supports, citing file paths.

## Status

A read-only thermal toolkit exists and is tested on a host. It has never run on an Android
device. Full description: `docs/ARCHITECTURE.md`; safety rules: `docs/SAFETY.md`.

## Overview

`zthermal` reads kernel thermal sysfs, hwmon, Android properties and framework dumps. It
recognizes vendor thermal implementations, and it parses, validates, diffs and backs up
vendor thermal configuration. A policy planner exists, but its validated control allowlist
is empty, so no write happens in this release.

## Components

| Path | Responsibility | Evidence |
|---|---|---|
| `crates/zthermal-core/src/{reading,identity,capability}.rs` | Data model: status, units, identity conclusions, capabilities | unit tests in each module |
| `crates/zthermal-core/src/config/` | Detection, Qualcomm and Pixel parsers, opaque detection, diff | `config::*::tests`; `docs/CONFIG_FORMATS.md` (24 published files) |
| `crates/zthermal-core/src/{profile,policy}.rs` | Profiles; modes, allowlist, deny-list, adaptive state machine, plan | `profile::tests`, `policy::tests` |
| `crates/zthermal-platform/src/{sysroot,exec,sysfs,android,device}.rs` | Readers with bounded I/O and no shell | fixture tests |
| `crates/zthermal-platform/src/adapters/` | generic, qcom, mediatek, pixel, xiaomi | adapter tests |
| `crates/zthermal-platform/src/{backup,snapshot,txn}.rs` | Backups with SHA-256, snapshot and redaction, write journal and rollback | fault-injection tests |
| `crates/zthermal/src/` | CLI: args, commands, text rendering, exit codes | `crates/zthermal/tests/cli.rs`, `tests/golden/` |
| `profiles/` | Evidence-backed device profiles (embedded in the binary) | `profile-validate`, `commands::tests` |
| `schemas/` | JSON schemas of the stable outputs | `scripts/check-schemas.py` |

## Entry points

- `zthermal <command>` (`crates/zthermal/src/main.rs`).
- `scripts/build-android.sh` builds the Android binaries; `scripts/check-schemas.py` checks the schemas.

## Data flow

Inputs are read-only:
- `/sys/class/thermal`, `/sys/class/hwmon`, `/proc/sys/kernel/osrelease`;
- `getprop`, `dumpsys thermalservice`, `dumpsys battery`;
- vendor configuration under `/vendor/etc`, `/system/etc` and `/odm/etc`.

Outputs are JSON or text on stdout or `--output`, and backup directories with
`manifest.json`. The transaction journal (`/data/local/tmp/zthermal/journal.json`) is
written only by `apply` with validated controls; none exist.

## External interfaces

The JSON outputs and exit codes are the contract for Core, the app, the SDK and AFID
(`docs/INTEGRATION.md`). There is no socket, daemon or network access.

## Trust boundaries

The binary runs as the invoking user and never calls `su`. Device data is untrusted input:
reads are bounded and parsers never execute or decode content. Writes are limited to `txn`,
gated by `policy::denied_target`, `txn::preflight` and the empty allowlist.

## Generated artifacts

`dist/` (Android binaries and `SHA256SUMS`) comes from `scripts/build-android.sh` and is
git-ignored. The golden files in `tests/golden/` are produced by the CLI; regenerate them
from the CLI when output changes intentionally.
