# Project context: Zairenkai-ThermalTools

> Owned by this repository. Update it whenever verified facts change. Label every statement
> **Verified** (with `Evidence:`), **Unverified**, or **Unknown / Not Configured**.

## Identity

| Field | Value | Status |
|---|---|---|
| Repository | `ZairenkaiPrjctX/Zairenkai-ThermalTools` | Verified. Evidence: GitHub API |
| GitHub description | (none set) | Verified. Evidence: GitHub API |
| Visibility | private | Verified. Evidence: GitHub API |
| Default branch | `dev` | Verified. Evidence: `git ls-remote` (origin/dev), 2026-10-20 |
| Purpose | Read-only thermal diagnostics, vendor thermal configuration analysis, safe policy planning | Verified. Evidence: `crates/`, `docs/ARCHITECTURE.md` |
| Licence | none chosen (no `LICENSE` file) | Verified. Evidence: repository tree |

## Contents

| Area | Location | Status |
|---|---|---|
| Rust workspace (MSRV 1.85; deps serde, serde_json, sha2) | `Cargo.toml`, `Cargo.lock` | Verified. Evidence: `cargo +1.85.0 test --workspace` passed, 2026-10-21 |
| Core model, parsers, policy (no I/O) | `crates/zthermal-core` | Verified. Evidence: 55 tests, 2026-10-21 |
| Platform readers, adapters, backups, snapshots, journal | `crates/zthermal-platform` | Verified on fixtures. Evidence: 39 tests, 2026-10-21. **Unverified** on a device |
| CLI | `crates/zthermal` | Verified on fixtures and a Linux host. Evidence: 14 tests, 6 golden outputs, 2026-10-21 |
| Profiles (1: `google-shusky`, public source only) | `profiles/` | Verified. Evidence: `profile-validate`, schema check |
| JSON schemas (4) | `schemas/` | Verified. Evidence: `scripts/check-schemas.py`, 2026-10-21 |
| Android build | `scripts/build-android.sh`, `.cargo/config.toml` | Verified. Evidence: NDK r27d, three ABIs, 16 KB LOAD alignment checked, 0.2.0, 2026-10-21 |
| CI | `.github/workflows/ci.yml` | Verified on GitHub. Evidence: Actions runs 39524831220 (PR #1) and 39525107809 (dev) for 0.1.0; 39528670954 (workflow_dispatch, c98d319, 0.2.0); all 4 jobs green, 2026-10-21 |
| Docs | `README.md`, `docs/` (incl. `LICENSING.md`, `validation/TEMPLATE.md`) | Verified. Evidence: files present |

## Key facts and limitations

- The validated control allowlist is empty: `apply` only plans and exits 4 without
  `--dry-run`. Verified. Evidence: `policy::VALIDATED_CONTROLS`, CLI test
  `apply_writes_nothing_and_rollback_is_a_no_op`.
- The Qualcomm parser round-trips 13 published `thermal-engine.conf` files byte for byte,
  and the Pixel validator accepts 4 published shusky configurations. Verified. Evidence:
  `docs/CONFIG_FORMATS.md`, re-checked with 0.2.0 on 2026-10-21.
- MediaTek `.tp` and Xiaomi `thermal-*.conf` files are enciphered or binary; they are only
  detected and hashed. Verified. Evidence: the same table.
- `dumpsys thermalservice` BCL entries are mV/mA/%, not temperatures. Verified. Evidence:
  AOSP `Temperature.aidl` at hardware/interfaces 1a56e38e.
- Capability states record whether a runtime probe ran (`probe`). Verified. Evidence:
  `adapters::generic` tests, CLI test `capabilities_record_the_probe_and_fixtures_never_probe_the_host`.
- Zairenkai-Core consumes `zthermal snapshot` through a read-only adapter. Verified. Evidence:
  Core commit 3562c7c, `crates/zcore-platform/src/thermaltools.rs`, 2026-10-21.
- No Android device was available: on-device behaviour, SELinux denials and real sensor
  names are **Unverified**.

## Authoritative sources

| Topic | Source |
|---|---|
| Repository facts | this file |
| Architecture | `AGENT/ARCHITECTURE.md` |
| Organization standards | `ZairenkaiPrjctX/.github` → `agent-foundation/standards/` |
| Repository list and relationships | `ZairenkaiPrjctX/.github` → `agent-foundation/inventory/repositories.json` |

## Related repositories

- `Zairenkai-Core`: read-only adapter on its branch `ccr-63f8aa87-1b0paj` (commit 3562c7c),
  not merged. Verified. Evidence: Core tests, and one host run with the real binary, 2026-10-21. **Unverified** on a device.
- `Zairenkai-Database-Source` (AFID) could run `inspect-config --json` on extracted
  firmware. **Unverified**: no integration exists.

## Open questions

- Which licence should the project use? Recommendation: Apache-2.0 (`docs/LICENSING.md`).
- Which device runs the first on-device validation (`docs/TESTING.md`)?
- Which control, if any, should be the first validated descriptor, and on which device is
  it hardware-tested?
- Should Core run `zthermal` directly, or should ThermalTools gain a library or socket API?
