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

| Vendor | Recognized from | Read support | Config support | Controls | Validation |
|---|---|---|---|---|---|
| Any Linux / Android | — | thermal zones, trip points, cdev bindings, cooling devices, hwmon | — | none | fixtures |
| Android framework | `ro.build.version.sdk` | `dumpsys thermalservice` (status, HAL temperatures, cooling devices), `dumpsys battery` | — | none | fixtures; dump format checked against AOSP frameworks/base source |
| Qualcomm | `ro.soc.manufacturer`, `ro.soc.model`, `ro.board.platform`, `thermal-engine*.conf`, `/vendor/bin/thermal-engine`, tsens zone names | generic | `thermal-engine.conf`: parse, validate, diff, backup | none | public source (13 published files) |
| MediaTek | `ro.soc.*`, `mt*` platforms, legacy `mtkts*` zones, `/vendor/etc/.tp`, `thermal_manager` | generic | `.tp` files detected and hashed only (enciphered) | none | public source (one vendor blob set) |
| Google Tensor (Pixel) | `ro.soc.*`, gs101/gs201/zuma/…, `thermal_info_config*.json`, `vendor.thermal.config` | generic | `thermal_info_config*.json`: parse, validate, diff, backup, includes | none | public source (4 shusky files, HAL parser source) |
| Xiaomi (OEM) | manufacturer/brand props, `init.svc.mi_thermald`, `/vendor/bin/mi_thermald` | generic | `thermal-*.conf` hashed only (binary); `thermald-devices.conf` recognized | none | public source (one blob set) |
| Samsung Exynos, Unisoc | identity only (`exynos`, `s5e`, `universal`, `ums`, `sc9`, `ud7`) | generic | none | none | fixtures |
| Other OEMs (Samsung, OnePlus, Motorola, …) | OEM family from properties | generic | none | none | fixtures |

No vendor-specific adapter exists without published evidence for it.

## Device profiles

| Profile | Devices | Evidence | Hardware tested |
|---|---|---|---|
| `google-shusky` | `shiba`, `husky` | AOSP device/google/shusky @ a3188edb (config install rule and configurations), device/google/zuma @ ed3585cd (`TARGET_SOC := zuma`) | no |

## What would move a row to "hardware"

Run `zthermal doctor`, `report --framework --no-redact` and `snapshot` on the device. Compare
the sensor readings with `dumpsys thermalservice`, and the identity with `getprop`. Record the
report in `docs/validation/<device>.md`.
