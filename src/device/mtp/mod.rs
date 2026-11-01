// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![forbid(unsafe_code)]

//! USB MTP 1.1 protocol implementation, hardware drivers, and session state machines.
//!
//! # Quickstart
//!
//! ```no_run
//! use ttzip_app_core::device::mtp::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = MtpClient::new();
//!     let devices: Vec<MtpDeviceInfo> = client.scan_devices()?;
//!     for dev in &devices {
//!         println!("Found MTP device: {} {} ({})", dev.manufacturer, dev.model, dev.serial_number);
//!     }
//!     Ok(())
//! }
//! ```

use crate::device::{ArchiveVfsProvider, DeviceVfsEntry};
use crate::error::AppCoreError;

mod bulk_directory;
use bulk_directory::execute_bulk_prop_list;

pub mod codes;
pub mod container;
pub mod driver;
pub mod directory;
pub mod error;
pub mod session;
pub mod transfer;
pub mod types;

pub use codes::{event, format, op, prop, resp, response_code_to_str, ContainerType};
pub use container::{
    decode_ptp_device_info, decode_ptp_string, decode_u16_array, decode_u32_array,
    encode_ptp_string, MtpContainer, PTP_CONTAINER_HEADER_SIZE,
};
pub use driver::{
    inspect_device_endpoints, scan_physical_mtp_devices, select_unique_mtp_device, MtpEndpoints,
    RusbTransport, UsbBulkTransport, DEFAULT_USB_TIMEOUT,
};
pub use error::{is_io_disconnected, ErrorChain, MtpError, MtpResult};
pub use session::{MtpSession, MtpSessionState};
pub use transfer::{
    allocate_non_colliding_filename, delete_mtp_object, is_mtp_transfer_cancelled,
    mtp_delete_remote_object, mtp_pull_file_to_local_staged, mtp_push_local_file_verified,
    open_hardware_mtp_session, pull_mtp_file_streaming, pull_object_streaming_with_session,
    push_local_file_path_to_mtp, push_local_file_to_mtp, push_object_with_session,
    push_reader_with_session, resolve_directory_handle_by_path, resolve_file_handle,
    resolve_storage_id, MtpConflictPolicy, MtpDownloadOutcome, MtpTransferProgressCallback,
    MtpUploadOutcome, DEFAULT_MAX_KEEP_BOTH_CANDIDATES, MTP_TRANSFER_CANCELLED_REASON,
};
pub use types::{
    MtpDeviceInfo, MtpObjectHandle, MtpObjectInfo, MtpStorageInfo, MTP_OBJECT_HANDLE_ALL,
    MTP_OBJECT_HANDLE_ROOT,
};

/// Lightweight VFS adapter bridging an active USB MTP session into the unified archive VFS model.
pub struct MtpVfsAdapter<T: UsbBulkTransport = RusbTransport> {
    session: Option<MtpSession<T>>,
    active_storage_id: Option<u32>,
    supports_bulk_opcode: bool,
}

impl<T: UsbBulkTransport> std::fmt::Debug for MtpVfsAdapter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MtpVfsAdapter")
            .field("active_storage_id", &self.active_storage_id)
            .field("supports_bulk_opcode", &self.supports_bulk_opcode)
            .finish_non_exhaustive()
    }
}

impl<T: UsbBulkTransport> MtpVfsAdapter<T> {
    /// Creates a new MTP VFS adapter wrapping an active MTP session.
    #[inline]
    pub fn new(session: MtpSession<T>) -> Self {
        Self {
            session: Some(session),
            active_storage_id: None,
            supports_bulk_opcode: false,
        }
    }

    /// Creates a new MTP VFS adapter configured with a specific storage partition ID.
    #[inline]
    pub fn with_storage(session: MtpSession<T>, storage_id: u32) -> Self {
        Self {
            session: Some(session),
            active_storage_id: Some(storage_id),
            supports_bulk_opcode: false,
        }
    }

    /// Configures whether the device reports support for MTP opcode 0x9805 (`GetObjectPropList`).
    #[inline]
    pub fn with_bulk_support(mut self, supported: bool) -> Self {
        self.supports_bulk_opcode = supported;
        self
    }

    /// Configures opcode 0x9805 support directly from an `MtpDeviceInfo` record.
    #[inline]
    pub fn with_device_info(mut self, info: &MtpDeviceInfo) -> Self {
        self.supports_bulk_opcode =
            info.supports_operation(crate::device::mtp_bulk::MtpBulkFetcher::OPCODE);
        self
    }

    /// Explicitly sets opcode 0x9805 support on this adapter instance.
    #[inline]
    pub fn set_supports_bulk_opcode(&mut self, supported: bool) {
        self.supports_bulk_opcode = supported;
    }

    /// Returns whether the adapter is configured to attempt opcode 0x9805 bulk fetches.
    #[inline]
    pub fn supports_bulk_opcode(&self) -> bool {
        self.supports_bulk_opcode
    }

    /// Dynamically queries connected device capabilities and updates opcode 0x9805 support.
    pub fn detect_bulk_support(&mut self) -> Result<bool, AppCoreError> {
        let info = self.session_mut().get_device_info()?;
        let supported = info.supports_operation(crate::device::mtp_bulk::MtpBulkFetcher::OPCODE);
        self.supports_bulk_opcode = supported;
        Ok(supported)
    }

    /// Accesses the underlying MTP session.
    #[inline]
    pub fn session(&self) -> &MtpSession<T> {
        self.session.as_ref().expect("active session")
    }

    /// Mutably accesses the underlying MTP session.
    #[inline]
    pub fn session_mut(&mut self) -> &mut MtpSession<T> {
        self.session.as_mut().expect("active session")
    }

    /// Consumes the adapter and returns the underlying MTP session.
    #[inline]
    pub fn into_inner(mut self) -> MtpSession<T> {
        self.session.take().expect("active session")
    }

    /// Ensures the active storage ID is initialized by querying available storages if unset.
    fn ensure_storage_id(&mut self) -> Result<u32, AppCoreError> {
        if let Some(sid) = self.active_storage_id {
            return Ok(sid);
        }
        let storages = self.session_mut().get_storage_ids()?;
        let first = storages.first().copied().ok_or_else(|| AppCoreError::Mtp {
            code: 0,
            message: "No storage partitions found on MTP device".to_string(),
        })?;
        self.active_storage_id = Some(first);
        Ok(first)
    }

    /// Helper to resolve a path string to a parent object handle.
    fn resolve_parent_handle(&self, path: &str) -> u32 {
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            MTP_OBJECT_HANDLE_ROOT
        } else if let Ok(h) = trimmed.parse::<u32>() {
            h
        } else {
            MTP_OBJECT_HANDLE_ROOT
        }
    }

    /// Executes single-RTT bulk directory enumeration via MTP opcode 0x9805 (`GetObjectPropList`).
    fn fetch_bulk_directory(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
        clean_path: &str,
    ) -> Result<Vec<DeviceVfsEntry>, AppCoreError> {
        let session = self
            .session
            .take()
            .ok_or_else(|| AppCoreError::InvalidState {
                reason: "Session is not active".to_string(),
            })?;
        let mut transport = session.into_transport();

        let payload_res = execute_bulk_prop_list(&mut transport, storage_id, parent_handle);
        self.session = Some(MtpSession::new(transport));

        let payload = payload_res?;
        let entries = crate::device::mtp_bulk::MtpBulkFetcher::parse_object_prop_list(&payload)?;
        Ok(map_bulk_entries(entries, clean_path))
    }
}

fn map_bulk_entries(
    entries: Vec<crate::device::entry::DeviceEntry>,
    clean_path: &str,
) -> Vec<DeviceVfsEntry> {
    entries
        .into_iter()
        .map(|entry| {
            let child_path = if clean_path.is_empty() || clean_path == "/" {
                format!("/{}", entry.name)
            } else {
                format!("{}/{}", clean_path, entry.name)
            };
            DeviceVfsEntry {
                path: child_path,
                name: entry.name,
                size: entry.size,
                is_dir: entry.is_dir,
                mtime_epoch_secs: entry.mtime,
                posix_mode: if entry.is_dir { 0o755 } else { 0o644 },
            }
        })
        .collect()
}

impl<T: UsbBulkTransport + Send> ArchiveVfsProvider for MtpVfsAdapter<T> {
    #[inline]
    fn scheme(&self) -> &'static str {
        "mtp"
    }

    fn stat(&mut self, path: &str) -> Result<DeviceVfsEntry, AppCoreError> {
        let handle = self.resolve_parent_handle(path);
        if handle == MTP_OBJECT_HANDLE_ROOT {
            return Ok(DeviceVfsEntry::directory("/", "", 0));
        }
        let info = self.session_mut().get_object_info(handle)?;
        let is_dir = info.is_directory();
        let file_size = info.file_size();
        let filename = info.filename;
        Ok(DeviceVfsEntry {
            path: path.to_string(),
            name: filename,
            size: file_size,
            is_dir,
            mtime_epoch_secs: 0,
            posix_mode: if is_dir { 0o755 } else { 0o644 },
        })
    }

    fn list_directory(&mut self, path: &str) -> Result<Vec<DeviceVfsEntry>, AppCoreError> {
        let storage_id = self.ensure_storage_id()?;
        let parent_handle = self.resolve_parent_handle(path);
        let clean_path = path.trim_end_matches('/');

        // Single-RTT bulk fetching when device reports support for opcode 0x9805 (GetObjectPropList)
        if self.supports_bulk_opcode {
            if let Ok(entries) = self.fetch_bulk_directory(storage_id, parent_handle, clean_path) {
                return Ok(entries);
            }
            // Fall back gracefully to sequential get_object_info if bulk fetching fails
        }

        let handles = self
            .session_mut()
            .get_object_handles(storage_id, 0, parent_handle)?;
        let mut results = Vec::with_capacity(handles.len());
        for h in handles {
            if let Ok(info) = self.session_mut().get_object_info(h) {
                let is_dir = info.is_directory();
                let file_size = info.file_size();
                let filename = info.filename;
                let child_path = if clean_path.is_empty() || clean_path == "/" {
                    format!("/{}", filename)
                } else {
                    format!("{}/{}", clean_path, filename)
                };
                results.push(DeviceVfsEntry {
                    path: child_path,
                    name: filename,
                    size: file_size,
                    is_dir,
                    mtime_epoch_secs: 0,
                    posix_mode: if is_dir { 0o755 } else { 0o644 },
                });
            }
        }
        Ok(results)
    }

    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, AppCoreError> {
        let handle = self.resolve_parent_handle(path);
        if handle == MTP_OBJECT_HANDLE_ROOT {
            return Err(AppCoreError::Io {
                reason: "Cannot read root directory as file payload".to_string(),
            });
        }
        self.session_mut().get_object(handle)
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), AppCoreError> {
        let storage_id = self.ensure_storage_id()?;
        let filename = path.rsplit('/').next().unwrap_or(path);
        self.session_mut()
            .send_object(storage_id, MTP_OBJECT_HANDLE_ROOT, filename, data)?;
        Ok(())
    }
}

/// Standard prelude exporting commonly used MTP protocol types, traits, and facades.
pub mod prelude {
    pub use super::codes::{event, format, op, prop, resp, ContainerType};
    pub use super::driver::{UsbBulkTransport, DEFAULT_USB_TIMEOUT};
    pub use super::session::{MtpSession, MtpSessionState};
    pub use super::types::{MtpDeviceInfo, MtpObjectHandle, MtpObjectInfo, MtpStorageInfo};
    pub use super::{MtpClient, MtpError, MtpResult, MtpVfsAdapter};
}

/// High-level client facade for MTP device discovery and transport instantiation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MtpClient {
    _private: (),
}

impl MtpClient {
    /// Instantiates a new MTP client facade.
    #[inline]
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Scans physical USB buses for connected MTP devices.
    #[inline]
    pub fn scan_devices(&self) -> Result<Vec<MtpDeviceInfo>, crate::error::AppCoreError> {
        scan_physical_mtp_devices()
    }
}

// Compile-time static assertions ensuring thread-safety, standard error traits, and conversions
const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    const fn assert_send<T: Send + 'static>() {}
    const fn assert_unpin<T: Unpin>() {}
    const fn assert_clone<T: Clone>() {}
    const fn assert_default<T: Default>() {}
    const fn assert_eq_bounds<T: Eq>() {}

    // Concurrency boundaries
    assert_send_sync::<MtpDeviceInfo>();
    assert_send_sync::<MtpStorageInfo>();
    assert_send_sync::<MtpObjectInfo>();
    assert_send_sync::<MtpClient>();

    // Value semantics and algebraic properties
    assert_default::<MtpClient>();
    assert_default::<MtpDeviceInfo>();

    assert_clone::<MtpClient>();
    assert_clone::<MtpDeviceInfo>();
    assert_clone::<MtpStorageInfo>();
    assert_clone::<MtpObjectInfo>();

    assert_eq_bounds::<MtpClient>();
    assert_eq_bounds::<MtpDeviceInfo>();
    assert_eq_bounds::<MtpStorageInfo>();
    assert_eq_bounds::<MtpObjectInfo>();

    // Stream and session concurrency
    assert_send::<MtpSession<RusbTransport>>();
    assert_send::<MtpVfsAdapter<RusbTransport>>();

    assert_unpin::<MtpClient>();
};

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::device::mtp::codes::{op, resp, ContainerType};
    use std::collections::VecDeque;

    /// Mock USB transport for deterministic protocol facade integration unit tests.
    pub(crate) struct MockUsbTransport {
        in_packets: VecDeque<Vec<u8>>,
        out_packets: Vec<Vec<u8>>,
    }

    impl MockUsbTransport {
        pub(crate) fn new() -> Self {
            Self {
                in_packets: VecDeque::new(),
                out_packets: Vec::new(),
            }
        }

        pub(crate) fn push_response(&mut self, code: u16, transaction_id: u32, params: &[u32]) {
            let mut payload = Vec::new();
            for &p in params {
                payload.extend_from_slice(&p.to_le_bytes());
            }
            let length = (12 + payload.len()) as u32;
            let mut buf = Vec::new();
            buf.extend_from_slice(&length.to_le_bytes());
            buf.extend_from_slice(&(ContainerType::Response as u16).to_le_bytes());
            buf.extend_from_slice(&code.to_le_bytes());
            buf.extend_from_slice(&transaction_id.to_le_bytes());
            buf.extend_from_slice(&payload);
            self.in_packets.push_back(buf);
        }

        pub(crate) fn push_data(&mut self, code: u16, transaction_id: u32, data: &[u8]) {
            let length = (12 + data.len()) as u32;
            let mut buf = Vec::new();
            buf.extend_from_slice(&length.to_le_bytes());
            buf.extend_from_slice(&(ContainerType::Data as u16).to_le_bytes());
            buf.extend_from_slice(&code.to_le_bytes());
            buf.extend_from_slice(&transaction_id.to_le_bytes());
            buf.extend_from_slice(data);
            self.in_packets.push_back(buf);
        }

        pub(crate) fn push_raw_packet(&mut self, raw: Vec<u8>) {
            self.in_packets.push_back(raw);
        }

        pub(crate) fn out_packets(&self) -> &[Vec<u8>] {
            &self.out_packets
        }
    }

    impl UsbBulkTransport for MockUsbTransport {
        fn write_bulk(
            &mut self,
            data: &[u8],
            _timeout: std::time::Duration,
        ) -> Result<usize, crate::error::AppCoreError> {
            self.out_packets.push(data.to_vec());
            Ok(data.len())
        }

        fn read_bulk(
            &mut self,
            buffer: &mut [u8],
            _timeout: std::time::Duration,
        ) -> Result<usize, crate::error::AppCoreError> {
            if let Some(packet) = self.in_packets.pop_front() {
                let len = packet.len().min(buffer.len());
                buffer[..len].copy_from_slice(&packet[..len]);
                Ok(len)
            } else {
                Ok(0)
            }
        }

        fn endpoint_in(&self) -> u8 {
            0x81
        }

        fn endpoint_out(&self) -> u8 {
            0x02
        }
    }

    #[test]
    fn test_mtp_facade_session_lifecycle_and_enumeration() {
        let mut mock = MockUsbTransport::new();
        // 1. OpenSession(tid=1, session_id=1) -> OK
        mock.push_response(resp::OK, 1, &[]);

        // 2. GetStorageIDs(tid=2) -> Data with 1 storage ID (0x00010001) then Response OK
        let mut data_payload = Vec::new();
        data_payload.extend_from_slice(&1u32.to_le_bytes()); // Count = 1
        data_payload.extend_from_slice(&0x00010001u32.to_le_bytes()); // Storage ID
        mock.push_data(op::GET_STORAGE_IDS, 2, &data_payload);
        mock.push_response(resp::OK, 2, &[]);

        let mut session = MtpSession::new(mock);
        session.open_session(1).expect("open_session failed");
        assert_eq!(
            session.state(),
            MtpSessionState::SessionOpen { session_id: 1 }
        );

        let storages = session.get_storage_ids().expect("get_storage_ids failed");
        assert_eq!(storages, vec![0x00010001]);

        let transport = session.into_transport();
        assert_eq!(transport.out_packets.len(), 2);

        let cmd1 = MtpContainer::parse(&transport.out_packets[0]).expect("parse cmd1");
        assert_eq!(cmd1.container_type, ContainerType::Command);
        assert_eq!(cmd1.code, op::OPEN_SESSION);
        assert_eq!(cmd1.transaction_id, 1);
        assert_eq!(cmd1.extract_params(), vec![1]);

        let cmd2 = MtpContainer::parse(&transport.out_packets[1]).expect("parse cmd2");
        assert_eq!(cmd2.container_type, ContainerType::Command);
        assert_eq!(cmd2.code, op::GET_STORAGE_IDS);
        assert_eq!(cmd2.transaction_id, 2);
    }

    #[test]
    fn test_mtp_client_facade_ctor() {
        let client1 = MtpClient::new();
        let client2 = MtpClient::default();
        assert_eq!(client1, client2);
    }

    #[test]
    fn test_mtp_prelude_reexports() {
        use crate::device::mtp::prelude::*;
        let client = MtpClient::new();
        assert_eq!(client, MtpClient::default());
        let _ = ContainerType::Command;

        let broken = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
        assert!(is_io_disconnected(&broken));
        let permitted = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(!is_io_disconnected(&permitted));
    }

    #[test]
    fn test_mtp_vfs_adapter_stat_and_vfs_table_integration() {
        let mut mock = MockUsbTransport::new();

        // 1. GetStorageIDs (tid=1) -> 1 storage (0x10001)
        let mut storages_payload = Vec::new();
        storages_payload.extend_from_slice(&1u32.to_le_bytes());
        storages_payload.extend_from_slice(&0x00010001u32.to_le_bytes());
        mock.push_data(op::GET_STORAGE_IDS, 1, &storages_payload);
        mock.push_response(resp::OK, 1, &[]);

        // 2. GetObjectHandles (tid=2) -> 1 handle (42)
        let mut handles_payload = Vec::new();
        handles_payload.extend_from_slice(&1u32.to_le_bytes());
        handles_payload.extend_from_slice(&42u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 2, &handles_payload);
        mock.push_response(resp::OK, 2, &[]);

        // 3. GetObjectInfo for handle 42 (tid=3)
        // Fixed 52 bytes header + 4 PTP strings (filename, date_created, date_modified, keywords)
        let mut info_payload = vec![0u8; 52];
        // Compressed size at offset 8..12
        info_payload[8..12].copy_from_slice(&8192u32.to_le_bytes());
        // filename PTP string
        info_payload.extend_from_slice(&encode_ptp_string("video.mp4"));
        info_payload.extend_from_slice(&encode_ptp_string(""));
        info_payload.extend_from_slice(&encode_ptp_string(""));
        info_payload.extend_from_slice(&encode_ptp_string(""));

        mock.push_data(op::GET_OBJECT_INFO, 3, &info_payload);
        mock.push_response(resp::OK, 3, &[]);

        let session = MtpSession::new(mock);
        let mut adapter = MtpVfsAdapter::new(session);

        assert_eq!(adapter.scheme(), "mtp");
        let table = adapter
            .to_unified_vfs_table("/")
            .expect("to_unified_vfs_table must succeed");
        assert_eq!(table.len(), 1);
        assert_eq!(table.get_node_path(0), Some("/video.mp4"));
        assert!(table.get_node(0).unwrap().is_file());
        assert_eq!(table.get_node(0).unwrap().uncompressed_size, 8192);
    }

    #[test]
    fn test_mtp_vfs_adapter_bulk_fetching_0x9805() {
        let mut mock = MockUsbTransport::new();

        // 1. GetStorageIDs (tid=1) -> 1 storage (0x10001)
        let mut storages_payload = Vec::new();
        storages_payload.extend_from_slice(&1u32.to_le_bytes());
        storages_payload.extend_from_slice(&0x00010001u32.to_le_bytes());
        mock.push_data(op::GET_STORAGE_IDS, 1, &storages_payload);
        mock.push_response(resp::OK, 1, &[]);

        // 2. Opcode 0x9805 (GetObjectPropList) data container (tid=1)
        let mut dataset = Vec::new();
        let num_elements: u32 = 4;
        dataset.extend_from_slice(&num_elements.to_le_bytes());

        // Object 101: photo.jpg
        dataset.extend_from_slice(&101u32.to_le_bytes());
        dataset.extend_from_slice(&crate::device::mtp::codes::prop::OBJECT_FILE_NAME.to_le_bytes());
        dataset.extend_from_slice(&0xFFFFu16.to_le_bytes()); // STRING
        dataset.extend_from_slice(&encode_ptp_string("photo.jpg"));

        dataset.extend_from_slice(&101u32.to_le_bytes());
        dataset.extend_from_slice(&crate::device::mtp::codes::prop::OBJECT_SIZE.to_le_bytes());
        dataset.extend_from_slice(&0x0006u16.to_le_bytes()); // UINT32
        dataset.extend_from_slice(&4096u32.to_le_bytes());

        // Object 202: DCIM (directory)
        dataset.extend_from_slice(&202u32.to_le_bytes());
        dataset.extend_from_slice(&crate::device::mtp::codes::prop::OBJECT_FILE_NAME.to_le_bytes());
        dataset.extend_from_slice(&0xFFFFu16.to_le_bytes()); // STRING
        dataset.extend_from_slice(&encode_ptp_string("DCIM"));

        dataset.extend_from_slice(&202u32.to_le_bytes());
        dataset.extend_from_slice(&crate::device::mtp::codes::prop::OBJECT_FORMAT.to_le_bytes());
        dataset.extend_from_slice(&0x0004u16.to_le_bytes()); // UINT16
        dataset.extend_from_slice(&crate::device::mtp::codes::format::ASSOCIATION.to_le_bytes());

        mock.push_data(0x9805, 1, &dataset);
        mock.push_response(resp::OK, 1, &[]);

        let session = MtpSession::new(mock);
        let mut adapter = MtpVfsAdapter::new(session).with_bulk_support(true);

        assert!(adapter.supports_bulk_opcode());
        let entries = adapter
            .list_directory("/")
            .expect("list_directory with bulk fetch must succeed");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "photo.jpg");
        assert!(!entries[0].is_dir);
        assert_eq!(entries[0].size, 4096);
        assert_eq!(entries[1].name, "DCIM");
        assert!(entries[1].is_dir);
    }

    #[test]
    fn test_mtp_vfs_adapter_bulk_fetching_fallback_on_error() {
        let mut mock = MockUsbTransport::new();

        // 1. GetStorageIDs (tid=1) -> 1 storage (0x10001)
        let mut storages_payload = Vec::new();
        storages_payload.extend_from_slice(&1u32.to_le_bytes());
        storages_payload.extend_from_slice(&0x00010001u32.to_le_bytes());
        mock.push_data(op::GET_STORAGE_IDS, 1, &storages_payload);
        mock.push_response(resp::OK, 1, &[]);

        // 2. Opcode 0x9805 fails: device returns error Response container
        mock.push_response(resp::GENERAL_ERROR, 1, &[]);

        // 3. Fallback: GetObjectHandles (tid=1) -> 1 handle (42)
        let mut handles_payload = Vec::new();
        handles_payload.extend_from_slice(&1u32.to_le_bytes());
        handles_payload.extend_from_slice(&42u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 1, &handles_payload);
        mock.push_response(resp::OK, 1, &[]);

        // 4. Fallback: GetObjectInfo for handle 42 (tid=2)
        let mut info_payload = vec![0u8; 52];
        info_payload[8..12].copy_from_slice(&2048u32.to_le_bytes());
        info_payload.extend_from_slice(&encode_ptp_string("fallback.txt"));
        info_payload.extend_from_slice(&encode_ptp_string(""));
        info_payload.extend_from_slice(&encode_ptp_string(""));
        info_payload.extend_from_slice(&encode_ptp_string(""));
        mock.push_data(op::GET_OBJECT_INFO, 2, &info_payload);
        mock.push_response(resp::OK, 2, &[]);

        let session = MtpSession::new(mock);
        let mut adapter = MtpVfsAdapter::new(session).with_bulk_support(true);

        let entries = adapter
            .list_directory("/")
            .expect("list_directory with fallback must succeed");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "fallback.txt");
        assert_eq!(entries[0].size, 2048);
    }

    #[test]
    fn test_push_reader_concurrent_same_name_object_appearing_during_upload_rolls_back_new_handle()
    {
        let build_info = |name: &str, size: u32| {
            let mut p = vec![0u8; 52];
            p[8..12].copy_from_slice(&size.to_le_bytes());
            p.extend_from_slice(&encode_ptp_string(name));
            p.extend_from_slice(&encode_ptp_string(""));
            p.extend_from_slice(&encode_ptp_string(""));
            p.extend_from_slice(&encode_ptp_string(""));
            p
        };

        let mut mock = MockUsbTransport::new();
        // Pre-upload GET_OBJECT_HANDLES (tid=1) -> [0x10 ("notes.txt")]
        let mut handles_payload = Vec::new();
        handles_payload.extend_from_slice(&1u32.to_le_bytes());
        handles_payload.extend_from_slice(&0x10u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 1, &handles_payload);
        mock.push_response(resp::OK, 1, &[]);
        mock.push_data(op::GET_OBJECT_INFO, 2, &build_info("notes.txt", 10));
        mock.push_response(resp::OK, 2, &[]);

        // KeepBoth selects "notes (1).txt" and creates handle 0x33 (tid=3, 4)
        mock.push_response(resp::OK, 3, &[0x0001_0001, MTP_OBJECT_HANDLE_ROOT, 0x33]);
        mock.push_response(resp::OK, 4, &[]);
        mock.push_data(op::GET_OBJECT_INFO, 5, &build_info("notes (1).txt", 4));
        mock.push_response(resp::OK, 5, &[]);

        // Post-upload GET_OBJECT_HANDLES (tid=6) reveals a concurrent handle 0x88 appeared during upload!
        let mut post_handles = Vec::new();
        post_handles.extend_from_slice(&3u32.to_le_bytes());
        post_handles.extend_from_slice(&0x10u32.to_le_bytes());
        post_handles.extend_from_slice(&0x33u32.to_le_bytes());
        post_handles.extend_from_slice(&0x88u32.to_le_bytes());
        mock.push_data(op::GET_OBJECT_HANDLES, 6, &post_handles);
        mock.push_response(resp::OK, 6, &[]);
        mock.push_data(op::GET_OBJECT_INFO, 7, &build_info("notes (1).txt", 99));
        mock.push_response(resp::OK, 7, &[]);
        // Rollback DELETE_OBJECT(0x33) (tid=8)
        mock.push_response(resp::OK, 8, &[]);

        let mut session = MtpSession::new(mock);
        let err = push_object_with_session(
            &mut session,
            0x0001_0001,
            "/",
            "notes.txt",
            b"data",
            MtpConflictPolicy::KeepBoth,
            |_, _| Ok(()),
        )
        .expect_err("concurrent same-name object appearing during upload must roll back and fail");
        assert!(err.to_string().contains("Concurrent same-name MTP object"));

        let transport = session.into_transport();
        let delete_cmds: Vec<_> = transport
            .out_packets
            .iter()
            .filter_map(|pkt| MtpContainer::parse(pkt).ok())
            .filter(|c| c.code == op::DELETE_OBJECT)
            .collect();
        // Must ONLY delete our newly created handle 0x33, NEVER 0x88 or 0x10!
        assert_eq!(delete_cmds.len(), 1);
        assert_eq!(delete_cmds[0].extract_params(), vec![0x33, 0]);
    }
}
