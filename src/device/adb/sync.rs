// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! ADB SYNC service client protocol stream (STAT, LIST, RECV, SEND).

use std::io::{Read, Write};
use bytes::{Buf, Bytes, BytesMut};

use crate::device::adb::types::AdbSyncEntry;
use crate::error::AppCoreError;

pub const SYNC_HEADER_SIZE: usize = 8;
pub const STAT_RESPONSE_SIZE: usize = 16;
pub const DENT_HEADER_SIZE: usize = 20;
pub const SYNC_MAX_DATA_CHUNK: usize = 65536;

pub mod sync_cmd {
    pub const STAT: &[u8; 4] = b"STAT";
    pub const LIST: &[u8; 4] = b"LIST";
    pub const DENT: &[u8; 4] = b"DENT";
    pub const RECV: &[u8; 4] = b"RECV";
    pub const SEND: &[u8; 4] = b"SEND";
    pub const DATA: &[u8; 4] = b"DATA";
    pub const DONE: &[u8; 4] = b"DONE";
    pub const OKAY: &[u8; 4] = b"OKAY";
    pub const FAIL: &[u8; 4] = b"FAIL";
    pub const QUIT: &[u8; 4] = b"QUIT";
}

/// Fixed 8-byte sync protocol header containing a 4-byte command ID and a 32-bit LE argument/length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AdbSyncHeader {
    pub id: [u8; 4],
    pub arg: u32,
}

impl AdbSyncHeader {
    #[inline]
    #[must_use]
    pub const fn new(id: [u8; 4], arg: u32) -> Self {
        Self { id, arg }
    }

    /// Serializes the 8-byte header into an immutable fixed array.
    /// Compiles into vectorized 64-bit store instructions.
    #[inline]
    #[must_use]
    pub fn to_bytes(&self) -> [u8; SYNC_HEADER_SIZE] {
        let parts = [self.id, self.arg.to_le_bytes()];
        let mut out = [0u8; SYNC_HEADER_SIZE];
        out.copy_from_slice(parts.as_flattened());
        out
    }

    /// Parses an 8-byte sync header from a byte slice with Bounds-Check Elimination (BCE).
    #[inline]
    pub fn parse(buf: &[u8]) -> Result<Self, AdbSyncError> {
        if buf.len() < SYNC_HEADER_SIZE {
            return Err(AdbSyncError::BufferTooSmall { expected: SYNC_HEADER_SIZE, actual: buf.len() });
        }
        let header = &buf[..SYNC_HEADER_SIZE];
        let id = [header[0], header[1], header[2], header[3]];
        let arg = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        Ok(Self { id, arg })
    }
}

/// Strongly typed ADB SYNC request message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbSyncRequest<'a> {
    Stat { path: &'a str },
    List { path: &'a str },
    Recv { path: &'a str },
    Send { path: &'a str, mode: u32 },
    Data { payload: &'a [u8] },
    Done { mtime: u32 },
    Quit,
}

impl<'a> AdbSyncRequest<'a> {
    /// Encodes the sync request into a contiguous byte buffer.
    /// Employs declarative vectorized header serialization and fast-path bypass for 0-byte payloads.
    pub fn encode(&self, dst: &mut BytesMut) {
        match self {
            Self::Stat { path } => Self::encode_path(sync_cmd::STAT, path, dst),
            Self::List { path } => Self::encode_path(sync_cmd::LIST, path, dst),
            Self::Recv { path } => Self::encode_path(sync_cmd::RECV, path, dst),
            Self::Send { path, mode } => Self::encode_path(sync_cmd::SEND, &format!("{path},{mode}"), dst),
            Self::Data { payload } => {
                let len = payload.len() as u32;
                dst.reserve(SYNC_HEADER_SIZE + payload.len());
                dst.extend_from_slice([*sync_cmd::DATA, len.to_le_bytes()].as_flattened());
                if !payload.is_empty() {
                    dst.extend_from_slice(payload);
                }
            }
            Self::Done { mtime } => Self::encode_hdr(sync_cmd::DONE, *mtime, dst),
            Self::Quit => Self::encode_hdr(sync_cmd::QUIT, 0, dst),
        }
    }

    #[inline]
    fn encode_hdr(id: &[u8; 4], arg: u32, dst: &mut BytesMut) {
        dst.reserve(SYNC_HEADER_SIZE);
        dst.extend_from_slice([*id, arg.to_le_bytes()].as_flattened());
    }

    #[inline]
    fn encode_path(id: &[u8; 4], path: &str, dst: &mut BytesMut) {
        let bytes = path.as_bytes();
        dst.reserve(SYNC_HEADER_SIZE + bytes.len());
        dst.extend_from_slice([*id, (bytes.len() as u32).to_le_bytes()].as_flattened());
        if !bytes.is_empty() {
            dst.extend_from_slice(bytes);
        }
    }
}

/// Helper that extracts mode, size, and mtime fields with Bounds-Check Elimination (BCE).
#[inline]
fn parse_stat_fields(buf: &[u8]) -> (u32, u64, u32) {
    let mode = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let size = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]) as u64;
    let mtime = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
    (mode, size, mtime)
}

/// Strongly typed ADB SYNC server response frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbSyncResponse {
    Stat { mode: u32, size: u64, mtime: u32 },
    Dent { mode: u32, size: u64, mtime: u32, name: String },
    Data { payload: Bytes },
    Done { arg: u32 },
    Okay { arg: u32 },
    Fail { message: String },
}

impl AdbSyncResponse {
    /// Attempts to parse a complete response frame from a `BytesMut` buffer.
    /// Fixed-size headers are sliced with `&src[..HEADER_SIZE]`, eliminating all inner bounds checks.
    pub fn parse_frame_zero_copy(src: &mut BytesMut) -> Result<Option<Self>, AdbSyncError> {
        if src.len() < SYNC_HEADER_SIZE {
            return Ok(None);
        }
        let header = &src[..SYNC_HEADER_SIZE];
        let id = [header[0], header[1], header[2], header[3]];
        let arg = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);

        match &id {
            id if id == sync_cmd::STAT => {
                if src.len() < STAT_RESPONSE_SIZE {
                    src.reserve(STAT_RESPONSE_SIZE - src.len());
                    return Ok(None);
                }
                let (mode, size, mtime) = parse_stat_fields(&src[..STAT_RESPONSE_SIZE]);
                src.advance(STAT_RESPONSE_SIZE);
                Ok(Some(Self::Stat { mode, size, mtime }))
            }
            id if id == sync_cmd::DENT => {
                if src.len() < DENT_HEADER_SIZE {
                    src.reserve(DENT_HEADER_SIZE - src.len());
                    return Ok(None);
                }
                let (mode, size, mtime) = parse_stat_fields(&src[..DENT_HEADER_SIZE]);
                let namelen = u32::from_le_bytes([src[16], src[17], src[18], src[19]]) as usize;
                let total = DENT_HEADER_SIZE.checked_add(namelen).ok_or(AdbSyncError::PayloadTooLarge {
                    max: usize::MAX, actual: namelen,
                })?;
                if src.len() < total {
                    src.reserve(total - src.len());
                    return Ok(None);
                }
                src.advance(DENT_HEADER_SIZE);
                let name = String::from_utf8_lossy(&src.split_to(namelen)).to_string();
                Ok(Some(Self::Dent { mode, size, mtime, name }))
            }
            id if id == sync_cmd::DATA => {
                let chunk_len = arg as usize;
                let total = SYNC_HEADER_SIZE.checked_add(chunk_len).ok_or(AdbSyncError::PayloadTooLarge {
                    max: usize::MAX, actual: chunk_len,
                })?;
                if src.len() < total {
                    src.reserve(total - src.len());
                    return Ok(None);
                }
                src.advance(SYNC_HEADER_SIZE);
                let payload = if chunk_len == 0 { Bytes::new() } else { src.split_to(chunk_len).freeze() };
                Ok(Some(Self::Data { payload }))
            }
            id if id == sync_cmd::DONE => {
                let adv = if src.len() >= 20 && src[8..20].iter().all(|&b| b == 0) { 20 } else { SYNC_HEADER_SIZE };
                src.advance(adv);
                Ok(Some(Self::Done { arg }))
            }
            id if id == sync_cmd::OKAY => { src.advance(SYNC_HEADER_SIZE); Ok(Some(Self::Okay { arg })) }
            id if id == sync_cmd::FAIL => {
                let msg_len = arg as usize;
                let total = SYNC_HEADER_SIZE.checked_add(msg_len).ok_or(AdbSyncError::PayloadTooLarge {
                    max: usize::MAX, actual: msg_len,
                })?;
                if src.len() < total {
                    src.reserve(total - src.len());
                    return Ok(None);
                }
                src.advance(SYNC_HEADER_SIZE);
                let message = String::from_utf8_lossy(&src.split_to(msg_len)).to_string();
                Ok(Some(Self::Fail { message }))
            }
            id if (id[0] == 0x01 || id[0] == 0x02) && id[3] == 0 => {
                let msg_len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
                let total = 5usize.checked_add(msg_len).ok_or(AdbSyncError::PayloadTooLarge {
                    max: usize::MAX, actual: msg_len,
                })?;
                if src.len() < total {
                    src.reserve(total - src.len());
                    return Ok(None);
                }
                src.advance(5);
                let message = String::from_utf8_lossy(&src.split_to(msg_len)).trim().to_string();
                Ok(Some(Self::Fail { message }))
            }
            _ => Err(AdbSyncError::UnexpectedResponse {
                operation: "parse_frame",
                expected: "STAT, DENT, DATA, DONE, OKAY, or FAIL",
                actual: String::from_utf8_lossy(&id).to_string(),
            }),
        }
    }
}

/// Client driver executing atomic ADB file operations over an active sync stream.
pub struct AdbSyncStream<S = std::net::TcpStream> {
    stream: S,
    read_buf: BytesMut,
}

impl<S> AdbSyncStream<S> {
    pub fn new(stream: S) -> Self { Self { stream, read_buf: BytesMut::new() } }
    pub fn with_capacity(stream: S, capacity: usize) -> Self { Self { stream, read_buf: BytesMut::with_capacity(capacity) } }
    #[inline] pub fn get_ref(&self) -> &S { &self.stream }
    #[inline] pub fn get_mut(&mut self) -> &mut S { &mut self.stream }
    #[inline] pub fn into_inner(self) -> S { self.stream }
    #[inline] pub fn buffer(&self) -> &[u8] { &self.read_buf }
    #[inline] pub fn buffer_mut(&mut self) -> &mut BytesMut { &mut self.read_buf }
}

impl<S: Read + Write> AdbSyncStream<S> {
    /// Reads from the underlying stream until a full sync response frame is decoded.
    pub fn read_response(&mut self) -> Result<AdbSyncResponse, AdbSyncError> {
        let mut chunk = [0u8; 4096];
        loop {
            if let Some(resp) = AdbSyncResponse::parse_frame_zero_copy(&mut self.read_buf)? {
                return Ok(resp);
            }
            let n = self.stream.read(&mut chunk)?;
            if n == 0 {
                let actual = self.read_buf.len();
                return if actual < SYNC_HEADER_SIZE {
                    Err(AdbSyncError::BufferTooSmall { expected: SYNC_HEADER_SIZE, actual })
                } else {
                    Err(AdbSyncError::PacketUnderflow { expected: SYNC_HEADER_SIZE + 1, actual })
                };
            }
            self.read_buf.extend_from_slice(&chunk[..n]);
        }
    }

    fn send_msg(&mut self, id: &[u8; 4], arg: u32, payload: &[u8]) -> Result<(), std::io::Error> {
        let mut req = BytesMut::with_capacity(SYNC_HEADER_SIZE + payload.len());
        req.extend_from_slice([*id, arg.to_le_bytes()].as_flattened());
        if !payload.is_empty() {
            req.extend_from_slice(payload);
        }
        self.stream.write_all(&req)?;
        self.stream.flush()
    }

    /// Queries POSIX file attributes of remote path on the Android device.
    pub fn stat(&mut self, remote_path: &str) -> Result<AdbSyncEntry, AppCoreError> {
        let bytes = remote_path.as_bytes();
        self.send_msg(sync_cmd::STAT, bytes.len() as u32, bytes)?;

        match self.read_response()? {
            AdbSyncResponse::Stat { mode, size, mtime } => {
                let name = remote_path.rsplit('/').next().unwrap_or(remote_path).to_string();
                Ok(AdbSyncEntry { name, mode, size, mtime })
            }
            AdbSyncResponse::Fail { message } => Err(AdbSyncError::RemoteFailure(message).into()),
            other => Err(AdbSyncError::UnexpectedResponse {
                operation: "STAT", expected: "STAT", actual: format!("{other:?}"),
            }.into()),
        }
    }

    /// Lists directory contents on the remote Android device.
    pub fn list(&mut self, remote_path: &str) -> Result<Vec<AdbSyncEntry>, AppCoreError> {
        let bytes = remote_path.as_bytes();
        self.send_msg(sync_cmd::LIST, bytes.len() as u32, bytes)?;

        let mut entries = Vec::new();
        loop {
            match self.read_response()? {
                AdbSyncResponse::Dent { mode, size, mtime, name } => {
                    if name != "." && name != ".." {
                        entries.push(AdbSyncEntry { name, mode, size, mtime });
                    }
                }
                AdbSyncResponse::Done { .. } => {
                    if self.read_buf.len() >= 12 && self.read_buf[..12].iter().all(|&b| b == 0) {
                        self.read_buf.advance(12);
                    }
                    break;
                }
                AdbSyncResponse::Fail { message } => return Err(AdbSyncError::RemoteFailure(message).into()),
                other => return Err(AdbSyncError::UnexpectedResponse {
                    operation: "LIST", expected: "DENT or DONE", actual: format!("{other:?}"),
                }.into()),
            }
        }
        Ok(entries)
    }

    /// Pulls a file from the Android device into memory.
    pub fn recv_file(&mut self, remote_path: &str) -> Result<Vec<u8>, AppCoreError> {
        let bytes = remote_path.as_bytes();
        self.send_msg(sync_cmd::RECV, bytes.len() as u32, bytes)?;

        let mut data = Vec::new();
        loop {
            match self.read_response()? {
                AdbSyncResponse::Data { payload } => {
                    if !payload.is_empty() {
                        data.extend_from_slice(&payload);
                    }
                }
                AdbSyncResponse::Done { .. } => break,
                AdbSyncResponse::Fail { message } => return Err(AdbSyncError::RemoteFailure(message).into()),
                other => return Err(AdbSyncError::UnexpectedResponse {
                    operation: "RECV", expected: "DATA or DONE", actual: format!("{other:?}"),
                }.into()),
            }
        }
        Ok(data)
    }

    /// Pushes a file payload to the remote Android device.
    pub fn send_file(&mut self, remote_path: &str, payload: &[u8], posix_mode: u32, mtime: u32) -> Result<(), AppCoreError> {
        let send_arg = format!("{remote_path},{posix_mode}");
        let arg_bytes = send_arg.as_bytes();
        self.send_msg(sync_cmd::SEND, arg_bytes.len() as u32, arg_bytes)?;

        for chunk in payload.chunks(SYNC_MAX_DATA_CHUNK) {
            let mut chunk_buf = BytesMut::with_capacity(SYNC_HEADER_SIZE + chunk.len());
            chunk_buf.extend_from_slice([*sync_cmd::DATA, (chunk.len() as u32).to_le_bytes()].as_flattened());
            if !chunk.is_empty() {
                chunk_buf.extend_from_slice(chunk);
            }
            self.stream.write_all(&chunk_buf)?;
        }

        self.stream.write_all([*sync_cmd::DONE, mtime.to_le_bytes()].as_flattened())?;
        self.stream.flush()?;

        match self.read_response()? {
            AdbSyncResponse::Okay { .. } => Ok(()),
            AdbSyncResponse::Fail { message } => Err(AdbSyncError::RemoteFailure(message).into()),
            other => Err(AdbSyncError::UnexpectedResponse {
                operation: "SEND", expected: "OKAY", actual: format!("{other:?}"),
            }.into()),
        }
    }

    /// Closes sync session cleanly.
    pub fn quit(&mut self) -> Result<(), AppCoreError> {
        self.stream.write_all([*sync_cmd::QUIT, 0u32.to_le_bytes()].as_flattened())?;
        self.stream.flush()?;
        Ok(())
    }
}

impl<S: Read> Read for AdbSyncStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.read_buf.is_empty() {
            let to_copy = std::cmp::min(self.read_buf.len(), buf.len());
            buf[..to_copy].copy_from_slice(&self.read_buf.split_to(to_copy));
            return Ok(to_copy);
        }
        self.stream.read(buf)
    }
}

impl<S: Write> Write for AdbSyncStream<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

/// Strongly-typed error cases for ADB file synchronization protocol sessions.
#[derive(Debug, thiserror::Error)]
pub enum AdbSyncError {
    #[error("Sync buffer too small: {actual} bytes (minimum {expected} bytes required)")]
    BufferTooSmall { expected: usize, actual: usize },

    #[error("Incomplete sync frame: expected {expected} bytes, received {actual} bytes")]
    PacketUnderflow { expected: usize, actual: usize },

    #[error("Sync payload too large: {actual} bytes (maximum {max} bytes permitted)")]
    PayloadTooLarge { max: usize, actual: usize },

    #[error("Unexpected response ID in {operation}: expected {expected:?}, got {actual:?}")]
    UnexpectedResponse { operation: &'static str, expected: &'static str, actual: String },

    #[error("Sync operation failed on remote device: {0}")]
    RemoteFailure(String),

    #[error("Sync I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl Clone for AdbSyncError {
    fn clone(&self) -> Self {
        match self {
            Self::BufferTooSmall { expected, actual } => Self::BufferTooSmall { expected: *expected, actual: *actual },
            Self::PacketUnderflow { expected, actual } => Self::PacketUnderflow { expected: *expected, actual: *actual },
            Self::PayloadTooLarge { max, actual } => Self::PayloadTooLarge { max: *max, actual: *actual },
            Self::UnexpectedResponse { operation, expected, actual } => Self::UnexpectedResponse {
                operation, expected, actual: actual.clone(),
            },
            Self::RemoteFailure(msg) => Self::RemoteFailure(msg.clone()),
            Self::Io(err) => Self::Io(std::io::Error::new(err.kind(), err.to_string())),
        }
    }
}

impl PartialEq for AdbSyncError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::BufferTooSmall { expected: e1, actual: a1 }, Self::BufferTooSmall { expected: e2, actual: a2 }) => e1 == e2 && a1 == a2,
            (Self::PacketUnderflow { expected: e1, actual: a1 }, Self::PacketUnderflow { expected: e2, actual: a2 }) => e1 == e2 && a1 == a2,
            (Self::PayloadTooLarge { max: m1, actual: a1 }, Self::PayloadTooLarge { max: m2, actual: a2 }) => m1 == m2 && a1 == a2,
            (Self::UnexpectedResponse { operation: o1, expected: e1, actual: a1 }, Self::UnexpectedResponse { operation: o2, expected: e2, actual: a2 }) => o1 == o2 && e1 == e2 && a1 == a2,
            (Self::RemoteFailure(m1), Self::RemoteFailure(m2)) => m1 == m2,
            (Self::Io(err1), Self::Io(err2)) => err1.kind() == err2.kind() && err1.to_string() == err2.to_string(),
            _ => false,
        }
    }
}

impl Eq for AdbSyncError {}

// Compile-time gates ensuring types satisfy concurrency bounds and zero-cost abstraction constraints.
const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    const fn assert_unpin<T: Unpin>() {}
    const fn assert_clone<T: Clone>() {}
    const fn assert_eq<T: Eq>() {}

    assert_send_sync::<AdbSyncHeader>();
    assert_send_sync::<AdbSyncResponse>();
    assert_send_sync::<AdbSyncError>();
    assert_unpin::<AdbSyncHeader>();
    assert_clone::<AdbSyncHeader>();
    assert_eq::<AdbSyncHeader>();
    assert_clone::<AdbSyncError>();
    assert_eq::<AdbSyncError>();

    assert!(std::mem::size_of::<AdbSyncHeader>() == 8 && std::mem::align_of::<AdbSyncHeader>() == 4);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_header_immutable_serialization_and_bce() {
        let header = AdbSyncHeader::new(*sync_cmd::DATA, 1024);
        let bytes = header.to_bytes();
        assert_eq!(bytes, [b'D', b'A', b'T', b'A', 0x00, 0x04, 0x00, 0x00]);

        let parsed = AdbSyncHeader::parse(&bytes).expect("parsing valid 8-byte header must succeed");
        assert_eq!(parsed, header);
        assert_eq!(parsed.id, *sync_cmd::DATA);
        assert_eq!(parsed.arg, 1024);

        let truncated = &bytes[..7];
        let err = AdbSyncHeader::parse(truncated).expect_err("sub-8-byte buffer must fail");
        assert_eq!(err, AdbSyncError::BufferTooSmall { expected: 8, actual: 7 });
    }

    #[test]
    fn test_sync_request_declarative_encoding_and_fast_path() {
        let mut buf = BytesMut::new();

        AdbSyncRequest::Data { payload: &[] }.encode(&mut buf);
        assert_eq!(buf.len(), SYNC_HEADER_SIZE);
        assert_eq!(&buf[..4], sync_cmd::DATA);
        assert_eq!(u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]), 0);
        buf.clear();

        let sample = b"hello adb sync";
        AdbSyncRequest::Data { payload: sample }.encode(&mut buf);
        assert_eq!(buf.len(), SYNC_HEADER_SIZE + sample.len());
        assert_eq!(&buf[SYNC_HEADER_SIZE..], sample);
        buf.clear();

        AdbSyncRequest::Stat { path: "/sdcard/foo" }.encode(&mut buf);
        assert_eq!(buf.len(), SYNC_HEADER_SIZE + 11);
        assert_eq!(&buf[..4], sync_cmd::STAT);
        buf.clear();

        AdbSyncRequest::Done { mtime: 123456 }.encode(&mut buf);
        assert_eq!(buf.len(), SYNC_HEADER_SIZE);
        assert_eq!(&buf[..4], sync_cmd::DONE);
        assert_eq!(u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]), 123456);
        buf.clear();

        AdbSyncRequest::Quit.encode(&mut buf);
        assert_eq!(buf.len(), SYNC_HEADER_SIZE);
        assert_eq!(&buf[..4], sync_cmd::QUIT);
    }

    #[test]
    fn test_sync_response_parse_frame_zero_copy_roundtrip() {
        let mut buf = BytesMut::new();

        let stat_bytes = [*b"STAT", 0o100644u32.to_le_bytes(), 4096u32.to_le_bytes(), 500000u32.to_le_bytes()];
        buf.extend_from_slice(stat_bytes.as_flattened());
        let resp = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        assert_eq!(resp, AdbSyncResponse::Stat { mode: 0o100644, size: 4096, mtime: 500000 });
        assert!(buf.is_empty());

        let dent = [b"DENT".as_slice(), &0o040777u32.to_le_bytes(), &0u32.to_le_bytes(), &0u32.to_le_bytes(), &4u32.to_le_bytes(), b"test"].concat();
        buf.extend_from_slice(&dent);
        let resp = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        assert_eq!(resp, AdbSyncResponse::Dent { mode: 0o040777, size: 0, mtime: 0, name: "test".to_string() });
        assert!(buf.is_empty());

        let fail = [b"FAIL".as_slice(), &5u32.to_le_bytes(), b"error"].concat();
        buf.extend_from_slice(&fail);
        let resp = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        assert_eq!(resp, AdbSyncResponse::Fail { message: "error".to_string() });
        assert!(buf.is_empty());
    }

    #[test]
    fn test_zero_payload_sync_frame() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[b'D', b'A', b'T', b'A', 0, 0, 0, 0]);
        let resp = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        assert_eq!(resp, AdbSyncResponse::Data { payload: Bytes::new() });
        assert!(buf.is_empty());

        // Standard 8-byte DONE
        buf.extend_from_slice(&[b'D', b'O', b'N', b'E', 0, 0, 0, 0]);
        let resp = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        assert_eq!(resp, AdbSyncResponse::Done { arg: 0 });
        assert!(buf.is_empty());

        // Legacy AOSP v1 20-byte DONE with 12 trailing zero bytes
        buf.extend_from_slice(&[b'D', b'O', b'N', b'E', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let resp = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        assert_eq!(resp, AdbSyncResponse::Done { arg: 0 });
        assert!(buf.is_empty());

        // Standard 8-byte DONE followed by subsequent command
        buf.extend_from_slice(&[b'D', b'O', b'N', b'E', 0, 0, 0, 0, b'O', b'K', b'A', b'Y', 0, 0, 0, 0]);
        let resp = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        assert_eq!(resp, AdbSyncResponse::Done { arg: 0 });
        assert_eq!(buf.len(), 8);
        let resp2 = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        assert_eq!(resp2, AdbSyncResponse::Okay { arg: 0 });
        assert!(buf.is_empty());
    }

    #[test]
    fn test_single_byte_incremental_stream_slicing() {
        let dent_frame = [b"DENT".as_slice(), &0o100755u32.to_le_bytes(), &42u32.to_le_bytes(), &1u32.to_le_bytes(), &5u32.to_le_bytes(), b"file1"].concat();
        let total_len = dent_frame.len();
        let mut buf = BytesMut::new();

        for (i, &byte) in dent_frame.iter().enumerate() {
            buf.extend_from_slice(&[byte]);
            let res = AdbSyncResponse::parse_frame_zero_copy(&mut buf).expect("partial frame parse must not error");
            if i + 1 < total_len {
                assert!(res.is_none(), "frame must not be complete at index {i}");
                assert_eq!(buf.len(), i + 1, "partial buffer must not be drained");
            } else {
                let frame = res.expect("frame must be complete on final byte");
                assert_eq!(frame, AdbSyncResponse::Dent { mode: 0o100755, size: 42, mtime: 1, name: "file1".to_string() });
                assert!(buf.is_empty(), "buffer must be fully drained on completion");
            }
        }
    }

    #[test]
    fn test_extreme_payload_boundaries() {
        let payload = vec![0xAB; SYNC_MAX_DATA_CHUNK];
        let mut buf = BytesMut::new();
        AdbSyncRequest::Data { payload: &payload }.encode(&mut buf);
        assert_eq!(buf.len(), SYNC_HEADER_SIZE + SYNC_MAX_DATA_CHUNK);

        let resp = AdbSyncResponse::parse_frame_zero_copy(&mut buf).unwrap().unwrap();
        match resp {
            AdbSyncResponse::Data { payload: decoded } => {
                assert_eq!(decoded.len(), SYNC_MAX_DATA_CHUNK);
                assert_eq!(&decoded[..], &payload[..]);
            }
            other => panic!("expected DATA frame, got {:?}", other),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn test_sync_stream_mock_data_roundtrip() {
        use std::os::unix::net::UnixStream;
        let (client_io, mut server_io) = UnixStream::pair().expect("UnixStream pair");
        let mut stream = AdbSyncStream::new(client_io);

        let header = [*sync_cmd::DATA, 10u32.to_le_bytes()];
        server_io.write_all(header.as_flattened()).unwrap();
        server_io.write_all(b"abcdefghij").unwrap();
        server_io.write_all([*sync_cmd::DONE, 0u32.to_le_bytes()].as_flattened()).unwrap();

        match stream.read_response().unwrap() {
            AdbSyncResponse::Data { payload } => assert_eq!(&payload[..], b"abcdefghij"),
            other => panic!("expected DATA frame, got {:?}", other),
        }
        match stream.read_response().unwrap() {
            AdbSyncResponse::Done { .. } => {}
            other => panic!("expected DONE frame, got {:?}", other),
        }
        assert!(stream.buffer().is_empty());
    }

    #[test]
    fn test_sync_stream_full_lifecycle_mock() {
        use std::os::unix::net::UnixStream;
        let (client_io, mut server_io) = UnixStream::pair().expect("UnixStream pair");
        let mut stream = AdbSyncStream::new(client_io);

        let handle = std::thread::spawn(move || {
            // 1. STAT
            let mut req = [0u8; SYNC_HEADER_SIZE + 12];
            server_io.read_exact(&mut req).unwrap();
            let stat_resp = [*b"STAT", 0o100644u32.to_le_bytes(), 8192u32.to_le_bytes(), 5u32.to_le_bytes()];
            server_io.write_all(stat_resp.as_flattened()).unwrap();

            // 2. LIST
            let mut l_req = [0u8; SYNC_HEADER_SIZE + 7];
            server_io.read_exact(&mut l_req).unwrap();
            let dent = [b"DENT".as_slice(), &0o100644u32.to_le_bytes(), &16u32.to_le_bytes(), &1u32.to_le_bytes(), &4u32.to_le_bytes(), b"file"].concat();
            server_io.write_all(&dent).unwrap();
            server_io.write_all(&[b'D', b'O', b'N', b'E', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();

            // 3. RECV
            let mut r_req = [0u8; SYNC_HEADER_SIZE + 16];
            server_io.read_exact(&mut r_req).unwrap();
            let chunk_data = b"ttzip adb sync bytes";
            let d_hdr = [*sync_cmd::DATA, (chunk_data.len() as u32).to_le_bytes()];
            server_io.write_all(d_hdr.as_flattened()).unwrap();
            server_io.write_all(chunk_data).unwrap();
            server_io.write_all([*sync_cmd::DONE, 0u32.to_le_bytes()].as_flattened()).unwrap();

            // 4. SEND
            let mut s_hdr = [0u8; SYNC_HEADER_SIZE];
            server_io.read_exact(&mut s_hdr).unwrap();
            let arg_len = u32::from_le_bytes([s_hdr[4], s_hdr[5], s_hdr[6], s_hdr[7]]) as usize;
            let mut arg_buf = vec![0u8; arg_len];
            server_io.read_exact(&mut arg_buf).unwrap();
            let mut d_hdr = [0u8; SYNC_HEADER_SIZE];
            server_io.read_exact(&mut d_hdr).unwrap();
            let d_len = u32::from_le_bytes([d_hdr[4], d_hdr[5], d_hdr[6], d_hdr[7]]) as usize;
            let mut d_buf = vec![0u8; d_len];
            server_io.read_exact(&mut d_buf).unwrap();
            let mut done_h = [0u8; SYNC_HEADER_SIZE];
            server_io.read_exact(&mut done_h).unwrap();
            server_io.write_all([*sync_cmd::OKAY, 0u32.to_le_bytes()].as_flattened()).unwrap();

            // 5. QUIT
            let mut q_hdr = [0u8; SYNC_HEADER_SIZE];
            server_io.read_exact(&mut q_hdr).unwrap();
        });

        let entry = stream.stat("/sdcard/test").unwrap();
        assert_eq!(entry.name, "test");
        assert_eq!(entry.size, 8192);

        let list = stream.list("/sdcard").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "file");

        let data = stream.recv_file("/sdcard/test.txt").unwrap();
        assert_eq!(&data, b"ttzip adb sync bytes");

        stream.send_file("/sdcard/put.txt", b"new payload", 0o644, 999).unwrap();
        stream.quit().unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn test_sync_stream_list_standard_8byte_done() {
        use std::os::unix::net::UnixStream;
        let (client_io, mut server_io) = UnixStream::pair().expect("UnixStream pair");
        let mut stream = AdbSyncStream::new(client_io);

        let handle = std::thread::spawn(move || {
            let mut l_req = [0u8; SYNC_HEADER_SIZE + 4];
            server_io.read_exact(&mut l_req).unwrap();
            let dent = [b"DENT".as_slice(), &0o100644u32.to_le_bytes(), &10u32.to_le_bytes(), &1u32.to_le_bytes(), &3u32.to_le_bytes(), b"foo"].concat();
            server_io.write_all(&dent).unwrap();
            // Standard 8-byte DONE without trailing padding
            server_io.write_all(&[b'D', b'O', b'N', b'E', 0, 0, 0, 0]).unwrap();
        });

        let list = stream.list("/dir").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "foo");
        handle.join().unwrap();
    }
}
