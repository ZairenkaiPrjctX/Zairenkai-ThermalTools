//! Xiaomi `mi_thermald`: `/vendor/etc/thermal-*.conf` and `thermald-devices.conf`.
//!
//! Published Xiaomi `thermal-*.conf` files are binary (encrypted or encoded); they are
//! hashed and reported, never decoded. `thermald-devices.conf` is plain text whose
//! semantics are not documented, so it is recognized but not interpreted.

use zthermal_core::identity::OemFamily;

use super::{config_capability, read_config, AdapterReport, Context};

pub const CONFIG_DIR: &str = "/vendor/etc";

pub fn is_mi_config(name: &str) -> bool {
    (name.starts_with("thermal-") && name.ends_with(".conf") && !name.starts_with("thermal-engine"))
        || name == "thermald-devices.conf"
}

pub fn probe(ctx: &Context<'_>) -> AdapterReport {
    let mut r = AdapterReport::new("xiaomi");
    let files = ctx.files_in(CONFIG_DIR, is_mi_config);
    r.services = ctx.services("mi_thermald");
    if ctx.identity.oem_family.value == OemFamily::Xiaomi {
        r.evidence.extend(
            ctx.identity
                .oem_family
                .evidence
                .iter()
                .map(|e| format!("{} = {}", e.source, e.value)),
        );
    }
    if !r.services.is_empty() {
        r.evidence.push("init.svc.mi_thermald is set".into());
    }
    if ctx.root.exists("/vendor/bin/mi_thermald") {
        r.evidence.push("/vendor/bin/mi_thermald exists".into());
    }
    // Files alone are not Xiaomi evidence: other OEMs ship thermal-*.conf too.
    r.applies = !r.evidence.is_empty();
    if !r.applies {
        return r;
    }
    r.config_files = files.iter().map(|p| read_config(ctx.root, p)).collect();
    r.capabilities.push(config_capability(
        "config.xiaomi_mi_thermald",
        "Xiaomi devices with mi_thermald",
        "list /vendor/etc for thermal-*.conf and thermald-devices.conf",
        &r.config_files,
        &[],
    ));
    r.notes.push(
        "Xiaomi thermal profiles are not publicly documented; files are hashed and classified only"
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
    fn recognized_but_not_interpreted() {
        let t = TempDir::new("xiaomi");
        t.file(
            "vendor/etc/thermal-normal.conf",
            "\u{1}\u{2}\u{3}binary\u{0}",
        );
        t.file(
            "vendor/etc/thermald-devices.conf",
            "#d\n[\nname:gpu\ncooling_name:mtk-cl-gpu\n]\n",
        );
        t.file("vendor/etc/thermal-engine.conf", "[A]\nalgo_type ss\n");
        let fx = Fixture::new(t.path(), &[("ro.product.manufacturer", "Xiaomi")]);
        let r = probe(&fx.ctx());
        assert!(r.applies);
        assert_eq!(r.config_files.len(), 2);
        assert_eq!(r.capabilities[0].state, CapabilityState::DetectedUnverified);
        let none = Fixture::new(t.path(), &[("ro.product.manufacturer", "OnePlus")]);
        assert!(!probe(&none.ctx()).applies, "files alone are not evidence");
    }
}
