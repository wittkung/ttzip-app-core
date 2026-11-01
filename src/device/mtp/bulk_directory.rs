// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Bounded bulk object-property-list transaction and response handling.

use super::{
    resp, session, AppCoreError, ContainerType, MtpContainer, UsbBulkTransport,
    DEFAULT_USB_TIMEOUT, PTP_CONTAINER_HEADER_SIZE,
};

pub(super) fn execute_bulk_prop_list<T: UsbBulkTransport>(
    transport: &mut T,
    storage_id: u32,
    parent_handle: u32,
) -> Result<Vec<u8>, AppCoreError> {
    let cmd = crate::device::mtp_bulk::MtpBulkFetcher::build_command_container(
        1,
        storage_id,
        parent_handle,
        1,
    );
    let cmd_bytes = cmd.to_bytes();
    let written = transport.write_bulk(&cmd_bytes, DEFAULT_USB_TIMEOUT)?;
    if written != cmd_bytes.len() {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: "Partial bulk command write".to_string(),
        });
    }

    let mut read_buf = vec![0u8; 65536];
    let read_bytes = transport.read_bulk(&mut read_buf, DEFAULT_USB_TIMEOUT)?;
    if read_bytes < PTP_CONTAINER_HEADER_SIZE {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: "Unexpected EOF during bulk read".to_string(),
        });
    }

    let container = MtpContainer::parse(&read_buf[..read_bytes])?;
    if container.container_type != ContainerType::Data {
        return Err(AppCoreError::Mtp {
            code: container.code,
            message: format!(
                "Expected Data container for 0x9805, got {:?}",
                container.container_type
            ),
        });
    }

    if (container.length as usize) > session::MAX_MTP_METADATA_PAYLOAD_BYTES {
        return Err(AppCoreError::Mtp {
            code: container.code,
            message: format!(
                "0x9805 container length {} exceeds maximum allowed metadata size ({} bytes)",
                container.length,
                session::MAX_MTP_METADATA_PAYLOAD_BYTES
            ),
        });
    }

    let total_expected = (container.length as usize).saturating_sub(PTP_CONTAINER_HEADER_SIZE);
    let mut payload = container.payload;
    while payload.len() < total_expected {
        let chunk_len = transport.read_bulk(&mut read_buf, DEFAULT_USB_TIMEOUT)?;
        if chunk_len == 0 {
            return Err(AppCoreError::Mtp {
                code: container.code,
                message: format!(
                    "Unexpected EOF during multi-packet 0x9805 read: received {} of {} payload bytes",
                    payload.len(),
                    total_expected
                ),
            });
        }
        payload.extend_from_slice(&read_buf[..chunk_len]);
    }
    payload.truncate(total_expected);

    check_bulk_response(transport)?;
    Ok(payload)
}

fn check_bulk_response<T: UsbBulkTransport>(transport: &mut T) -> Result<(), AppCoreError> {
    let mut resp_buf = [0u8; 64];
    let resp_bytes = transport.read_bulk(&mut resp_buf, DEFAULT_USB_TIMEOUT)?;
    if resp_bytes >= PTP_CONTAINER_HEADER_SIZE {
        let resp = MtpContainer::parse(&resp_buf[..resp_bytes])?;
        if resp.container_type == ContainerType::Response && resp.code != resp::OK {
            return Err(AppCoreError::Mtp {
                code: resp.code,
                message: format!("0x9805 returned error response: 0x{:04X}", resp.code),
            });
        }
    }
    Ok(())
}
