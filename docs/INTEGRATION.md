# Integration contract

ThermalTools is a command-line tool. Other Zairenkai components consume its JSON output; it
offers no daemon, socket or library API in this release, so no consumer can issue a write
through it.

## Consumers

| Consumer | Use | How |
|---|---|---|
| Zairenkai-Core | read-only diagnostics: `thermaltools.diagnostics` and `thermaltools.sensors` capabilities, and the `zairenkai_thermaltools` module entry | **Implemented** on Core branch `ccr-63f8aa87-1b0paj` (`crates/zcore-platform/src/thermaltools.rs`): runs `<paths.thermaltools_binary> snapshot` and parses schema 1. See "Core adapter" below |
| Zairenkai-App | show sensors, status, capabilities, config inspections | through Core. If it runs the binary itself, only read-only commands |
| Zairenkai-FAS | thermal input for frame-aware boost limits | FAS keeps its own thermal reader; `summary.framework_status` and `summary.hottest_zone` are the fields to compare with |
| Zairenkai-Database-Source (AFID) | thermal configuration metadata from firmware | `inspect-config --json` on extracted files (offline): format, SHA-256, findings, safety facts |
| Zairenkai-SDK | public types | the JSON schemas in `schemas/` |

## Stable outputs (schema_version 1)

| Command | Schema | Stable fields |
|---|---|---|
| `snapshot` | `schemas/snapshot.schema.json` | top level, `summary`, `sources`, `capabilities` |
| `capabilities --json` | `schemas/capability.schema.json` (items) | every capability field |
| `inspect-config --json`, `validate-config --json` | `schemas/inspection.schema.json` | every field; `summary` content depends on the format |
| profiles | `schemas/profile.schema.json` | every field |

Nested diagnostic records (`sysfs`, `thermalservice`, `adapters`) may gain fields in a minor
version. A field is never repurposed; an incompatible change bumps `schema_version`.

### Additions in 0.2.0 (still schema_version 1)

All additions are new optional fields or enum values. No field was removed or renamed; one
classification was corrected (see the migration notes below).
- **Capability `probe`:** the runtime probe behind `state` in this run (`source`, `status`,
  `detail`). It is `null` when the state comes from static detection only. Framework
  capabilities are `VALIDATION_REQUIRED` with `probe: null` unless `--framework` was given;
  with it, they follow the probe result (`AVAILABLE_READ_ONLY`, `PERMISSION_DENIED`,
  `UNSUPPORTED`, `ERROR` for unrecognized output, `TEMPORARILY_UNAVAILABLE`).
- **Adapter services:**
  - `services` lists only names taken from published init files;
  - the new `candidate_services` lists name matches, kept for diagnosis and never confirmed.
- **Diff changes:** new `category` (`weakens_protection`, `strengthens_protection`,
  `changes_thermal_behaviour`, or `null`) and `effect` fields. `safety` and
  `safety_relevant` keep their meaning, weakening only.
- **Inspection format:** new value `suspected_encoded`.

Migration for consumers:
- Treat `probe` as optional.
- Treat an unknown `detection.format` value as "not interpreted".
- A diff that changed a Pixel `BindedCdevInfo` or `Type` used to count as `safety`; it is now
  `changes_thermal_behaviour`. Removing all bindings still weakens.

Exit codes are part of the contract: `0` success, `1` runtime error, `2` usage, `3`
validation problems, `4` refused by the safety policy, `5` not available on this device.

## Rules for consumers

- Treat any `state` other than `AVAILABLE_READ_ONLY` as not usable, and show `reason`.
- Never show a temperature whose `status` is not `valid`. An `inconsistent` value keeps
  `millicelsius` for diagnosis only.
- A `framework_status` of `null` means unknown, not cool.
- Do not upload snapshots or reports without the user's explicit action. Snapshots are
  redacted by default; `--no-redact` is a user choice.
- `apply` and `rollback` are not for automation. A consumer that calls `apply` gets exit 4
  in this release.

## Core adapter

What Core does (`crates/zcore-platform/src/thermaltools.rs` in Zairenkai-Core):
- **Configuration.** `[paths] thermaltools_binary = "/absolute/path/zthermal"`. There is no
  default; without it Core reports `zairenkai_thermaltools` as `not_installed` and
  `thermaltools.diagnostics` as `unsupported`.
- **Checks before running.** The file must be named `zthermal`, a regular executable, and
  owned by root, with no group- or world-writable component on its path (Core runs as root).
  `/data/local/tmp` therefore never qualifies.
- **What runs.** Only `zthermal snapshot`, with no shell, a cleared environment, a 15 s timeout
  and a 1 MiB output cap. Write subcommands (`apply`, `rollback`, `backup-config`) are refused
  by name before anything runs.
- **What is read.** `schema_version` (must be 1), `tool_version`,
  `summary.{zones,zones_valid,hottest_zone,framework_status}` and `capabilities[].{id,state}`.
  Identity fields are ignored (Core has its own discovery), and redaction stays on.
- **Error categories.**
  - `not_configured` and `not_installed`: Core reports the capability as unsupported.
  - `unsafe_binary`: permission restricted.
  - `timeout` and `failed`: temporarily unavailable.
  - `malformed`, `unsupported_schema` and `output_too_large`: unknown.
  - `write_not_supported` and `not_allowed` can never reach the process.

Verified in Core:
- unit tests with fakes;
- a snapshot generated by zthermal 0.2.0 from `tests/fixtures/devices/synthetic-qcom`;
- one run of the real binary through Core's `SystemRunner` on a Linux host, from a
  root-owned path. A copy in world-writable `/tmp` was refused, and a missing path was
  reported as not installed.

Not verified: any run on an Android device.

