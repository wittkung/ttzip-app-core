// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! MTP 1.1 protocol transaction state machine executing commands over USB bulk endpoints.

use std::time::Duration;

use crate::device::mtp::codes::{format, op, prop, resp, response_code_to_str, ContainerType};
use crate::device::mtp::container::{
    decode_ptp_device_info, decode_ptp_string, decode_u32_array, encode_ptp_string, MtpContainer,
};
use crate::device::mtp::driver::{UsbBulkTransport, DEFAULT_USB_TIMEOUT};
use crate::device::mtp::types::{MtpDeviceInfo, MtpObjectHandle, MtpObjectInfo, MtpStorageInfo};
use crate::error::AppCoreError;

mod streaming;

/// Maximum allowed metadata/control container payload size (64 MiB).
pub const MAX_MTP_METADATA_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Operational lifecycle state of an MTP device session.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtpSessionState {
    Disconnected,
    Connected,
    SessionOpen { session_id: u32 },
    Closed,
}

/// Fully compliant MTP 1.1 state machine executing atomic transactions.
pub struct MtpSession<T: UsbBulkTransport> {
    transport: T,
    state: MtpSessionState,
    transaction_id: u32,
    timeout: Duration,
    read_buf: Vec<u8>,
}

impl<T: UsbBulkTransport> MtpSession<T> {
    /// Instantiates a new MTP session wrapping a USB transport driver.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            state: MtpSessionState::Connected,
            transaction_id: 1,
            timeout: DEFAULT_USB_TIMEOUT,
            read_buf: vec![0u8; 65536],
        }
    }

    /// Current session lifecycle state.
    pub fn state(&self) -> MtpSessionState {
        self.state
    }

    /// Sets communication timeout for bulk transfers.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Accesses underlying USB bulk transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Unwraps session and returns underlying USB bulk transport.
    pub fn into_transport(self) -> T {
        self.transport
    }

    fn next_transaction_id(&mut self) -> u32 {
        let tid = self.transaction_id;
        self.transaction_id = self.transaction_id.wrapping_add(1);
        if self.transaction_id == 0 {
            self.transaction_id = 1;
        }
        tid
    }

    fn send_container(&mut self, container: &MtpContainer) -> Result<(), AppCoreError> {
        let bytes = container.to_bytes();
        let written = self.transport.write_bulk(&bytes, self.timeout)?;
        if written != bytes.len() {
            return Err(AppCoreError::Mtp {
                code: 0,
                message: format!(
                    "Partial bulk write: {} of {} bytes sent",
                    written,
                    bytes.len()
                ),
            });
        }
        Ok(())
    }

    /// Maximum allowed metadata/control container payload size (64 MiB) in `read_container`.
    pub const MAX_MTP_METADATA_PAYLOAD_BYTES: usize = MAX_MTP_METADATA_PAYLOAD_BYTES;

    fn read_container(&mut self) -> Result<MtpContainer, AppCoreError> {
        if self.read_buf.len() < 65536 {
            self.read_buf.resize(65536, 0);
        }
        let read_bytes = self.transport.read_bulk(&mut self.read_buf, self.timeout)?;
        if read_bytes == 0 {
            return Err(AppCoreError::Mtp {
                code: 0,
                message: "Zero bytes received during MTP read".to_string(),
            });
        }
        let mut container = MtpContainer::parse(&self.read_buf[..read_bytes])?;
        let expected_payload_len = (container.length as usize).saturating_sub(12);
        if expected_payload_len > Self::MAX_MTP_METADATA_PAYLOAD_BYTES {
            return Err(AppCoreError::Mtp {
                code: 0,
                message: format!(
                    "MTP metadata container payload length {} exceeds maximum allowed limit ({} bytes)",
                    expected_payload_len,
                    Self::MAX_MTP_METADATA_PAYLOAD_BYTES
                ),
            });
        }
        if container.payload.len() > expected_payload_len {
            container.payload.truncate(expected_payload_len);
        }
        while container.payload.len() < expected_payload_len {
            let remaining = expected_payload_len - container.payload.len();
            let to_read = remaining.min(self.read_buf.len());
            let chunk_bytes = self
                .transport
                .read_bulk(&mut self.read_buf[..to_read], self.timeout)?;
            if chunk_bytes == 0 {
                return Err(AppCoreError::Mtp {
                    code: 0,
                    message: format!(
                        "Unexpected EOF during multi-packet MTP container read: received {} of {} payload bytes",
                        container.payload.len(),
                        expected_payload_len
                    ),
                });
            }
            container
                .payload
                .extend_from_slice(&self.read_buf[..chunk_bytes]);
        }
        Ok(container)
    }

    fn check_response(
        &self,
        resp_container: &MtpContainer,
        expected_tid: u32,
    ) -> Result<(), AppCoreError> {
        if resp_container.container_type != ContainerType::Response {
            return Err(AppCoreError::Mtp {
                code: resp_container.code,
                message: format!(
                    "Expected response container, got {:?}",
                    resp_container.container_type
                ),
            });
        }
        if resp_container.transaction_id != expected_tid {
            return Err(AppCoreError::Mtp {
                code: resp::INVALID_TRANSACTION_ID,
                message: format!(
                    "Transaction ID mismatch: expected {}, got {}",
                    expected_tid, resp_container.transaction_id
                ),
            });
        }
        if resp_container.code != resp::OK {
            return Err(AppCoreError::Mtp {
                code: resp_container.code,
                message: format!(
                    "MTP operation returned error: 0x{:04X} ({})",
                    resp_container.code,
                    response_code_to_str(resp_container.code)
                ),
            });
        }
        Ok(())
    }

    /// Establishes an active logical MTP session on the device.
    pub fn open_session(&mut self, session_id: u32) -> Result<(), AppCoreError> {
        if let MtpSessionState::SessionOpen { .. } = self.state {
            return Err(AppCoreError::InvalidState {
                reason: "Session is already active".to_string(),
            });
        }

        let tid = self.next_transaction_id();
        let cmd = MtpContainer::new_command(op::OPEN_SESSION, tid, &[session_id]);
        self.send_container(&cmd)?;

        let resp_container = self.read_container()?;
        if resp_container.container_type == ContainerType::Response
            && resp_container.code == resp::SESSION_ALREADY_OPEN
        {
            let close_tid = self.next_transaction_id();
            let close_cmd = MtpContainer::new_command(op::CLOSE_SESSION, close_tid, &[]);
            if self.send_container(&close_cmd).is_ok() && self.read_container().is_ok() {
                self.transaction_id = 0;
                let reopen_tid = self.next_transaction_id();
                let reopen_cmd =
                    MtpContainer::new_command(op::OPEN_SESSION, reopen_tid, &[session_id]);
                if self.send_container(&reopen_cmd).is_ok() {
                    let _ = self.read_container();
                }
            }
            self.state = MtpSessionState::SessionOpen { session_id };
            return Ok(());
        }
        self.check_response(&resp_container, tid)?;

        self.state = MtpSessionState::SessionOpen { session_id };
        Ok(())
    }

    /// Opens a directory session without taking over an already-open foreign session.
    pub fn open_directory_session(&mut self, session_id: u32) -> Result<(), AppCoreError> {
        if self.state != MtpSessionState::Connected {
            return Err(AppCoreError::InvalidState { reason: "MTP directory session is not connected".into() });
        }
        let tid = self.next_transaction_id();
        self.send_container(&MtpContainer::new_command(op::OPEN_SESSION, tid, &[session_id]))?;
        let response = self.read_container()?;
        if response.container_type == ContainerType::Response && response.code == resp::SESSION_ALREADY_OPEN {
            return Err(AppCoreError::usb(
                crate::error::DeviceErrorKind::Unauthorized,
                "MTP session already open or interrupted; close other transfer apps or reconnect the device, then retry. No existing session was reset.",
            ));
        }
        self.check_response(&response, tid)?;
        self.state = MtpSessionState::SessionOpen { session_id };
        Ok(())
    }

    /// Closes the active MTP session.
    pub fn close_session(&mut self) -> Result<(), AppCoreError> {
        let tid = self.next_transaction_id();
        let cmd = MtpContainer::new_command(op::CLOSE_SESSION, tid, &[]);
        self.send_container(&cmd)?;

        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;

        self.state = MtpSessionState::Connected;
        Ok(())
    }

    /// Queries all available storage IDs present on the device.
    pub fn get_storage_ids(&mut self) -> Result<Vec<u32>, AppCoreError> {
        let tid = self.next_transaction_id();
        let cmd = MtpContainer::new_command(op::GET_STORAGE_IDS, tid, &[]);
        self.send_container(&cmd)?;

        let mut data_container = self.read_container()?;
        if data_container.container_type == ContainerType::Response
            && (data_container.code == resp::SESSION_ALREADY_OPEN
                || data_container.transaction_id < tid)
        {
            data_container = self.read_container()?;
        }
        if data_container.container_type != ContainerType::Data {
            return Err(AppCoreError::Mtp {
                code: data_container.code,
                message: "Expected Data container in get_storage_ids".to_string(),
            });
        }

        let (storage_ids, _) = decode_u32_array(&data_container.payload)?;
        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;

        Ok(storage_ids)
    }

    /// Queries detailed storage partition metadata for the specified storage ID.
    pub fn get_storage_info(&mut self, storage_id: u32) -> Result<MtpStorageInfo, AppCoreError> {
        let tid = self.next_transaction_id();
        let cmd = MtpContainer::new_command(op::GET_STORAGE_INFO, tid, &[storage_id]);
        self.send_container(&cmd)?;

        let data = self.read_container()?;
        if data.container_type != ContainerType::Data {
            return Err(AppCoreError::Mtp {
                code: data.code,
                message: "Expected Data container in get_storage_info".to_string(),
            });
        }

        let p = &data.payload;
        if p.len() < 26 {
            return Err(AppCoreError::Mtp {
                code: 0,
                message: format!("Storage info payload too short: {} bytes", p.len()),
            });
        }

        let storage_type = u16::from_le_bytes([p[0], p[1]]);
        let filesystem_type = u16::from_le_bytes([p[2], p[3]]);
        let access_capability = u16::from_le_bytes([p[4], p[5]]);
        let max_capacity = u64::from_le_bytes([p[6], p[7], p[8], p[9], p[10], p[11], p[12], p[13]]);
        let free_space_bytes =
            u64::from_le_bytes([p[14], p[15], p[16], p[17], p[18], p[19], p[20], p[21]]);
        let free_space_in_images = u32::from_le_bytes([p[22], p[23], p[24], p[25]]);

        let mut offset = 26;
        let (storage_description, consumed1) = decode_ptp_string(&p[offset..])?;
        offset += consumed1;
        let (volume_identifier, _) = decode_ptp_string(&p[offset..])?;

        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;

        Ok(MtpStorageInfo {
            storage_id,
            storage_type,
            filesystem_type,
            access_capability,
            max_capacity,
            free_space_in_bytes: free_space_bytes,
            free_space_in_images,
            storage_description,
            volume_identifier,
        })
    }

    /// Queries complete device capabilities and descriptor dataset from the device.
    pub fn get_device_info(&mut self) -> Result<MtpDeviceInfo, AppCoreError> {
        let tid = self.next_transaction_id();
        let cmd = MtpContainer::new_command(op::GET_DEVICE_INFO, tid, &[]);
        self.send_container(&cmd)?;

        let data = self.read_container()?;
        if data.container_type != ContainerType::Data {
            return Err(AppCoreError::Mtp {
                code: data.code,
                message: "Expected Data container in get_device_info".to_string(),
            });
        }

        let info = decode_ptp_device_info(&data.payload)?;
        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;

        Ok(info)
    }

    /// Enumerates object handles belonging to a storage partition and parent folder.
    pub fn get_object_handles(
        &mut self,
        storage_id: u32,
        format_code: u32,
        parent_handle: u32,
    ) -> Result<Vec<MtpObjectHandle>, AppCoreError> {
        let tid = self.next_transaction_id();
        let wire_parent = if parent_handle == 0 {
            0xFFFF_FFFF
        } else {
            parent_handle
        };
        let cmd = MtpContainer::new_command(
            op::GET_OBJECT_HANDLES,
            tid,
            &[storage_id, format_code, wire_parent],
        );
        self.send_container(&cmd)?;

        let data = self.read_container()?;
        if data.container_type != ContainerType::Data {
            return Err(AppCoreError::Mtp {
                code: data.code,
                message: "Expected Data container in get_object_handles".to_string(),
            });
        }

        let (handles, _) = decode_u32_array(&data.payload)?;
        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;

        Ok(handles)
    }

    /// Fetches metadata description for a specific object handle.
    pub fn get_object_info(
        &mut self,
        handle: MtpObjectHandle,
    ) -> Result<MtpObjectInfo, AppCoreError> {
        let tid = self.next_transaction_id();
        let cmd = MtpContainer::new_command(op::GET_OBJECT_INFO, tid, &[handle]);
        self.send_container(&cmd)?;

        let data = self.read_container()?;
        if data.container_type != ContainerType::Data {
            return Err(AppCoreError::Mtp {
                code: data.code,
                message: "Expected Data container in get_object_info".to_string(),
            });
        }

        let p = &data.payload;
        if p.len() < 52 {
            return Err(AppCoreError::Mtp {
                code: 0,
                message: format!(
                    "ObjectInfo payload too short: {} bytes (expected >= 52)",
                    p.len()
                ),
            });
        }

        let storage_id = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let format_code = u16::from_le_bytes([p[4], p[5]]);
        let protection_status = u16::from_le_bytes([p[6], p[7]]);
        let compressed_size = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as u64;
        let thumb_format = u16::from_le_bytes([p[12], p[13]]);
        let thumb_size = u32::from_le_bytes([p[14], p[15], p[16], p[17]]);
        let parent_handle = u32::from_le_bytes([p[38], p[39], p[40], p[41]]);
        let association_type = u16::from_le_bytes([p[42], p[43]]);

        let mut offset = 52;
        let (filename, c1) = decode_ptp_string(&p[offset..])?;
        offset += c1;
        let (date_created, c2) = decode_ptp_string(&p[offset..])?;
        offset += c2;
        let (date_modified, _) = decode_ptp_string(&p[offset..])?;

        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;

        Ok(MtpObjectInfo {
            handle,
            storage_id,
            format_code,
            protection_status,
            compressed_size,
            thumb_format,
            thumb_size,
            parent_handle,
            association_type,
            filename,
            date_created,
            date_modified,
        })
    }

    /// Retrieves object size in bytes, transparently querying the MTP 1.1 64-bit ObjectSize property
    /// if the 32-bit PTP compressed size overflows (0xFFFFFFFF).
    pub fn get_object_size(&mut self, handle: MtpObjectHandle) -> Result<u64, AppCoreError> {
        let info = self.get_object_info(handle)?;
        if info.is_large_file_overflow() {
            if let Ok(size_bytes) = self.get_object_property_raw(handle, prop::OBJECT_SIZE) {
                if size_bytes.len() >= 8 {
                    let size = u64::from_le_bytes([
                        size_bytes[0],
                        size_bytes[1],
                        size_bytes[2],
                        size_bytes[3],
                        size_bytes[4],
                        size_bytes[5],
                        size_bytes[6],
                        size_bytes[7],
                    ]);
                    return Ok(size);
                }
            }
        }
        Ok(info.compressed_size)
    }

    /// Queries raw object property value payload using MTP 1.1 GetObjectPropValue (0x9803).
    pub fn get_object_property_raw(
        &mut self,
        handle: MtpObjectHandle,
        prop_code: u16,
    ) -> Result<Vec<u8>, AppCoreError> {
        let tid = self.next_transaction_id();
        let cmd =
            MtpContainer::new_command(op::GET_OBJECT_PROP_VALUE, tid, &[handle, prop_code as u32]);
        self.send_container(&cmd)?;

        let data = self.read_container()?;
        if data.container_type != ContainerType::Data {
            return Err(AppCoreError::Mtp {
                code: data.code,
                message: "Expected Data container in get_object_property_raw".to_string(),
            });
        }

        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;

        Ok(data.payload)
    }

    /// Reads raw binary file payload of the specified object according to ISO 15740:2013 PTP streaming rules.
    pub fn get_object(&mut self, handle: MtpObjectHandle) -> Result<Vec<u8>, AppCoreError> {
        let mut data_payload = Vec::new();
        self.get_object_streaming(handle, |chunk| {
            data_payload.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(data_payload)
    }

    /// Maximum payload size supported by standard 32-bit PTP `SendObjectInfo` (`0x100C`) and `SendObject` (`0x100D`).
    pub const MAX_MTP_SEND_OBJECT_BYTES: u64 = (u32::MAX - 12) as u64;

    fn send_object_info_dataset(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
        format_code: u16,
        size_u32: u32,
        assoc_type: u16,
        name: &str,
    ) -> Result<MtpObjectHandle, AppCoreError> {
        let tid = self.next_transaction_id();
        let wire_parent = if parent_handle == 0 {
            0xFFFF_FFFF
        } else {
            parent_handle
        };
        let dataset_parent = if parent_handle == 0xFFFF_FFFF {
            0
        } else {
            parent_handle
        };
        let cmd = MtpContainer::new_command(op::SEND_OBJECT_INFO, tid, &[storage_id, wire_parent]);
        self.send_container(&cmd)?;

        let mut info_payload = Vec::with_capacity(128);
        info_payload.extend_from_slice(&storage_id.to_le_bytes());
        info_payload.extend_from_slice(&format_code.to_le_bytes());
        info_payload.extend_from_slice(&0u16.to_le_bytes());
        info_payload.extend_from_slice(&size_u32.to_le_bytes());
        info_payload.extend_from_slice(&[0u8; 26]);
        info_payload.extend_from_slice(&dataset_parent.to_le_bytes());
        info_payload.extend_from_slice(&assoc_type.to_le_bytes());
        info_payload.extend_from_slice(&0u32.to_le_bytes());
        info_payload.extend_from_slice(&0u32.to_le_bytes());
        for s in [name, "", "", ""] {
            info_payload.extend_from_slice(&encode_ptp_string(s));
        }

        let data_info = MtpContainer::new_data(op::SEND_OBJECT_INFO, tid, info_payload);
        self.send_container(&data_info)?;
        let resp_info = self.read_container()?;
        self.check_response(&resp_info, tid)?;
        Ok(resp_info.extract_params().get(2).copied().unwrap_or(0))
    }

    fn read_exact_from_source<R: std::io::Read>(
        reader: &mut R,
        dst: &mut [u8],
        offset_before: u64,
        total_size: u64,
    ) -> Result<(), AppCoreError> {
        let mut filled = 0usize;
        while filled < dst.len() {
            let n = reader
                .read(&mut dst[filled..])
                .map_err(|e| AppCoreError::Io {
                    reason: format!("MTP upload source read failed: {}", e),
                })?;
            if n == 0 {
                return Err(AppCoreError::Mtp {
                    code: resp::INCOMPLETE_TRANSFER,
                    message: format!(
                        "Source stream ended prematurely at {} of {} bytes",
                        offset_before + filled as u64,
                        total_size
                    ),
                });
            }
            filled += n;
        }
        Ok(())
    }

    /// Uploads an in-memory object payload in 64 KiB packets with progress & cancellation callback.
    pub fn send_object_with_progress<F>(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
        filename: &str,
        data: &[u8],
        mut on_progress: F,
    ) -> Result<MtpObjectHandle, AppCoreError>
    where
        F: FnMut(u64) -> Result<(), AppCoreError>,
    {
        let mut cursor = std::io::Cursor::new(data);
        self.send_object_from_reader(
            storage_id,
            parent_handle,
            filename,
            data.len() as u64,
            &mut cursor,
            |_chunk, sent| on_progress(sent),
        )
    }

    /// Uploads an object payload to the device.
    pub fn send_object(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
        filename: &str,
        data: &[u8],
    ) -> Result<MtpObjectHandle, AppCoreError> {
        self.send_object_with_progress(storage_id, parent_handle, filename, data, |_| Ok(()))
    }

    /// Creates a new directory (PTP Association) on the MTP storage partition.
    pub fn create_directory(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
        dirname: &str,
    ) -> Result<MtpObjectHandle, AppCoreError> {
        self.send_object_info_dataset(
            storage_id,
            parent_handle,
            format::ASSOCIATION,
            0,
            1,
            dirname,
        )
    }

    /// Deletes an object on the device.
    pub fn delete_object(&mut self, handle: MtpObjectHandle) -> Result<(), AppCoreError> {
        let tid = self.next_transaction_id();
        let cmd = MtpContainer::new_command(op::DELETE_OBJECT, tid, &[handle, 0]);
        self.send_container(&cmd)?;

        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;
        Ok(())
    }
}
