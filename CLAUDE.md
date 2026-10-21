# CLAUDE.md: Zairenkai-ThermalTools

Instructions for Claude Code in `ZairenkaiPrjctX/Zairenkai-ThermalTools`. Keep this file under 200 lines;
put detail in `AGENT/`.

@AGENTS.md
@AGENT/PROJECT_CONTEXT.md

## Current state (read first)

This repository is **Zairenkai ThermalTools**: a Rust workspace (MSRV 1.85) that builds the
`zthermal` CLI. Architecture: `docs/ARCHITECTURE.md`.
- `crates/zthermal-core`: no I/O. Readings, identity, capabilities, config parsers (Qualcomm
  thermal-engine, Pixel thermal_info_config, opaque detection), diff, profiles, policy.
- `crates/zthermal-platform`: SysRoot reads, Runner commands, sysfs, dumpsys, vendor
  adapters, backups, snapshots, the transaction journal.
- `crates/zthermal`: the CLI.

Nothing has run on an Android device yet (`docs/COMPATIBILITY_MATRIX.md`).

Non-negotiables in this repo (`docs/SAFETY.md`):
- Never write trip points, zone `mode`/`policy`/`emul_temp`, cooling-device states, CPU/GPU
  frequency limits, or vendor/system configuration. `policy::denied_target` enforces it.
- `VALIDATED_CONTROLS` stays empty until a control has a recorded hardware test. Never infer
  write capability from a writable node, root access, a vendor name or a profile.
- Never decode, decrypt or de-obfuscate vendor files. Opaque files are hashed and reported.
- Parsers keep unknown lines and keys and report them. Nothing is dropped silently.
- Never substitute a value for a failed read. Every reading carries a `DataStatus`.
- Tests use fixtures (`SysRoot`, `FakeRunner`, `tests/fixtures`), never the host's `/sys`,
  `/proc` or `/vendor`, and never need root.
- Do not commit vendor configuration files. Reference them by URL and commit
  (`docs/CONFIG_FORMATS.md`). Profiles need published evidence; no fictional devices.
- A capability state never claims a runtime probe that did not run (`probe` is `null` then).
  Service names are "confirmed" only from a published init file; matches are candidates.
- Golden files in `tests/golden/` change only with an explained, intended output change.

## Commands

| Purpose | Command | Status |
|---|---|---|
| Build | `cargo build --workspace` | Verified. Evidence: 2026-10-21 |
| Test | `cargo test --workspace` | Verified. Evidence: 109 passed (55 core + 40 platform + 3 CLI unit + 11 CLI integration), 2026-10-21 |
| Lint / format | `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check` | Verified. Evidence: clean, 2026-10-21 |
| MSRV | `cargo +1.85.0 test --workspace` | Verified. Evidence: 109 passed, 2026-10-21 |
| JSON schemas | `python3 scripts/check-schemas.py target/debug/zthermal` (needs `jsonschema`) | Verified. Evidence: 4 snapshots, 7 inspections, 1 profile valid, 2026-10-21 |
| Shell lint | `shellcheck scripts/*.sh` | Verified. Evidence: clean (0.10.0), 2026-10-21 |
| Android binaries | `ANDROID_NDK_HOME=<ndk> scripts/build-android.sh` | Verified. Evidence: NDK r27d, arm64-v8a/x86_64/armeabi-v7a, 16 KB LOAD alignment checked, 0.2.0, 2026-10-21 |
| Validate agent foundation | `python3 <path-to-.github>/agent-foundation/scripts/validate_agent_setup.py .` | Configured |

Only list commands that exist in this repository. Never invent one.

## Working rules

- Inspect the relevant code before editing it. Preserve unrelated and uncommitted changes.
- Generated files are edited at their source, then regenerated. Managed foundation files
  (listed in `AGENT/.foundation-manifest.json`) are edited in `ZairenkaiPrjctX/.github`, not here.
- Destructive or outward-facing actions (deleting data, force-push, history rewrites,
  publishing releases, deploying, flashing devices) need explicit human approval each time.
- Do not claim a build, test, or signature succeeded without showing the output.

## Git

- Work on a topic branch; open **draft** pull requests; never merge or approve.
- Commits must be cryptographically signed. If signing fails, stop and report the error.
  Trailer format and verification: `AGENT/policies/git-and-commit-signing.md`.

## Where things are

- `AGENT/README.md`: index of the agent library (prompts, policies, playbooks, templates).
- `.claude/skills/`: run `/repo-audit`, `/implement-feature`, `/bug-fix`, `/test-and-verify`,
  `/security-review`, `/release-readiness`.
- `.claude/rules/`: path-scoped rules, loaded automatically.
