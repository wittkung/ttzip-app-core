// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![forbid(unsafe_code)]

//! AOSP ADB TCP 5037 Smart Socket client.
//!
//! Directly speaks the standard AOSP Smart Socket protocol over TCP port 5037
//! (`localhost:5037`), bypassing legacy sub-process spawning and proprietary USB protocols.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Default local ADB daemon TCP port.
pub const DEFAULT_ADB_DAEMON_PORT: u16 = 5037;

/// Default connect and I/O timeout for local smart socket operations.
pub const DEFAULT_SOCKET_TIMEOUT: Duration = Duration::from_millis(500);

/// Encodes a command into standard 4-byte hexadecimal length-prefixed AOSP wire format.
#[inline]
pub fn encode_request(command: &str) -> Vec<u8> {
    let len = command.len();
    let mut payload = Vec::with_capacity(4 + len);
    let hex_prefix = format!("{:04x}", len);
    payload.extend_from_slice(hex_prefix.as_bytes());
    payload.extend_from_slice(command.as_bytes());
    payload
}

/// Reads and validates standard 4-byte AOSP response status (`OKAY` or `FAIL`).
pub fn read_status_from<R: Read>(reader: &mut R) -> std::io::Result<()> {
    let mut status = [0u8; 4];
    reader.read_exact(&mut status)?;
    if &status == b"OKAY" {
        Ok(())
    } else if &status == b"FAIL" {
        let mut len_hex = [0u8; 4];
        reader.read_exact(&mut len_hex)?;
        let err_len_str = std::str::from_utf8(&len_hex).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 in error length hex prefix: {}", e),
            )
        })?;
        let err_len = usize::from_str_radix(err_len_str, 16).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid hex in error length prefix '{}': {}", err_len_str, e),
            )
        })?;
        let mut err_msg_buf = vec![0u8; err_len];
        reader.read_exact(&mut err_msg_buf)?;
        let err_msg = String::from_utf8_lossy(&err_msg_buf).to_string();
        Err(std::io::Error::other(format!(
            "ADB server returned FAIL: {}",
            err_msg
        )))
    } else {
        let raw = String::from_utf8_lossy(&status);
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid ADB status header: expected 'OKAY' or 'FAIL', got '{}'", raw),
        ))
    }
}

/// Reads a 4-byte hex-prefixed payload string following an `OKAY` status.
pub fn read_hex_prefixed_payload<R: Read>(reader: &mut R) -> std::io::Result<String> {
    let mut len_hex = [0u8; 4];
    reader.read_exact(&mut len_hex)?;
    let len_str = std::str::from_utf8(&len_hex).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid UTF-8 in hex payload length prefix: {}", e),
        )
    })?;
    let len = usize::from_str_radix(len_str, 16).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid hex payload length prefix '{}': {}", len_str, e),
        )
    })?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid UTF-8 in ADB payload body: {}", e),
        )
    })
}

/// Parses the raw text output of `host:devices` into `(serial, state)` tuples.
pub fn parse_device_list(output: &str) -> Vec<(String, String)> {
    let mut devices = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        if let (Some(serial), Some(state)) = (parts.next(), parts.next()) {
            devices.push((serial.to_string(), state.to_string()));
        }
    }
    devices
}

/// AOSP ADB Smart Socket client managing command transport over TCP stream `127.0.0.1:5037`.
#[derive(Debug)]
pub struct AdbSmartSocketClient<S = TcpStream> {
    stream: S,
}

impl AdbSmartSocketClient<TcpStream> {
    /// Connects to the local ADB daemon at `127.0.0.1:5037`.
    pub fn connect() -> std::io::Result<Self> {
        let addr: SocketAddr = format!("127.0.0.1:{}", DEFAULT_ADB_DAEMON_PORT)
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        Self::connect_timeout(addr, DEFAULT_SOCKET_TIMEOUT)
    }

    /// Connects to a custom socket target.
    pub fn connect_to(addr: impl ToSocketAddrs) -> std::io::Result<Self> {
        let socket = TcpStream::connect(addr)?;
        socket.set_read_timeout(Some(DEFAULT_SOCKET_TIMEOUT))?;
        socket.set_write_timeout(Some(DEFAULT_SOCKET_TIMEOUT))?;
        Ok(Self { stream: socket })
    }

    /// Connects to a specific socket address with explicit connection timeout.
    pub fn connect_timeout(addr: SocketAddr, timeout: Duration) -> std::io::Result<Self> {
        let socket = TcpStream::connect_timeout(&addr, timeout)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        Ok(Self { stream: socket })
    }

    /// Connects to the local ADB daemon (`127.0.0.1:5037`) and lists connected devices.
    pub fn list_devices() -> std::io::Result<Vec<(String, String)>> {
        let addr = format!("127.0.0.1:{}", DEFAULT_ADB_DAEMON_PORT);
        Self::list_devices_from(addr)
    }

    /// Connects to a specified ADB daemon socket and queries `host:devices`.
    pub fn list_devices_from(addr: impl ToSocketAddrs) -> std::io::Result<Vec<(String, String)>> {
        let mut client = Self::connect_to(addr)?;
        client.send_command("host:devices")?;
        client.read_status()?;
        let payload = client.read_payload()?;
        Ok(parse_device_list(&payload))
    }
}

impl<S: Read + Write> AdbSmartSocketClient<S> {
    /// Wraps an existing stream into an ADB smart socket client.
    #[inline]
    pub const fn from_stream(stream: S) -> Self {
        Self { stream }
    }

    /// Sends a length-prefixed service command string over the wire.
    pub fn send_command(&mut self, command: &str) -> std::io::Result<()> {
        let packet = encode_request(command);
        self.stream.write_all(&packet)?;
        self.stream.flush()
    }

    /// Reads 4-byte response status (`OKAY` or `FAIL`).
    #[inline]
    pub fn read_status(&mut self) -> std::io::Result<()> {
        read_status_from(&mut self.stream)
    }

    /// Reads 4-byte hex length-prefixed payload body.
    #[inline]
    pub fn read_payload(&mut self) -> std::io::Result<String> {
        read_hex_prefixed_payload(&mut self.stream)
    }

    /// Switches the transport of the active smart socket to target a specific device serial.
    ///
    /// Sends `host:transport:<serial>` and verifies the `OKAY` confirmation.
    pub fn switch_transport(&mut self, serial: &str) -> std::io::Result<()> {
        let cmd = format!("host:transport:{}", serial);
        self.send_command(&cmd)?;
        self.read_status()
    }

    /// Borrows the underlying stream.
    #[inline]
    pub fn stream(&self) -> &S {
        &self.stream
    }

    /// Mutably borrows the underlying stream.
    #[inline]
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// Unwraps the client and yields the underlying stream.
    #[inline]
    pub fn into_inner(self) -> S {
        self.stream
    }
}

// Compile-time static gate ensuring AdbSmartSocketClient satisfies Send and Sync invariants.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AdbSmartSocketClient>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn test_encode_request() {
        assert_eq!(encode_request("host:devices"), b"000chost:devices");
        assert_eq!(
            encode_request("host:transport:serial123"),
            b"0018host:transport:serial123"
        );
    }

    #[test]
    fn test_read_status_okay() {
        let mut cursor = Cursor::new(b"OKAYrest");
        assert!(read_status_from(&mut cursor).is_ok());
        assert_eq!(cursor.position(), 4);
    }

    #[test]
    fn test_read_status_fail() {
        let mut cursor = Cursor::new(b"FAIL000edevice offline");
        let err = read_status_from(&mut cursor).unwrap_err();
        assert!(err.to_string().contains("device offline"));
    }

    #[test]
    fn test_read_status_invalid() {
        let mut cursor = Cursor::new(b"WHAT");
        let err = read_status_from(&mut cursor).unwrap_err();
        assert!(err.to_string().contains("Invalid ADB status header"));
    }

    #[test]
    fn test_read_hex_prefixed_payload() {
        let mut cursor = Cursor::new(b"000bhello world");
        let payload = read_hex_prefixed_payload(&mut cursor).unwrap();
        assert_eq!(payload, "hello world");
    }

    #[test]
    fn test_parse_device_list() {
        let sample = "emulator-5554\tdevice\n# comment line\nRF8M123456\tunauthorized\n\n";
        let parsed = parse_device_list(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], ("emulator-5554".to_string(), "device".to_string()));
        assert_eq!(parsed[1], ("RF8M123456".to_string(), "unauthorized".to_string()));
    }

    #[test]
    fn test_switch_transport_and_list_devices_mock_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");

        let handle = thread::spawn(move || {
            // First client: list_devices
            if let Ok((mut socket, _)) = listener.accept() {
                let mut req = [0u8; 16];
                let _ = socket.read_exact(&mut req);
                assert_eq!(&req, b"000chost:devices");
                // Respond OKAY + length + list
                let body = "device123\tdevice\n";
                let resp = format!("OKAY{:04x}{}", body.len(), body);
                socket.write_all(resp.as_bytes()).unwrap();
            }

            // Second client: switch_transport
            if let Ok((mut socket, _)) = listener.accept() {
                let mut req = [0u8; 25];
                let _ = socket.read_exact(&mut req);
                assert_eq!(&req, b"0015host:transport:dev001");
                socket.write_all(b"OKAY").unwrap();
            }
        });

        // Test list_devices_from
        let devices = AdbSmartSocketClient::list_devices_from(addr).expect("list devices");
        assert_eq!(devices, vec![("device123".to_string(), "device".to_string())]);

        // Test switch_transport
        let mut client = AdbSmartSocketClient::connect_to(addr).expect("connect");
        client.switch_transport("dev001").expect("switch transport");

        handle.join().expect("server thread completes");
    }
}
