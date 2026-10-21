// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Vendor adapters: which thermal implementation a device ships, where its configuration
//! lives, and what can be read from it.
//!
//! An adapter applies when the device shows evidence for it (a SoC property, a file, a
//! sensor naming scheme). Recognizing a vendor never grants a control: adapters produce
//! read-only capabilities and inspections, and say what is unverified.

pub mod generic;
pub mod mediatek;
pub mod pixel;
pub mod qcom;
pub mod xiaomi;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use zthermal_core::capability::{Capability, CapabilityState, Privilege, Validation};
use zthermal_core::config::{inspect, ConfigFormat, Inspection};
use zthermal_core::identity::DeviceIdentity;
use zthermal_core::reading::DataStatus;

use crate::device::SourceStatus;
use crate::sysfs::SysfsInventory;
use crate::sysroot::SysRoot;

/// Vendor configuration files are small; anything larger is refused, not truncated.
pub const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

pub struct Context<'a> {
    pub root: &'a SysRoot,
    pub props: &'a BTreeMap<String, String>,
    pub identity: &'a DeviceIdentity,
    pub inventory: &'a SysfsInventory,
    /// Results of the framework probes, when they ran in this invocation.
    pub framework: Option<&'a FrameworkProbe>,
}

/// Outcome of `dumpsys thermalservice` and `dumpsys battery` in one invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameworkProbe {
    pub thermalservice: SourceStatus,
    pub battery: SourceStatus,
}

impl Context<'_> {
    pub fn prop(&self, key: &str) -> Option<&str> {
        self.props
            .get(key)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }

    /// `init.svc.<name>` states for the given exact service names (names taken from a
    /// published init `.rc` file). The state is what init reports; it does not show that the
    /// service is authoritative for thermal management, and nothing here can change it.
    pub fn services_named(&self, names: &[&str]) -> BTreeMap<String, String> {
        names
            .iter()
            .filter_map(|n| {
                self.props
                    .get(&format!("init.svc.{n}"))
                    .map(|v| (n.to_string(), v.clone()))
            })
            .collect()
    }

    /// Other `init.svc.*` entries whose name contains `needle`: kept for diagnosis only,
    /// never treated as a confirmed thermal service.
    pub fn service_candidates(&self, needle: &str, confirmed: &[&str]) -> BTreeMap<String, String> {
        self.props
            .iter()
            .filter_map(|(k, v)| {
                k.strip_prefix("init.svc.")
                    .filter(|s| s.contains(needle) && !confirmed.contains(s))
                    .map(|s| (s.to_string(), v.clone()))
            })
            .collect()
    }

    /// Files in `dir` (absolute) whose name satisfies `pred`, as absolute paths.
    pub fn files_in(&self, dir: &str, pred: impl Fn(&str) -> bool) -> Vec<String> {
        self.root
            .list_dir(dir)
            .into_iter()
            .filter(|n| pred(n))
            .map(|n| format!("{dir}/{n}"))
            .collect()
    }

    pub fn zone_types(&self) -> Vec<&str> {
        self.inventory
            .zones
            .iter()
            .filter_map(|z| z.zone_type.value.as_deref())
            .collect()
    }
}

/// One configuration file found on the device, inspected read-only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigFile {
    pub path: String,
    pub status: DataStatus,
    pub detail: Option<String>,
    pub inspection: Option<Inspection>,
}

pub fn read_config(root: &SysRoot, path: &str) -> ConfigFile {
    read_config_data(root, path).0
}

/// Reads and inspects one file, and also returns the bytes so a caller that needs them
/// (Pixel `Include` lists) does not read the file a second time.
pub fn read_config_data(root: &SysRoot, path: &str) -> (ConfigFile, Option<Vec<u8>>) {
    match root.read_bounded(path, MAX_CONFIG_BYTES) {
        Ok(data) => (
            ConfigFile {
                path: path.into(),
                status: DataStatus::Valid,
                detail: None,
                inspection: Some(inspect(path, &data)),
            },
            Some(data),
        ),
        Err(e) => (
            ConfigFile {
                path: path.into(),
                status: e.status,
                detail: Some(e.detail),
                inspection: None,
            },
            None,
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterReport {
    pub adapter: String,
    pub applies: bool,
    /// Why the adapter applies (or not).
    pub evidence: Vec<String>,
    /// Services whose names come from published init files, with their `init.svc` state.
    pub services: BTreeMap<String, String>,
    /// Other services whose names merely match; unconfirmed, for diagnosis only.
    pub candidate_services: BTreeMap<String, String>,
    pub config_files: Vec<ConfigFile>,
    pub capabilities: Vec<Capability>,
    pub notes: Vec<String>,
}

impl AdapterReport {
    pub fn new(adapter: &str) -> Self {
        Self {
            adapter: adapter.into(),
            applies: false,
            evidence: vec![],
            services: BTreeMap::new(),
            candidate_services: BTreeMap::new(),
            config_files: vec![],
            capabilities: vec![],
            notes: vec![],
        }
    }
}

/// The capability for inspecting a vendor's configuration files.
pub fn config_capability(
    id: &str,
    scope: &str,
    discovery: &str,
    files: &[ConfigFile],
    parsers: &[ConfigFormat],
) -> Capability {
    let cap = Capability::read(
        id,
        "read, parse and validate the vendor thermal configuration (never written back)",
        scope,
        discovery,
        "read access to the file (usually world-readable under /vendor/etc)",
        Privilege::Unprivileged,
    );
    if files.is_empty() {
        return cap.with(
            CapabilityState::Unsupported,
            Some("no configuration file found".into()),
        );
    }
    let parsed: Vec<&ConfigFile> = files
        .iter()
        .filter(|f| {
            f.inspection
                .as_ref()
                .is_some_and(|i| parsers.contains(&i.detection.format))
        })
        .collect();
    let mut cap = files.iter().fold(cap, |c, f| c.evidence(f.path.clone()));
    if !parsed.is_empty() {
        cap = cap
            .with(CapabilityState::AvailableReadOnly, None)
            .validated(Validation::PublicSource);
    } else if files
        .iter()
        .all(|f| f.status == DataStatus::PermissionDenied)
    {
        cap = cap.with(
            CapabilityState::PermissionDenied,
            Some("files exist but cannot be read".into()),
        );
    } else if files.iter().any(|f| f.inspection.is_some()) {
        cap = cap.with(
            CapabilityState::DetectedUnverified,
            Some("files found, but their format has no parser (opaque or unrecognized); not interpreted".into()),
        );
    } else {
        cap = cap.with(
            CapabilityState::Error,
            files.iter().find_map(|f| f.detail.clone()),
        );
    }
    cap
}

/// Runs every adapter.
pub fn probe_all(ctx: &Context<'_>) -> Vec<AdapterReport> {
    vec![
        generic::probe(ctx),
        qcom::probe(ctx),
        mediatek::probe(ctx),
        pixel::probe(ctx),
        xiaomi::probe(ctx),
    ]
}

#[cfg(test)]
pub mod testutil {
    use super::*;
    use crate::sysfs::discover;

    pub struct Fixture {
        pub root: SysRoot,
        pub props: BTreeMap<String, String>,
        pub identity: DeviceIdentity,
        pub inventory: SysfsInventory,
        pub framework: Option<FrameworkProbe>,
    }

    impl Fixture {
        pub fn new(root: &std::path::Path, props: &[(&str, &str)]) -> Self {
            let root = SysRoot::at(root);
            let props: BTreeMap<String, String> = props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let identity = DeviceIdentity::from_props(&props, None, None);
            let inventory = discover(&root);
            Self {
                root,
                props,
                identity,
                inventory,
                framework: None,
            }
        }

        pub fn ctx(&self) -> Context<'_> {
            Context {
                root: &self.root,
                props: &self.props,
                identity: &self.identity,
                inventory: &self.inventory,
                framework: self.framework.as_ref(),
            }
        }
    }
}
