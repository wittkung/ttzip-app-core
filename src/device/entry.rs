// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![forbid(unsafe_code)]

//! Canonical metadata descriptor for remote peripheral and device entries (ADB and MTP).

use serde::{Deserialize, Serialize};
use ttzip_types::{UnifiedEntryNode, NODE_FLAG_DIRECTORY};

/// Canonical metadata descriptor for an entry on a remote peripheral device (ADB, MTP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct DeviceEntry {
    /// Normalized virtual path in device namespace (e.g., "/sdcard/Download/document.pdf").
    pub path: String,
    /// Base filename or directory name.
    pub name: String,
    /// Logical uncompressed file size in bytes. Directories report 0.
    pub size: u64,
    /// Whether this entry represents a directory container.
    pub is_dir: bool,
    /// Last modification timestamp in POSIX integer epoch seconds.
    pub mtime: u64,
    /// Physical 32-bit MTP object handle (Some for MTP hardware streaming, None for ADB).
    pub object_id: Option<u32>,
    /// POSIX file mode bits (Some for ADB, None for MTP).
    pub posix_mode: Option<u32>,
}

impl DeviceEntry {
    /// Creates a new directory entry descriptor.
    #[inline]
    pub fn directory(path: impl Into<String>, name: impl Into<String>, mtime: u64) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            size: 0,
            is_dir: true,
            mtime,
            object_id: None,
            posix_mode: Some(0o755),
        }
    }

    /// Creates a new regular file descriptor.
    #[inline]
    pub fn regular_file(
        path: impl Into<String>,
        name: impl Into<String>,
        size: u64,
        mtime: u64,
    ) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            size,
            is_dir: false,
            mtime,
            object_id: None,
            posix_mode: Some(0o644),
        }
    }

    /// Legacy constructor used by MTP bulk parser for directory entries: `(handle, name, mtime)`.
    #[inline]
    pub fn dir(id: u64, name: impl Into<String>, mtime: i64) -> Self {
        let name_str = name.into();
        Self {
            path: format!("/{}", name_str),
            name: name_str,
            size: 0,
            is_dir: true,
            mtime: if mtime > 0 { mtime as u64 } else { 0 },
            object_id: if id <= u32::MAX as u64 { Some(id as u32) } else { None },
            posix_mode: Some(0o755),
        }
    }

    /// Legacy constructor used by MTP bulk parser for file entries: `(handle, name, size, mtime)`.
    #[inline]
    pub fn file(id: u64, name: impl Into<String>, size: u64, mtime: i64) -> Self {
        let name_str = name.into();
        Self {
            path: format!("/{}", name_str),
            name: name_str,
            size,
            is_dir: false,
            mtime: if mtime > 0 { mtime as u64 } else { 0 },
            object_id: if id <= u32::MAX as u64 { Some(id as u32) } else { None },
            posix_mode: Some(0o644),
        }
    }

    /// Attaches an MTP physical object handle.
    #[inline]
    pub fn with_object_id(mut self, object_id: u32) -> Self {
        self.object_id = Some(object_id);
        self
    }

    /// Attaches a POSIX permission mode bitmask.
    #[inline]
    pub fn with_posix_mode(mut self, mode: u32) -> Self {
        self.posix_mode = Some(mode);
        self
    }

    /// Returns `true` if this entry represents a regular file.
    #[inline]
    pub fn is_file(&self) -> bool {
        !self.is_dir
    }

    /// Returns `true` if this entry represents a directory container.
    #[inline]
    pub fn is_directory(&self) -> bool {
        self.is_dir
    }

    /// Converts this peripheral entry into a microkernel-compatible [`UnifiedEntryNode`].
    #[inline]
    pub fn to_unified_node(&self) -> UnifiedEntryNode {
        let mut node = UnifiedEntryNode::empty();
        node.uncompressed_size = self.size;
        node.compressed_size = self.size;
        node.mtime_epoch_secs = self.mtime as u32;
        if self.is_dir {
            node.flags |= NODE_FLAG_DIRECTORY;
        }
        node
    }
}

// Compile-time static gate ensuring DeviceEntry satisfies Send and Sync invariants.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DeviceEntry>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_entry_file_and_dir_constructors() {
        let entry = DeviceEntry::file(101, "photo.jpg", 2048, 1712345678);
        assert_eq!(entry.object_id, Some(101));
        assert_eq!(entry.name, "photo.jpg");
        assert_eq!(entry.path, "/photo.jpg");
        assert_eq!(entry.size, 2048);
        assert!(!entry.is_dir);
        assert!(entry.is_file());
        assert!(!entry.is_directory());
        assert_eq!(entry.mtime, 1712345678);
        assert_eq!(entry.posix_mode, Some(0o644));

        let dir = DeviceEntry::dir(202, "DCIM", 1712340000);
        assert_eq!(dir.object_id, Some(202));
        assert_eq!(dir.name, "DCIM");
        assert_eq!(dir.path, "/DCIM");
        assert_eq!(dir.size, 0);
        assert!(dir.is_dir);
        assert!(!dir.is_file());
        assert!(dir.is_directory());
        assert_eq!(dir.mtime, 1712340000);
        assert_eq!(dir.posix_mode, Some(0o755));
    }

    #[test]
    fn test_device_entry_regular_and_directory_constructors() {
        let file = DeviceEntry::regular_file("/sdcard/doc.pdf", "doc.pdf", 4096, 1700000000)
            .with_posix_mode(0o600);
        assert_eq!(file.path, "/sdcard/doc.pdf");
        assert_eq!(file.name, "doc.pdf");
        assert_eq!(file.size, 4096);
        assert!(!file.is_dir);
        assert_eq!(file.mtime, 1700000000);
        assert_eq!(file.posix_mode, Some(0o600));
        assert_eq!(file.object_id, None);

        let node = file.to_unified_node();
        assert!(node.is_file());
        assert_eq!(node.uncompressed_size, 4096);
        assert_eq!(node.mtime_epoch_secs, 1700000000);

        let dir = DeviceEntry::directory("/sdcard/Download", "Download", 1700001000)
            .with_object_id(42);
        assert!(dir.is_dir);
        assert_eq!(dir.object_id, Some(42));
        let dir_node = dir.to_unified_node();
        assert!(dir_node.is_dir());
    }

    #[test]
    fn test_device_entry_equality_and_clone() {
        let a = DeviceEntry::file(1, "test.txt", 100, 1000);
        let b = a.clone();
        assert_eq!(a, b);

        let c = DeviceEntry::dir(1, "test.txt", 1000);
        assert_ne!(a, c);
    }

    #[test]
    fn test_device_entry_serde_roundtrip() {
        let original = DeviceEntry::regular_file("/data/archive.tar", "archive.tar", 1048576, 500)
            .with_object_id(999);
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: DeviceEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_device_entry_extreme_bounds() {
        let entry = DeviceEntry::file(u64::MAX, "", u64::MAX, i64::MIN);
        assert_eq!(entry.object_id, None);
        assert_eq!(entry.size, u64::MAX);
        assert_eq!(entry.mtime, 0);
        assert!(entry.is_file());
    }
}
