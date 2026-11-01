// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Streaming PTP object transactions with callback cancellation and source bounds.

use super::{
    format, op, resp, AppCoreError, ContainerType, MtpContainer, MtpObjectHandle, MtpSession,
    UsbBulkTransport,
};

impl<T: UsbBulkTransport> MtpSession<T> {
    fn begin_object_stream(
        &mut self,
        handle: MtpObjectHandle,
    ) -> Result<(u32, usize, usize), AppCoreError> {
        let tid = self.next_transaction_id();
        let cmd = MtpContainer::new_command(op::GET_OBJECT, tid, &[handle]);
        self.send_container(&cmd)?;

        if self.read_buf.len() < 65536 {
            self.read_buf.resize(65536, 0);
        }
        let n = self.transport.read_bulk(&mut self.read_buf, self.timeout)?;
        if n == 0 {
            return Err(AppCoreError::Mtp {
                code: 0,
                message: "Zero bytes received during MTP get_object read".to_string(),
            });
        }

        let container = MtpContainer::parse(&self.read_buf[..n])?;
        if container.container_type != ContainerType::Data {
            return Err(AppCoreError::Mtp {
                code: container.code,
                message: format!(
                    "Expected Data container in get_object, got {:?}",
                    container.container_type
                ),
            });
        }
        if container.transaction_id != tid {
            return Err(AppCoreError::Mtp {
                code: resp::INVALID_TRANSACTION_ID,
                message: format!(
                    "Transaction ID mismatch in get_object: expected {}, got {}",
                    tid, container.transaction_id
                ),
            });
        }
        let total_payload_len = (container.length as usize).saturating_sub(12);
        Ok((tid, n, total_payload_len))
    }

    /// Streams raw binary file payload of the specified object in chunks to `on_chunk`.
    pub fn get_object_streaming<F>(
        &mut self,
        handle: MtpObjectHandle,
        mut on_chunk: F,
    ) -> Result<u64, AppCoreError>
    where
        F: FnMut(&[u8]) -> Result<(), AppCoreError>,
    {
        let (tid, n, total_payload_len) = self.begin_object_stream(handle)?;
        let mut received = 0usize;
        if n > 12 {
            let initial_chunk_len = (n - 12).min(total_payload_len);
            on_chunk(&self.read_buf[12..12 + initial_chunk_len])?;
            received += initial_chunk_len;
        }

        while received < total_payload_len {
            let remaining = total_payload_len - received;
            let to_read = remaining.min(self.read_buf.len());
            let chunk_bytes = self
                .transport
                .read_bulk(&mut self.read_buf[..to_read], self.timeout)?;
            if chunk_bytes == 0 {
                return Err(AppCoreError::Mtp {
                    code: 0,
                    message: format!(
                        "Unexpected EOF during MTP streaming: received {} of {} bytes",
                        received, total_payload_len
                    ),
                });
            }
            on_chunk(&self.read_buf[..chunk_bytes])?;
            received += chunk_bytes;
        }

        let resp_container = self.read_container()?;
        self.check_response(&resp_container, tid)?;

        Ok(received as u64)
    }

    /// Streams an object payload from a bounded `Read` source in USB-aligned 64 KiB packets.
    /// Rejects sizes exceeding `MAX_MTP_SEND_OBJECT_BYTES` before sending `SEND_OBJECT_INFO`.
    pub fn send_object_from_reader<R, F>(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
        filename: &str,
        total_size: u64,
        reader: &mut R,
        mut on_chunk_sent: F,
    ) -> Result<MtpObjectHandle, AppCoreError>
    where
        R: std::io::Read,
        F: FnMut(&[u8], u64) -> Result<(), AppCoreError>,
    {
        if total_size > Self::MAX_MTP_SEND_OBJECT_BYTES {
            return Err(AppCoreError::Mtp {
                code: resp::PARAMETER_NOT_SUPPORTED,
                message: format!(
                    "MTP upload size {} bytes exceeds 32-bit PTP SendObjectInfo limit ({} bytes); rejected before creating remote object",
                    total_size,
                    Self::MAX_MTP_SEND_OBJECT_BYTES
                ),
            });
        }

        let total_container_len = (12u64 + total_size) as u32;
        let created_handle = self.send_object_info_dataset(
            storage_id,
            parent_handle,
            format::UNDEFINED,
            total_size as u32,
            0,
            filename,
        )?;

        let tid_data = self.next_transaction_id();
        let cmd_data = MtpContainer::new_command(op::SEND_OBJECT, tid_data, &[]);
        if let Err(e) = self.send_container(&cmd_data) {
            if created_handle != 0 {
                let _ = self.delete_object(created_handle);
            }
            return Err(e);
        }

        let stream_res = self.send_object_payload_from_reader(
            total_size,
            total_container_len,
            tid_data,
            reader,
            &mut on_chunk_sent,
        );

        if let Err(err) = stream_res {
            if created_handle != 0 {
                let _ = self.delete_object(created_handle);
            }
            return Err(err);
        }

        Ok(created_handle)
    }

    fn send_object_payload_from_reader<R, F>(
        &mut self,
        total_size: u64,
        total_container_len: u32,
        tid_data: u32,
        reader: &mut R,
        on_chunk_sent: &mut F,
    ) -> Result<(), AppCoreError>
    where
        R: std::io::Read,
        F: FnMut(&[u8], u64) -> Result<(), AppCoreError>,
    {
        let mut buf = vec![0u8; 65_536];
        buf[0..4].copy_from_slice(&total_container_len.to_le_bytes());
        buf[4..6].copy_from_slice(&(ContainerType::Data as u16).to_le_bytes());
        buf[6..8].copy_from_slice(&op::SEND_OBJECT.to_le_bytes());
        buf[8..12].copy_from_slice(&tid_data.to_le_bytes());

        let first_target = (total_size as usize).min(65_536 - 12);
        Self::read_exact_from_source(reader, &mut buf[12..12 + first_target], 0, total_size)?;
        self.transport
            .write_bulk(&buf[..12 + first_target], self.timeout)?;
        let mut sent = first_target as u64;
        on_chunk_sent(&buf[12..12 + first_target], sent)?;

        while sent < total_size {
            let want = ((total_size - sent) as usize).min(buf.len());
            Self::read_exact_from_source(reader, &mut buf[..want], sent, total_size)?;
            self.transport.write_bulk(&buf[..want], self.timeout)?;
            sent += want as u64;
            on_chunk_sent(&buf[..want], sent)?;
        }

        let mut extra = [0u8; 1];
        let trailing = reader.read(&mut extra).map_err(|e| AppCoreError::Io {
            reason: format!("MTP upload trailing source check failed: {}", e),
        })?;
        if trailing > 0 {
            return Err(AppCoreError::Mtp {
                code: resp::INCOMPLETE_TRANSFER,
                message: format!(
                    "Source stream grew beyond declared {} bytes during upload",
                    total_size
                ),
            });
        }

        let resp_data = self.read_container()?;
        self.check_response(&resp_data, tid_data)?;
        Ok(())
    }
}
