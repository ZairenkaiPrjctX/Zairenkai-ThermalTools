// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! MediaTek: legacy `mtkts*` thermal zones and the `/vendor/etc/.tp/` policy files.
//!
//! Published `.tp` files are printable but enciphered. They are detected and hashed, never
//! decoded; no MediaTek control is offered.

use zthermal_core::identity::SocVendor;

use super::{config_capability, read_config, AdapterReport, Context};

pub const TP_DIR: &str = "/vendor/etc/.tp";

/// Service names from the init files of a published MediaTek vendor blob set
/// (TheMuppets proprietary_vendor_xiaomi_mt6768-common b62d70f6:
/// `init.thermal.rc`, `init.thermal_manager.rc`, `init.thermalloadalgod.rc`). Other MediaTek
/// builds may name them differently; those show up as candidates.
pub const SERVICES: &[&str] = &["thermal", "thermal_manager", "thermalloadalgod"];

/// Legacy zone types registered by the MediaTek kernel drivers (MiCode kernel source,
/// drivers/misc/mediatek/thermal/common/thermal_zones).
pub fn is_legacy_zone(t: &str) -> bool {
    t.starts_with("mtkts")
}

pub fn probe(ctx: &Context<'_>) -> AdapterReport {
    let mut r = AdapterReport::new("mediatek");
    if ctx.identity.soc_vendor.value == SocVendor::Mediatek {
        r.evidence.extend(
            ctx.identity
                .soc_vendor
                .evidence
                .iter()
                .map(|e| format!("{} = {}", e.source, e.value)),
        );
    }
    let legacy = ctx
        .zone_types()
        .iter()
        .filter(|t| is_legacy_zone(t))
        .count();
    if legacy > 0 {
        r.evidence
            .push(format!("{legacy} zones with legacy mtkts* names"));
    }
    let files = ctx.files_in(TP_DIR, |_| true);
    if !files.is_empty() {
        r.evidence
            .push(format!("{} files in {TP_DIR}", files.len()));
    }
    for bin in [
        "/vendor/bin/thermal_manager",
        "/vendor/bin/thermalloadalgod",
        "/vendor/bin/thermal",
    ] {
        if ctx.root.exists(bin) {
            r.evidence.push(format!("{bin} exists"));
        }
    }
    r.services = ctx.services_named(SERVICES);
    r.candidate_services = ctx.service_candidates("thermal", SERVICES);
    r.applies = !r.evidence.is_empty();
    if !r.applies {
        return r;
    }
    r.config_files = files.iter().map(|p| read_config(ctx.root, p)).collect();
    r.capabilities.push(config_capability(
        "config.mediatek_thermal_policy",
        "MediaTek devices with /vendor/etc/.tp",
        "list /vendor/etc/.tp",
        &r.config_files,
        &[],
    ));
    r.notes.push("MediaTek .tp policy files are enciphered by the vendor; they are hashed and reported, never decoded".into());
    if legacy == 0 && !ctx.inventory.zones.is_empty() {
        r.notes.push("zone names do not follow the legacy mtkts* scheme (newer kernels name zones after device tree nodes)".into());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testutil::Fixture;
    use crate::sysroot::testutil::TempDir;
    use zthermal_core::capability::CapabilityState;
    use zthermal_core::config::ConfigFormat;

    #[test]
    fn enciphered_policy_files_are_detected_not_decoded() {
        let t = TempDir::new("mtk");
        t.file("sys/class/thermal/thermal_zone0/type", "mtktscpu\n");
        t.file("sys/class/thermal/thermal_zone0/temp", "45000\n");
        let mut noise = String::new();
        let mut x: u32 = 7;
        for i in 0..1500 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            noise.push(if i % 50 == 49 {
                '\n'
            } else {
                (b'!' + ((x >> 16) % 94) as u8) as char
            });
        }
        t.file("vendor/etc/.tp/thermal.conf", &noise);
        let fx = Fixture::new(t.path(), &[("ro.board.platform", "mt6768")]);
        let r = probe(&fx.ctx());
        assert!(r.applies);
        let i = r.config_files[0].inspection.as_ref().unwrap();
        assert_eq!(i.detection.format, ConfigFormat::Opaque);
        assert_eq!(r.capabilities[0].state, CapabilityState::DetectedUnverified);
    }

    fn probe_with_getprop(getprop: &str) -> AdapterReport {
        let t = TempDir::new("mtk-svc");
        let props = zthermal_core::identity::parse_getprop(getprop);
        let pairs: Vec<(&str, &str)> = props
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let fx = Fixture::new(t.path(), &pairs);
        probe(&fx.ctx())
    }

    #[test]
    fn services_are_confirmed_only_by_documented_names() {
        let r = probe_with_getprop(
            "[ro.board.platform]: [mt6768]\n\
             [init.svc.thermal_manager]: [running]\n\
             [init.svc.thermal]: [running]\n\
             [init.svc.vendor.thermal-hal-2-0.mtk]: [running]\n\
             [init.svc.thermalservice-proxy]: [stopped]\n\
             [init.svc.audioserver]: [running]\n",
        );
        assert_eq!(
            r.services.keys().collect::<Vec<_>>(),
            ["thermal", "thermal_manager"]
        );
        // Name matches are kept as unconfirmed candidates, unrelated services are dropped.
        assert_eq!(
            r.candidate_services.keys().collect::<Vec<_>>(),
            ["thermalservice-proxy", "vendor.thermal-hal-2-0.mtk"]
        );
        assert!(r
            .capabilities
            .iter()
            .all(|c| c.access == zthermal_core::capability::Access::Read));
    }

    #[test]
    fn duplicate_malformed_and_missing_service_output() {
        // getprop prints a property twice: the last value wins, and it is listed once.
        let r = probe_with_getprop(
            "[ro.board.platform]: [mt6768]\n[init.svc.thermal]: [stopped]\n[init.svc.thermal]: [running]\n",
        );
        assert_eq!(
            r.services.get("thermal").map(String::as_str),
            Some("running")
        );
        assert_eq!(r.services.len(), 1);
        // Malformed lines are ignored, not guessed.
        let r = probe_with_getprop("[ro.board.platform]: [mt6768]\ninit.svc.thermal = running\n[init.svc.thermal_manager: [x]\n");
        assert!(
            r.services.is_empty() && r.candidate_services.is_empty(),
            "{:?}",
            r.services
        );
        // No getprop output at all: nothing is listed and the adapter needs other evidence.
        let r = probe_with_getprop("");
        assert!(!r.applies && r.services.is_empty());
    }
}
