# Validation record: Linux host, synthetic fixtures, 2026-10-21

**Test category: host + fixture.** This is not a device test. No Android device, emulator
or vendor stack was involved. It checks the parts of the protocol that can run off-device:
fixture isolation, refusal paths and the no-change guarantee.

| Field | Value |
|---|---|
| Test category | host (Linux x86_64 container) running fixture devices (`tests/fixtures/devices/*`) |
| OEM / model / codename / SoC | not applicable (synthetic fixture `synthetic-qcom`; values invented, see `tests/fixtures/README.md`) |
| zthermal version | 0.2.0, `cargo build --release`, rustc 1.97.0, at commit `e6127fb` plus this change set |
| Mode | host root user; fixture mode never reads the host identity (see below) |
| Tester | Claude Code session, for the maintainer |

## Observations

| Section | Command | Expected | Actual | Result |
|---|---|---|---|---|
| Fixture isolation | `strace -f -e trace=openat,open,execve,stat,statx,newfstatat,readlink` around `doctor`, `snapshot --framework`, `report --framework`, `capabilities --framework` with `--root tests/fixtures/devices/synthetic-qcom` | one `execve` (zthermal); no host `/sys`, `/proc/sys`, `/vendor`; no getprop or dumpsys | 1 `execve` each. Paths outside the fixture: `/etc/ld.so.cache`, libc, libm, libgcc_s (dynamic loader), `/proc/self/maps` (Rust runtime) | pass |
| Host identity | `doctor --root …synthetic-qcom` | privilege line says not applicable | "not applicable in fixture mode (no live process identity is read)" | pass |
| Refused write | `apply --mode performance` | exit 4 | exit 4 | pass |
| Dry run | `apply --mode gaming --dry-run` | exit 0, no writes | exit 0, "writes: none" | pass |
| Rollback without journal | `rollback` | exit 0, nothing to roll back | exit 0, "no journal … nothing to roll back" | pass |
| Missing file | `inspect-config /nonexistent` | exit 1 | exit 1, "No such file or directory" | pass |
| Backup never overwrites | `backup-config --dest D` twice | 0, then 1 | 0, then 1 ("is not empty; a backup never overwrites") | pass |
| Unsupported device | `sensors --root <empty dir>` | exit 5 | exit 5 | pass |
| Permission-denied path | `capabilities --framework` on `synthetic-pixel` (canned `Permission Denial`) | framework capabilities show the probe | `framework.thermal_status` PERMISSION_DENIED (probe `permission_denied`); `framework.battery_temperature` UNSUPPORTED (probe `unsupported`, no fixture) | pass |
| Redaction | `snapshot` | fingerprint redacted | `redacted:sha256:1319fec65ffa` | pass |
| No change | SHA-256 over every fixture file before and after all of the above | identical | `59327715…7d8a50` before and after; `git status` clean for `tests/` | pass |

## Not covered here

Everything that needs a device: real sensor names and values, unit agreement with sysfs and
`dumpsys`, SELinux denials, vendor configuration paths, timing, and USB interruption. These
remain **unverified** until a physical-device record exists.
