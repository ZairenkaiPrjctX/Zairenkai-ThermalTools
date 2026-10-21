# Licensing and provenance

## Current state

- **No licence has been chosen** for this repository: there is no `LICENSE` file, and no
  terms are granted. This is an owner decision and has not been made.
- Organization convention: only one Zairenkai repository has a licence,
  Zairenkai-Database-Source (Apache-2.0, `LICENSE` and `NOTICE`, copyright "ZairenkaiPrjctX
  contributors"). Core, FAS, ART and App record "none chosen" as an open question. No
  organization-wide policy exists in `ZairenkaiPrjctX/.github`.
- Dependencies: `serde`, `serde_json`, `sha2` and their transitive crates (`Cargo.lock`).
  Their licences, all permissive:
  - most are MIT OR Apache-2.0;
  - `foldhash`: Zlib;
  - `memchr`: Unlicense OR MIT;
  - `generic-array`, `zmij`: MIT;
  - `unicode-ident`: (MIT OR Apache-2.0) AND Unicode-3.0, build time only.

  Checked with `cargo metadata --locked` on 2026-10-21.

## Recommendation (not applied)

**Apache-2.0**, matching Zairenkai-Database-Source:
- It is compatible with every source and dependency this project uses. AOSP is Apache-2.0,
  and the dependency licences above are permissive and Apache-2.0-compatible.
- Its patent grant suits system software.
- It lets other Zairenkai components reuse the contract types.

If chosen, add the Apache-2.0 `LICENSE` text, and a `NOTICE` naming the copyright holder as
the owner decides (the AFID repository uses "ZairenkaiPrjctX contributors"). Copyright holders
cannot be invented here.

## Upstream-derived material

| Material | Source | How it is used | Obligation |
|---|---|---|---|
| Pixel `thermal_info_config` validation rules and severity semantics | AOSP `hardware/google/pixel` `thermal/utils/thermal_info.cpp`, `thermal/thermal-helper.cpp` @ 8612656e (Apache-2.0) | Rules re-implemented in Rust from reading the source; no code or comments copied | None for the code; the source is cited in `config::pixel` and `config::diff` docs |
| AIDL enum names (`TemperatureType`, `CoolingType`, `ThrottlingSeverity`) | AOSP `hardware/interfaces` @ 1a56e38e (Apache-2.0) | Identifier lists | None (names of an interface) |
| `dumpsys thermalservice` / `battery` formats | AOSP `frameworks/base` @ 1cdfff55 (Apache-2.0) | Parser written against the format | None |
| Service names (`vendor.thermal-hal`, `vendor.thermal-engine`) | AOSP init `.rc` files (Apache-2.0) | Identifiers | None |
| MediaTek service names (`thermal`, `thermal_manager`, `thermalloadalgod`) | TheMuppets proprietary_vendor_xiaomi_mt6768-common @ b62d70f6 (proprietary blobs) | Identifiers only; files not copied | None; vendor files are not redistributed |
| MediaTek legacy zone names (`mtkts*`) | MiCode Xiaomi_Kernel_OpenSource @ 0f68431c (GPL-2.0) | Identifier prefix only; no code copied | None |
| `profiles/google-shusky.json` | Facts from AOSP `device/google/shusky` @ a3188edb and `device/google/zuma` @ ed3585cd (Apache-2.0) | Written by hand; cites the sources | None |

## What is not in the repository

The repository contains no vendor configuration files:
- Qualcomm `thermal-engine.conf` files have no licence statement.
- MediaTek and Xiaomi files are proprietary.
- Pixel configurations are Apache-2.0 but are kept out for consistency.

All of them are referenced by URL and commit in `CONFIG_FORMATS.md`. Everything under
`tests/fixtures/` was written for this project (`tests/fixtures/README.md`).
