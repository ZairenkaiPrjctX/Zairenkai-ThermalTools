# Compatibility matrix

Validation levels: **fixtures** (synthetic test data only), **public source** (checked
against published source or configuration files), **hardware** (run on a physical device
with a recorded report). No row has reached **hardware**.

## Environments

| Environment | Build | Tests | Run |
|---|---|---|---|
| Linux x86_64 host | yes (rustc 1.97.0, MSRV 1.85.0) | all pass | yes: against fixtures (`--root`), and live on a container host with no thermal zones and no Android tools (each source reported `unsupported`, exit 5) |
| Android arm64-v8a, x86_64 (API 29+) | yes (NDK r27d; LOAD segments aligned to 16 KB, checked by the build script) | — | **not run** |
| Android armeabi-v7a (API 29+) | yes (NDK r27d) | — | **not run** |

## SoC and OEM support

Each column is a separate claim. "Identified" never implies the columns to its right.

| Vendor | Identified from | Generic diagnostics | Config format parsed | Parser validated against published files | Hardware tested | Control modification tested |
|---|---|---|---|---|---|---|
| Any Linux / Android | — | yes: thermal zones, trips, cdev bindings, cooling devices, hwmon (fixtures) | — | — | no | no (none exists) |
| Android framework | `ro.build.version.sdk` | yes: `dumpsys thermalservice`, `dumpsys battery`, probed only with `--framework` (fixtures; format checked against AOSP frameworks/base) | — | — | no | no |
| Qualcomm | `ro.soc.manufacturer`, `ro.soc.model`, `ro.board.platform`, `thermal-engine*.conf`, `/vendor/bin/thermal-engine`, tsens zone names; service `vendor.thermal-engine` (AOSP marlin init) | yes | `thermal-engine.conf`: parse, validate, diff, backup | yes: 13 files, byte-exact round trip | no | no |
| MediaTek | `ro.soc.*`, `mt*` platforms, legacy `mtkts*` zones, `/vendor/etc/.tp`; services `thermal`, `thermal_manager`, `thermalloadalgod` (one published blob set) | yes | no: `.tp` files are enciphered, detected and hashed only | detection only (one blob set) | no | no |
| Google Tensor (Pixel) | `ro.soc.*`, gs101/gs201/zuma/…, `thermal_info_config*.json`, `vendor.thermal.config`; service `vendor.thermal-hal` | yes | `thermal_info_config*.json`: parse, validate, diff, backup, includes | yes: 4 shusky files, rules from the HAL source | no | no |
| Xiaomi (OEM) | manufacturer/brand props, `/vendor/bin/mi_thermald`; service `mi_thermald` (published Xiaomi sm8250 vendor init) | yes | no: `thermal-*.conf` are binary (hashed only); `thermald-devices.conf` recognized, not interpreted | detection only (one blob set) | no | no |
| Samsung Exynos, Unisoc | identity only (`exynos`, `s5e`, `universal`, `ums`, `sc9`, `ud7`) | yes | none | — | no | no |
| Other OEMs (Samsung, OnePlus, Motorola, …) | OEM family from properties | yes | none | — | no | no |

No vendor-specific adapter exists without published evidence for it. Recognizing a SoC or
OEM does not mean the device is supported beyond generic diagnostics.

## Device profiles

| Profile | Devices | Evidence | Hardware tested |
|---|---|---|---|
| `google-shusky` | `shiba`, `husky` | AOSP device/google/shusky @ a3188edb (config install rule and configurations), device/google/zuma @ ed3585cd (`TARGET_SOC := zuma`) | no |

## What would move a row to "hardware tested"

Follow the physical-device protocol in `TESTING.md`, and commit the filled record
`docs/validation/<oem>-<codename>-<date>.md` (template: `docs/validation/TEMPLATE.md`).
That would only make the row "hardware tested (read-only)": no control exists to test.
