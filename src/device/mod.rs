// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Universal hardware and remote device abstraction layer (MTP 1.1 USB and ADB).
//!
//! # Microkernel Boundary Invariant
//! Peripheral drivers (ADB, MTP) operate strictly as edge I/O adapters. The core
//! compression and archive engines (`ttzip-archive`, `ttzip-engine`) remain entirely
//! agnostic to hardware transport details. Remote device trees are projected into the
//! microkernel via the unified [`ArchiveVfsProvider`] trait and converted into in-memory
//! [`ttzip_types::UnifiedVfsTable`] instances.

pub mod adb;
pub mod adb_socket;
pub mod checkpoint;
pub mod entry;
pub mod mtp;
pub mod mtp_bulk;

pub use entry::DeviceEntry;
pub use adb_socket::AdbSmartSocketClient;
pub use mtp_bulk::{parse_object_prop_list, MtpBulkFetcher};

pub use checkpoint::{
    crc32c, crc32c_update, CHUNK_PROBE_SIZE,
};

pub use adb::{
    mode as adb_mode, parse_adb_device_line, AdbClient, AdbDevice, AdbDeviceState, AdbError,
    AdbResult, AdbSyncEntry, AdbSyncError, AdbSyncStream,
    AdbTransportError, AdbTransportType, AdbVfsAdapter, DEFAULT_ADB_DAEMON_PORT,
    DEFAULT_ADB_PORT, DEFAULT_CONNECT_TIMEOUT,
};
pub use mtp::{
    allocate_non_colliding_filename, delete_mtp_object, format as mtp_format,
    mtp_delete_remote_object, mtp_pull_file_to_local_staged, mtp_push_local_file_verified,
    op as mtp_op, open_hardware_mtp_session, pull_mtp_file_streaming,
    pull_object_streaming_with_session, push_local_file_path_to_mtp, push_local_file_to_mtp,
    push_object_with_session, push_reader_with_session, resp as mtp_resp,
    scan_physical_mtp_devices, select_unique_mtp_device, ContainerType as MtpContainerType,
    MtpConflictPolicy, MtpContainer, MtpDeviceInfo, MtpDownloadOutcome, MtpEndpoints,
    MtpObjectHandle, MtpObjectInfo, MtpSession, MtpSessionState, MtpStorageInfo,
    MtpTransferProgressCallback, MtpUploadOutcome, MtpVfsAdapter, RusbTransport, UsbBulkTransport,
    DEFAULT_MAX_KEEP_BOTH_CANDIDATES, MTP_OBJECT_HANDLE_ALL, MTP_OBJECT_HANDLE_ROOT,
};

use crate::error::AppCoreError;
use ttzip_types::{UnifiedEntryNode, UnifiedVfsTable, NODE_FLAG_DIRECTORY};

/// Canonical metadata descriptor for an entry on a remote peripheral device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceVfsEntry {
    /// Absolute or relative path within the device namespace.
    pub path: String,
    /// File or directory base name.
    pub name: String,
    /// Logical uncompressed size in bytes.
    pub size: u64,
    /// Whether this entry represents a directory container.
    pub is_dir: bool,
    /// Last modification timestamp in POSIX epoch seconds.
    pub mtime_epoch_secs: u64,
    /// POSIX file mode / permission bits if available.
    pub posix_mode: u32,
}

impl DeviceVfsEntry {
    /// Creates a new directory entry descriptor.
    #[inline]
    pub fn directory(path: impl Into<String>, name: impl Into<String>, mtime: u64) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            size: 0,
            is_dir: true,
            mtime_epoch_secs: mtime,
            posix_mode: 0o755,
        }
    }

    /// Creates a new regular file entry descriptor.
    #[inline]
    pub fn file(path: impl Into<String>, name: impl Into<String>, size: u64, mtime: u64) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            size,
            is_dir: false,
            mtime_epoch_secs: mtime,
            posix_mode: 0o644,
        }
    }

    /// Converts this peripheral entry into a microkernel-compatible [`UnifiedEntryNode`].
    #[inline]
    pub fn to_unified_node(&self) -> UnifiedEntryNode {
        let mut node = UnifiedEntryNode::empty();
        node.uncompressed_size = self.size;
        node.compressed_size = self.size;
        node.mtime_epoch_secs = self.mtime_epoch_secs as u32;
        if self.is_dir {
            node.flags |= NODE_FLAG_DIRECTORY;
        }
        node
    }
}

impl From<DeviceVfsEntry> for DeviceEntry {
    #[inline]
    fn from(entry: DeviceVfsEntry) -> Self {
        Self {
            path: entry.path,
            name: entry.name,
            size: entry.size,
            is_dir: entry.is_dir,
            mtime: entry.mtime_epoch_secs,
            object_id: None,
            posix_mode: Some(entry.posix_mode),
        }
    }
}

impl From<DeviceEntry> for DeviceVfsEntry {
    #[inline]
    fn from(entry: DeviceEntry) -> Self {
        Self {
            path: entry.path,
            name: entry.name,
            size: entry.size,
            is_dir: entry.is_dir,
            mtime_epoch_secs: entry.mtime,
            posix_mode: entry.posix_mode.unwrap_or(if entry.is_dir { 0o755 } else { 0o644 }),
        }
    }
}

/// Standardized VFS provider interface for bridging peripheral devices (ADB, MTP)
/// and external I/O sources into the TTZip archive microkernel.
pub trait ArchiveVfsProvider {
    /// Returns the URI scheme identifying this provider (e.g., `"adb"`, `"mtp"`).
    fn scheme(&self) -> &'static str;

    /// Queries metadata for a specific remote path.
    fn stat(&mut self, path: &str) -> Result<DeviceVfsEntry, AppCoreError>;

    /// Lists direct children of a directory at the remote path.
    fn list_directory(&mut self, path: &str) -> Result<Vec<DeviceVfsEntry>, AppCoreError>;

    /// Lists direct children of a directory at the remote path as canonical [`DeviceEntry`] records.
    fn list_device_entries(&mut self, path: &str) -> Result<Vec<DeviceEntry>, AppCoreError> {
        let entries = self.list_directory(path)?;
        Ok(entries.into_iter().map(DeviceEntry::from).collect())
    }

    /// Reads binary file content from the remote path.
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, AppCoreError>;

    /// Writes binary file content to the remote path.
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), AppCoreError>;

    /// Populates an in-memory [`UnifiedVfsTable`] from this provider starting at `root_path`.
    fn populate_vfs_table(
        &mut self,
        root_path: &str,
        table: &mut UnifiedVfsTable,
    ) -> Result<(), AppCoreError> {
        let entries = self.list_directory(root_path)?;
        for entry in entries {
            let node = entry.to_unified_node();
            table.add_entry(node, &entry.path);
        }
        table.build_lcrs_indices();
        Ok(())
    }

    /// Converts remote device hierarchy starting at `root_path` directly into an in-memory
    /// [`UnifiedVfsTable`], seamlessly bridging peripheral hardware trees into the compression microkernel.
    fn to_unified_vfs_table(&mut self, root_path: &str) -> Result<UnifiedVfsTable, AppCoreError> {
        let mut table = UnifiedVfsTable::new();
        self.populate_vfs_table(root_path, &mut table)?;
        Ok(table)
    }
}

/// Unified peripheral device VFS adapter providing polymorphic dispatch
/// across ADB and MTP device streams.
pub enum DeviceVfsAdapter<S = std::net::TcpStream, T = mtp::RusbTransport>
where
    S: std::io::Read + std::io::Write + Send,
    T: mtp::UsbBulkTransport + Send,
{
    Adb(adb::AdbVfsAdapter<S>),
    Mtp(mtp::MtpVfsAdapter<T>),
}

impl<S, T> ArchiveVfsProvider for DeviceVfsAdapter<S, T>
where
    S: std::io::Read + std::io::Write + Send,
    T: mtp::UsbBulkTransport + Send,
{
    #[inline]
    fn scheme(&self) -> &'static str {
        match self {
            Self::Adb(a) => a.scheme(),
            Self::Mtp(m) => m.scheme(),
        }
    }

    #[inline]
    fn stat(&mut self, path: &str) -> Result<DeviceVfsEntry, AppCoreError> {
        match self {
            Self::Adb(a) => a.stat(path),
            Self::Mtp(m) => m.stat(path),
        }
    }

    #[inline]
    fn list_directory(&mut self, path: &str) -> Result<Vec<DeviceVfsEntry>, AppCoreError> {
        match self {
            Self::Adb(a) => a.list_directory(path),
            Self::Mtp(m) => m.list_directory(path),
        }
    }

    #[inline]
    fn list_device_entries(&mut self, path: &str) -> Result<Vec<DeviceEntry>, AppCoreError> {
        match self {
            Self::Adb(a) => a.list_device_entries(path),
            Self::Mtp(m) => m.list_device_entries(path),
        }
    }

    #[inline]
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, AppCoreError> {
        match self {
            Self::Adb(a) => a.read_file(path),
            Self::Mtp(m) => m.read_file(path),
        }
    }

    #[inline]
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), AppCoreError> {
        match self {
            Self::Adb(a) => a.write_file(path, data),
            Self::Mtp(m) => m.write_file(path, data),
        }
    }

    #[inline]
    fn populate_vfs_table(
        &mut self,
        root_path: &str,
        table: &mut UnifiedVfsTable,
    ) -> Result<(), AppCoreError> {
        match self {
            Self::Adb(a) => a.populate_vfs_table(root_path, table),
            Self::Mtp(m) => m.populate_vfs_table(root_path, table),
        }
    }
}

impl<S: std::io::Read + std::io::Write + Send> From<adb::AdbVfsAdapter<S>>
    for DeviceVfsAdapter<S, mtp::RusbTransport>
{
    #[inline]
    fn from(adapter: adb::AdbVfsAdapter<S>) -> Self {
        Self::Adb(adapter)
    }
}

impl<T: mtp::UsbBulkTransport + Send> From<mtp::MtpVfsAdapter<T>>
    for DeviceVfsAdapter<std::net::TcpStream, T>
{
    #[inline]
    fn from(adapter: mtp::MtpVfsAdapter<T>) -> Self {
        Self::Mtp(adapter)
    }
}

// Compile-time static assertions ensuring thread-safety and concurrency invariants
const _: () = {
    const fn assert_send<T: Send + 'static>() {}
    assert_send::<DeviceVfsEntry>();
    assert_send::<DeviceVfsAdapter<std::net::TcpStream, mtp::RusbTransport>>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mtp::codes::{op, resp};
    use crate::device::mtp::container::encode_ptp_string;
    use crate::device::mtp::tests::MockUsbTransport;
    use crate::device::mtp::{is_mtp_transfer_cancelled, MTP_TRANSFER_CANCELLED_REASON};
    use sha2::{Digest, Sha256};

    fn build_object_info_payload(filename: &str, size: u32, is_dir: bool) -> Vec<u8> {
        let mut payload = vec![0u8; 52];
        let fmt: u16 = if is_dir { 0x3001 } else { 0x3000 };
        payload[4..6].copy_from_slice(&fmt.to_le_bytes());
        payload[8..12].copy_from_slice(&size.to_le_bytes());
        payload.extend_from_slice(&encode_ptp_string(filename));
        payload.extend_from_slice(&encode_ptp_string(""));
        payload.extend_from_slice(&encode_ptp_string(""));
        payload.extend_from_slice(&encode_ptp_string(""));
        payload
    }

    #[test]
    fn test_device_vfs_entry_constructors_and_unified_node_conversion() {
        let dir = DeviceVfsEntry::directory("/sdcard/Download", "Download", 1700000000);
        assert!(dir.is_dir);
        assert_eq!(dir.size, 0);
        assert_eq!(dir.posix_mode, 0o755);

        let dir_node = dir.to_unified_node();
        assert!(dir_node.is_dir());
        assert_eq!(dir_node.mtime_epoch_secs, 1700000000);

        let file = DeviceVfsEntry::file("/sdcard/Download/test.zip", "test.zip", 1048576, 1700001000);
        assert!(!file.is_dir);
        assert_eq!(file.size, 1048576);
        assert_eq!(file.posix_mode, 0o644);

        let file_node = file.to_unified_node();
        assert!(file_node.is_file());
        assert_eq!(file_node.uncompressed_size, 1048576);
        assert_eq!(file_node.compressed_size, 1048576);
        assert_eq!(file_node.mtime_epoch_secs, 1700001000);
    }

    #[test]
    fn test_populate_unified_vfs_table() {
        struct MockVfsProvider;
        impl ArchiveVfsProvider for MockVfsProvider {
            fn scheme(&self) -> &'static str {
                "mock"
            }
            fn stat(&mut self, _path: &str) -> Result<DeviceVfsEntry, AppCoreError> {
                Ok(DeviceVfsEntry::directory("/root", "root", 100))
            }
            fn list_directory(&mut self, _path: &str) -> Result<Vec<DeviceVfsEntry>, AppCoreError> {
                Ok(vec![
                    DeviceVfsEntry::directory("/root/docs", "docs", 100),
                    DeviceVfsEntry::file("/root/file.txt", "file.txt", 512, 100),
                ])
            }
            fn read_file(&mut self, _path: &str) -> Result<Vec<u8>, AppCoreError> {
                Ok(vec![1, 2, 3])
            }
            fn write_file(&mut self, _path: &str, _data: &[u8]) -> Result<(), AppCoreError> {
                Ok(())
            }
        }

        let mut provider = MockVfsProvider;
        let table = provider.to_unified_vfs_table("/root").expect("to_unified_vfs_table must succeed");
        assert_eq!(table.len(), 2);
        assert_eq!(table.get_node_path(0), Some("/root/docs"));
        assert_eq!(table.get_node_path(1), Some("/root/file.txt"));
        assert!(table.get_node(0).unwrap().is_dir());
        assert!(table.get_node(1).unwrap().is_file());

        let device_entries = provider.list_device_entries("/root").expect("list_device_entries must succeed");
        assert_eq!(device_entries.len(), 2);
        assert_eq!(device_entries[0].name, "docs");
        assert!(device_entries[0].is_dir);
        assert_eq!(device_entries[1].name, "file.txt");
        assert!(!device_entries[1].is_dir);
        assert_eq!(device_entries[1].size, 512);
    }

    #[test]
    fn test_device_vfs_entry_bidirectional_conversion() {
        let vfs_entry = DeviceVfsEntry {
            path: "/storage/emulated/0/DCIM/photo.jpg".into(),
            name: "photo.jpg".into(),
            size: 2048,
            is_dir: false,
            mtime_epoch_secs: 1710000000,
            posix_mode: 0o644,
        };

        let device_entry: DeviceEntry = vfs_entry.clone().into();
        assert_eq!(device_entry.path, "/storage/emulated/0/DCIM/photo.jpg");
        assert_eq!(device_entry.name, "photo.jpg");
        assert_eq!(device_entry.size, 2048);
        assert!(!device_entry.is_dir);
        assert_eq!(device_entry.mtime, 1710000000);
        assert_eq!(device_entry.object_id, None);
        assert_eq!(device_entry.posix_mode, Some(0o644));

        let converted_back: DeviceVfsEntry = device_entry.into();
        assert_eq!(converted_back, vfs_entry);
    }

    #[test]
    fn test_push_object_overwrite_preserves_old_handle_until_new_handle_verified() {
        let mut mock = MockUsbTransport::new();
        let mut handles_payload = Vec::new();
        handles_payload.extend_from_slice(&1u32.to_le_bytes());
        handles_payload.extend_from_slice(&0x10u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 1, &handles_payload);
        mock.push_response(resp::OK, 1, &[]);

        mock.push_data(
            op::GET_OBJECT_INFO,
            2,
            &build_object_info_payload("report.txt", 5, false),
        );
        mock.push_response(resp::OK, 2, &[]);

        mock.push_response(resp::OK, 3, &[0x0001_0001, MTP_OBJECT_HANDLE_ROOT, 0x20]);
        mock.push_response(resp::OK, 4, &[]);
        mock.push_data(
            op::GET_OBJECT_INFO,
            5,
            &build_object_info_payload("report.txt", 12, false),
        );
        mock.push_response(resp::OK, 5, &[]);
        // Post-upload GET_OBJECT_HANDLES (tid=6) -> [0x10, 0x20]
        let mut post_handles = Vec::new();
        post_handles.extend_from_slice(&2u32.to_le_bytes());
        post_handles.extend_from_slice(&0x10u32.to_le_bytes());
        post_handles.extend_from_slice(&0x20u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 6, &post_handles);
        mock.push_response(resp::OK, 6, &[]);
        mock.push_response(resp::OK, 7, &[]);

        let mut session = MtpSession::new(mock);
        let outcome = push_object_with_session(
            &mut session,
            0x0001_0001,
            "/",
            "report.txt",
            b"updated-data",
            MtpConflictPolicy::Overwrite,
            |_, _| Ok(()),
        )
        .expect("overwrite upload must succeed");

        assert_eq!(outcome.created_handle, 0x20);
        assert_eq!(outcome.final_filename, "report.txt");
        assert_eq!(outcome.bytes_transferred, 12);
        assert!(outcome.source_stream_sha256_verified);
        assert!(outcome.remote_size_verified);
        assert!(!outcome.remote_readback_sha256_verified);
        assert!(!outcome.skipped);

        let transport = session.into_transport();
        let last_cmd = crate::device::mtp::MtpContainer::parse(
            transport.out_packets().last().expect("must have commands"),
        )
        .expect("parse last command");
        assert_eq!(last_cmd.code, op::DELETE_OBJECT);
        assert_eq!(last_cmd.extract_params(), vec![0x10, 0]);
    }

    #[test]
    fn test_push_reader_same_length_source_mutation_deletes_new_handle_and_preserves_old_handle() {
        let mut mock = MockUsbTransport::new();
        let mut handles_payload = Vec::new();
        handles_payload.extend_from_slice(&1u32.to_le_bytes());
        handles_payload.extend_from_slice(&0x10u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 1, &handles_payload);
        mock.push_response(resp::OK, 1, &[]);

        mock.push_data(
            op::GET_OBJECT_INFO,
            2,
            &build_object_info_payload("report.txt", 12, false),
        );
        mock.push_response(resp::OK, 2, &[]);

        mock.push_response(resp::OK, 3, &[0x0001_0001, MTP_OBJECT_HANDLE_ROOT, 0x20]);
        mock.push_response(resp::OK, 4, &[]);
        mock.push_response(resp::OK, 5, &[]);

        let mut session = MtpSession::new(mock);
        let mut expected_hasher = Sha256::new();
        expected_hasher.update(b"initial-data");
        let expected_sha256 = format!("{:x}", expected_hasher.finalize());

        let mut mutated_reader = std::io::Cursor::new(b"mutated-data");
        let err = push_reader_with_session(
            &mut session,
            0x0001_0001,
            "/",
            "report.txt",
            12,
            &mut mutated_reader,
            Some(&expected_sha256),
            MtpConflictPolicy::Overwrite,
            DEFAULT_MAX_KEEP_BOTH_CANDIDATES,
            |_, _| Ok(()),
        )
        .expect_err("same-length source mutation must fail pre-commit stream hash verification");
        assert!(err.to_string().contains("MTP source stream SHA-256 mismatch before commit"));

        let transport = session.into_transport();
        let delete_cmds: Vec<_> = transport
            .out_packets()
            .iter()
            .filter_map(|pkt| crate::device::mtp::MtpContainer::parse(pkt).ok())
            .filter(|c| c.code == op::DELETE_OBJECT)
            .collect();
        assert_eq!(delete_cmds.len(), 1);
        assert_eq!(delete_cmds[0].extract_params(), vec![0x20, 0]);
    }

    #[test]
    fn test_push_reader_conflict_probe_failure_stops_before_writing() {
        let mut mock = MockUsbTransport::new();
        let mut handles_payload = Vec::new();
        handles_payload.extend_from_slice(&1u32.to_le_bytes());
        handles_payload.extend_from_slice(&0x10u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 1, &handles_payload);
        mock.push_response(resp::OK, 1, &[]);
        mock.push_response(resp::GENERAL_ERROR, 2, &[]);

        let mut session = MtpSession::new(mock);
        let mut reader = std::io::Cursor::new(b"payload");
        let err = push_reader_with_session(
            &mut session,
            0x0001_0001,
            "/",
            "report.txt",
            7,
            &mut reader,
            None,
            MtpConflictPolicy::Overwrite,
            DEFAULT_MAX_KEEP_BOTH_CANDIDATES,
            |_, _| Ok(()),
        )
        .expect_err("conflict probe failure must stop before SEND_OBJECT_INFO");
        assert!(err.to_string().contains("0x2002"));

        let transport = session.into_transport();
        let send_cmds: Vec<_> = transport
            .out_packets()
            .iter()
            .filter_map(|pkt| crate::device::mtp::MtpContainer::parse(pkt).ok())
            .filter(|c| c.code == op::SEND_OBJECT_INFO || c.code == op::SEND_OBJECT)
            .collect();
        assert!(send_cmds.is_empty(), "No SEND_OBJECT_INFO may be sent when conflict probing fails");
    }

    #[test]
    fn test_allocate_non_colliding_filename_exhaustion_returns_explicit_error() {
        let existing = vec![
            (1, "notes.txt".to_string()),
            (2, "notes (1).txt".to_string()),
            (3, "notes (2).txt".to_string()),
        ];
        let err = allocate_non_colliding_filename("notes.txt", &existing, 2)
            .expect_err("exhausting candidate limit must return explicit error");
        assert!(err.to_string().contains("MTP KeepBoth candidate names exhausted"));
    }

    #[test]
    fn test_push_object_cancelled_mid_stream_deletes_new_handle_and_preserves_old_handle() {
        let mut mock = MockUsbTransport::new();
        let mut handles_payload = Vec::new();
        handles_payload.extend_from_slice(&1u32.to_le_bytes());
        handles_payload.extend_from_slice(&0x10u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 1, &handles_payload);
        mock.push_response(resp::OK, 1, &[]);

        mock.push_data(
            op::GET_OBJECT_INFO,
            2,
            &build_object_info_payload("report.txt", 5, false),
        );
        mock.push_response(resp::OK, 2, &[]);

        mock.push_response(resp::OK, 3, &[0x0001_0001, MTP_OBJECT_HANDLE_ROOT, 0x20]);
        mock.push_response(resp::OK, 5, &[]);

        let mut session = MtpSession::new(mock);
        let payload = vec![0xABu8; 100_000];
        let err = push_object_with_session(
            &mut session,
            0x0001_0001,
            "/",
            "report.txt",
            &payload,
            MtpConflictPolicy::Overwrite,
            |sent, _| {
                if sent > 0 {
                    return Err(AppCoreError::Transfer {
                        reason: MTP_TRANSFER_CANCELLED_REASON.to_string(),
                    });
                }
                Ok(())
            },
        )
        .expect_err("cancelled upload must fail");
        assert!(is_mtp_transfer_cancelled(&err));

        let transport = session.into_transport();
        let delete_cmds: Vec<_> = transport
            .out_packets()
            .iter()
            .filter_map(|pkt| crate::device::mtp::MtpContainer::parse(pkt).ok())
            .filter(|c| c.code == op::DELETE_OBJECT)
            .collect();
        assert_eq!(delete_cmds.len(), 1);
        assert_eq!(delete_cmds[0].extract_params(), vec![0x20, 0]);
    }

    #[test]
    fn test_push_object_keep_both_and_skip_policies() {
        let mut mock = MockUsbTransport::new();
        let mut handles_payload = Vec::new();
        handles_payload.extend_from_slice(&1u32.to_le_bytes());
        handles_payload.extend_from_slice(&0x10u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 1, &handles_payload);
        mock.push_response(resp::OK, 1, &[]);
        mock.push_data(
            op::GET_OBJECT_INFO,
            2,
            &build_object_info_payload("notes.txt", 10, false),
        );
        mock.push_response(resp::OK, 2, &[]);

        mock.push_data(op::GET_OBJECT_HANDLES, 3, &handles_payload);
        mock.push_response(resp::OK, 3, &[]);
        mock.push_data(
            op::GET_OBJECT_INFO,
            4,
            &build_object_info_payload("notes.txt", 10, false),
        );
        mock.push_response(resp::OK, 4, &[]);
        mock.push_response(resp::OK, 5, &[0x0001_0001, MTP_OBJECT_HANDLE_ROOT, 0x33]);
        mock.push_response(resp::OK, 6, &[]);
        mock.push_data(
            op::GET_OBJECT_INFO,
            7,
            &build_object_info_payload("notes (1).txt", 4, false),
        );
        mock.push_response(resp::OK, 7, &[]);
        // Post-upload GET_OBJECT_HANDLES (tid=8) -> [0x10, 0x33]
        let mut post_handles = Vec::new();
        post_handles.extend_from_slice(&2u32.to_le_bytes());
        post_handles.extend_from_slice(&0x10u32.to_le_bytes());
        post_handles.extend_from_slice(&0x33u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 8, &post_handles);
        mock.push_response(resp::OK, 8, &[]);

        let mut session = MtpSession::new(mock);
        let skip_res = push_object_with_session(
            &mut session,
            0x0001_0001,
            "/",
            "notes.txt",
            b"data",
            MtpConflictPolicy::Skip,
            |_, _| Ok(()),
        )
        .expect("skip policy must succeed");
        assert!(skip_res.skipped);
        assert_eq!(skip_res.created_handle, 0x10);

        let keep_res = push_object_with_session(
            &mut session,
            0x0001_0001,
            "/",
            "notes.txt",
            b"data",
            MtpConflictPolicy::KeepBoth,
            |_, _| Ok(()),
        )
        .expect("keepBoth policy must succeed");
        assert!(!keep_res.skipped);
        assert_eq!(keep_res.final_filename, "notes (1).txt");
        assert_eq!(keep_res.created_handle, 0x33);
    }

    #[test]
    fn test_pull_object_rejects_large_file_overflow_and_push_reader_uses_bounded_64kib_reads() {
        struct BoundedReadTracker {
            remaining: usize,
            max_requested_buf_len: usize,
        }
        impl std::io::Read for BoundedReadTracker {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.max_requested_buf_len = self.max_requested_buf_len.max(buf.len());
                assert!(
                    buf.len() <= 65_536,
                    "MTP streaming upload must never request > 64 KiB read buffer, got {}",
                    buf.len()
                );
                let n = self.remaining.min(buf.len());
                buf[..n].fill(0x5A);
                self.remaining -= n;
                Ok(n)
            }
        }

        let total_size = 512 * 1024; // 512 KiB streamed without whole-file buffering
        let mut mock = MockUsbTransport::new();
        let handles_payload = 0u32.to_le_bytes();
        mock.push_data(op::GET_OBJECT_HANDLES, 1, &handles_payload);
        mock.push_response(resp::OK, 1, &[]);
        mock.push_response(resp::OK, 2, &[0x0001_0001, MTP_OBJECT_HANDLE_ROOT, 0x44]);
        mock.push_response(resp::OK, 3, &[]);
        mock.push_data(
            op::GET_OBJECT_INFO,
            4,
            &build_object_info_payload("streamed.bin", total_size as u32, false),
        );
        mock.push_response(resp::OK, 4, &[]);
        // Post-upload GET_OBJECT_HANDLES (tid=5) -> [0x44]
        let mut post_handles = Vec::new();
        post_handles.extend_from_slice(&1u32.to_le_bytes());
        post_handles.extend_from_slice(&0x44u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 5, &post_handles);
        mock.push_response(resp::OK, 5, &[]);

        let mut session = MtpSession::new(mock);
        let mut tracker = BoundedReadTracker {
            remaining: total_size,
            max_requested_buf_len: 0,
        };
        let outcome = push_reader_with_session(
            &mut session,
            0x0001_0001,
            "/",
            "streamed.bin",
            total_size as u64,
            &mut tracker,
            None,
            MtpConflictPolicy::Overwrite,
            DEFAULT_MAX_KEEP_BOTH_CANDIDATES,
            |_, _| Ok(()),
        )
        .expect("bounded 64 KiB streaming upload must succeed");
        assert_eq!(outcome.bytes_transferred, total_size as u64);
        assert_eq!(tracker.max_requested_buf_len, 65_536);

        // Verify pull_object_streaming_with_session rejects compressed_size == u32::MAX overflow
        let mut mock_pull = MockUsbTransport::new();
        mock_pull.push_data(
            op::GET_OBJECT_INFO,
            1,
            &build_object_info_payload("huge.bin", u32::MAX, false),
        );
        mock_pull.push_response(resp::OK, 1, &[]);
        mock_pull.push_data(
            op::GET_OBJECT_INFO,
            2,
            &build_object_info_payload("huge.bin", u32::MAX, false),
        );
        mock_pull.push_response(resp::OK, 2, &[]);
        mock_pull.push_data(
            op::GET_OBJECT_PROP_VALUE,
            3,
            &(5u64 * 1024 * 1024 * 1024).to_le_bytes(),
        );
        mock_pull.push_response(resp::OK, 3, &[]);

        let mut pull_session = MtpSession::new(mock_pull);
        let err = pull_object_streaming_with_session(
            &mut pull_session,
            0x0001_0001,
            0x99,
            "/huge.bin",
            |_, _, _| Ok(()),
        )
        .expect_err("pulling >= u32::MAX object over 32-bit PTP GetObject must be rejected");
        assert!(err.to_string().contains("exceeds 32-bit PTP GetObject container limit"));
    }
}

