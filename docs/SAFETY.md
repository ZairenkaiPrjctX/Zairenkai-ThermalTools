# Safety model

Thermal protection belongs to the device: the kernel thermal framework, the Thermal HAL and
vendor daemons. ThermalTools observes it and may, in the future, adjust a small set of
validated, reversible controls. It never competes with it.

## What the toolkit never does

Enforced in code, not only documented:

| Rule | Where |
|---|---|
| Never write trip points, thermal zone `mode`, `policy`, `emul_temp`, `k_*`, `sustainable_power`, `integral_cutoff`, `slope`, `offset` | `policy::denied_target` |
| Never write cooling device `cur_state` / `max_state` | `policy::denied_target` |
| Never force CPU/GPU frequencies (`scaling_*`, `*_freq`, devfreq, kgsl power levels) | `policy::denied_target` |
| Never write `/vendor`, `/odm`, `/system`, `/product`, `/system_ext`, or anything outside `/sys` and `/proc/sys`; no globs or `..` | `policy::denied_target` |
| Never write a thermal path that mentions shutdown, critical or emergency | `policy::denied_target` |
| Never write without a descriptor in the compiled-in allowlist | `policy::VALIDATED_CONTROLS` (empty) |
| Never write a descriptor that lacks a hardware test record | `txn::preflight` |
| Never create a file while writing | `SysRoot::write_existing` |
| Never claim success without reading the value back | `txn::apply` |
| Never hide a failed restore (`rollback_failed`) | `txn::rollback_entries` |
| Never start a new change while an earlier one is not rolled back | `txn::apply` |
| Never decode, decrypt or de-obfuscate a vendor file | `config::opaque`, `config::detect` |
| Never follow a Pixel `Include` that is not a plain file name | `adapters::pixel::safe_name` |
| Never substitute a value for a failed read | `DataStatus` on every reading |
| Never treat a missing reading as cool | `policy::Adaptive` |
| Never run a shell; commands are fixed argv lists | `exec::ProcessRunner` |

## What does not grant a control

None of these makes a capability `AVAILABLE_WRITABLE`:
- a writable mode bit on a node;
- running as root;
- a matching manufacturer, brand, SoC or device name;
- a profile (profiles may only name allowlist entries, and none exist);
- a successful parse of a vendor configuration.

## Modes

| Mode | Behaviour in this release |
|---|---|
| `diagnostic` | reads only, by definition |
| `balanced` | leaves the system's behaviour unchanged |
| `power_saver` | would use validated power-reducing controls; none exist, so nothing changes |
| `performance`, `gaming` | never disable throttling, raise thresholds or force frequencies; nothing changes |
| `adaptive` | picks a mode from observed severity: escalates after 3 samples at or above `moderate`, relaxes after 5 at or below `light`, holds each mode for at least 60 s, never treats missing readings as cool, and never picks `performance` |

`apply` without `--dry-run` exits 4 (refused) because the plan has no writes.

## Adding a control (future work)

A descriptor needs all of:
1. a target that passes `denied_target`;
2. scoping to a SoC vendor, board platform or device list;
3. an explicit set of allowed values;
4. `restore_original` and `verify_after_write`;
5. evidence (source, documentation or measurement);
6. a recorded hardware test on a matching device (`hardware_tested`).

Then it goes into `VALIDATED_CONTROLS` by a reviewed change.

## Privacy

Snapshots and reports stay where they are written. Build fingerprints are replaced with a
short hash unless `--no-redact` is given. Serial numbers, IMEIs, accounts and network
identifiers are never read.
