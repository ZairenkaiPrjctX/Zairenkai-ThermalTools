//! Qualcomm: the proprietary `thermal-engine` daemon and its `thermal-engine*.conf`.
//!
//! Newer Qualcomm platforms may ship no thermal-engine configuration at all; absence is
//! reported as absence, never filled in from another device.

use zthermal_core::config::ConfigFormat;
use zthermal_core::identity::SocVendor;

use super::{config_capability, read_config, AdapterReport, Context};

pub const CONFIG_DIRS: &[&str] = &["/vendor/etc", "/system/etc", "/odm/etc"];

pub fn is_engine_config(name: &str) -> bool {
    name.starts_with("thermal-engine") && name.ends_with(".conf")
}

pub fn probe(ctx: &Context<'_>) -> AdapterReport {
    let mut r = AdapterReport::new("qualcomm");
    if ctx.identity.soc_vendor.value == SocVendor::Qualcomm {
        r.evidence.extend(
            ctx.identity
                .soc_vendor
                .evidence
                .iter()
                .map(|e| format!("{} = {}", e.source, e.value)),
        );
    }
    let files: Vec<String> = CONFIG_DIRS
        .iter()
        .flat_map(|d| ctx.files_in(d, is_engine_config))
        .collect();
    if !files.is_empty() {
        r.evidence.push(format!(
            "{} thermal-engine configuration files",
            files.len()
        ));
    }
    for bin in [
        "/vendor/bin/thermal-engine",
        "/system/vendor/bin/thermal-engine",
    ] {
        if ctx.root.exists(bin) {
            r.evidence.push(format!("{bin} exists"));
        }
    }
    let tsens = ctx
        .zone_types()
        .iter()
        .filter(|t| t.starts_with("tsens_tz_sensor") || t.ends_with("-usr"))
        .count();
    if tsens > 0 {
        r.evidence
            .push(format!("{tsens} zones use Qualcomm tsens names"));
    }
    r.services = ctx.services("thermal-engine");
    r.applies = !r.evidence.is_empty();
    if !r.applies {
        return r;
    }
    r.config_files = files.iter().map(|p| read_config(ctx.root, p)).collect();
    r.capabilities.push(config_capability(
        "config.qcom_thermal_engine",
        "Qualcomm devices with thermal-engine",
        "list /vendor/etc, /system/etc, /odm/etc for thermal-engine*.conf",
        &r.config_files,
        &[ConfigFormat::QcomThermalEngine],
    ));
    if r.config_files.len() > 1 {
        r.notes.push(
            "several configuration files exist; which one the daemon loads depends on the build (init script, properties) and is not inferred"
                .into(),
        );
    }
    r.notes.push(
        "thermal-engine syntax is not publicly documented; the parser follows published files"
            .into(),
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::Fixture;
    use crate::sysroot::testutil::TempDir;
    use zthermal_core::capability::CapabilityState;

    #[test]
    fn finds_and_inspects_engine_configs() {
        let t = TempDir::new("qcom");
        t.file("vendor/etc/thermal-engine.conf", "[SKIN]\nalgo_type monitor\nsensor s\nthresholds 1 2\nthresholds_clr 0 1\nactions cpu0 shutdown\naction_info 1 1\n");
        t.file("vendor/etc/thermal-engine-8998.conf", "\x00\x01binary");
        t.file("vendor/etc/thermal_info_config.json", "{}");
        let fx = Fixture::new(
            t.path(),
            &[
                ("ro.board.platform", "msm8998"),
                ("init.svc.thermal-engine", "running"),
            ],
        );
        let r = probe(&fx.ctx());
        assert!(r.applies);
        assert_eq!(
            r.services.get("thermal-engine").map(String::as_str),
            Some("running")
        );
        assert_eq!(r.config_files.len(), 2);
        assert_eq!(r.capabilities[0].state, CapabilityState::AvailableReadOnly);
        let i = r
            .config_files
            .iter()
            .find(|f| f.path.ends_with("thermal-engine.conf"))
            .unwrap()
            .inspection
            .as_ref()
            .unwrap();
        assert_eq!(i.safety_facts.len(), 1);
    }

    #[test]
    fn does_not_apply_without_evidence() {
        let t = TempDir::new("qcom-none");
        let fx = Fixture::new(t.path(), &[("ro.board.platform", "mt6768")]);
        let r = probe(&fx.ctx());
        assert!(!r.applies && r.capabilities.is_empty());
    }
}
