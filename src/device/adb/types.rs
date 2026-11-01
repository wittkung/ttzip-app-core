// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Strongly typed ADB device descriptors and filesystem sync entries.

use std::collections::HashSet;
use serde::{Deserialize, Serialize};

/// Connection transport medium used to talk to the ADB daemon or device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdbTransportType {
    /// Physical USB cable transport.
    Usb,
    /// Local Android emulator (e.g. emulator-5554 communicating via local TCP loopback).
    Emulator,
    /// Direct TCP network transport (Ethernet, Wi-Fi, WSL2, Docker container, or virtual bridge).
    Tcp,
    /// Local ADB server bridge daemon (127.0.0.1:5037).
    LocalDaemonBridge,
}

/// Operational status of an ADB device node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdbDeviceState {
    Device,
    Unauthorized,
    Authorizing,
    NoPermissions,
    Offline,
    Connecting,
    Recovery,
    Sideload,
    Bootloader,
    Unknown(String),
}

impl std::fmt::Display for AdbDeviceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Device => write!(f, "device"),
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::Authorizing => write!(f, "authorizing"),
            Self::NoPermissions => write!(f, "no permissions"),
            Self::Offline => write!(f, "offline"),
            Self::Connecting => write!(f, "connecting"),
            Self::Recovery => write!(f, "recovery"),
            Self::Sideload => write!(f, "sideload"),
            Self::Bootloader => write!(f, "bootloader"),
            Self::Unknown(s) => write!(f, "{}", s),
        }
    }
}

/// Description of an ADB-connected Android device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdbDevice {
    pub serial: String,
    pub state: AdbDeviceState,
    pub model: Option<String>,
    pub product: Option<String>,
    pub device: Option<String>,
    pub transport_type: AdbTransportType,
    pub transport_id: Option<u64>,
    #[serde(default)]
    pub features: HashSet<String>,
}

impl AdbDevice {
    /// Returns true if this device negotiated or reported the specified feature flag.
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.contains(feature)
    }
}

/// Standard POSIX file mode bit constants for ADB filesystem entries.
pub mod mode {
    pub const S_IFMT: u32 = 0o170000; // File type bitmask
    pub const S_IFDIR: u32 = 0o040000; // Directory
    pub const S_IFCHR: u32 = 0o020000; // Character device
    pub const S_IFBLK: u32 = 0o060000; // Block device
    pub const S_IFREG: u32 = 0o100000; // Regular file
    pub const S_IFLNK: u32 = 0o120000; // Symbolic link
    pub const S_IFSOCK: u32 = 0o140000; // Socket
    pub const S_IFIFO: u32 = 0o010000; // FIFO
}

/// Directory item metadata parsed from an ADB SYNC `DENT` or `STAT` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdbSyncEntry {
    pub name: String,
    pub mode: u32,
    pub size: u64,
    pub mtime: u32,
}

impl AdbSyncEntry {
    /// Returns true if the entry exists on the remote filesystem (mode != 0).
    /// In the ADB SYNC protocol, querying a non-existent file returns STAT with all zeroes.
    pub fn exists(&self) -> bool {
        self.mode != 0
    }

    /// Returns true if the entry is a directory (POSIX S_IFDIR bit: 0o040000).
    pub fn is_directory(&self) -> bool {
        self.exists() && (self.mode & mode::S_IFMT) == mode::S_IFDIR
    }

    /// Returns true if the entry is a regular file (POSIX S_IFREG bit: 0o100000).
    pub fn is_file(&self) -> bool {
        self.exists() && (self.mode & mode::S_IFMT) == mode::S_IFREG
    }

    /// Returns true if the entry is a symbolic link (POSIX S_IFLNK bit: 0o120000).
    pub fn is_symlink(&self) -> bool {
        self.exists() && (self.mode & mode::S_IFMT) == mode::S_IFLNK
    }

    /// Extracts the POSIX permission bits (lower 12 bits: setuid/setgid/sticky + rwxrwxrwx).
    pub fn permissions(&self) -> u32 {
        self.mode & 0o7777
    }

    /// Extracts the file type mask portion (e.g. mode::S_IFDIR, mode::S_IFREG).
    pub fn file_type(&self) -> u32 {
        self.mode & mode::S_IFMT
    }

    /// Returns the remote modification timestamp as a standard `SystemTime`.
    pub fn modified_time(&self) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(self.mtime as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adb_sync_entry_flags() {
        let dir_entry = AdbSyncEntry {
            name: "Download".to_string(),
            mode: mode::S_IFDIR | 0o755, // Directory with rwxr-xr-x
            size: 4096,
            mtime: 1700000000,
        };
        assert!(dir_entry.exists());
        assert!(dir_entry.is_directory());
        assert!(!dir_entry.is_file());
        assert!(!dir_entry.is_symlink());
        assert_eq!(dir_entry.permissions(), 0o755);
        assert_eq!(dir_entry.file_type(), mode::S_IFDIR);

        let file_entry = AdbSyncEntry {
            name: "photo.jpg".to_string(),
            mode: mode::S_IFREG | 0o644, // Regular file with rw-r--r--
            size: 1048576,
            mtime: 1700000000,
        };
        assert!(file_entry.exists());
        assert!(file_entry.is_file());
        assert!(!file_entry.is_directory());
        assert!(!file_entry.is_symlink());
        assert_eq!(file_entry.permissions(), 0o644);
        assert_eq!(file_entry.file_type(), mode::S_IFREG);
        assert_eq!(
            file_entry.modified_time(),
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1700000000)
        );

        let symlink_entry = AdbSyncEntry {
            name: "link.txt".to_string(),
            mode: mode::S_IFLNK | 0o777, // Symlink with lrwxrwxrwx
            size: 10,
            mtime: 1700000000,
        };
        assert!(symlink_entry.exists());
        assert!(symlink_entry.is_symlink());
        assert!(!symlink_entry.is_file());
        assert!(!symlink_entry.is_directory());
        assert_eq!(symlink_entry.permissions(), 0o777);
        assert_eq!(symlink_entry.file_type(), mode::S_IFLNK);

        // File does not exist (mode == 0 returned by ADB STAT)
        let non_existent = AdbSyncEntry {
            name: "missing.txt".to_string(),
            mode: 0,
            size: 0,
            mtime: 0,
        };
        assert!(!non_existent.exists());
        assert!(!non_existent.is_file());
        assert!(!non_existent.is_directory());
        assert!(!non_existent.is_symlink());
    }

    #[test]
    fn test_adb_device_features_query() {
        let mut features = HashSet::new();
        features.insert("shell_v2".to_string());
        features.insert("stat_v2".to_string());

        let dev = AdbDevice {
            serial: "emulator-5554".to_string(),
            state: AdbDeviceState::Device,
            model: None,
            product: None,
            device: None,
            transport_type: AdbTransportType::Emulator,
            transport_id: Some(1),
            features,
        };

        assert!(dev.has_feature("shell_v2"));
        assert!(dev.has_feature("stat_v2"));
        assert!(!dev.has_feature("apex"));
    }
}

