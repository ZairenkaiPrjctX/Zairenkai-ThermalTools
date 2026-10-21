//! Pixel / Google Tensor: the Pixel Thermal HAL and `thermal_info_config*.json`.
//!
//! The HAL reads `/vendor/etc/` + the `vendor.thermal.config` property (default
//! `thermal_info_config.json`) and merges the files listed in `Include` (pixel
//! `thermal-helper.cpp`, `thermal_info.cpp`).

use std::collections::BTreeSet;

use zthermal_core::config::pixel::PixelConfig;
use zthermal_core::config::ConfigFormat;
use zthermal_core::identity::SocVendor;

use super::{config_capability, read_config, read_config_data, AdapterReport, ConfigFile, Context};

pub const CONFIG_DIR: &str = "/vendor/etc";
pub const DEFAULT_CONFIG: &str = "thermal_info_config.json";

/// The Pixel Thermal HAL service (`thermal/android.hardware.thermal-service.pixel.rc` in
/// hardware/google/pixel at 8612656e).
pub const SERVICES: &[&str] = &["vendor.thermal-hal"];

pub fn is_info_config(name: &str) -> bool {
    name.starts_with("thermal_info_config") && name.ends_with(".json")
}

/// A file name the HAL would join to /vendor/etc/, refused if it could escape it.
fn safe_name(name: &str) -> Option<&str> {
    (!name.is_empty() && !name.contains("..") && !name.starts_with('/')).then_some(name)
}

pub fn probe(ctx: &Context<'_>) -> AdapterReport {
    let mut r = AdapterReport::new("pixel");
    if ctx.identity.soc_vendor.value == SocVendor::GoogleTensor {
        r.evidence.extend(
            ctx.identity
                .soc_vendor
                .evidence
                .iter()
                .map(|e| format!("{} = {}", e.source, e.value)),
        );
    }
    let present = ctx.files_in(CONFIG_DIR, is_info_config);
    if !present.is_empty() {
        r.evidence
            .push(format!("{} thermal_info_config*.json files", present.len()));
    }
    let prop = ctx.prop("vendor.thermal.config");
    if let Some(p) = prop {
        r.evidence.push(format!("vendor.thermal.config = {p}"));
    }
    r.services = ctx.services_named(SERVICES);
    r.candidate_services = ctx.service_candidates("thermal", SERVICES);
    r.applies = !r.evidence.is_empty();
    if !r.applies {
        return r;
    }

    // The active file first, then its includes, then every other config file present.
    let active = match prop.and_then(safe_name) {
        Some(n) => n.to_string(),
        None => {
            if let Some(p) = prop {
                r.notes.push(format!(
                    "vendor.thermal.config {p:?} is not a plain file name; ignored"
                ));
            }
            DEFAULT_CONFIG.to_string()
        }
    };
    let mut order: Vec<String> = vec![format!("{CONFIG_DIR}/{active}")];
    let mut seen: BTreeSet<String> = order.iter().cloned().collect();
    let mut i = 0;
    while i < order.len() && order.len() < 32 {
        let (f, data) = read_config_data(ctx.root, &order[i]);
        if let Some(text) = data.and_then(|b| String::from_utf8(b).ok()) {
            if let Ok(cfg) = PixelConfig::parse(&text) {
                for inc in cfg.includes() {
                    match safe_name(&inc) {
                        Some(n) => {
                            let p = format!("{CONFIG_DIR}/{n}");
                            if seen.insert(p.clone()) {
                                order.push(p);
                            }
                        }
                        None => r.notes.push(format!(
                            "{}: include {inc:?} is not a plain file name; not followed",
                            order[i]
                        )),
                    }
                }
            }
        }
        r.config_files.push(f);
        i += 1;
    }
    for p in present {
        if seen.insert(p.clone()) {
            r.config_files.push(read_config(ctx.root, &p));
        }
    }
    r.notes.push(format!("active configuration: {CONFIG_DIR}/{active} (from vendor.thermal.config or the HAL default)"));
    let found: Vec<ConfigFile> = r
        .config_files
        .iter()
        .filter(|f| {
            f.inspection.is_some() || f.status != zthermal_core::reading::DataStatus::Unsupported
        })
        .cloned()
        .collect();
    r.capabilities.push(config_capability(
        "config.pixel_thermal_info",
        "Pixel devices with the Pixel Thermal HAL",
        "vendor.thermal.config, /vendor/etc/thermal_info_config*.json and their Include lists",
        &found,
        &[ConfigFormat::PixelThermalInfoJson],
    ));
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::Fixture;
    use crate::sysroot::testutil::TempDir;
    use crate::sysroot::SysRoot;
    use zthermal_core::capability::CapabilityState;
    use zthermal_core::reading::DataStatus;

    #[test]
    fn follows_the_property_and_includes() {
        let t = TempDir::new("pixel");
        t.file("vendor/etc/thermal_info_config_proto.json", r#"{"Include": ["thermal_info_config_extra.json", "../../etc/passwd"], "Sensors": [{"Name": "skin", "Type": "SKIN"}]}"#);
        t.file(
            "vendor/etc/thermal_info_config_extra.json",
            r#"{"Sensors": [{"Name": "x", "Type": "CPU"}]}"#,
        );
        t.file("vendor/etc/thermal_info_config.json", r#"{"Sensors": []}"#);
        let fx = Fixture::new(
            t.path(),
            &[
                ("ro.soc.model", "zuma"),
                ("vendor.thermal.config", "thermal_info_config_proto.json"),
            ],
        );
        let r = probe(&fx.ctx());
        assert!(r.applies);
        let paths: Vec<&str> = r.config_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/vendor/etc/thermal_info_config_proto.json",
                "/vendor/etc/thermal_info_config_extra.json",
                "/vendor/etc/thermal_info_config.json"
            ]
        );
        assert!(r.notes.iter().any(|n| n.contains("not followed")));
        assert_eq!(r.capabilities[0].state, CapabilityState::AvailableReadOnly);
    }

    #[test]
    fn missing_active_file_is_reported() {
        let t = TempDir::new("pixel-missing");
        let fx = Fixture::new(t.path(), &[("ro.soc.model", "gs101")]);
        let r = probe(&fx.ctx());
        assert!(r.applies);
        assert_eq!(r.config_files[0].status, DataStatus::Unsupported);
        assert_eq!(r.capabilities[0].state, CapabilityState::Unsupported);
    }

    #[test]
    fn each_file_is_read_once_per_inspection() {
        let t = TempDir::new("pixel-reads");
        t.file(
            "vendor/etc/thermal_info_config.json",
            r#"{"Include": ["thermal_info_config_a.json", "thermal_info_config_a.json"], "Sensors": []}"#,
        );
        t.file(
            "vendor/etc/thermal_info_config_a.json",
            r#"{"Include": ["thermal_info_config.json"], "Sensors": []}"#,
        );
        t.file(
            "vendor/etc/thermal_info_config_b.json",
            r#"{"Sensors": []}"#,
        );
        let mut fx = Fixture::new(t.path(), &[("ro.soc.model", "zuma")]);
        fx.root = SysRoot::recording(t.path());
        let r = probe(&fx.ctx());
        assert_eq!(r.config_files.len(), 3);
        for f in [
            "thermal_info_config.json",
            "thermal_info_config_a.json",
            "thermal_info_config_b.json",
        ] {
            assert_eq!(fx.root.reads_of(&format!("/vendor/etc/{f}")), 1, "{f}");
        }
        // A second inspection reads the files again: nothing is cached across requests.
        probe(&fx.ctx());
        assert_eq!(fx.root.reads_of("/vendor/etc/thermal_info_config.json"), 2);
    }
}
