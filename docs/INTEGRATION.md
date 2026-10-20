# Integration contract

ThermalTools is a command-line tool. Other Zairenkai components consume its JSON output; it
offers no daemon, socket or library API in this release, so no consumer can issue a write
through it.

## Consumers

| Consumer | Use | How |
|---|---|---|
| Zairenkai-Core | thermal telemetry and capability reporting (`docs/MODULES.md` there lists ThermalTools as "not available") | run `zthermal snapshot` (or `capabilities --json`) through its own allowlisted runner; parse `schema_version: 1` |
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
