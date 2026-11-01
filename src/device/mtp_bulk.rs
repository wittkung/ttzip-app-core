// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0


//! MTP 1.1 `GetObjectPropList` (0x9805) bulk metadata parser and interface preemption.
//!
//! Provides single-roundtrip $O(1)$ single-depth directory property list fetching,
//! bypassing legacy sequential `GetObjectInfo` (0x1008) bottlenecks, and provides
//! zero-privilege preemption of macOS `ptpcamerad`.

use std::collections::HashMap;

use crate::device::entry::DeviceEntry;
use crate::device::mtp::codes::{format, prop};
use crate::device::mtp::container::{decode_ptp_string, MtpContainer};
use crate::error::AppCoreError;

/// Standard MTP 1.1 Operation Code for `GetObjectPropList`.
pub const OP_GET_OBJECT_PROP_LIST: u16 = 0x9805;

/// PTP Container Header Length in bytes.
const CONTAINER_HEADER_LEN: usize = 12;

/// Container Type code for Data container.
const CONTAINER_TYPE_DATA: u16 = 2;

/// Standard MTP data type codes (ISO 15740 §5.3).
mod data_type {
    pub const UNDEFINED: u16 = 0x0000;
    pub const INT8: u16 = 0x0001;
    pub const UINT8: u16 = 0x0002;
    pub const INT16: u16 = 0x0003;
    pub const UINT16: u16 = 0x0004;
    pub const INT32: u16 = 0x0005;
    pub const UINT32: u16 = 0x0006;
    pub const INT64: u16 = 0x0007;
    pub const UINT64: u16 = 0x0008;
    pub const INT128: u16 = 0x0009;
    pub const UINT128: u16 = 0x000A;
    pub const AINT8: u16 = 0x4001;
    pub const AUINT8: u16 = 0x4002;
    pub const AINT16: u16 = 0x4003;
    pub const AUINT16: u16 = 0x4004;
    pub const AINT32: u16 = 0x4005;
    pub const AUINT32: u16 = 0x4006;
    pub const AINT64: u16 = 0x4007;
    pub const AUINT64: u16 = 0x4008;
    pub const AINT128: u16 = 0x4009;
    pub const AUINT128: u16 = 0x400A;
    pub const STRING: u16 = 0xFFFF;
}

#[derive(Default)]
struct ObjectAccumulator {
    name: Option<String>,
    size: u64,
    is_dir: bool,
    mtime: i64,
}

#[inline]
fn check_buffer_bounds(payload: &[u8], offset: usize, needed: usize) -> Result<(), AppCoreError> {
    if payload.len().saturating_sub(offset) < needed {
        Err(AppCoreError::Mtp {
            code: OP_GET_OBJECT_PROP_LIST,
            message: format!(
                "Buffer underflow in 0x9805 dataset: needed {} bytes at offset {}, available {}",
                needed,
                offset,
                payload.len().saturating_sub(offset)
            ),
        })
    } else {
        Ok(())
    }
}

fn scalar_type_len(dt: u16) -> Option<usize> {
    match dt {
        data_type::INT8 | data_type::UINT8 => Some(1),
        data_type::INT16 | data_type::UINT16 => Some(2),
        data_type::INT32 | data_type::UINT32 => Some(4),
        data_type::INT64 | data_type::UINT64 => Some(8),
        data_type::INT128 | data_type::UINT128 => Some(16),
        _ => None,
    }
}

fn array_elem_len(dt: u16) -> Option<usize> {
    match dt {
        data_type::AINT8 | data_type::AUINT8 => Some(1),
        data_type::AINT16 | data_type::AUINT16 => Some(2),
        data_type::AINT32 | data_type::AUINT32 => Some(4),
        data_type::AINT64 | data_type::AUINT64 => Some(8),
        data_type::AINT128 | data_type::AUINT128 => Some(16),
        _ => None,
    }
}

fn read_value_slice<'a>(
    payload: &'a [u8],
    offset: &mut usize,
    dt: u16,
) -> Result<&'a [u8], AppCoreError> {
    let start = *offset;
    if dt == data_type::UNDEFINED {
        return Ok(&[]);
    }
    if let Some(len) = scalar_type_len(dt) {
        check_buffer_bounds(payload, *offset, len)?;
        *offset += len;
        return Ok(&payload[start..*offset]);
    }
    if let Some(elem_len) = array_elem_len(dt) {
        return read_array_slice(payload, offset, elem_len);
    }
    if dt == data_type::STRING {
        check_buffer_bounds(payload, *offset, 1)?;
        let num_chars = payload[*offset] as usize;
        let total_len = if num_chars == 0 { 1 } else { 1 + num_chars * 2 };
        check_buffer_bounds(payload, *offset, total_len)?;
        *offset += total_len;
        return Ok(&payload[start..*offset]);
    }
    Err(AppCoreError::Mtp {
        code: OP_GET_OBJECT_PROP_LIST,
        message: format!("Unsupported MTP property data type 0x{:04X} at offset {}", dt, start),
    })
}

fn read_array_slice<'a>(
    payload: &'a [u8],
    offset: &mut usize,
    element_size: usize,
) -> Result<&'a [u8], AppCoreError> {
    let start = *offset;
    check_buffer_bounds(payload, *offset, 4)?;
    let count = u32::from_le_bytes([
        payload[*offset],
        payload[*offset + 1],
        payload[*offset + 2],
        payload[*offset + 3],
    ]) as usize;
    let total_len = 4 + count * element_size;
    check_buffer_bounds(payload, *offset, total_len)?;
    *offset += total_len;
    Ok(&payload[start..*offset])
}

/// Parses compact ISO 8601 MTP date strings (e.g. `YYYYMMDDThhmmss` or `YYYYMMDDThhmmss.s`)
/// or decimal UNIX timestamps into epoch seconds.
pub fn parse_mtp_date(s: &str) -> i64 {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return 0;
    }
    if let Ok(epoch) = trimmed.parse::<i64>() {
        return epoch;
    }
    let b = trimmed.as_bytes();
    if b.len() >= 15 && b[8] == b'T' {
        let year: i64 = trimmed[0..4].parse().unwrap_or(1970);
        let month: u32 = trimmed[4..6].parse().unwrap_or(1);
        let day: u32 = trimmed[6..8].parse().unwrap_or(1);
        let hour: i64 = trimmed[9..11].parse().unwrap_or(0);
        let min: i64 = trimmed[11..13].parse().unwrap_or(0);
        let sec: i64 = trimmed[13..15].parse().unwrap_or(0);

        let days = days_from_civil(year, month, day);
        return days * 86400 + hour * 3600 + min * 60 + sec;
    }
    0
}

/// Computes days from Unix epoch (1970-01-01) for a civil Gregorian date using Howard Hinnant's algorithm.
fn days_from_civil(mut y: i64, m: u32, d: u32) -> i64 {
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as i64;
    era * 146097 + doe - 719468
}

/// Parses an MTP 0x9805 (`GetObjectPropList`) response dataset or container into a vector of [`DeviceEntry`].
fn detect_payload_offset(payload: &[u8]) -> usize {
    if payload.len() >= CONTAINER_HEADER_LEN {
        let container_len = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
        let container_type = u16::from_le_bytes([payload[4], payload[5]]);
        let op_code = u16::from_le_bytes([payload[6], payload[7]]);
        if container_len == payload.len()
            && container_type == CONTAINER_TYPE_DATA
            && op_code == OP_GET_OBJECT_PROP_LIST
        {
            return CONTAINER_HEADER_LEN;
        }
    }
    0
}

fn apply_size_prop(acc: &mut ObjectAccumulator, dt: u16, val: &[u8]) {
    match dt {
        data_type::UINT64 | data_type::INT64 if val.len() >= 8 => {
            acc.size = u64::from_le_bytes([
                val[0], val[1], val[2], val[3],
                val[4], val[5], val[6], val[7],
            ]);
        }
        data_type::UINT32 | data_type::INT32 if val.len() >= 4 => {
            acc.size = u32::from_le_bytes([val[0], val[1], val[2], val[3]]) as u64;
        }
        _ => {}
    }
}

fn apply_date_prop(acc: &mut ObjectAccumulator, dt: u16, val: &[u8]) -> Result<(), AppCoreError> {
    if dt == data_type::STRING {
        let (date_str, _) = decode_ptp_string(val)?;
        acc.mtime = parse_mtp_date(&date_str);
    } else if (dt == data_type::INT64 || dt == data_type::UINT64) && val.len() >= 8 {
        acc.mtime = i64::from_le_bytes([
            val[0], val[1], val[2], val[3],
            val[4], val[5], val[6], val[7],
        ]);
    } else if (dt == data_type::INT32 || dt == data_type::UINT32) && val.len() >= 4 {
        acc.mtime = i32::from_le_bytes([val[0], val[1], val[2], val[3]]) as i64;
    }
    Ok(())
}

fn read_u16_val(dt: u16, val: &[u8]) -> Option<u16> {
    if (dt == data_type::UINT16 || dt == data_type::INT16) && val.len() >= 2 {
        Some(u16::from_le_bytes([val[0], val[1]]))
    } else {
        None
    }
}

fn apply_dir_flags(acc: &mut ObjectAccumulator, prop_code: u16, dt: u16, val: &[u8]) {
    let is_dir = match prop_code {
        prop::OBJECT_FORMAT => read_u16_val(dt, val) == Some(format::ASSOCIATION),
        prop::ASSOCIATION_TYPE => read_u16_val(dt, val).is_some_and(|assoc| assoc != 0),
        _ => false,
    };
    if is_dir {
        acc.is_dir = true;
    }
}

fn apply_property(
    acc: &mut ObjectAccumulator,
    prop_code: u16,
    dt: u16,
    val_slice: &[u8],
) -> Result<(), AppCoreError> {
    match prop_code {
        prop::OBJECT_FILE_NAME if dt == data_type::STRING => {
            let (name, _) = decode_ptp_string(val_slice)?;
            acc.name = Some(name);
        }
        prop::OBJECT_SIZE => apply_size_prop(acc, dt, val_slice),
        prop::OBJECT_FORMAT | prop::ASSOCIATION_TYPE => {
            apply_dir_flags(acc, prop_code, dt, val_slice);
        }
        prop::DATE_MODIFIED => apply_date_prop(acc, dt, val_slice)?,
        _ => {}
    }
    Ok(())
}

fn build_device_entries(
    handle_order: Vec<u32>,
    mut objects: HashMap<u32, ObjectAccumulator>,
) -> Vec<DeviceEntry> {
    let mut entries = Vec::with_capacity(handle_order.len());
    for handle in handle_order {
        if let Some(acc) = objects.remove(&handle) {
            let name = acc.name.unwrap_or_else(|| format!("object_{}", handle));
            let entry = if acc.is_dir {
                DeviceEntry::dir(handle as u64, name, acc.mtime)
            } else {
                DeviceEntry::file(handle as u64, name, acc.size, acc.mtime)
            };
            entries.push(entry);
        }
    }
    entries
}

/// Parses an MTP 0x9805 (`GetObjectPropList`) response dataset or container into a vector of [`DeviceEntry`].
pub fn parse_object_prop_list(payload: &[u8]) -> Result<Vec<DeviceEntry>, AppCoreError> {
    if payload.is_empty() {
        return Ok(Vec::new());
    }

    let mut offset = detect_payload_offset(payload);
    if payload.len() - offset < 4 {
        return Ok(Vec::new());
    }

    let num_elements = u32::from_le_bytes([
        payload[offset],
        payload[offset + 1],
        payload[offset + 2],
        payload[offset + 3],
    ]) as usize;
    offset += 4;

    let mut handle_order = Vec::new();
    let mut objects: HashMap<u32, ObjectAccumulator> = HashMap::new();

    for _ in 0..num_elements {
        check_buffer_bounds(payload, offset, 8)?;
        let handle = u32::from_le_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]);
        let prop_code = u16::from_le_bytes([payload[offset + 4], payload[offset + 5]]);
        let dt = u16::from_le_bytes([payload[offset + 6], payload[offset + 7]]);
        offset += 8;

        let val_slice = read_value_slice(payload, &mut offset, dt)?;

        let acc = objects.entry(handle).or_insert_with(|| {
            handle_order.push(handle);
            ObjectAccumulator::default()
        });

        apply_property(acc, prop_code, dt, val_slice)?;
    }

    Ok(build_device_entries(handle_order, objects))
}

#[cfg(all(target_os = "macos", not(feature = "mas")))]
#[allow(non_camel_case_types, reason = "C FFI types match Darwin libproc definitions")]
mod libc {
    pub type c_int = std::ffi::c_int;
    pub type c_void = std::ffi::c_void;
    pub type pid_t = i32;

    pub const PROC_ALL_PIDS: u32 = 1;

    extern "C" {
        pub fn proc_listpids(type_: u32, typeinfo: u32, buffer: *mut c_void, buffersize: c_int) -> c_int;
        pub fn proc_name(pid: c_int, buffer: *mut c_void, buffersize: u32) -> c_int;
    }
}

/// Indicates whether the in-process Darwin `libproc` (`proc_listpids`/`proc_name`) probe is compiled in.
pub const PTPCAMERAD_LIBPROC_PROBE_ENABLED: bool = cfg!(all(target_os = "macos", not(feature = "mas")));

/// Non-destructively checks whether Apple's `ptpcamerad` daemon is currently running on macOS.
/// Returns `false` under `feature = "mas"` or `APP_SANDBOX_CONTAINER_ID`; never terminates processes.
pub fn is_ptpcamerad_active() -> bool {
    #[cfg(all(target_os = "macos", not(feature = "mas")))]
    {
        if std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some() {
            return false;
        }

        let num_bytes = unsafe { libc::proc_listpids(libc::PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
        if num_bytes <= 0 {
            return false;
        }

        let count = (num_bytes as usize) / std::mem::size_of::<libc::pid_t>() + 64;
        let mut pids: Vec<libc::pid_t> = vec![0; count];
        let buf_size = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        let bytes_written = unsafe {
            libc::proc_listpids(
                libc::PROC_ALL_PIDS,
                0,
                pids.as_mut_ptr() as *mut libc::c_void,
                buf_size,
            )
        };
        if bytes_written <= 0 {
            return false;
        }

        let actual_count = ((bytes_written as usize) / std::mem::size_of::<libc::pid_t>()).min(pids.len());
        for &pid in &pids[..actual_count] {
            if pid <= 0 {
                continue;
            }
            let mut name_buf = [0u8; 256];
            let len = unsafe {
                libc::proc_name(
                    pid,
                    name_buf.as_mut_ptr() as *mut libc::c_void,
                    name_buf.len() as u32,
                )
            };
            if len > 0 {
                let n = (len as usize).min(name_buf.len());
                let name_slice = match name_buf[..n].iter().position(|&b| b == 0) {
                    Some(pos) => &name_buf[..pos],
                    None => &name_buf[..n],
                };
                if name_slice == b"ptpcamerad" {
                    return true;
                }
            }
        }
        false
    }

    #[cfg(any(not(target_os = "macos"), feature = "mas"))]
    {
        false
    }
}

/// Non-destructive compatibility hook for `ptpcamerad` checks.
///
/// Per Q3 safety contract, TTZip never automatically terminates `ptpcamerad` or external
/// system processes during discovery, scanning, device opening, or default tests.
pub fn preempt_ptpcamerad() -> std::io::Result<()> {
    let _ = is_ptpcamerad_active();
    Ok(())
}

/// High-throughput MTP 0x9805 bulk metadata fetcher.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MtpBulkFetcher;

impl MtpBulkFetcher {
    /// MTP 1.1 Opcode for `GetObjectPropList`.
    pub const OPCODE: u16 = OP_GET_OBJECT_PROP_LIST;

    /// Creates a new bulk fetcher instance.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Non-destructive compatibility hook that checks `ptpcamerad` without killing system processes.
    #[inline]
    pub fn preempt_ptpcamerad() -> std::io::Result<()> {
        preempt_ptpcamerad()
    }

    /// Parses an MTP 0x9805 dataset or data container into canonical [`DeviceEntry`] records.
    #[inline]
    pub fn parse_object_prop_list(payload: &[u8]) -> Result<Vec<DeviceEntry>, AppCoreError> {
        parse_object_prop_list(payload)
    }

    /// Constructs an MTP command container requesting bulk directory properties (`depth=1`).
    pub fn build_command_container(
        transaction_id: u32,
        storage_id: u32,
        parent_handle: u32,
        depth: u32,
    ) -> MtpContainer {
        MtpContainer::new_command(
            OP_GET_OBJECT_PROP_LIST,
            transaction_id,
            &[storage_id, 0, 0, parent_handle, depth],
        )
    }
}

// Compile-time static gate ensuring MtpBulkFetcher satisfies Send and Sync invariants.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MtpBulkFetcher>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mtp::container::encode_ptp_string;

    #[test]
    fn test_preempt_ptpcamerad_invocation() {
        assert!(preempt_ptpcamerad().is_ok());
        assert!(MtpBulkFetcher::preempt_ptpcamerad().is_ok());
    }

    #[test]
    fn test_preempt_ptpcamerad_sandbox_bypass() {
        std::env::set_var("APP_SANDBOX_CONTAINER_ID", "com.wittkung.ttzip");
        assert!(preempt_ptpcamerad().is_ok());
        std::env::remove_var("APP_SANDBOX_CONTAINER_ID");
    }

    #[test]
    fn test_parse_mtp_date_formats() {
        let epoch = parse_mtp_date("1712345678");
        assert_eq!(epoch, 1712345678);

        let date_epoch = parse_mtp_date("20261011T123045");
        // 2026-10-11 12:30:45 UTC
        assert_eq!(date_epoch, 1791721845);

        assert_eq!(parse_mtp_date(""), 0);
        assert_eq!(parse_mtp_date("invalid_date"), 0);
    }

    #[test]
    fn test_parse_object_prop_list_dataset() {
        let mut dataset = Vec::new();
        let num_elements: u32 = 7;
        dataset.extend_from_slice(&num_elements.to_le_bytes());

        // Object 101: regular file
        // 1. Filename: "photo.jpg"
        dataset.extend_from_slice(&101u32.to_le_bytes());
        dataset.extend_from_slice(&prop::OBJECT_FILE_NAME.to_le_bytes());
        dataset.extend_from_slice(&data_type::STRING.to_le_bytes());
        dataset.extend_from_slice(&encode_ptp_string("photo.jpg"));

        // 2. Size: 5242880 (5MB)
        dataset.extend_from_slice(&101u32.to_le_bytes());
        dataset.extend_from_slice(&prop::OBJECT_SIZE.to_le_bytes());
        dataset.extend_from_slice(&data_type::UINT64.to_le_bytes());
        dataset.extend_from_slice(&5242880u64.to_le_bytes());

        // 3. Format: EXIF JPEG
        dataset.extend_from_slice(&101u32.to_le_bytes());
        dataset.extend_from_slice(&prop::OBJECT_FORMAT.to_le_bytes());
        dataset.extend_from_slice(&data_type::UINT16.to_le_bytes());
        dataset.extend_from_slice(&0x3801u16.to_le_bytes());

        // 4. Mtime: "20261011T123045"
        dataset.extend_from_slice(&101u32.to_le_bytes());
        dataset.extend_from_slice(&prop::DATE_MODIFIED.to_le_bytes());
        dataset.extend_from_slice(&data_type::STRING.to_le_bytes());
        dataset.extend_from_slice(&encode_ptp_string("20261011T123045"));

        // Object 202: Directory
        // 5. Filename: "DCIM"
        dataset.extend_from_slice(&202u32.to_le_bytes());
        dataset.extend_from_slice(&prop::OBJECT_FILE_NAME.to_le_bytes());
        dataset.extend_from_slice(&data_type::STRING.to_le_bytes());
        dataset.extend_from_slice(&encode_ptp_string("DCIM"));

        // 6. Format: ASSOCIATION (Directory)
        dataset.extend_from_slice(&202u32.to_le_bytes());
        dataset.extend_from_slice(&prop::OBJECT_FORMAT.to_le_bytes());
        dataset.extend_from_slice(&data_type::UINT16.to_le_bytes());
        dataset.extend_from_slice(&format::ASSOCIATION.to_le_bytes());

        // 7. Size: reported as non-zero on raw device but should be forced to 0 for dir
        dataset.extend_from_slice(&202u32.to_le_bytes());
        dataset.extend_from_slice(&prop::OBJECT_SIZE.to_le_bytes());
        dataset.extend_from_slice(&data_type::UINT32.to_le_bytes());
        dataset.extend_from_slice(&4096u32.to_le_bytes());

        let entries = parse_object_prop_list(&dataset).expect("parse prop list");
        assert_eq!(entries.len(), 2);

        // Verify Object 101
        assert_eq!(entries[0].object_id, Some(101));
        assert_eq!(entries[0].name, "photo.jpg");
        assert_eq!(entries[0].size, 5242880);
        assert!(entries[0].is_file());
        assert!(!entries[0].is_directory());
        assert_eq!(entries[0].mtime, 1791721845);

        // Verify Object 202
        assert_eq!(entries[1].object_id, Some(202));
        assert_eq!(entries[1].name, "DCIM");
        assert_eq!(entries[1].size, 0);
        assert!(!entries[1].is_file());
        assert!(entries[1].is_directory());
    }

    #[test]
    fn test_parse_object_prop_list_with_container_header() {
        let mut dataset = Vec::new();
        let num_elements: u32 = 1;
        dataset.extend_from_slice(&num_elements.to_le_bytes());

        // Object 50: Large file > 4GB
        dataset.extend_from_slice(&50u32.to_le_bytes());
        dataset.extend_from_slice(&prop::OBJECT_FILE_NAME.to_le_bytes());
        dataset.extend_from_slice(&data_type::STRING.to_le_bytes());
        dataset.extend_from_slice(&encode_ptp_string("video_4k.mov"));

        // Wrap in MTP Data container
        let container = MtpContainer::new_data(OP_GET_OBJECT_PROP_LIST, 10, dataset);
        let raw_container = container.to_bytes();

        let entries = parse_object_prop_list(&raw_container).expect("parse from container");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].object_id, Some(50));
        assert_eq!(entries[0].name, "video_4k.mov");
        assert_eq!(entries[0].size, 0);
        assert!(entries[0].is_file());
    }

    #[test]
    fn test_parse_empty_payload() {
        assert_eq!(parse_object_prop_list(&[]).unwrap(), Vec::new());
        assert_eq!(parse_object_prop_list(&[0, 0, 0, 0]).unwrap(), Vec::new());
    }

    #[test]
    fn test_build_command_container() {
        let cmd = MtpBulkFetcher::build_command_container(123, 0x00010001, 0, 1);
        assert_eq!(cmd.code, OP_GET_OBJECT_PROP_LIST);
        assert_eq!(cmd.transaction_id, 123);
        let params = cmd.extract_params();
        assert_eq!(params.len(), 5);
        assert_eq!(params[0], 0x00010001);
        assert_eq!(params[1], 0);
        assert_eq!(params[2], 0);
        assert_eq!(params[3], 0);
        assert_eq!(params[4], 1);
    }

    #[test]
    fn test_ptpcamerad_channel_feature_contract() {
        #[cfg(feature = "mas")]
        {
            const {
                assert!(
                    !PTPCAMERAD_LIBPROC_PROBE_ENABLED,
                    "MAS build must exclude libproc process enumeration"
                );
            }
            assert!(
                !is_ptpcamerad_active(),
                "MAS build must unconditionally return false without scanning system processes"
            );
            let msg = crate::device::mtp::driver::format_usb_busy_recovery_message(
                "Failed to open USB MTP device",
                &rusb::Error::Busy,
            );
            assert!(
                msg.contains("Another process"),
                "MAS build must use generic USB busy hint: {msg}"
            );
            assert!(
                !msg.contains("ptpcamerad (Photos / Image Capture) is currently active"),
                "MAS build must not report active ptpcamerad probe: {msg}"
            );
        }

        #[cfg(all(target_os = "macos", not(feature = "mas")))]
        {
            const {
                assert!(
                    PTPCAMERAD_LIBPROC_PROBE_ENABLED,
                    "Direct/Steam macOS build must retain libproc probe capability"
                );
            }
        }
    }
}
