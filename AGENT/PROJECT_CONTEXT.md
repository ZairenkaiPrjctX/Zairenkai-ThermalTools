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
| Rust workspace (MSRV 1.85; deps serde, serde_json, sha2) | `Cargo.toml`, `Cargo.lock` | Verified. Evidence: `cargo +1.85.0 test --workspace` passed, 2026-10-20 |
| Core model, parsers, policy (no I/O) | `crates/zthermal-core` | Verified. Evidence: 42 tests |
| Platform readers, adapters, backups, snapshots, journal | `crates/zthermal-platform` | Verified on fixtures. Evidence: 33 tests. **Unverified** on a device |
| CLI | `crates/zthermal` | Verified on fixtures and a Linux host. Evidence: 10 tests, golden outputs |
| Profiles (1: `google-shusky`, public source only) | `profiles/` | Verified. Evidence: `profile-validate`, schema check |
| JSON schemas (4) | `schemas/` | Verified. Evidence: `scripts/check-schemas.py`, 2026-10-20 |
| Android build | `scripts/build-android.sh`, `.cargo/config.toml` | Verified. Evidence: NDK r27d, three ABIs, 16 KB LOAD alignment checked, 2026-10-20 |
| CI | `.github/workflows/ci.yml` | Verified locally. Evidence: actionlint 1.7.12 clean, 2026-10-20. **Unverified** on GitHub |
| Docs | `README.md`, `docs/` | Verified. Evidence: files present |

## Key facts and limitations

- The validated control allowlist is empty: `apply` only plans and exits 4 without
  `--dry-run`. Verified. Evidence: `policy::VALIDATED_CONTROLS`, CLI test
  `apply_writes_nothing_and_rollback_is_a_no_op`.
- The Qualcomm parser round-trips 13 published `thermal-engine.conf` files byte for byte,
  and the Pixel validator accepts 4 published shusky configurations. Verified. Evidence:
  `docs/CONFIG_FORMATS.md`, 2026-10-20.
- MediaTek `.tp` and Xiaomi `thermal-*.conf` files are enciphered or binary; they are only
  detected and hashed. Verified. Evidence: the same table.
- `dumpsys thermalservice` BCL entries are mV/mA/%, not temperatures. Verified. Evidence:
  AOSP `Temperature.aidl` at hardware/interfaces 1a56e38e.
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

- `Zairenkai-Core` lists ThermalTools as "not available" in `docs/MODULES.md`. The JSON
  contract for Core is `docs/INTEGRATION.md`. **Unverified**: Core has no adapter yet.
- `Zairenkai-Database-Source` (AFID) could run `inspect-config --json` on extracted
  firmware. **Unverified**: no integration exists.

## Open questions

- Which licence should the project use?
- Which device runs the first on-device validation (`docs/TESTING.md`)?
- Which control, if any, should be the first validated descriptor, and on which device is
  it hardware-tested?
- Should Core run `zthermal` directly, or should ThermalTools gain a library or socket API?
