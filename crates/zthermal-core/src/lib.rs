//! Zairenkai ThermalTools core: device identity, readings, capabilities, configuration
//! parsers, profiles and policy rules. No I/O: everything here is testable without a device.

pub mod capability;
pub mod config;
pub mod identity;
pub mod policy;
pub mod profile;
pub mod reading;
