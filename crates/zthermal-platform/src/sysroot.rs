// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ZairenkaiPrjctX contributors
//! File-system root abstraction.
//!
//! Every platform read and write goes through a [`SysRoot`]. Production uses `/`; tests
//! point it at a fixture directory, so no test can touch the host's `/sys` or `/proc`.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zthermal_core::reading::DataStatus;

/// sysfs attributes are at most one page; anything larger is not a thermal attribute.
pub const MAX_ATTR_BYTES: u64 = 4096;

#[derive(Debug, Clone)]
pub struct SysRoot {
    root: PathBuf,
    /// Test-only log of `read_bounded` calls, to prove an operation reads a file once.
    #[cfg(test)]
    log: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
}

impl PartialEq for SysRoot {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

impl Eq for SysRoot {}

/// A failed read, with the status it maps to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadError {
    pub status: DataStatus,
    pub detail: String,
}

impl ReadError {
    fn from_io(path: &str, e: &io::Error) -> Self {
        let status = match e.kind() {
            io::ErrorKind::NotFound => DataStatus::Unsupported,
            io::ErrorKind::PermissionDenied => DataStatus::PermissionDenied,
            io::ErrorKind::InvalidData => DataStatus::ParseError,
            // EIO, EAGAIN, ENODATA, EINVAL ... : the node exists but gave no value now.
            _ => DataStatus::Unavailable,
        };
        Self {
            status,
            detail: format!("{path}: {e}"),
        }
    }
}

/// File mode facts of a node. Writable-by-caller is never inferred from these bits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeMode {
    pub mode: String,
    pub owner_writable: bool,
    pub world_readable: bool,
}

impl SysRoot {
    pub fn real() -> Self {
        Self::at("/")
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            #[cfg(test)]
            log: None,
        }
    }

    /// A root that records every bounded read (tests only).
    #[cfg(test)]
    pub fn recording(root: impl Into<PathBuf>) -> Self {
        Self {
            log: Some(Default::default()),
            ..Self::at(root)
        }
    }

    /// How often `abs` was read through [`SysRoot::read_bounded`] (tests only).
    #[cfg(test)]
    pub fn reads_of(&self, abs: &str) -> usize {
        self.log
            .as_ref()
            .map(|l| l.lock().unwrap().iter().filter(|p| *p == abs).count())
            .unwrap_or(0)
    }

    pub fn is_real(&self) -> bool {
        self.root == Path::new("/")
    }

    /// Map an absolute device path (`/sys/...`) into this root.
    pub fn path(&self, abs: &str) -> PathBuf {
        let rel = abs.strip_prefix('/').unwrap_or(abs);
        self.root.join(rel)
    }

    pub fn exists(&self, abs: &str) -> bool {
        self.path(abs).exists()
    }

    pub fn is_dir(&self, abs: &str) -> bool {
        self.path(abs).is_dir()
    }

    /// Read a small text attribute, trimmed, bounded to [`MAX_ATTR_BYTES`].
    pub fn read_attr(&self, abs: &str) -> Result<String, ReadError> {
        self.read_bounded(abs, MAX_ATTR_BYTES)
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
    }

    /// Read a file of at most `limit` bytes; larger files are refused, not truncated.
    pub fn read_bounded(&self, abs: &str, limit: u64) -> Result<Vec<u8>, ReadError> {
        #[cfg(test)]
        if let Some(l) = &self.log {
            l.lock().unwrap().push(abs.to_string());
        }
        let f = fs::File::open(self.path(abs)).map_err(|e| ReadError::from_io(abs, &e))?;
        let mut buf = Vec::new();
        f.take(limit + 1)
            .read_to_end(&mut buf)
            .map_err(|e| ReadError::from_io(abs, &e))?;
        if buf.len() as u64 > limit {
            return Err(ReadError {
                status: DataStatus::Unavailable,
                detail: format!("{abs}: larger than {limit} bytes; refused"),
            });
        }
        Ok(buf)
    }

    pub fn mode(&self, abs: &str) -> Option<NodeMode> {
        let m = fs::metadata(self.path(abs)).ok()?.permissions().mode();
        Some(NodeMode {
            mode: format!("{:04o}", m & 0o7777),
            owner_writable: m & 0o200 != 0,
            world_readable: m & 0o004 != 0,
        })
    }

    /// Directory entries, sorted. Missing directory → empty list.
    pub fn list_dir(&self, abs: &str) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(self.path(abs))
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|a| natural_key(a));
        v
    }

    /// Final path component a symlink points to (sysfs `cdevN` links), if it is one.
    pub fn link_name(&self, abs: &str) -> Option<String> {
        let target = fs::read_link(self.path(abs)).ok()?;
        target.file_name().map(|n| n.to_string_lossy().into_owned())
    }

    /// Write an existing node. Never creates files: a missing node is an error, so a wrong
    /// path cannot create a regular file in sysfs or elsewhere.
    pub fn write_existing(&self, abs: &str, value: &str) -> io::Result<()> {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(false)
            .open(self.path(abs))?;
        f.write_all(value.as_bytes())?;
        f.flush()
    }
}

/// Sort key that orders "thermal_zone2" before "thermal_zone10".
pub fn natural_key(s: &str) -> (String, u64, String) {
    let split = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    let (head, tail) = s.split_at(split);
    let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = tail[digits.len()..].to_string();
    (head.to_string(), digits.parse().unwrap_or(0), rest)
}

#[cfg(test)]
pub mod testutil {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Temporary fixture directory, removed on drop.
    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "zthermal-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn file(&self, rel: &str, content: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, content).unwrap();
            p
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::TempDir;
    use super::*;

    #[test]
    fn reads_map_errors_to_statuses() {
        let d = TempDir::new("sysroot");
        d.file("sys/a", " 42 \n");
        d.file("sys/big", &"x".repeat(5000));
        let r = SysRoot::at(d.path());
        assert_eq!(r.read_attr("/sys/a").unwrap(), "42");
        assert_eq!(
            r.read_attr("/sys/missing").unwrap_err().status,
            DataStatus::Unsupported
        );
        let big = r.read_attr("/sys/big").unwrap_err();
        assert!(big.detail.contains("refused"));
        // Reading a directory is EISDIR: "exists but no value", not "unsupported".
        assert_eq!(
            r.read_attr("/sys").unwrap_err().status,
            DataStatus::Unavailable
        );
    }

    #[test]
    fn writes_never_create_files() {
        let d = TempDir::new("sysroot-w");
        d.file("sys/node", "1");
        let r = SysRoot::at(d.path());
        r.write_existing("/sys/node", "2").unwrap();
        assert_eq!(r.read_attr("/sys/node").unwrap(), "2");
        assert!(r.write_existing("/sys/new", "1").is_err());
        assert!(!r.exists("/sys/new"));
    }

    #[test]
    fn natural_ordering() {
        let mut v = vec![
            "thermal_zone10",
            "thermal_zone2",
            "thermal_zone1",
            "cooling_device0",
        ];
        v.sort_by_key(|a| natural_key(a));
        assert_eq!(
            v,
            vec![
                "cooling_device0",
                "thermal_zone1",
                "thermal_zone2",
                "thermal_zone10"
            ]
        );
    }
}
