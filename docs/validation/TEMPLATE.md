# Validation record: <OEM> <model> (<codename>), <YYYY-MM-DD>

Procedure: `docs/TESTING.md` "Physical-device validation protocol". Fill in every field;
write "not done" rather than leaving one out.

| Field | Value |
|---|---|
| OEM / model | |
| Codename (`ro.product.device`) | |
| SoC and evidence (`ro.soc.*`, `ro.board.platform`) | |
| Android version / API level | |
| Build fingerprint | redacted hash from `zthermal device-info`, or full (owner's consent) |
| Kernel (`uname -r`) | |
| Mode | non-root shell / root (state which steps used root) |
| zthermal version and ABI | |
| Tester | |

## Steps

| Step | Command | Expected | Actual | Pass/fail |
|---|---|---|---|---|
| 1 | `doctor` | | | |
| 2 | `device-info` | | | |
| 3 | `sensors` | | | |
| 4 | compare with sysfs | | | |
| 5 | `sensors --framework`, `thermal-status` | | | |
| 6 | `capabilities --framework` | | | |
| 7 | `cooling-devices` | | | |
| 8 | `inspect-config` (each file) | | | |
| 9 | `apply --mode performance` | exit 4, no change | | |
| 10 | `snapshot`, `report` | | | |
| 11 | repeated sampling | | | |
| 12 | disconnect during a command | | | |

## Findings

- Permission limitations (SELinux denials, `Permission Denial`):
- Sensors (count, implausible or invalid zones, names not classified):
- Capabilities (states that differ from expectations):
- Configuration files (paths, formats, findings):
- Artifacts (snapshot and report file names, hashes):
- Repeatable (yes/no, how many runs):
- Notes:
