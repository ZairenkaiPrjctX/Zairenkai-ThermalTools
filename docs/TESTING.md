# Testing

## Automated (host, no root, no device)

| Check | Command |
|---|---|
| Unit and integration tests | `cargo test --workspace` |
| MSRV | `cargo +1.85.0 test --workspace` |
| Lint and format | `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check` |
| JSON schemas | `python3 scripts/check-schemas.py target/debug/zthermal` (needs `jsonschema`) |
| Shell scripts | `shellcheck scripts/*.sh` |
| Android binaries | `ANDROID_NDK_HOME=<ndk> scripts/build-android.sh` |

What the tests cover:
- **Core:** normalization and plausibility; identity conflicts; profile rules; policy
  deny-list; the adaptive state machine; every Qualcomm and Pixel validation rule; lossless
  round trips (CRLF, mixed endings, no final newline); opaque and enciphered detection;
  diff safety flags.
- **Platform:** sysfs discovery on fixture trees; dumpsys parsing, including Permission
  Denial, missing sections and BCL units; adapters for each vendor; backups with
  tamper detection; the transaction journal with injected write, read-back and restore
  failures; `ProcessRunner` timeouts and no-shell arguments.
- **CLI:** every command against `tests/fixtures/devices/*`, golden text outputs
  (`tests/golden/`), exit codes, redaction, and refusal of `apply`.

Fixtures are synthetic (`tests/fixtures/README.md`). Tests never read the host's `/sys`,
`/proc` or `/vendor`; device commands run with `--root`.

## Published files

The parsers were run against 24 published files (see `CONFIG_FORMATS.md`). These files are
not in the repository, so that check is not part of `cargo test`.

## On-device procedure (not yet performed)

1. Build with `scripts/build-android.sh`, then
   `adb push dist/zthermal-<version>-arm64-v8a /data/local/tmp/zthermal`.
2. As the shell user, run:
   - `zthermal doctor`
   - `zthermal sensors --framework`
   - `zthermal thermal-status`
   - `zthermal report --framework`
3. Compare zone readings with `cat /sys/class/thermal/thermal_zone*/temp`, and HAL values
   with `dumpsys thermalservice`.
4. Run `zthermal inspect-config` on each vendor configuration found. Check the detected format
   and confirm the safety facts by hand.
5. `zthermal apply --mode performance` must exit 4, and nothing on the device may change:
   compare snapshots taken before and after.
6. Record device, build fingerprint, outputs and discrepancies in
   `docs/validation/<device>.md`. Only then may the compatibility matrix say "hardware".
