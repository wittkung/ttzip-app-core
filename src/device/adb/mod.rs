// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Android Debug Bridge (ADB) protocol client, TCP transport, and file sync streams.
//!
//! # Quickstart
//!
//! ```no_run
//! use ttzip_app_core::device::adb::prelude::*;
//!
//! fn main() -> AdbResult<()> {
//!     let client = AdbClient::default();
//!     let devices = client.list_devices()?;
//!     for dev in devices {
//!         println!("Found: {} ({:?})", dev.serial, dev.state);
//!     }
//!     Ok(())
//! }
//! ```

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use crate::device::{ArchiveVfsProvider, DeviceVfsEntry};
use crate::error::AppCoreError;

pub mod error;
pub mod sync;
pub mod transport;
pub mod types;

pub use error::{is_io_disconnected, AdbError, AdbResult, ErrorChain};
pub use sync::{AdbSyncError, AdbSyncStream};
pub use transport::{
    parse_adb_device_line, AdbClient, AdbTransportError, DEFAULT_ADB_DAEMON_PORT,
    DEFAULT_ADB_PORT, DEFAULT_CONNECT_TIMEOUT,
};
pub use types::{mode, AdbDevice, AdbDeviceState, AdbSyncEntry, AdbTransportType};

/// Lightweight VFS adapter bridging an active ADB sync stream into the unified archive VFS model.
pub struct AdbVfsAdapter<S = std::net::TcpStream> {
    stream: AdbSyncStream<S>,
}

impl<S> std::fmt::Debug for AdbVfsAdapter<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdbVfsAdapter").finish_non_exhaustive()
    }
}

impl<S> AdbVfsAdapter<S> {
    /// Creates a new ADB VFS adapter wrapping an active sync stream.
    #[inline]
    pub fn new(stream: AdbSyncStream<S>) -> Self {
        Self { stream }
    }

    /// Borrows the underlying sync stream.
    #[inline]
    pub fn stream(&self) -> &AdbSyncStream<S> {
        &self.stream
    }

    /// Mutably borrows the underlying sync stream.
    #[inline]
    pub fn stream_mut(&mut self) -> &mut AdbSyncStream<S> {
        &mut self.stream
    }

    /// Consumes the adapter and returns the underlying sync stream.
    #[inline]
    pub fn into_inner(self) -> AdbSyncStream<S> {
        self.stream
    }
}

impl<S: Read + Write + Send> ArchiveVfsProvider for AdbVfsAdapter<S> {
    #[inline]
    fn scheme(&self) -> &'static str {
        "adb"
    }

    fn stat(&mut self, path: &str) -> Result<DeviceVfsEntry, AppCoreError> {
        let entry = self.stream.stat(path)?;
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        Ok(DeviceVfsEntry {
            path: path.to_string(),
            name,
            size: entry.size,
            is_dir: entry.is_directory(),
            mtime_epoch_secs: entry.mtime as u64,
            posix_mode: entry.mode,
        })
    }

    fn list_directory(&mut self, path: &str) -> Result<Vec<DeviceVfsEntry>, AppCoreError> {
        let entries = self.stream.list(path)?;
        let clean_path = path.trim_end_matches('/');
        let mut results = Vec::with_capacity(entries.len());
        for e in entries {
            let is_dir = e.is_directory();
            let size = e.size;
            let mtime = e.mtime as u64;
            let mode = e.mode;
            let name = e.name;
            let child_path = if clean_path.is_empty() || clean_path == "/" {
                format!("/{}", name)
            } else {
                format!("{}/{}", clean_path, name)
            };
            results.push(DeviceVfsEntry {
                path: child_path,
                name,
                size,
                is_dir,
                mtime_epoch_secs: mtime,
                posix_mode: mode,
            });
        }
        Ok(results)
    }

    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, AppCoreError> {
        self.stream.recv_file(path)
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), AppCoreError> {
        self.stream.send_file(path, data, 0o644, 0)
    }
}

/// Standard prelude for convenient glob-importing common ADB error types, and domain models.
pub mod prelude {
    pub use super::{
        is_io_disconnected, mode, AdbClient, AdbDevice, AdbDeviceState, AdbError,
        AdbResult, AdbSyncEntry, AdbSyncError, AdbSyncStream,
        AdbTransportError, AdbTransportType, AdbVfsAdapter, ErrorChain,
    };
}

// Compile-time gate ensuring all domain, transport, and stream types satisfy async concurrency bounds.
const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    const fn assert_send<T: Send + 'static>() {}
    const fn assert_clone<T: Clone>() {}
    const fn assert_copy<T: Copy>() {}
    const fn assert_unpin<T: Unpin>() {}
    const fn assert_eq_bounds<T: Eq>() {}
    const fn assert_hash<T: std::hash::Hash>() {}
    const fn assert_default<T: Default>() {}

    // Concurrency boundaries
    assert_send_sync::<AdbClient>();
    assert_send_sync::<AdbDevice>();
    assert_send_sync::<AdbDeviceState>();
    assert_send_sync::<AdbSyncEntry>();
    assert_send_sync::<AdbTransportType>();

    // Value semantics and algebraic properties
    assert_default::<AdbClient>();

    assert_clone::<AdbClient>();
    assert_clone::<AdbDevice>();
    assert_clone::<AdbDeviceState>();
    assert_clone::<AdbSyncEntry>();
    assert_clone::<AdbTransportType>();

    // Copy semantics: AdbClient is intentionally NOT constrained to Copy to allow stateful connection pools and keys
    assert_copy::<AdbTransportType>();

    // Equivalence and hashing
    assert_eq_bounds::<AdbClient>();
    assert_eq_bounds::<AdbDevice>();
    assert_eq_bounds::<AdbDeviceState>();
    assert_eq_bounds::<AdbSyncEntry>();
    assert_eq_bounds::<AdbTransportType>();

    assert_hash::<AdbDeviceState>();
    assert_hash::<AdbTransportType>();

    // Stream dispatching and thread safety verification
    assert_send::<AdbSyncStream<std::net::TcpStream>>();
    assert_unpin::<AdbSyncStream<std::net::TcpStream>>();

    // VFS Adapter thread safety
    assert_send::<AdbVfsAdapter<std::net::TcpStream>>();
};

#[cfg(test)]
mod tests {
    use super::prelude::*;
    use crate::device::ArchiveVfsProvider;

    #[test]
    fn test_adb_facade_integration_smoke() {
        let client = AdbClient::default();
        assert_eq!(client.clone(), AdbClient::new());

        let dev = AdbDevice {
            serial: "emulator-5554".to_string(),
            state: AdbDeviceState::Device,
            model: Some("Pixel_8".to_string()),
            product: Some("shiba".to_string()),
            device: Some("shiba".to_string()),
            transport_type: AdbTransportType::Emulator,
            transport_id: Some(1),
            features: Default::default(),
        };
        assert_eq!(dev.serial, "emulator-5554");
        assert_eq!(dev.state, AdbDeviceState::Device);
        assert_eq!(dev.clone(), dev);

        let entry = AdbSyncEntry {
            name: "test.txt".to_string(),
            mode: mode::S_IFREG | 0o644,
            size: 1024,
            mtime: 1700000000,
        };
        assert!(entry.is_file());
        assert_eq!(entry.clone(), entry);

        let broken = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
        assert!(is_io_disconnected(&broken));

        let err: AdbError = AdbTransportError::DeviceOffline.into();
        assert!(err.is::<AdbTransportError>());
    }

    #[test]
    fn test_adb_vfs_adapter_stat_and_vfs_table_integration() {
        use std::os::unix::net::UnixStream;
        use std::io::Write;

        let (client_io, mut server_io) = UnixStream::pair().expect("UnixStream pair");

        // Prepare STAT response on server side
        let stat_bytes = [
            *b"STAT",
            (mode::S_IFREG | 0o644).to_le_bytes(),
            4096u32.to_le_bytes(),
            1700000000u32.to_le_bytes(),
        ];
        server_io.write_all(stat_bytes.as_flattened()).unwrap();

        let sync_stream = AdbSyncStream::new(client_io);
        let mut adapter = AdbVfsAdapter::new(sync_stream);

        assert_eq!(adapter.scheme(), "adb");
        let entry = adapter.stat("/sdcard/Download/test.zip").expect("stat must succeed");
        assert_eq!(entry.path, "/sdcard/Download/test.zip");
        assert_eq!(entry.name, "test.zip");
        assert_eq!(entry.size, 4096);
        assert!(!entry.is_dir);
        assert_eq!(entry.mtime_epoch_secs, 1700000000);
        assert_eq!(entry.posix_mode, mode::S_IFREG | 0o644);
    }

    #[test]
    fn test_adb_vfs_adapter_list_and_populate_table() {
        use std::os::unix::net::UnixStream;
        use std::io::Write;

        let (client_io, mut server_io) = UnixStream::pair().expect("UnixStream pair");

        // 1. DENT entry: "photos" (dir)
        let dent1_hdr = [*b"DENT", (mode::S_IFDIR | 0o755).to_le_bytes(), 0u32.to_le_bytes(), 100u32.to_le_bytes(), 6u32.to_le_bytes()];
        server_io.write_all(dent1_hdr.as_flattened()).unwrap();
        server_io.write_all(b"photos").unwrap();

        // 2. DENT entry: "readme.txt" (file)
        let dent2_hdr = [*b"DENT", (mode::S_IFREG | 0o644).to_le_bytes(), 1024u32.to_le_bytes(), 200u32.to_le_bytes(), 10u32.to_le_bytes()];
        server_io.write_all(dent2_hdr.as_flattened()).unwrap();
        server_io.write_all(b"readme.txt").unwrap();

        // 3. DONE entry + 12 trailing padding bytes
        let done_hdr = [*b"DONE", 0u32.to_le_bytes()];
        server_io.write_all(done_hdr.as_flattened()).unwrap();
        server_io.write_all(&[0u8; 12]).unwrap();

        let sync_stream = AdbSyncStream::new(client_io);
        let mut adapter = AdbVfsAdapter::new(sync_stream);

        let table = adapter.to_unified_vfs_table("/sdcard").expect("to_unified_vfs_table must succeed");
        assert_eq!(table.len(), 2);
        assert_eq!(table.get_node_path(0), Some("/sdcard/photos"));
        assert_eq!(table.get_node_path(1), Some("/sdcard/readme.txt"));
        assert!(table.get_node(0).unwrap().is_dir());
        assert!(table.get_node(1).unwrap().is_file());
        assert_eq!(table.get_node(1).unwrap().uncompressed_size, 1024);
    }

    #[test]
    fn test_pair_wireless_via_daemon_success_and_wrong_pin_rejection() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral listener");
        let port = listener.local_addr().expect("local addr").port();

        let server_thread = thread::spawn(move || {
            // 1. Valid PIN 654321
            let (mut s1, _) = listener.accept().expect("accept 1");
            let expected_cmd1 = "0023host:pair:654321:192.168.1.50:37123";
            let mut buf1 = vec![0u8; expected_cmd1.len()];
            s1.read_exact(&mut buf1).expect("read cmd 1");
            assert_eq!(String::from_utf8_lossy(&buf1), expected_cmd1);
            let ok_body = "Successfully paired to 192.168.1.50:37123 [guid=adb-pixel8]";
            let resp1 = format!("OKAY{:04x}{}", ok_body.len(), ok_body);
            s1.write_all(resp1.as_bytes()).expect("write resp 1");

            // 2. Wrong PIN 000000 -> FAIL
            let (mut s2, _) = listener.accept().expect("accept 2");
            let mut buf2 = vec![0u8; expected_cmd1.len()];
            s2.read_exact(&mut buf2).expect("read cmd 2");
            let fail_body = "Failed: Wrong password or connection was dropped.";
            let resp2 = format!("FAIL{:04x}{}", fail_body.len(), fail_body);
            s2.write_all(resp2.as_bytes()).expect("write resp 2");
        });

        let ok_res = AdbClient::pair_wireless_via_daemon("654321", "192.168.1.50", 37123, Some(port))
            .expect("valid PIN should succeed");
        assert!(ok_res.contains("Successfully paired to 192.168.1.50:37123"));

        let err_res = AdbClient::pair_wireless_via_daemon("000000", "192.168.1.50", 37123, Some(port));
        assert!(err_res.is_err(), "wrong PIN must be rejected");

        // Invalid PIN format rejected locally before socket I/O
        assert!(AdbClient::pair_wireless_via_daemon("12a456", "192.168.1.50", 37123, Some(port)).is_err());

        server_thread.join().expect("server thread join");
    }

    #[test]
    fn test_connect_wireless_via_daemon_verifies_payload_and_rejects_soft_failure() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral listener");
        let port = listener.local_addr().expect("local addr").port();

        let server_thread = thread::spawn(move || {
            // 1. Soft failure: OKAY followed by "failed to connect to ..."
            let (mut s1, _) = listener.accept().expect("accept 1");
            let mut buf1 = [0u8; 34]; // "001ehost:connect:192.168.1.50:5555"
            s1.read_exact(&mut buf1).expect("read 1");
            let soft_err = "failed to connect to '192.168.1.50:5555': Connection refused";
            let resp1 = format!("OKAY{:04x}{}", soft_err.len(), soft_err);
            s1.write_all(resp1.as_bytes()).expect("write 1");

            // 2. Auth failure: OKAY followed by "failed to authenticate to ..."
            let (mut s2, _) = listener.accept().expect("accept 2");
            let mut buf2 = [0u8; 34];
            s2.read_exact(&mut buf2).expect("read 2");
            let auth_err = "failed to authenticate to 192.168.1.50:5555";
            let resp2 = format!("OKAY{:04x}{}", auth_err.len(), auth_err);
            s2.write_all(resp2.as_bytes()).expect("write 2");
        });

        let err1 = AdbClient::connect_wireless_via_daemon("192.168.1.50", Some(5555), Some(port))
            .expect_err("soft failure must be rejected");
        assert!(matches!(err1, AdbTransportError::ConnectionFailed(_)));

        let err2 = AdbClient::connect_wireless_via_daemon("192.168.1.50", Some(5555), Some(port))
            .expect_err("auth failure must return Unauthorized");
        assert!(matches!(err2, AdbTransportError::Unauthorized));

        server_thread.join().expect("server thread join");
    }

    #[test]
    fn test_adb_vfs_adapter_cjk_spaces_emoji_nested_hierarchy() {
        use std::os::unix::net::UnixStream;
        use std::io::Write;

        let (client_io, mut server_io) = UnixStream::pair().expect("UnixStream pair");
        let name = "测试 文件夹 🚀/报告 (2026) ✨.pdf";
        let leaf_name = "报告 (2026) ✨.pdf";
        let name_bytes = leaf_name.as_bytes();
        let dent_hdr = [
            *b"DENT",
            (mode::S_IFREG | 0o644).to_le_bytes(),
            65536u32.to_le_bytes(),
            1710000000u32.to_le_bytes(),
            (name_bytes.len() as u32).to_le_bytes(),
        ];
        server_io.write_all(dent_hdr.as_flattened()).unwrap();
        server_io.write_all(name_bytes).unwrap();
        server_io.write_all([*b"DONE", 0u32.to_le_bytes()].as_flattened()).unwrap();

        let mut adapter = AdbVfsAdapter::new(AdbSyncStream::new(client_io));
        let entries = adapter
            .list_directory("/storage/emulated/0/测试 文件夹 🚀")
            .expect("list CJK/emoji directory");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, leaf_name);
        assert_eq!(entries[0].path, format!("/storage/emulated/0/{}", name));
        assert_eq!(entries[0].size, 65536);
    }

    #[test]
    fn test_list_devices_via_daemon_with_smart_socket_mock() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral listener");
        let port = listener.local_addr().expect("local addr").port();

        let server_thread = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut req_buf = [0u8; 18]; // "000ehost:devices-l"
            socket.read_exact(&mut req_buf).expect("read command");
            assert_eq!(&req_buf, b"000ehost:devices-l");

            let body = "emulator-5554          device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:1\n";
            let hex_len = format!("{:04x}", body.len());
            socket.write_all(b"OKAY").expect("write OKAY");
            socket.write_all(hex_len.as_bytes()).expect("write hex len");
            socket.write_all(body.as_bytes()).expect("write body");
            socket.flush().expect("flush");
        });

        let devices = AdbClient::list_devices_via_daemon(Some(port))
            .expect("list_devices_via_daemon failed");
        server_thread.join().expect("server thread join");

        assert_eq!(devices.len(), 1);
        let dev = &devices[0];
        assert_eq!(dev.serial, "emulator-5554");
        assert_eq!(dev.state, AdbDeviceState::Device);
        assert_eq!(dev.model.as_deref(), Some("sdk_gphone64_arm64"));
        assert_eq!(dev.product.as_deref(), Some("sdk_gphone64_arm64"));
        assert_eq!(dev.transport_id, Some(1));
    }
}
