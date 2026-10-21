//! Zairenkai ThermalTools platform layer: everything that touches a device. Reads go through
//! [`sysroot::SysRoot`] (a fixture directory in tests) and commands through [`exec::Runner`]
//! (never a shell).

pub mod adapters;
pub mod android;
pub mod backup;
pub mod device;
pub mod exec;
pub mod snapshot;
pub mod sysfs;
pub mod sysroot;
pub mod txn;
