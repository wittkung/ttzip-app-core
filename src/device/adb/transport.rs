// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! TCP socket transport layer for ADB, supporting wireless TCP and local daemon bridge (127.0.0.1:5037).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::device::adb::sync::AdbSyncStream;
use crate::device::adb::types::{AdbDevice, AdbDeviceState, AdbTransportType};

pub const DEFAULT_ADB_PORT: u16 = 5555;
pub const DEFAULT_ADB_DAEMON_PORT: u16 = 5037;
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// Thread-safe abort handle for interrupting blocking ADB socket operations.
#[derive(Debug, Clone)]
pub struct AdbAbortHandle {
    stream: std::sync::Arc<TcpStream>,
}

impl AdbAbortHandle {
    /// Creates a new abort handle wrapping the given socket stream reference counter.
    #[must_use]
    pub fn new(stream: std::sync::Arc<TcpStream>) -> Self {
        Self { stream }
    }

    /// Creates a new abort handle by duplicating the underlying socket descriptor.
    pub fn try_from_stream(stream: &TcpStream) -> std::io::Result<Self> {
        let cloned = stream.try_clone()?;
        Ok(Self {
            stream: std::sync::Arc::new(cloned),
        })
    }

    /// Immediately interrupts ongoing socket I/O by shutting down both read and write channels.
    pub fn abort(&self) -> std::io::Result<()> {
        self.stream.shutdown(std::net::Shutdown::Both)
    }

    /// Returns a reference to the underlying shared stream.
    #[must_use]
    pub fn stream(&self) -> &std::sync::Arc<TcpStream> {
        &self.stream
    }
}

impl From<std::sync::Arc<TcpStream>> for AdbAbortHandle {
    fn from(stream: std::sync::Arc<TcpStream>) -> Self {
        Self::new(stream)
    }
}

// Compile-time static gate ensuring AdbAbortHandle satisfies Send and Sync invariants.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AdbAbortHandle>();
};

/// High-level client for managing ADB transport connections and streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbClient {
    _private: (),
}

impl Default for AdbClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AdbClient {
    /// Creates a new default ADB transport client.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Lists all devices connected to the default local ADB daemon.
    pub fn list_devices(&self) -> Result<Vec<AdbDevice>, AdbTransportError> {
        Self::list_devices_via_daemon(None)
    }

    /// Enumerates devices connected to the local ADB daemon server at 127.0.0.1:5037.
    /// Returns an explicit error if the local ADB server is not running or unreachable.
    pub fn list_devices_via_daemon(
        server_port: Option<u16>,
    ) -> Result<Vec<AdbDevice>, AdbTransportError> {
        let port = server_port.unwrap_or(DEFAULT_ADB_DAEMON_PORT);
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port)
            .parse()
            .map_err(|e| AdbTransportError::ConnectionFailed(format!("{}", e)))?;
        let mut client = crate::device::adb_socket::AdbSmartSocketClient::connect_timeout(
            addr,
            DEFAULT_CONNECT_TIMEOUT,
        )
        .map_err(|e| {
            AdbTransportError::ConnectionFailed(format!(
                "Failed to connect to local ADB daemon on port {}: {}",
                port, e
            ))
        })?;

        client
            .send_command("host:devices-l")
            .map_err(AdbTransportError::Io)?;
        client.read_status().map_err(|e| {
            AdbTransportError::DaemonRejected(format!("ADB server rejected devices-l: {}", e))
        })?;
        let text = client.read_payload().map_err(|e| {
            AdbTransportError::Protocol(format!("Invalid length header from ADB daemon: {}", e))
        })?;

        let mut devices = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(dev) = parse_adb_device_line(line) {
                devices.push(dev);
            }
        }

        Ok(devices)
    }

    /// Establishes an active SyncStream to a device via the local ADB server bridge.
    pub fn open_sync_via_daemon(
        serial: &str,
        server_port: Option<u16>,
    ) -> Result<AdbSyncStream<TcpStream>, AdbTransportError> {
        Self::open_sync_with_abort_via_daemon(serial, server_port).map(|(sync, _abort)| sync)
    }

    /// Establishes an active SyncStream to a device via the local ADB server bridge,
    /// returning both the stream and a thread-safe abort handle for out-of-band cancellation.
    pub fn open_sync_with_abort_via_daemon(
        serial: &str,
        server_port: Option<u16>,
    ) -> Result<(AdbSyncStream<TcpStream>, AdbAbortHandle), AdbTransportError> {
        let port = server_port.unwrap_or(DEFAULT_ADB_DAEMON_PORT);
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port)
            .parse()
            .map_err(|e| AdbTransportError::ConnectionFailed(format!("{}", e)))?;
        let mut stream = TcpStream::connect_timeout(&addr, DEFAULT_CONNECT_TIMEOUT)
            .map_err(|e| AdbTransportError::ConnectionFailed(format!("Cannot connect to ADB daemon: {}", e)))?;
        stream.set_read_timeout(Some(DEFAULT_CONNECT_TIMEOUT))?;
        stream.set_write_timeout(Some(DEFAULT_CONNECT_TIMEOUT))?;

        // 1. Switch transport to device: "host:transport:<serial>"
        let transport_cmd = format!("host:transport:{}", serial);
        let req1 = format!("{:04x}{}", transport_cmd.len(), transport_cmd);
        stream.write_all(req1.as_bytes())?;
        stream.flush()?;

        let mut status = [0u8; 4];
        stream.read_exact(&mut status)?;
        if &status != b"OKAY" {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Err(AdbTransportError::DaemonRejected(format!(
                "Device '{}' transport selection rejected by ADB daemon",
                serial
            )));
        }

        // 2. Request sync service: "sync:"
        let sync_cmd = "sync:";
        let req2 = format!("{:04x}{}", sync_cmd.len(), sync_cmd);
        stream.write_all(req2.as_bytes())?;
        stream.flush()?;

        stream.read_exact(&mut status)?;
        if &status != b"OKAY" {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return Err(AdbTransportError::DaemonRejected(
                "ADB daemon rejected sync service on selected device".to_string(),
            ));
        }

        let abort_handle = AdbAbortHandle::try_from_stream(&stream)?;
        Ok((AdbSyncStream::new(stream), abort_handle))
    }

    /// Connects to a network Android device via the local ADB daemon server at 127.0.0.1:5037.
    pub fn connect_wireless_direct(
        host: &str,
        port: Option<u16>,
    ) -> Result<AdbDevice, AdbTransportError> {
        Self::connect_wireless_via_daemon(host, port, None)
    }

    /// Connects to a network Android device via a specified local ADB daemon port,
    /// validating both the Smart Socket status header and the `connect_service` payload.
    pub fn connect_wireless_via_daemon(
        host: &str,
        port: Option<u16>,
        server_port: Option<u16>,
    ) -> Result<AdbDevice, AdbTransportError> {
        let trimmed_host = host.trim();
        if trimmed_host.is_empty() {
            return Err(AdbTransportError::Protocol(
                "Wireless host address must not be empty".to_string(),
            ));
        }
        let target_port = port.unwrap_or(DEFAULT_ADB_PORT);
        if target_port == 0 {
            return Err(AdbTransportError::Protocol(
                "Wireless port must be greater than zero".to_string(),
            ));
        }
        let daemon_port = server_port.unwrap_or(DEFAULT_ADB_DAEMON_PORT);
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", daemon_port)
            .parse()
            .map_err(|e| AdbTransportError::ConnectionFailed(format!("{}", e)))?;
        let mut client = crate::device::adb_socket::AdbSmartSocketClient::connect_timeout(
            addr,
            DEFAULT_CONNECT_TIMEOUT,
        )
        .map_err(|e| {
            AdbTransportError::ConnectionFailed(format!(
                "Failed to connect to local ADB daemon on port {}: {}",
                daemon_port, e
            ))
        })?;

        let cmd_str = format!("host:connect:{}:{}", trimmed_host, target_port);
        client
            .send_command(&cmd_str)
            .map_err(AdbTransportError::Io)?;
        client.read_status().map_err(|e| {
            AdbTransportError::DaemonRejected(format!("ADB server rejected connect: {}", e))
        })?;

        let payload = client.read_payload().unwrap_or_default();
        verify_connect_payload(&payload)?;

        let serial = format!("{}:{}", trimmed_host, target_port);
        if let Ok(devices) = Self::list_devices_via_daemon(Some(daemon_port)) {
            if let Some(found) = devices.into_iter().find(|d| d.serial == serial) {
                return match found.state {
                    AdbDeviceState::Device => Ok(found),
                    AdbDeviceState::Unauthorized | AdbDeviceState::Authorizing => {
                        Err(AdbTransportError::Unauthorized)
                    }
                    AdbDeviceState::Offline => Err(AdbTransportError::DeviceOffline),
                    other => Err(AdbTransportError::DaemonRejected(format!(
                        "Device '{}' connected in non-ready state {:?}",
                        serial, other
                    ))),
                };
            }
        }

        Ok(AdbDevice {
            serial,
            state: AdbDeviceState::Device,
            model: None,
            product: None,
            device: None,
            transport_type: AdbTransportType::Tcp,
            transport_id: None,
            features: std::collections::HashSet::new(),
        })
    }

    /// Pairs with an Android 11+ wireless debugging endpoint using a 6-digit PIN code
    /// via the default local ADB daemon (`127.0.0.1:5037`).
    pub fn pair_wireless(
        pin: &str,
        host: &str,
        port: u16,
    ) -> Result<String, AdbTransportError> {
        Self::pair_wireless_via_daemon(pin, host, port, None)
    }

    /// Pairs with an Android 11+ wireless debugging endpoint via a specified local ADB daemon port,
    /// sending `host:pair:<pin>:<host>:<port>` and verifying the `"Successfully paired to "` response.
    pub fn pair_wireless_via_daemon(
        pin: &str,
        host: &str,
        port: u16,
        server_port: Option<u16>,
    ) -> Result<String, AdbTransportError> {
        validate_pair_params(pin, host, port)?;
        let daemon_port = server_port.unwrap_or(DEFAULT_ADB_DAEMON_PORT);
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", daemon_port)
            .parse()
            .map_err(|e| AdbTransportError::ConnectionFailed(format!("{}", e)))?;
        let mut client = crate::device::adb_socket::AdbSmartSocketClient::connect_timeout(
            addr,
            Duration::from_secs(4),
        )
        .map_err(|e| {
            AdbTransportError::ConnectionFailed(format!(
                "Failed to connect to local ADB daemon on port {}: {}",
                daemon_port, e
            ))
        })?;

        let cmd_str = format!("host:pair:{}:{}:{}", pin.trim(), host.trim(), port);
        client
            .send_command(&cmd_str)
            .map_err(AdbTransportError::Io)?;
        client.read_status().map_err(|e| {
            AdbTransportError::DaemonRejected(format!("Wireless pairing rejected: {}", e))
        })?;

        let payload = client.read_payload().map_err(|e| {
            AdbTransportError::Protocol(format!("Invalid pairing response payload: {}", e))
        })?;
        verify_pair_payload(&payload)
    }
}

fn validate_pair_params(pin: &str, host: &str, port: u16) -> Result<(), AdbTransportError> {
    let clean_pin = pin.trim();
    if clean_pin.len() != 6 || !clean_pin.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AdbTransportError::Protocol(
            "Wireless pairing requires a 6-digit numeric PIN code".to_string(),
        ));
    }
    if host.trim().is_empty() {
        return Err(AdbTransportError::Protocol(
            "Wireless pairing requires a non-empty host address".to_string(),
        ));
    }
    if port == 0 {
        return Err(AdbTransportError::Protocol(
            "Wireless pairing requires a non-zero pairing port".to_string(),
        ));
    }
    Ok(())
}

fn verify_pair_payload(payload: &str) -> Result<String, AdbTransportError> {
    let trimmed = payload.trim();
    if trimmed.starts_with("Successfully paired to ") {
        return Ok(trimmed.to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("wrong password") || lower.contains("authenticate") {
        return Err(AdbTransportError::Unauthorized);
    }
    Err(AdbTransportError::DaemonRejected(if trimmed.is_empty() {
        "ADB daemon returned empty pairing result".to_string()
    } else {
        trimmed.to_string()
    }))
}

fn verify_connect_payload(payload: &str) -> Result<(), AdbTransportError> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("connected to ") || lower.starts_with("already connected to ") {
        return Ok(());
    }
    if lower.contains("failed to authenticate") || lower.contains("unauthorized") {
        return Err(AdbTransportError::Unauthorized);
    }
    Err(AdbTransportError::ConnectionFailed(format!(
        "ADB wireless connect failed: {}",
        trimmed
    )))
}

/// Parses an ADB host:devices-l output line into an AdbDevice struct.
/// Returns `None` for empty lines, daemon headers ("List of devices attached"),
/// server logs ("* daemon ... *"), or unrecognized non-device output lines.
pub fn parse_adb_device_line(line: &str) -> Option<AdbDevice> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('*')
        || trimmed.starts_with("List of devices")
        || trimmed.starts_with("error:")
        || trimmed.starts_with("adb:")
    {
        return None;
    }

    let mut parts = trimmed.split_whitespace().peekable();
    let serial = parts.next()?.trim().to_string();
    let state_token = parts.next()?.trim();

    let state = match state_token {
        "device" => AdbDeviceState::Device,
        "unauthorized" => AdbDeviceState::Unauthorized,
        "authorizing" => AdbDeviceState::Authorizing,
        "offline" => AdbDeviceState::Offline,
        "connecting" => AdbDeviceState::Connecting,
        "recovery" => AdbDeviceState::Recovery,
        "sideload" => AdbDeviceState::Sideload,
        "bootloader" | "fastboot" => AdbDeviceState::Bootloader,
        "rescue" => AdbDeviceState::Recovery,
        "no" if parts.peek().map(|s| s.trim()) == Some("permissions") => {
            let _ = parts.next(); // Consume "permissions"
            AdbDeviceState::NoPermissions
        }
        unknown_state => AdbDeviceState::Unknown(unknown_state.to_string()),
    };

    let mut model = None;
    let mut product = None;
    let mut device = None;
    let mut transport_id = None;
    let mut features = std::collections::HashSet::new();

    for part in parts {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("model:") {
            model = Some(val.trim().to_string());
        } else if let Some(val) = part.strip_prefix("product:") {
            product = Some(val.trim().to_string());
        } else if let Some(val) = part.strip_prefix("device:") {
            device = Some(val.trim().to_string());
        } else if let Some(val) = part.strip_prefix("transport_id:") {
            transport_id = val.trim().parse::<u64>().ok();
        } else if let Some(val) = part.strip_prefix("features:") {
            for feat in val.trim().split(',') {
                let f = feat.trim();
                if !f.is_empty() {
                    features.insert(f.to_string());
                }
            }
        }
    }

    let transport_type = if serial.starts_with("emulator-") {
        AdbTransportType::Emulator
    } else if serial.starts_with('[') {
        if serial.contains("]:") {
            AdbTransportType::Tcp
        } else {
            AdbTransportType::Usb
        }
    } else if serial.contains(':') {
        AdbTransportType::Tcp
    } else {
        AdbTransportType::Usb
    };

    Some(AdbDevice {
        serial,
        state,
        model,
        product,
        device,
        transport_type,
        transport_id,
        features,
    })
}

/// Strongly-typed error cases encountered in ADB socket transports and daemon bridges.
#[derive(Debug, thiserror::Error)]
pub enum AdbTransportError {
    #[error("Connection timeout: {0:?}")]
    Timeout(Duration),

    #[error("Failed to connect to ADB endpoint: {0}")]
    ConnectionFailed(String),

    #[error("ADB daemon rejected request: {0}")]
    DaemonRejected(String),

    #[error("Unauthorized: device requires RSA authorization key confirmation")]
    Unauthorized,

    #[error("Device is offline or unreachable")]
    DeviceOffline,

    #[error("Socket I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol handshake error: {0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_adb_daemon_device_line() {
        let line = "emulator-5554          device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:1 features:shell_v2,cmd,stat_v2";
        let dev = parse_adb_device_line(line).expect("Failed to parse emulator line");
        assert_eq!(dev.serial, "emulator-5554");
        assert_eq!(dev.state, AdbDeviceState::Device);
        assert_eq!(dev.model.as_deref(), Some("sdk_gphone64_arm64"));
        assert_eq!(dev.product.as_deref(), Some("sdk_gphone64_arm64"));
        assert_eq!(dev.device.as_deref(), Some("emu64a"));
        assert_eq!(dev.transport_type, AdbTransportType::Emulator);
        assert_eq!(dev.transport_id, Some(1));
        assert!(dev.has_feature("shell_v2"));
        assert!(dev.has_feature("cmd"));
        assert!(dev.has_feature("stat_v2"));
        assert!(!dev.has_feature("ls_v2"));
    }

    #[test]
    fn test_parse_adb_daemon_device_line_edge_cases() {
        // Physical USB device with usb bus address, transport_id and features
        let usb_line = "0a388e93 device usb:3-1 product:walleye model:Pixel_2 device:walleye transport_id:2 features:cmd,stat_v2";
        let dev_usb = parse_adb_device_line(usb_line).expect("Failed to parse physical USB device");
        assert_eq!(dev_usb.serial, "0a388e93");
        assert_eq!(dev_usb.state, AdbDeviceState::Device);
        assert_eq!(dev_usb.transport_type, AdbTransportType::Usb);
        assert_eq!(dev_usb.transport_id, Some(2));
        assert!(dev_usb.has_feature("cmd"));
        assert!(dev_usb.has_feature("stat_v2"));

        // Direct TCP network device with port
        let network_line = "192.168.1.150:5555   unauthorized product:pixel model:Pixel_8 device:shiba";
        let dev = parse_adb_device_line(network_line).expect("Failed to parse network device line");
        assert_eq!(dev.serial, "192.168.1.150:5555");
        assert_eq!(dev.state, AdbDeviceState::Unauthorized);
        assert_eq!(dev.transport_type, AdbTransportType::Tcp);
        assert_eq!(dev.transport_id, None);

        // IPv6 device with interface scope ID
        let ipv6_line = "[fe80::1%wlan0]:5555   device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:5";
        let dev_ipv6 = parse_adb_device_line(ipv6_line).expect("Failed to parse IPv6 with scope ID");
        assert_eq!(dev_ipv6.serial, "[fe80::1%wlan0]:5555");
        assert_eq!(dev_ipv6.state, AdbDeviceState::Device);
        assert_eq!(dev_ipv6.transport_type, AdbTransportType::Tcp);
        assert_eq!(dev_ipv6.transport_id, Some(5));

        // IPv6 loopback endpoint
        let ipv6_loopback = "[::1]:5555   device transport_id:6";
        let dev_ipv6_loopback = parse_adb_device_line(ipv6_loopback).expect("Failed to parse IPv6 loopback");
        assert_eq!(dev_ipv6_loopback.serial, "[::1]:5555");
        assert_eq!(dev_ipv6_loopback.state, AdbDeviceState::Device);
        assert_eq!(dev_ipv6_loopback.transport_type, AdbTransportType::Tcp);
        assert_eq!(dev_ipv6_loopback.transport_id, Some(6));

        // State completeness: offline, authorizing, recovery, sideload with tab separators
        let offline_line = "192.168.1.10:5555\toffline\ttransport_id:7";
        let dev_offline = parse_adb_device_line(offline_line).expect("Failed to parse offline line");
        assert_eq!(dev_offline.state, AdbDeviceState::Offline);
        assert_eq!(dev_offline.transport_type, AdbTransportType::Tcp);

        let authorizing_line = "emulator-5556\tauthorizing\ttransport_id:8";
        let dev_auth = parse_adb_device_line(authorizing_line).expect("Failed to parse authorizing line");
        assert_eq!(dev_auth.state, AdbDeviceState::Authorizing);
        assert_eq!(dev_auth.transport_type, AdbTransportType::Emulator);

        let recovery_line = "usb_recovery_device\trecovery\ttransport_id:9";
        let dev_rec = parse_adb_device_line(recovery_line).expect("Failed to parse recovery line");
        assert_eq!(dev_rec.state, AdbDeviceState::Recovery);
        assert_eq!(dev_rec.transport_type, AdbTransportType::Usb);

        let sideload_line = "usb_sideload_device  sideload  transport_id:10";
        let dev_side = parse_adb_device_line(sideload_line).expect("Failed to parse sideload line");
        assert_eq!(dev_side.state, AdbDeviceState::Sideload);
        assert_eq!(dev_side.transport_type, AdbTransportType::Usb);

        // Linux udev missing permissions line
        let noperm_line = "0123456789ABCDEF   no permissions (missing udev rules? user not in plugdev group); see [http://developer.android.com/tools/device.html]";
        let dev_noperm = parse_adb_device_line(noperm_line).expect("Failed to parse no permissions line");
        assert_eq!(dev_noperm.serial, "0123456789ABCDEF");
        assert_eq!(dev_noperm.state, AdbDeviceState::NoPermissions);
        assert_eq!(dev_noperm.transport_type, AdbTransportType::Usb);

        // Daemon output and header lines must be rejected (return None)
        assert!(parse_adb_device_line("List of devices attached").is_none());
        assert!(parse_adb_device_line("List of devices attached   ").is_none());
        assert!(parse_adb_device_line("* daemon not running; starting now at tcp:5037 *").is_none());
        assert!(parse_adb_device_line("* daemon started successfully *").is_none());
        assert!(parse_adb_device_line("error: cannot connect to daemon").is_none());

        // Windows CRLF line endings (\r\n) must not corrupt transport_id parsing
        let win_line = "emulator-5554\tdevice transport_id:1\r\n";
        let dev_win = parse_adb_device_line(win_line).expect("Failed to parse Windows CRLF line");
        assert_eq!(dev_win.serial, "emulator-5554");
        assert_eq!(dev_win.state, AdbDeviceState::Device);
        assert_eq!(dev_win.transport_id, Some(1));

        // Forward compatibility: unknown and future Android states must be captured via Unknown variant
        let unknown_line = "vendor_device_001\tcustom_oem_state model:OEM_Phone transport_id:12\r\n";
        let dev_unknown = parse_adb_device_line(unknown_line).expect("Failed to parse unknown state line");
        assert_eq!(dev_unknown.serial, "vendor_device_001");
        assert_eq!(dev_unknown.state, AdbDeviceState::Unknown("custom_oem_state".to_string()));
        assert_eq!(dev_unknown.model.as_deref(), Some("OEM_Phone"));
        assert_eq!(dev_unknown.transport_id, Some(12));

        // Rescue and fastboot mapping
        let rescue_line = "rescue_device\trescue transport_id:14";
        let dev_rescue = parse_adb_device_line(rescue_line).expect("Failed to parse rescue line");
        assert_eq!(dev_rescue.state, AdbDeviceState::Recovery);

        let fastboot_line = "fastboot_device\tfastboot transport_id:15";
        let dev_fastboot = parse_adb_device_line(fastboot_line).expect("Failed to parse fastboot line");
        assert_eq!(dev_fastboot.state, AdbDeviceState::Bootloader);

        // Incomplete line (< 2 parts) or whitespace only
        assert!(parse_adb_device_line("").is_none());
        assert!(parse_adb_device_line("single_token").is_none());
        assert!(parse_adb_device_line("   \t   \r\n").is_none());
    }

    #[test]
    fn test_adb_abort_handle_send_sync_traits() {
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<AdbAbortHandle>();

        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("Failed to bind ephemeral test listener");
        let addr = listener.local_addr().expect("Failed to get local address");
        let client = TcpStream::connect(addr).expect("Failed to connect client stream");
        let _server = listener.accept().expect("Failed to accept server stream");

        let client_arc = std::sync::Arc::new(client);
        let abort_handle = AdbAbortHandle::new(client_arc.clone());
        let cloned_handle = abort_handle.clone();

        assert!(std::sync::Arc::ptr_eq(
            abort_handle.stream(),
            cloned_handle.stream()
        ));
        assert!(format!("{:?}", abort_handle).contains("AdbAbortHandle"));

        let handle_from: AdbAbortHandle = client_arc.into();
        assert!(std::sync::Arc::ptr_eq(
            abort_handle.stream(),
            handle_from.stream()
        ));
    }

    #[test]
    fn test_adb_abort_handle_interrupts_blocking_read() {
        use std::io::Read;
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::mpsc;
        use std::sync::Arc;
        use std::thread;
        use std::time::{Duration, Instant};

        let listener = TcpListener::bind("127.0.0.1:0")
            .expect("Failed to bind ephemeral test listener");
        let addr = listener.local_addr().expect("Failed to get local address");

        let client = TcpStream::connect(addr).expect("Failed to connect to local test listener");
        let (server_stream, _) = listener.accept().expect("Failed to accept connection");

        // Set a long 10-second timeout to verify that abort preempts it immediately.
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("Failed to set read timeout");

        let client_arc = Arc::new(client);
        let abort_handle = AdbAbortHandle::new(client_arc.clone());

        let (ready_tx, ready_rx) = mpsc::channel();
        let read_finished = Arc::new(AtomicBool::new(false));
        let read_finished_clone = read_finished.clone();

        let reader_handle = thread::spawn(move || {
            let mut buf = [0u8; 128];
            let mut stream_ref = &*client_arc;
            ready_tx.send(()).expect("Failed to signal reader ready");
            // Blocking read: server will not send any data.
            let read_res = stream_ref.read(&mut buf);
            read_finished_clone.store(true, Ordering::SeqCst);
            read_res
        });

        // Wait until the reader thread is actively entering the read syscall
        ready_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Reader failed to signal readiness");
        thread::sleep(Duration::from_millis(25));

        let start_abort = Instant::now();
        abort_handle.abort().expect("Abort call should succeed");

        let reader_result = reader_handle.join().expect("Reader thread panicked");
        let elapsed = start_abort.elapsed();

        // Verification: Blocking read must unblock almost instantly (< 200ms vs 10s timeout).
        assert!(
            elapsed < Duration::from_millis(200),
            "Abort should unblock blocking socket read within 200ms, took {:?}",
            elapsed
        );
        assert!(
            read_finished.load(Ordering::SeqCst),
            "Reader must have finished"
        );

        // Shutdown causes blocking read to return Ok(0) (EOF) or connection reset/aborted error.
        match reader_result {
            Ok(bytes_read) => {
                assert_eq!(bytes_read, 0, "Read after shutdown must return 0 bytes (EOF)");
            }
            Err(e) => {
                assert!(
                    matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::NotConnected
                            | std::io::ErrorKind::BrokenPipe
                            | std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::Interrupted
                    ),
                    "Unexpected I/O error kind after abort: {:?}",
                    e.kind()
                );
            }
        }

        drop(server_stream);
    }

    #[test]
    fn test_adb_abort_handle_try_from_stream_and_repeat_abort() {
        use std::io::Read;
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;
        use std::time::{Duration, Instant};

        let listener = TcpListener::bind("127.0.0.1:0")
            .expect("Failed to bind ephemeral test listener");
        let addr = listener.local_addr().expect("Failed to get local address");

        let client = TcpStream::connect(addr).expect("Failed to connect to local test listener");
        let (server_stream, _) = listener.accept().expect("Failed to accept connection");

        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("Failed to set read timeout");

        let abort_handle =
            AdbAbortHandle::try_from_stream(&client).expect("try_from_stream must succeed");

        let (ready_tx, ready_rx) = mpsc::channel();

        let reader_handle = thread::spawn(move || {
            let mut client = client;
            let mut buf = [0u8; 64];
            ready_tx.send(()).expect("Failed to signal ready");
            client.read(&mut buf)
        });

        ready_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Reader failed to signal readiness");
        thread::sleep(Duration::from_millis(25));

        let start = Instant::now();
        let res1 = abort_handle.abort();
        assert!(res1.is_ok(), "First abort should succeed: {:?}", res1);

        let reader_res = reader_handle.join().expect("Reader thread panicked");
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(200),
            "Abort via cloned handle took too long: {:?}",
            elapsed
        );

        // Repeated abort call should not panic or corrupt state
        let _ = abort_handle.abort();

        drop(server_stream);
        let _ = reader_res;
    }

    #[test]
    fn test_default_connect_timeout_constant() {
        assert_eq!(DEFAULT_CONNECT_TIMEOUT, Duration::from_millis(500));
    }
}
