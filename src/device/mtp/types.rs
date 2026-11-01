// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! MTP 1.1 device, storage, and object data structures.

use serde::{Deserialize, Serialize};

/// Unique identifier for an MTP object handle.
pub type MtpObjectHandle = u32;

/// Root / parent handle constants.
pub const MTP_OBJECT_HANDLE_ROOT: MtpObjectHandle = 0x00000000;
pub const MTP_OBJECT_HANDLE_ALL: MtpObjectHandle = 0xFFFFFFFF;

/// Information about a connected physical MTP device, including operational capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MtpDeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus_number: u8,
    pub device_address: u8,
    pub manufacturer: String,
    pub model: String,
    pub serial_number: String,
    pub device_version: String,
    pub standard_version: u16,
    pub vendor_extension_id: u32,
    pub vendor_extension_version: u16,
    pub vendor_extension_desc: String,
    pub functional_mode: u16,
    pub operations_supported: Vec<u16>,
    pub events_supported: Vec<u16>,
    pub device_properties_supported: Vec<u16>,
    pub capture_formats: Vec<u16>,
    pub playback_formats: Vec<u16>,
}

impl MtpDeviceInfo {
    /// Returns true if the device supports the specified PTP/MTP operation code.
    #[inline]
    pub fn supports_operation(&self, op: u16) -> bool {
        self.operations_supported.contains(&op)
    }

    /// Returns true if the device supports the specified PTP/MTP event code.
    #[inline]
    pub fn supports_event(&self, evt: u16) -> bool {
        self.events_supported.contains(&evt)
    }

    /// Returns true if the device supports the specified object format code
    /// either for playback/storage or for capture.
    #[inline]
    pub fn supports_format(&self, fmt: u16) -> bool {
        self.playback_formats.contains(&fmt) || self.capture_formats.contains(&fmt)
    }

    /// Returns true if the device supports the specified device property code.
    #[inline]
    pub fn supports_device_property(&self, prop: u16) -> bool {
        self.device_properties_supported.contains(&prop)
    }
}

/// Metadata describing a physical or virtual storage partition on an MTP device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MtpStorageInfo {
    pub storage_id: u32,
    pub storage_type: u16,
    pub filesystem_type: u16,
    pub access_capability: u16,
    pub max_capacity: u64,
    pub free_space_in_bytes: u64,
    pub free_space_in_images: u32,
    pub storage_description: String,
    pub volume_identifier: String,
}

impl MtpStorageInfo {
    /// Returns the available storage space in bytes.
    #[inline]
    pub fn free_space_in_bytes(&self) -> u64 {
        self.free_space_in_bytes
    }

    /// Backward-compatible alias for `free_space_in_bytes`.
    #[inline]
    pub fn free_space_bytes(&self) -> u64 {
        self.free_space_in_bytes
    }
}

/// Metadata describing a file or directory object on an MTP storage volume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MtpObjectInfo {
    pub handle: MtpObjectHandle,
    pub storage_id: u32,
    pub format_code: u16,
    pub protection_status: u16,
    pub compressed_size: u64,
    pub thumb_format: u16,
    pub thumb_size: u32,
    pub parent_handle: MtpObjectHandle,
    pub association_type: u16,
    pub filename: String,
    pub date_created: String,
    pub date_modified: String,
}

impl MtpObjectInfo {
    /// Returns true if this object represents a folder/directory (PTP Association).
    #[inline]
    pub fn is_directory(&self) -> bool {
        self.format_code == crate::device::mtp::codes::format::ASSOCIATION
    }

    /// Effective size in bytes of the object, supporting files > 4GB natively.
    #[inline]
    pub fn file_size(&self) -> u64 {
        self.compressed_size
    }

    /// Returns true if the 32-bit PTP compressed size field indicated overflow (>= 4GB).
    #[inline]
    pub fn is_large_file_overflow(&self) -> bool {
        self.compressed_size >= 0xFFFF_FFFF
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mtp::codes::{event, format, op};

    #[test]
    fn test_mtp_device_info_capabilities_negotiation() {
        let info = MtpDeviceInfo {
            operations_supported: vec![op::OPEN_SESSION, op::GET_STORAGE_IDS, op::GET_OBJECT],
            events_supported: vec![event::OBJECT_ADDED, event::DEVICE_RESET],
            playback_formats: vec![format::EXIF_JPEG, format::MP4],
            capture_formats: vec![format::PNG],
            ..Default::default()
        };

        // Operation support check
        assert!(info.supports_operation(op::OPEN_SESSION));
        assert!(info.supports_operation(op::GET_STORAGE_IDS));
        assert!(!info.supports_operation(op::SEND_OBJECT));

        // Event support check
        assert!(info.supports_event(event::OBJECT_ADDED));
        assert!(!info.supports_event(event::STORE_REMOVED));

        // Format support check across playback and capture
        assert!(info.supports_format(format::EXIF_JPEG));
        assert!(info.supports_format(format::MP4));
        assert!(info.supports_format(format::PNG));
        assert!(!info.supports_format(format::MATROSKA));
    }

    #[test]
    fn test_mtp_storage_info_64bit_capacity_and_free_space() {
        let five_hundred_twelve_gb: u64 = 512 * 1024 * 1024 * 1024;
        let one_hundred_twenty_eight_gb: u64 = 128 * 1024 * 1024 * 1024;

        let storage = MtpStorageInfo {
            storage_id: 0x00010001,
            storage_type: 0x0003,
            filesystem_type: 0x0002,
            access_capability: 0x0000,
            max_capacity: five_hundred_twelve_gb,
            free_space_in_bytes: one_hundred_twenty_eight_gb,
            free_space_in_images: 10_000,
            storage_description: "Internal Storage".to_string(),
            volume_identifier: "PRIMARY".to_string(),
        };

        assert_eq!(storage.max_capacity, 549_755_813_888);
        assert_eq!(storage.free_space_in_bytes, 137_438_953_472);
        assert_eq!(storage.free_space_bytes(), 137_438_953_472);
    }

    #[test]
    fn test_mtp_object_info_64bit_size_and_directory_check() {
        let eight_gb: u64 = 8 * 1024 * 1024 * 1024;
        let large_video = MtpObjectInfo {
            handle: 1001,
            storage_id: 0x00010001,
            format_code: format::MP4,
            protection_status: 0,
            compressed_size: eight_gb,
            thumb_format: 0,
            thumb_size: 0,
            parent_handle: MTP_OBJECT_HANDLE_ROOT,
            association_type: 0,
            filename: "4K_Movie.mp4".to_string(),
            date_created: "20260928T000000".to_string(),
            date_modified: "20260928T120000".to_string(),
        };

        assert_eq!(large_video.file_size(), 8_589_934_592);
        assert!(large_video.is_large_file_overflow());
        assert!(!large_video.is_directory());

        let folder = MtpObjectInfo {
            handle: 1002,
            storage_id: 0x00010001,
            format_code: format::ASSOCIATION,
            protection_status: 0,
            compressed_size: 0,
            thumb_format: 0,
            thumb_size: 0,
            parent_handle: MTP_OBJECT_HANDLE_ROOT,
            association_type: 1,
            filename: "DCIM".to_string(),
            date_created: "20260928T000000".to_string(),
            date_modified: "20260928T120000".to_string(),
        };

        assert!(folder.is_directory());
        assert!(!folder.is_large_file_overflow());
    }
}
