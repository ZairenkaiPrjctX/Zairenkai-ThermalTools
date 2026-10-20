# Architecture

A Rust workspace (MSRV 1.85, edition 2021) with three crates. Dependencies: `serde`,
`serde_json` (`preserve_order`) and `sha2`; nothing else.

```
zthermal (CLI)            args, commands, text rendering, exit codes
   │
zthermal-platform         everything that touches a device: SysRoot reads, Runner commands,
   │                      sysfs discovery, dumpsys parsing, vendor adapters, backups,
   │                      snapshots, the transaction journal
zthermal-core             no I/O: readings, identity, capabilities, config parsers, diff,
                          profiles, policy rules
```

## zthermal-core

| Module | Responsibility |
|---|---|
| `reading` | `DataStatus` for every value; `Temperature` normalization from millidegrees (sysfs, hwmon), tenths (battery) and degrees (HAL); plausibility range −40…150 °C; whole-degree suspicion; freshness; name-based sensor classification (never by index) |
| `identity` | `DeviceIdentity` from properties: SoC vendor and OEM family as conclusions with confidence, evidence and conflicts. Never reads serial numbers or IMEIs |
| `capability` | `Capability` with state, access, required privilege, validation level, evidence, reason; `hardware_tested` |
| `config` | format detection by content, SHA-256, `inspect`; `qcom` (lossless thermal-engine parser and validator), `pixel` (strict JSON, HAL-equivalent validation), `opaque` (binary, compressed, encrypted, enciphered), `diff` (semantic diff with safety flags) |
| `profile` | device profiles with evidence records; `profile_problems`, `database_problems` |
| `policy` | modes, the validated control allowlist (`VALIDATED_CONTROLS`, empty), the hard deny-list (`denied_target`), descriptor rules, the adaptive state machine, `plan` |

## zthermal-platform

| Module | Responsibility |
|---|---|
| `sysroot` | `SysRoot`: all file access under a root (`/` or a fixture), bounded reads, `write_existing` that never creates files |
| `exec` | `Runner` trait; `ProcessRunner` (argv, no shell, timeout, 2 MiB cap), `FakeRunner`, `FixtureRunner` (`--root`) |
| `sysfs` | thermal zones, trip points, cdev bindings, cooling devices with statistics, hwmon |
| `android` | `dumpsys thermalservice` and `dumpsys battery` parsers; BCL entries are kept as mV/mA/%, never as temperatures |
| `device` | properties, kernel release, identity, framework dumps, each with a `SourceStatus` |
| `adapters` | `generic`, `qcom`, `mediatek`, `pixel`, `xiaomi`: evidence for the vendor, config discovery and inspection, read-only capabilities |
| `backup` | copies discovered configs into a new directory with a SHA-256 manifest; `verify` |
| `snapshot` | one document with identity, sources, summary, sysfs, framework, adapters, capabilities; `redact` |
| `txn` | preflight → read originals → journal → write → read back → rollback in reverse; journal survives crashes |

## Data flow

```
getprop ─┐                         ┌─ identity ──────────────┐
sysfs ───┼─> platform readers ─────┼─ SysfsInventory ────────┼─> adapters ─> capabilities
dumpsys ─┘   (status on failure)   └─ framework dumps ───────┘        │
                                                                      ├─> config inspections
vendor configs (/vendor/etc ...) ──> core::config::inspect ───────────┘
snapshot = all of the above (+ redaction) ──> JSON / text report
apply: snapshot facts + validated controls ──> policy::plan ──> txn (only if writes exist)
```

## Trust boundaries

- The binary runs as the invoking user. It never calls `su`; root-only sources report
  `permission_denied`.
- No network access, no telemetry upload, no listening socket.
- Inputs from the device (sysfs text, command output, config files) are untrusted: reads are
  bounded, parsers never execute or decode content, and include paths that could leave
  `/vendor/etc` are not followed.
- Writes exist only in `txn` and only for validated descriptors (none in this release); see
  [SAFETY.md](SAFETY.md).
