// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! Read-only backups of vendor configuration files, with a SHA-256 manifest.
//!
//! A backup copies files out of the device tree into a new directory. There is no restore
//! into `/vendor`, `/odm` or `/system`: the toolkit never writes vendor configuration.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zthermal_core::config::sha256_hex;
use zthermal_core::reading::DataStatus;

use crate::adapters::MAX_CONFIG_BYTES;
use crate::sysroot::SysRoot;

pub const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupEntry {
    pub source: String,
    /// Path relative to the backup directory.
    pub stored_as: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub source: String,
    pub status: DataStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema_version: u32,
    pub tool_version: String,
    pub created_unix_ms: u64,
    pub entries: Vec<BackupEntry>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyResult {
    pub stored_as: String,
    pub ok: bool,
    pub detail: Option<String>,
}

fn valid_source(p: &str) -> bool {
    p.starts_with('/') && !p.split('/').any(|c| c == ".." || c == ".")
}

fn write_new(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(data)?;
    f.sync_all()
}

/// Copies `sources` (absolute device paths) into `dest`, which must not exist or be empty.
pub fn create(
    root: &SysRoot,
    sources: &[String],
    dest: &Path,
    now_ms: u64,
) -> Result<BackupManifest, String> {
    if dest.exists()
        && fs::read_dir(dest)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err(format!(
            "{} is not empty; a backup never overwrites",
            dest.display()
        ));
    }
    fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut m = BackupManifest {
        schema_version: 1,
        tool_version: env!("CARGO_PKG_VERSION").into(),
        created_unix_ms: now_ms,
        entries: vec![],
        skipped: vec![],
    };
    for src in sources {
        if !valid_source(src) {
            m.skipped.push(Skipped {
                source: src.clone(),
                status: DataStatus::ParseError,
                detail: "not an absolute path without . or ..".into(),
            });
            continue;
        }
        match root.read_bounded(src, MAX_CONFIG_BYTES) {
            Ok(data) => {
                let stored_as = format!("files{src}");
                write_new(&dest.join(&stored_as), &data)
                    .map_err(|e| format!("{stored_as}: {e}"))?;
                m.entries.push(BackupEntry {
                    source: src.clone(),
                    stored_as,
                    size: data.len() as u64,
                    sha256: sha256_hex(&data),
                });
            }
            Err(e) => m.skipped.push(Skipped {
                source: src.clone(),
                status: e.status,
                detail: e.detail,
            }),
        }
    }
    let json = serde_json::to_vec_pretty(&m).map_err(|e| e.to_string())?;
    let tmp = dest.join(format!("{MANIFEST}.tmp"));
    write_new(&tmp, &json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dest.join(MANIFEST)).map_err(|e| e.to_string())?;
    Ok(m)
}

/// Re-hashes every stored file against the manifest.
pub fn verify(dest: &Path) -> Result<(BackupManifest, Vec<VerifyResult>), String> {
    let text = fs::read_to_string(dest.join(MANIFEST)).map_err(|e| format!("{MANIFEST}: {e}"))?;
    let m: BackupManifest = serde_json::from_str(&text).map_err(|e| format!("{MANIFEST}: {e}"))?;
    let results = m
        .entries
        .iter()
        .map(|e| {
            let p: PathBuf = dest.join(&e.stored_as);
            match (e.stored_as.contains(".."), fs::read(&p)) {
                (true, _) => VerifyResult {
                    stored_as: e.stored_as.clone(),
                    ok: false,
                    detail: Some("path escapes the backup".into()),
                },
                (_, Ok(d)) if sha256_hex(&d) == e.sha256 && d.len() as u64 == e.size => {
                    VerifyResult {
                        stored_as: e.stored_as.clone(),
                        ok: true,
                        detail: None,
                    }
                }
                (_, Ok(_)) => VerifyResult {
                    stored_as: e.stored_as.clone(),
                    ok: false,
                    detail: Some("hash or size mismatch".into()),
                },
                (_, Err(err)) => VerifyResult {
                    stored_as: e.stored_as.clone(),
                    ok: false,
                    detail: Some(err.to_string()),
                },
            }
        })
        .collect();
    Ok((m, results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysroot::testutil::TempDir;

    #[test]
    fn backup_and_verify() {
        let dev = TempDir::new("bk-dev");
        dev.file("vendor/etc/thermal-engine.conf", "[A]\nalgo_type ss\n");
        let out = TempDir::new("bk-out");
        let dest = out.path().join("b1");
        let srcs = vec![
            "/vendor/etc/thermal-engine.conf".to_string(),
            "/vendor/etc/missing.conf".into(),
            "/vendor/../etc/x".into(),
        ];
        let m = create(&SysRoot::at(dev.path()), &srcs, &dest, 1).unwrap();
        assert_eq!(m.entries.len(), 1);
        assert_eq!(m.entries[0].sha256, sha256_hex(b"[A]\nalgo_type ss\n"));
        assert_eq!(m.skipped.len(), 2);
        let (_, r) = verify(&dest).unwrap();
        assert!(r.iter().all(|v| v.ok));
        // Tampering is detected; a second backup into the same directory is refused.
        fs::write(dest.join("files/vendor/etc/thermal-engine.conf"), "changed").unwrap();
        assert!(!verify(&dest).unwrap().1[0].ok);
        assert!(create(&SysRoot::at(dev.path()), &srcs, &dest, 2)
            .unwrap_err()
            .contains("not empty"));
    }
}
