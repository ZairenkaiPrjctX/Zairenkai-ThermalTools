# Licensing and provenance

## Licence

**Apache License 2.0** (`Apache-2.0`), chosen by the maintainer on 2026-10-21 for this
repository and for Zairenkai-Core. It matches Zairenkai-Database-Source, the only other
licensed Zairenkai repository.

| Item | Where |
|---|---|
| Licence text | `LICENSE`: the official text from https://www.apache.org/licenses/LICENSE-2.0.txt, byte-identical to Zairenkai-Database-Source's `LICENSE` (SHA-256 `cfc7749b…3d30`) |
| Notice | `NOTICE`: copyright holder, scope, and third-party statements |
| Cargo metadata | `license = "Apache-2.0"` in `[workspace.package]`; every crate sets `license.workspace = true` |
| File headers | `SPDX-License-Identifier: Apache-2.0` and `Copyright 2026 ZairenkaiPrjctX contributors` in every `.rs`, `.sh` and `.py` file |
| Distribution | `scripts/build-android.sh` copies `LICENSE` and `NOTICE` into `dist/` next to the binaries |

### Copyright holder and rights

- The copyright line "ZairenkaiPrjctX contributors" follows the convention already used by
  Zairenkai-Database-Source's `NOTICE`. The year 2026 is the year of the first commit.
- Every commit in this repository is the maintainer's work. Agent-made commits carry the
  maintainer's `Signed-off-by: FebriCahyaa`, and the one GitHub-authored merge is the
  maintainer's own.
- No third-party code is vendored. `scripts/build-android.sh` was adapted from Zairenkai-ART,
  which has the same owner and no separate licence.
- Nothing here asserts who owns the rights beyond that convention. If the maintainer wants a
  named copyright holder, change `NOTICE` and the headers together.

### Dependencies

Dependencies: `serde`, `serde_json`, `sha2` and their transitive crates (`Cargo.lock`). Their
licences are all permissive and Apache-2.0-compatible:
- most are MIT OR Apache-2.0;
- `foldhash`: Zlib;
- `memchr`: Unlicense OR MIT;
- `generic-array`, `zmij`: MIT;
- `unicode-ident`: (MIT OR Apache-2.0) AND Unicode-3.0, build time only.

Re-checked with `cargo metadata --locked --filter-platform aarch64-linux-android` on
2026-10-21 (unchanged).

Open item for binary distribution: the MIT- and Zlib-licensed crates are statically linked.
Their licences ask that the copyright notice travel with copies. A release should therefore
add a generated third-party licence bundle (for example with `cargo about`). Not done yet;
CI artifacts are not a release.

## Upstream-derived material

| Material | Source | How it is used | Obligation |
|---|---|---|---|
| Pixel `thermal_info_config` validation rules and severity semantics | AOSP `hardware/google/pixel` `thermal/utils/thermal_info.cpp`, `thermal/thermal-helper.cpp` @ 8612656e (Apache-2.0) | Rules re-implemented in Rust from reading the source; no code or comments copied | None for the code; the source is cited in `config::pixel` and `config::diff` docs |
| AIDL enum names (`TemperatureType`, `CoolingType`, `ThrottlingSeverity`) | AOSP `hardware/interfaces` @ 1a56e38e (Apache-2.0) | Identifier lists | None (names of an interface) |
| `dumpsys thermalservice` / `battery` formats | AOSP `frameworks/base` @ 1cdfff55 (Apache-2.0) | Parser written against the format | None |
| Service names (`vendor.thermal-hal`, `vendor.thermal-engine`) | AOSP init `.rc` files (Apache-2.0) | Identifiers | None |
| MediaTek service names (`thermal`, `thermal_manager`, `thermalloadalgod`) | TheMuppets proprietary_vendor_xiaomi_mt6768-common @ b62d70f6 (proprietary blobs) | Identifiers only; files not copied | None; vendor files are not redistributed |
| Xiaomi service name (`mi_thermald`) | TheMuppets proprietary_vendor_xiaomi_sm8250-common @ 8e68dad4 `init.mi_thermald.rc` (proprietary blobs) | Identifier only; file not copied | None |
| MediaTek legacy zone names (`mtkts*`) | MiCode Xiaomi_Kernel_OpenSource @ 0f68431c (GPL-2.0) | Identifier prefix only; no code copied | None |
| `profiles/google-shusky.json` | Facts from AOSP `device/google/shusky` @ a3188edb and `device/google/zuma` @ ed3585cd (Apache-2.0) | Written by hand; cites the sources | None |

## What is not in the repository

The repository contains no vendor configuration files:
- Qualcomm `thermal-engine.conf` files have no licence statement.
- MediaTek and Xiaomi files are proprietary.
- Pixel configurations are Apache-2.0 but are kept out for consistency.

All of them are referenced by URL and commit in `CONFIG_FORMATS.md`. Everything under
`tests/fixtures/` was written for this project (`tests/fixtures/README.md`).
