# Configuration formats

Detection uses content, never the file name (`config::detect`). Every inspection reports the
size, SHA-256, findings (`info`, `warning`, `error`), safety facts (shutdown and emergency
behaviour found in the file) and a round-trip result.

| Format | Detected by | Parser | Round trip |
|---|---|---|---|
| `qcom_thermal_engine` | `[SECTION]` headers plus `algo_type`, `thresholds` or `set_point` keys (also when commented out) | lossless, `config::qcom` | byte exact |
| `pixel_thermal_info_json` | JSON object with `Sensors` or `CoolingDevices` | strict JSON, `config::pixel` | model equal (whitespace not kept) |
| `xiaomi_thermald_devices` | `[` … `]` blocks with `cooling_name:` lines | recognized only | — |
| `opaque` | magic bytes (gzip, zip, ELF, zstd, xz, bzip2, lz4), non-text bytes, or printable text with enciphered statistics (entropy > 5.5 bits/byte and < 5 % spaces) | never decoded | — |
| `unknown_text` | anything else | not interpreted | — |

The toolkit never writes a configuration file to a device; serialization exists to prove
that nothing was dropped.

## Qualcomm `thermal-engine.conf`

No public specification exists. The syntax is taken from published files (table below):
- `[NAME]` starts a section. Lines before the first section are globals (`sampling 5000`).
- `key value...`, separated by runs of spaces or tabs, mixed in one file.
- `#` starts a full-line comment. It is also used to disable a key (`#algo_type ss`).
- Bare flags: `descending`, `disable`.
- Concurrent actions in one column are joined with `+` (`cpu0+cpu4+gpu`).
- CRLF and LF endings, and a missing final newline, are preserved per line.

`algo_type` values seen: `monitor`, `ss`, `virtual`. Anything else is reported as
`unverified_algo_type`. Other keys and action names outside the reviewed vocabulary are kept
and reported as `info`.

Validation:
- duplicate sections (error), duplicate keys (warning);
- `monitor`: the counts of thresholds, thresholds_clr, actions and action_info must match;
  each column must have as many `+` parts in actions as in action_info; thresholds ascend
  (or descend with `descending`); a clear value must lie past its trigger (warning);
- `ss`: needs sensor, device, set_point and set_point_clr; the clear value lies past the set point;
- `virtual`: `list_cnt` and the `weights` count must match the `sensors` count;
- non-integer thresholds or set points (error); lines that cannot be classified are kept
  verbatim (warning).

Units are not normalized. Tsens and skin sensors use millidegrees, but other sensors differ
(hammerhead's `batt_therm` values do not fit millidegrees), and the unit of `sampling` is
unverified.

## Pixel `thermal_info_config*.json`

The checks follow the HAL parser, `hardware/google/pixel` `thermal/utils/thermal_info.cpp`
at `8612656e216075f79a1efa3bb9be6b6943d6e8cc`. An `error` here is a file that parser
rejects:
- `Name` must be non-empty and unique among enabled entries; `isDisabled` entries are skipped;
- `Type` must exactly match an AIDL `TemperatureType` or `CoolingType` name;
- `HotThreshold`, `HotHysteresis`, `ColdThreshold` and `ColdHysteresis` have 0 or 7 entries
  (one per `ThrottlingSeverity`, NONE…SHUTDOWN);
- values are numbers or strings (`"NaN"`); a string that `std::stof` cannot read is an error;
- non-NaN hot thresholds are non-decreasing and at least `FLT_MIN` (the HAL starts its
  running minimum there, so 0 and negative values are rejected); cold thresholds are
  non-increasing;
- hysteresis values are never NaN;
- overlap: `hot[j] ≤ hot[k] − hyst[k]` for the next non-NaN `k`; mirrored for cold;
- `Hidden` and `Monitor`/`SendCallback` are exclusive;
- `StepRatio` is in 0…1 and needs an explicit `PassiveDelay`;
- virtual sensors: known `Formula`; `Coefficient`, `CombinationType` and `CoefficientType`
  counts match `Combination`, except for the model-based formulas;
- `BindedCdevInfo`: `CdevCeiling`/`CdevCeilingFrequency` and `LimitInfo`/`LimitInfoFrequency`
  are exclusive.

Differences from the HAL:
- Duplicate JSON keys are an error here; a last-wins reader would silently drop a value.
- Files with `//` comments are rejected as not strict JSON. Whether the HAL's jsoncpp
  configuration accepts comments is unverified.
- All problems are reported, where the HAL stops at the first.

The HAL loads `/vendor/etc/` + `vendor.thermal.config` (default `thermal_info_config.json`)
and merges `Include` files, with the main file winning. The Pixel adapter follows the same
order and refuses include names containing `/` at the start or `..`.

Temperatures are in °C, except the BCL types (mV, mA, %).

## MediaTek and Xiaomi

- MediaTek `/vendor/etc/.tp/*` (`thermal.conf`, `.thermal_policy_*`, `.ht120.mtc`) are
  printable ASCII but enciphered: entropy is about 5.8 bits/byte and the content has no
  readable structure. They are classified `opaque` and hashed.
- Xiaomi `thermal-*.conf` (`mi_thermald`) files are binary (entropy 7.3–7.95) and are `opaque`.
- Xiaomi `thermald-devices.conf` is plain text, but its semantics are undocumented, so it is
  recognized and not interpreted.

No decryption, decoding or key search is attempted for any of them.

## Verification against published files

These files were **not** committed: most carry no licence statement, and vendor blobs are
proprietary. They were fetched read-only on 2026-10-20 and run through
`zthermal validate-config --json`. Results:

| File (source, commit) | Format | Sections / sensors | Errors | Warnings | Safety facts | Round trip |
|---|---|---|---|---|---|---|
| AOSP device/lge/hammerhead `thermal-engine-8974.conf` @ e2e99344 | qcom | 7 | 0 | 0 | 4 | exact |
| AOSP device/huawei/angler `thermal-engine-angler.conf` @ cf92cd8e | qcom | 34 | 0 | 0 | 1 | exact |
| AOSP device/google/marlin `thermal-engine-marlin.conf` @ 2bd4cc2e | qcom | 10 | 0 | 0 | 1 | exact |
| AOSP device/google/crosshatch `thermal-engine-crosshatch-novr-prod.conf` @ 6ed0910a | qcom | 9 | 0 | 0 | 1 | exact |
| AOSP device/google/taimen `thermal-engine.conf` @ main 1c1a29ca | qcom | 18 | 0 | 0 | 2 | exact |
| AOSP device/google/bonito `thermal-engine-bonito.conf` @ main 76dc0741 | qcom | 6 | 0 | 0 | 1 | exact |
| AOSP device/google/sunfish `thermal-engine-sunfish.conf` @ main a6face6f | qcom | 7 | 0 | 0 | 1 | exact |
| AOSP device/google/coral `thermal-engine-coral-normal_mode.conf` @ main 7b795ebe | qcom | 10 | 0 | 0 | 1 | exact |
| LineageOS android_device_oneplus_oneplus3 `configs/thermal-engine.conf` @ a275007f | qcom | 8 | 0 | 0 | 1 | exact |
| LineageOS android_device_lge_g3-common `thermal-engine-8974.conf` @ cm-14.1 8d237e0a | qcom | 16 | 0 | 0 | 4 | exact |
| LineageOS android_device_samsung_klte-common `thermal-engine-8974.conf` @ lineage-16.0 aae79e88 | qcom | 11 | 0 | 0 | 4 | exact |
| LineageOS android_device_motorola_msm8916-common `thermal-engine.conf` @ cm-14.1 8dd2b25e | qcom | 6 | 0 | 0 | 3 | exact |
| LineageOS android_device_xiaomi_msm8953-common `thermal-engine.conf` @ lineage-16.0 3ad93f2f | qcom (every algo_type commented out) | 24 | 0 | 0 | 0 | exact |
| AOSP device/google/shusky `thermal_info_config_shiba.json` @ a3188edb | pixel | 62 | 0 | 0 | 20 | model |
| AOSP device/google/shusky `thermal_info_config_charge_shiba.json` @ a3188edb | pixel | 27 | 0 | 0 | 8 | model |
| AOSP device/google/shusky `thermal_info_config_husky.json` @ a3188edb | pixel | 59 | 0 | 0 | 20 | model |
| AOSP device/google/shusky `thermal_info_config_charge_husky.json` @ a3188edb | pixel | 24 | 0 | 0 | 8 | model |
| TheMuppets proprietary_vendor_xiaomi_mt6768-common `.tp/thermal.conf`, `.tp/thermal.off.conf`, `.tp/.thermal_policy_00`, `.tp/.ht120.mtc` @ b62d70f6 | opaque (enciphered text) | — | 0 | 0 | 0 | — |
| same, `thermal-phone.conf`, `thermal-map.conf` | opaque (binary) | — | 0 | 0 | 0 | — |
| same, `thermald-devices.conf` | xiaomi_thermald_devices | — | 0 | 0 | 0 | — |

AOSP repositories are under `https://android.googlesource.com/`. The LineageOS and TheMuppets
files were read from `raw.githubusercontent.com` at the commit shown. Rows marked "main" or
with a branch name were fetched by ref; the hash shown is what `git ls-remote` returned at
fetch time. The other rows were fetched by commit.

"0 errors" means the published files pass the toolkit's checks. It does not show that the
checks are complete. The rejection paths are covered by the unit tests in `config::qcom`
and `config::pixel`.
