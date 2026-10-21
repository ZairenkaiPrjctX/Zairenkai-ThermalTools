# Validation record: <OEM> <model> (<codename>), <YYYY-MM-DD>

Procedure: `docs/TESTING.md` "Physical-device validation protocol". Fill in every field;
write "not done" rather than leaving one out. Never fill this in from expectations: only
observed output counts.

| Field | Value |
|---|---|
| Test category | physical device / emulator / host / fixture (one; a host run is never a device run) |
| OEM / model | |
| Codename (`ro.product.device`) | |
| SoC and evidence (`ro.soc.*`, `ro.board.platform`) | |
| Android version / API level | |
| Build fingerprint | redacted hash from `zthermal device-info`, or full (owner's consent) |
| Kernel (`uname -r`) | |
| Mode | non-root shell / root (state which steps used root) |
| zthermal version, ABI and binary SHA-256 | |
| Tester | |

## Steps

| Section | Commands | Expected | Actual | Pass/fail |
|---|---|---|---|---|
| 1 Identity | getprop, `device-info`, `doctor` | conclusions match getprop; 0 controls | | |
| 2 Before-state | trip/mode/policy hash, config hashes, thermal props | recorded | | |
| 3 Status, sensors, units | `thermal-status`, `sensors`, raw sysfs, `--framework`, dumpsys | values match sysfs/1000 and dumpsys; BCL in mV/mA/% | | |
| 4 Cooling, capabilities | `cooling-devices`, raw sysfs, `capabilities --framework` | match sysfs; framework entries carry `probe` | | |
| 5 Configuration files | `report`, `inspect-config`, `backup-config` | expected formats; backup verified | | |
| 6 Refused / unsupported | `apply`, `apply --dry-run`, `rollback`, missing file, second backup, empty root | exit 4, 0 (no writes), 0, 1, 1, 5 | | |
| 7 Snapshot, report | `snapshot`, `report`, `check-schemas.py --snapshot` | schema-valid; fingerprint redacted | | |
| 8 Repeated sampling, interruption | 10 × `thermal-status`, unplug during `sensors` | stable; clean failure | | |
| 9 After-state | same hashes as step 2 | UNCHANGED | | |

## Findings

- Permission limitations (SELinux denials, unreadable zones or files):
- Sensors (count, implausible or invalid zones, names not classified):
- Capabilities (states that differ from expectations):
- Configuration files (paths, formats, findings):
- Artifacts (snapshot and report file names, hashes):
- Repeatable (yes/no, how many runs):
- Notes:
