// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! MTP 1.1 binary packet container serialization, deserialization, and PTP string codecs.

use bytes::{Buf, BytesMut};

use crate::device::mtp::codes::{resp, ContainerType};
use crate::device::mtp::types::MtpDeviceInfo;
use crate::error::AppCoreError;

/// Standard PTP/MTP container header length in bytes (ISO 15740 §5.3).
pub const PTP_CONTAINER_HEADER_SIZE: usize = 12;

/// Strongly typed failure reasons encountered while parsing wire PTP/MTP containers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MtpContainerError {
    #[error("Container buffer too small: {actual} bytes (minimum {expected} bytes required)")]
    BufferTooSmall { expected: usize, actual: usize },
    #[error("Container length too small: {actual} bytes (minimum {min} bytes required)")]
    LengthTooSmall { min: usize, actual: usize },
    #[error("Invalid PTP container type: 0x{container_type:04X}")]
    InvalidContainerType { container_type: u16 },
    #[error("Incomplete container: expected {expected} bytes, received {actual} bytes")]
    ContainerUnderflow { expected: usize, actual: usize },
    #[error("I/O error: {0}")]
    Io(String),
}

impl From<std::io::Error> for MtpContainerError {
    #[inline]
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

impl From<MtpContainerError> for AppCoreError {
    fn from(err: MtpContainerError) -> Self {
        match err {
            MtpContainerError::Io(reason) => AppCoreError::Io { reason },
            _ => AppCoreError::Mtp {
                code: 0,
                message: err.to_string(),
            },
        }
    }
}

/// Validated fixed-size 12-byte PTP container header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtpHeader {
    pub length: u32,
    pub container_type: ContainerType,
    pub code: u16,
    pub transaction_id: u32,
}

impl PtpHeader {
    /// Parses and validates 12-byte container header with BCE and priority error reporting.
    #[inline]
    pub fn parse(bytes: &[u8]) -> Result<Self, MtpContainerError> {
        if bytes.len() < PTP_CONTAINER_HEADER_SIZE {
            return Err(MtpContainerError::BufferTooSmall {
                expected: PTP_CONTAINER_HEADER_SIZE,
                actual: bytes.len(),
            });
        }
        let header = &bytes[..PTP_CONTAINER_HEADER_SIZE];
        let length = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let container_type_raw = u16::from_le_bytes([header[4], header[5]]);
        let code = u16::from_le_bytes([header[6], header[7]]);
        let transaction_id = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);

        if length < PTP_CONTAINER_HEADER_SIZE as u32 {
            return Err(MtpContainerError::LengthTooSmall {
                min: PTP_CONTAINER_HEADER_SIZE,
                actual: length as usize,
            });
        }
        let container_type = ContainerType::from_u16(container_type_raw);
        if container_type == ContainerType::Undefined {
            return Err(MtpContainerError::InvalidContainerType {
                container_type: container_type_raw,
            });
        }
        Ok(Self {
            length,
            container_type,
            code,
            transaction_id,
        })
    }
}

#[inline]
fn pack_params(params: &[u32]) -> (u32, Vec<u8>) {
    if params.is_empty() {
        (PTP_CONTAINER_HEADER_SIZE as u32, Vec::new())
    } else {
        let mut p = Vec::with_capacity(params.len() * 4);
        for &param in params {
            p.extend_from_slice(&param.to_le_bytes());
        }
        ((PTP_CONTAINER_HEADER_SIZE + p.len()) as u32, p)
    }
}

/// Strongly typed PTP/MTP packet container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtpContainer {
    pub length: u32,
    pub container_type: ContainerType,
    pub code: u16,
    pub transaction_id: u32,
    pub payload: Vec<u8>,
}

impl MtpContainer {
    pub fn new_command(code: u16, transaction_id: u32, params: &[u32]) -> Self {
        let (length, payload) = pack_params(params);
        Self {
            length,
            container_type: ContainerType::Command,
            code,
            transaction_id,
            payload,
        }
    }

    pub fn new_data(code: u16, transaction_id: u32, data: Vec<u8>) -> Self {
        Self {
            length: (PTP_CONTAINER_HEADER_SIZE + data.len()) as u32,
            container_type: ContainerType::Data,
            code,
            transaction_id,
            payload: data,
        }
    }

    pub fn new_response(code: u16, transaction_id: u32, params: &[u32]) -> Self {
        let (length, payload) = pack_params(params);
        Self {
            length,
            container_type: ContainerType::Response,
            code,
            transaction_id,
            payload,
        }
    }

    #[inline]
    #[must_use]
    pub fn new_response_ok(transaction_id: u32) -> Self {
        Self {
            length: PTP_CONTAINER_HEADER_SIZE as u32,
            container_type: ContainerType::Response,
            code: resp::OK,
            transaction_id,
            payload: Vec::new(),
        }
    }

    pub fn new_event(code: u16, transaction_id: u32, params: &[u32]) -> Self {
        let (length, payload) = pack_params(params);
        Self {
            length,
            container_type: ContainerType::Event,
            code,
            transaction_id,
            payload,
        }
    }

    #[inline]
    pub fn is_empty_payload(&self) -> bool {
        self.payload.is_empty()
    }

    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[inline]
    fn serialize_header(&self) -> [u8; PTP_CONTAINER_HEADER_SIZE] {
        let type_and_code = (self.container_type as u16 as u32) | ((self.code as u32) << 16);
        let l = self.length.to_le_bytes();
        let tc = type_and_code.to_le_bytes();
        let tx = self.transaction_id.to_le_bytes();
        [
            l[0], l[1], l[2], l[3], tc[0], tc[1], tc[2], tc[3], tx[0], tx[1], tx[2], tx[3],
        ]
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(PTP_CONTAINER_HEADER_SIZE + self.payload.len());
        buf.extend_from_slice(&self.serialize_header());
        if !self.payload.is_empty() {
            buf.extend_from_slice(&self.payload);
        }
        buf
    }

    pub fn encode_into(&self, dst: &mut BytesMut) {
        dst.reserve(PTP_CONTAINER_HEADER_SIZE + self.payload.len());
        dst.extend_from_slice(&self.serialize_header());
        if !self.payload.is_empty() {
            dst.extend_from_slice(&self.payload);
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, AppCoreError> {
        if bytes.len() < PTP_CONTAINER_HEADER_SIZE {
            return Err(AppCoreError::Mtp {
                code: 0,
                message: format!(
                    "Packet too short for PTP header: {} bytes (minimum {})",
                    bytes.len(),
                    PTP_CONTAINER_HEADER_SIZE
                ),
            });
        }
        let header = &bytes[..PTP_CONTAINER_HEADER_SIZE];
        let length = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let container_type = ContainerType::from_u16(u16::from_le_bytes([header[4], header[5]]));
        let code = u16::from_le_bytes([header[6], header[7]]);
        let transaction_id = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);

        let payload_len = (length as usize).saturating_sub(PTP_CONTAINER_HEADER_SIZE);
        let actual_len = payload_len.min(bytes.len() - PTP_CONTAINER_HEADER_SIZE);
        let payload =
            bytes[PTP_CONTAINER_HEADER_SIZE..PTP_CONTAINER_HEADER_SIZE + actual_len].to_vec();

        Ok(Self {
            length,
            container_type,
            code,
            transaction_id,
            payload,
        })
    }

    pub fn parse_frame(bytes: &[u8]) -> Result<(Self, usize), MtpContainerError> {
        let hdr = PtpHeader::parse(bytes)?;
        let total_required = hdr.length as usize;
        if bytes.len() < total_required {
            return Err(MtpContainerError::ContainerUnderflow {
                expected: total_required,
                actual: bytes.len(),
            });
        }
        let payload = if total_required == PTP_CONTAINER_HEADER_SIZE {
            Vec::new()
        } else {
            bytes[PTP_CONTAINER_HEADER_SIZE..total_required].to_vec()
        };
        Ok((
            Self {
                length: hdr.length,
                container_type: hdr.container_type,
                code: hdr.code,
                transaction_id: hdr.transaction_id,
                payload,
            },
            total_required,
        ))
    }

    pub fn parse_frame_zero_copy(src: &mut BytesMut) -> Result<Option<Self>, MtpContainerError> {
        if src.len() < PTP_CONTAINER_HEADER_SIZE {
            return Ok(None);
        }
        let hdr = PtpHeader::parse(src)?;
        let total_frame_len = hdr.length as usize;
        if src.len() < total_frame_len {
            let missing = total_frame_len.saturating_sub(src.len());
            src.reserve(missing);
            return Ok(None);
        }

        src.advance(PTP_CONTAINER_HEADER_SIZE);
        let payload_len = total_frame_len - PTP_CONTAINER_HEADER_SIZE;
        let payload = if payload_len == 0 {
            Vec::new()
        } else {
            src.split_to(payload_len).to_vec()
        };
        Ok(Some(Self {
            length: hdr.length,
            container_type: hdr.container_type,
            code: hdr.code,
            transaction_id: hdr.transaction_id,
            payload,
        }))
    }

    pub fn extract_params(&self) -> Vec<u32> {
        if self.payload.is_empty() {
            return Vec::new();
        }
        self.payload
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }
}
const _: () = {
    const fn assert_sink_compatible<
        E: From<std::io::Error> + std::error::Error + Send + Sync + 'static,
    >() {
    }

    assert_sink_compatible::<MtpContainerError>();

    assert!(PTP_CONTAINER_HEADER_SIZE == 12);
    assert!(std::mem::size_of::<PtpHeader>() == 12 && std::mem::align_of::<PtpHeader>() == 4);
    assert!(
        std::mem::size_of::<ContainerType>() == 2 && std::mem::align_of::<ContainerType>() == 2
    );

    #[cfg(target_pointer_width = "64")]
    assert!(std::mem::size_of::<MtpContainer>() == 40 && std::mem::align_of::<MtpContainer>() == 8);
};

/// Decodes standard PTP UTF-16LE string from byte slice.
pub fn decode_ptp_string(bytes: &[u8]) -> Result<(String, usize), AppCoreError> {
    if bytes.is_empty() {
        return Ok((String::new(), 0));
    }
    let num_chars = bytes[0] as usize;
    if num_chars == 0 {
        return Ok((String::new(), 1));
    }

    let required_bytes = 1 + num_chars * 2;
    if bytes.len() < required_bytes {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: format!(
                "PTP string buffer underflow: need {} bytes, got {}",
                required_bytes,
                bytes.len()
            ),
        });
    }

    let mut u16_chars = Vec::with_capacity(num_chars);
    for chunk in bytes[1..required_bytes].chunks_exact(2) {
        u16_chars.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    if let Some(&0) = u16_chars.last() {
        u16_chars.pop();
    }

    Ok((String::from_utf16_lossy(&u16_chars), required_bytes))
}

/// Encodes UTF-8 string into standard PTP UTF-16LE string with null terminator.
pub fn encode_ptp_string(s: &str) -> Vec<u8> {
    if s.is_empty() {
        return vec![0u8];
    }
    let char_count = s.encode_utf16().count();
    let num_chars = (char_count + 1).min(255) as u8;
    let mut buf = Vec::with_capacity(1 + (num_chars as usize) * 2);
    buf.push(num_chars);
    for ch in s.encode_utf16().take(num_chars as usize - 1) {
        buf.extend_from_slice(&ch.to_le_bytes());
    }
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf
}

/// Decodes a 32-bit integer array from PTP data payload.
pub fn decode_u32_array(bytes: &[u8]) -> Result<(Vec<u32>, usize), AppCoreError> {
    if bytes.len() < 4 {
        return Ok((Vec::new(), 0));
    }
    let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let needed = 4 + count * 4;
    if bytes.len() < needed {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: format!(
                "Underflow reading u32 array: expected {} elements ({} bytes), got {}",
                count,
                needed,
                bytes.len()
            ),
        });
    }
    let result = bytes[4..needed]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok((result, needed))
}

/// Decodes a 16-bit integer array from PTP data payload.
pub fn decode_u16_array(bytes: &[u8]) -> Result<(Vec<u16>, usize), AppCoreError> {
    if bytes.len() < 4 {
        return Ok((Vec::new(), 0));
    }
    let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let needed = 4 + count * 2;
    if bytes.len() < needed {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: format!(
                "Underflow reading u16 array: expected {} elements ({} bytes), got {}",
                count,
                needed,
                bytes.len()
            ),
        });
    }
    let result = bytes[4..needed]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok((result, needed))
}

/// Decodes standard PTP/MTP DeviceInfo dataset from payload bytes according to ISO 15740 §5.5.2.
pub fn decode_ptp_device_info(bytes: &[u8]) -> Result<MtpDeviceInfo, AppCoreError> {
    if bytes.len() < 8 {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: format!(
                "DeviceInfo payload too short: {} bytes (minimum 8)",
                bytes.len()
            ),
        });
    }
    let hdr = &bytes[..8];
    let standard_version = u16::from_le_bytes([hdr[0], hdr[1]]);
    let vendor_extension_id = u32::from_le_bytes([hdr[2], hdr[3], hdr[4], hdr[5]]);
    let vendor_extension_version = u16::from_le_bytes([hdr[6], hdr[7]]);

    let mut offset = 8;
    let (vendor_extension_desc, c_ext) = decode_ptp_string(&bytes[offset..])?;
    offset += c_ext;

    if bytes.len() < offset + 2 {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: "Truncated DeviceInfo functional mode field".to_string(),
        });
    }
    let functional_mode = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    offset += 2;

    let (operations_supported, c) = decode_u16_array(&bytes[offset..])?;
    offset += c;
    let (events_supported, c) = decode_u16_array(&bytes[offset..])?;
    offset += c;
    let (device_properties_supported, c) = decode_u16_array(&bytes[offset..])?;
    offset += c;
    let (capture_formats, c) = decode_u16_array(&bytes[offset..])?;
    offset += c;
    let (playback_formats, c) = decode_u16_array(&bytes[offset..])?;
    offset += c;
    let (manufacturer, c) = decode_ptp_string(&bytes[offset..])?;
    offset += c;
    let (model, c) = decode_ptp_string(&bytes[offset..])?;
    offset += c;
    let (device_version, c) = decode_ptp_string(&bytes[offset..])?;
    offset += c;
    let (serial_number, _) = decode_ptp_string(&bytes[offset..])?;

    Ok(MtpDeviceInfo {
        vendor_id: 0,
        product_id: 0,
        bus_number: 0,
        device_address: 0,
        standard_version,
        vendor_extension_id,
        vendor_extension_version,
        vendor_extension_desc,
        functional_mode,
        operations_supported,
        events_supported,
        device_properties_supported,
        capture_formats,
        playback_formats,
        manufacturer,
        model,
        device_version,
        serial_number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mtp::codes::{event, format, op};

    #[test]
    fn test_ptp_string_roundtrip_and_null_terminator() {
        let (decoded, consumed) = decode_ptp_string(&encode_ptp_string("IMG_2026.JPG")).unwrap();
        assert_eq!(decoded, "IMG_2026.JPG");
        assert_eq!(consumed, 27);

        let (empty, c_empty) = decode_ptp_string(&encode_ptp_string("")).unwrap();
        assert_eq!(empty, "");
        assert_eq!(c_empty, 1);

        let (single, _) = decode_ptp_string(&encode_ptp_string("A")).unwrap();
        assert_eq!(single, "A");
    }

    #[test]
    fn test_container_command_serialization_and_parsing() {
        let params = [0x00010001u32, 0x00000000, 0x12345678];
        let cmd = MtpContainer::new_command(op::GET_OBJECT_HANDLES, 42, &params);
        assert_eq!(cmd.length, 24);
        assert_eq!(cmd.container_type, ContainerType::Command);
        assert_eq!(cmd.code, op::GET_OBJECT_HANDLES);
        assert_eq!(cmd.transaction_id, 42);

        let bytes = cmd.to_bytes();
        assert_eq!(bytes.len(), 24);
        let parsed = MtpContainer::parse(&bytes).expect("parse command container failed");
        assert_eq!(parsed, cmd);
        assert_eq!(
            parsed.extract_params(),
            vec![0x00010001, 0x00000000, 0x12345678]
        );
    }

    #[test]
    fn test_container_data_and_response() {
        let payload = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02];
        let data_container = MtpContainer::new_data(op::SEND_OBJECT, 99, payload.clone());
        assert_eq!(data_container.container_type, ContainerType::Data);
        assert_eq!(data_container.payload, payload);

        let bytes = data_container.to_bytes();
        let parsed = MtpContainer::parse(&bytes).expect("parse data container failed");
        assert_eq!(parsed, data_container);

        let truncated = &bytes[..10];
        let err = MtpContainer::parse(truncated).expect_err("should fail on truncated header");
        match err {
            AppCoreError::Mtp { code, message } => {
                assert_eq!(code, 0);
                assert!(message.contains("Packet too short"));
            }
            _ => panic!("Unexpected error variant: {:?}", err),
        }
    }

    #[test]
    fn test_decode_u32_and_u16_arrays() {
        let mut u32_buf = Vec::new();
        for &v in &[3u32, 100, 200, 300] {
            u32_buf.extend_from_slice(&v.to_le_bytes());
        }
        let (u32_vals, consumed32) = decode_u32_array(&u32_buf).expect("decode u32 array");
        assert_eq!(u32_vals, vec![100, 200, 300]);
        assert_eq!(consumed32, 16);

        let mut u16_buf = Vec::new();
        u16_buf.extend_from_slice(&2u32.to_le_bytes());
        u16_buf.extend_from_slice(&op::OPEN_SESSION.to_le_bytes());
        u16_buf.extend_from_slice(&op::GET_STORAGE_IDS.to_le_bytes());
        let (u16_vals, consumed16) = decode_u16_array(&u16_buf).expect("decode u16 array");
        assert_eq!(u16_vals, vec![op::OPEN_SESSION, op::GET_STORAGE_IDS]);
        assert_eq!(consumed16, 8);
    }

    #[test]
    fn test_decode_ptp_device_info_synthetic() {
        let mut payload = Vec::new();
        for slice in &[
            &100u16.to_le_bytes()[..],
            &6u32.to_le_bytes(),
            &100u16.to_le_bytes(),
            &encode_ptp_string("microsoft.com: 1.0;"),
            &0u16.to_le_bytes(),
            &2u32.to_le_bytes(),
            &op::OPEN_SESSION.to_le_bytes(),
            &op::GET_STORAGE_IDS.to_le_bytes(),
            &1u32.to_le_bytes(),
            &event::OBJECT_ADDED.to_le_bytes(),
            &0u32.to_le_bytes(),
            &0u32.to_le_bytes(),
            &1u32.to_le_bytes(),
            &format::MP4.to_le_bytes(),
            &encode_ptp_string("Google"),
            &encode_ptp_string("Pixel 9"),
            &encode_ptp_string("1.0"),
            &encode_ptp_string("XYZ123456789"),
        ] {
            payload.extend_from_slice(slice);
        }

        let dev_info = decode_ptp_device_info(&payload).expect("decode DeviceInfo failed");
        assert_eq!(dev_info.manufacturer, "Google");
        assert_eq!(dev_info.model, "Pixel 9");
        assert_eq!(dev_info.serial_number, "XYZ123456789");
        assert!(dev_info.supports_operation(op::OPEN_SESSION));
        assert!(dev_info.supports_operation(op::GET_STORAGE_IDS));
        assert!(!dev_info.supports_operation(op::SEND_OBJECT));
        assert!(dev_info.supports_event(event::OBJECT_ADDED));
        assert!(dev_info.supports_format(format::MP4));
        assert!(!dev_info.supports_format(format::EXIF_JPEG));
    }

    #[test]
    fn test_get_object_single_and_multi_chunk_iso15740_no_header_stripping() {
        use crate::device::mtp::session::MtpSession;
        use crate::device::mtp::tests::MockUsbTransport;

        let mut mock = MockUsbTransport::new();
        let payload = b"Hello, ISO 15740 PTP streaming!";
        mock.push_data(op::GET_OBJECT, 1, payload);
        mock.push_response(resp::OK, 1, &[]);
        let mut session = MtpSession::new(mock);
        assert_eq!(session.get_object(42).expect("get_object"), payload);

        let mut mock2 = MockUsbTransport::new();
        let total_data_len = 100_000usize;
        let full_payload: Vec<u8> = (0..total_data_len).map(|i| (i % 251) as u8).collect();
        let total_container_len = (12 + total_data_len) as u32;
        let mut pkt1 = Vec::new();
        pkt1.extend_from_slice(&total_container_len.to_le_bytes());
        pkt1.extend_from_slice(&(ContainerType::Data as u16).to_le_bytes());
        pkt1.extend_from_slice(&op::GET_OBJECT.to_le_bytes());
        pkt1.extend_from_slice(&1u32.to_le_bytes());
        pkt1.extend_from_slice(&full_payload[..60_000]);
        mock2.push_raw_packet(pkt1);
        mock2.push_raw_packet(full_payload[60_000..].to_vec());
        mock2.push_response(resp::OK, 1, &[]);

        let mut session2 = MtpSession::new(mock2);
        let result = session2
            .get_object(99)
            .expect("multi-chunk get_object must succeed");
        assert_eq!(result, full_payload);
    }

    #[test]
    fn test_read_container_multi_packet_premature_eof_and_cap_errors() {
        use crate::device::mtp::session::MtpSession;
        use crate::device::mtp::tests::MockUsbTransport;

        let mut mock = MockUsbTransport::new();
        let declared_len = (12 + 1024) as u32;
        let mut pkt1 = Vec::new();
        pkt1.extend_from_slice(&declared_len.to_le_bytes());
        pkt1.extend_from_slice(&(ContainerType::Data as u16).to_le_bytes());
        pkt1.extend_from_slice(&op::GET_OBJECT_HANDLES.to_le_bytes());
        pkt1.extend_from_slice(&1u32.to_le_bytes());
        pkt1.extend_from_slice(&[0u8; 200]);
        mock.push_raw_packet(pkt1);

        let mut session = MtpSession::new(mock);
        let err = session
            .get_object_handles(0x00010001, 0, 0)
            .expect_err("truncated multi-packet read must fail");
        assert!(err
            .to_string()
            .contains("Unexpected EOF during multi-packet MTP container read"));

        let mut mock_cap = MockUsbTransport::new();
        let excessive_len =
            (MtpSession::<MockUsbTransport>::MAX_MTP_METADATA_PAYLOAD_BYTES as u32) + 1024;
        let mut pkt_cap = Vec::new();
        pkt_cap.extend_from_slice(&excessive_len.to_le_bytes());
        pkt_cap.extend_from_slice(&(ContainerType::Data as u16).to_le_bytes());
        pkt_cap.extend_from_slice(&op::GET_OBJECT_HANDLES.to_le_bytes());
        pkt_cap.extend_from_slice(&1u32.to_le_bytes());
        pkt_cap.extend_from_slice(&[0u8; 64]);
        mock_cap.push_raw_packet(pkt_cap);

        let mut session_cap = MtpSession::new(mock_cap);
        let err_cap = session_cap
            .get_object_handles(0x00010001, 0, 0)
            .expect_err("metadata container exceeding max cap must fail");
        assert!(err_cap
            .to_string()
            .contains("exceeds maximum allowed limit"));
    }

    #[test]
    fn test_send_object_cancellation_and_32bit_overflow_rejection() {
        use crate::device::mtp::session::MtpSession;
        use crate::device::mtp::tests::MockUsbTransport;

        let mut mock = MockUsbTransport::new();
        mock.push_response(resp::OK, 1, &[0x00010001, 0, 777]);
        mock.push_response(resp::OK, 3, &[]);

        let mut session = MtpSession::new(mock);
        let payload = vec![0xABu8; 100_000];
        let err = session
            .send_object_with_progress(0x00010001, 0, "cancel_test.bin", &payload, |_sent| {
                Err(AppCoreError::Transfer {
                    reason: "Cancelled by user".to_string(),
                })
            })
            .expect_err("cancelled upload must fail");
        assert!(err.to_string().contains("Cancelled by user"));

        let transport = session.into_transport();
        let last_out = transport.out_packets().last().expect("delete command sent");
        let parsed = MtpContainer::parse(last_out).expect("valid container");
        assert_eq!(parsed.code, op::DELETE_OBJECT);
        assert_eq!(parsed.extract_params().first().copied(), Some(777));

        let mut session_overflow = MtpSession::new(MockUsbTransport::new());
        let mut empty = std::io::empty();
        let err_ovf = session_overflow
            .send_object_from_reader(
                0x0001_0001,
                0,
                "4gb_overflow.bin",
                u32::MAX as u64,
                &mut empty,
                |_, _| Ok(()),
            )
            .expect_err("size >= u32::MAX - 12 must be rejected before SEND_OBJECT_INFO");
        assert!(err_ovf
            .to_string()
            .contains("exceeds 32-bit PTP SendObjectInfo limit"));
        assert!(session_overflow.into_transport().out_packets().is_empty());
    }
}
