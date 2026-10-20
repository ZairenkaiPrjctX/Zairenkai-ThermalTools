# Test fixtures

Everything here is **synthetic**. The files imitate the formats of real interfaces (Linux
thermal sysfs, `getprop`, `dumpsys`, Qualcomm `thermal-engine.conf`, Pixel
`thermal_info_config.json`), but no file was captured from a device or copied from a vendor
image. Values are invented and say nothing about any real device.

- `devices/<name>/` is a fake root for `zthermal --root <dir>`. `_commands/<argv>.txt` holds
  the output of a command (`getprop.txt`, `dumpsys_thermalservice.txt`), and a `.fail`
  file makes that command fail.
- `configs/` holds configuration files for `inspect-config`, `validate-config` and
  `diff-config` tests.
