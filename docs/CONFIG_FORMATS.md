# Configuration formats

Detection uses content, never the file name (`config::detect`). Every inspection reports the
size, SHA-256, findings (`info`, `warning`, `error`), safety facts (shutdown and emergency
behaviour found in the file) and a round-trip result.

| Format | Detected by | Parser | Round trip |
|---|---|---|---|
| `qcom_thermal_engine` | `[SECTION]` headers plus `algo_type`, `thresholds` or `set_point` keys (also when commented out) | lossless, `config::qcom` | byte exact |
| `pixel_thermal_info_json` | JSON object with `Sensors` or `CoolingDevices` | strict JSON, `config::pixel` | model equal (whitespace not kept) |
| `xiaomi_thermald_devices` | `[` … `]` blocks with `cooling_name:` lines | recognized only | — |
| `opaque` | magic bytes (gzip, zip, ELF, zstd, xz, bzip2, lz4), non-text bytes, invalid UTF-8, or printable text with enciphered statistics (all of: entropy > 5.5 bits/byte, < 5 % spaces, ≥ 16 distinct punctuation characters) | never decoded | — |
| `suspected_encoded` | text made only of Base64/Base64url characters (≥ 64 characters, no spaces), or dense high-entropy text that is neither Base64 nor enciphered-looking | never decoded | — |
| `unknown_text` | anything else, including empty and whitespace-only files | not interpreted | — |

Entropy is treated as evidence, never as proof of encryption. Base64 and dense identifier
lists have high entropy too, so `opaque` for text additionally needs a broad set of
punctuation (published MediaTek `.tp` files use 24 distinct characters; Base64 uses at most
3). When the evidence does not decide, the result is `suspected_encoded` with the measured
statistics in the reason. `suspected_encoded` was added in 0.2.0; before it, Base64-like
text was reported as `opaque`.

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

Active and commented directives:
- Only a non-comment line is an active directive. `#algo_type ss`, with any spacing or tabs
  after `#`, is a comment; it is kept byte for byte and never treated as the section's
  algorithm.
- A section without an active `algo_type` gets `algo_type_commented_out` (naming the commented
  value) or `no_algo_type`. Algorithm-specific checks are skipped, because the engine's
  behaviour for such a section is unverified. Key-type checks still run: thresholds and set
  points must be integers.
- An active `algo_type` with no value is an error (`algo_type_empty`). Several values give
  `algo_type_extra_values`, and only the first value is checked.
- Malformed headers (`[ ]`, `[A]x`, `[[A]]`, `[A`) are kept verbatim and reported as
  `unparsed_line`; they never start a section.

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

## Semantic diff (`diff-config`)

Each change can carry a `category` and an `effect`; `safety` is set only for
`weakens_protection`, and only those changes count in `safety_relevant` and give exit 3.

| Category | Meaning |
|---|---|
| `weakens_protection` | Mitigation starts later, ends sooner, or disappears |
| `strengthens_protection` | The opposite direction on the same fields |
| `changes_thermal_behaviour` | Thermal handling changes, but the file alone cannot say in which direction |
| none | No thermal effect (comments, log levels, unknown keys) |

Pixel rules follow the HAL's severity computation (`thermal-helper.cpp`,
`getSeverityFromThresholds`): a hot severity starts at `value >= hot[i]` and holds while
`value > hot[i] - hot_hyst[i]`; a cold severity starts at `value <= cold[i]` and holds while
`value < cold[i] + cold_hyst[i]`. A missing array means NaN thresholds and zero hysteresis.

| Field | Weakens | Strengthens |
|---|---|---|
| `HotThreshold[i]` | raised, or a level removed (value → NaN) | lowered, or a level added |
| `ColdThreshold[i]` | lowered, or a level removed | raised, or a level added |
| `HotHysteresis[i]`, `ColdHysteresis[i]` | reduced (mitigation clears sooner), or the array removed while non-zero | increased |
| `isDisabled` | set to true | set to false |
| `BindedCdevInfo` | removed entirely | — (any other change is `changes_thermal_behaviour`) |
| removed sensor or cooling device | always | — |

`Type`, `Multiplier`, `Offset`, `TempPath`, virtual-sensor inputs (`Combination`,
`Coefficient`, `Formula`, ...), `PIDInfo`, `Profile`, polling delays, `Hidden`/`Monitor` and
cooling-device paths are `changes_thermal_behaviour`. These directions apply to the Pixel
HAL's threshold arrays only; they are not assumed for other algorithms. A malformed array on
either side is also `changes_thermal_behaviour`, with a note that the HAL would reject it.

Qualcomm rules: a removed mitigation section, a removed `shutdown` action, an added
`disable` flag, and a trigger or clear value moved later (higher, or lower with
`descending`) weaken protection.

These rules are deliberately conservative. A changed `algo_type` or `descending` flag is also
reported as `weakens_protection` (exit 3), although its direction cannot be determined from
the file: it changes how every value in the section is read. Treat it as "needs review", not
as proven weakening. Qualcomm diffs never report `strengthens_protection` or
`changes_thermal_behaviour`, because the engine's semantics are not publicly documented.

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

These results were re-checked with zthermal 0.2.0 on 2026-10-21 and are unchanged; the
0.2.0 detection rules classify the MediaTek and Xiaomi files exactly as 0.1.0 did.

"0 errors" means the published files pass the toolkit's checks. It does not show that the
checks are complete. The rejection paths are covered by the unit tests in `config::qcom`
and `config::pixel`.
