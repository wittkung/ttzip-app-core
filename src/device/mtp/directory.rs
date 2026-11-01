// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Lossless, bounded, non-preemptive MTP directory reads used by the GUI.

use super::driver::{
    inspect_device_endpoints, scan_physical_mtp_devices, select_unique_mtp_device,
};
use super::{MtpObjectInfo, MtpSession, RusbTransport, UsbBulkTransport, MTP_OBJECT_HANDLE_ROOT};
use crate::device::entry::DeviceEntry;
use crate::error::{AppCoreError, DeviceErrorKind};
use rusb::UsbContext;
use std::time::Duration;

mod budget;
use budget::DirectoryTransport;

/// Shared budget for session establishment and all directory protocol transactions.
pub const DIRECTORY_PROTOCOL_BUDGET: Duration = Duration::from_secs(15);

fn invalid(message: impl Into<String>) -> AppCoreError {
    AppCoreError::Mtp {
        code: 0,
        message: message.into(),
    }
}

/// Opens an unambiguous device without terminating or resetting other sessions.
pub fn list_hardware_directory(serial: &str, path: &str) -> Result<Vec<DeviceEntry>, AppCoreError> {
    let discovered = scan_physical_mtp_devices()?;
    let target = select_unique_mtp_device(&discovered, serial)?;
    let transport = open_target_transport(target)?;
    let bounded = DirectoryTransport::new(transport, DIRECTORY_PROTOCOL_BUDGET);
    let mut session = MtpSession::new(bounded);
    session.open_directory_session(1)?;
    let result = list_session_directory(&mut session, path);
    // Never send another transaction into a stream with an unknown protocol boundary.
    if result.is_ok() {
        session.close_session()?;
    }
    result
}

fn open_target_transport(target: &super::MtpDeviceInfo) -> Result<RusbTransport, AppCoreError> {
    let context = rusb::Context::new().map_err(|err| {
        AppCoreError::usb(DeviceErrorKind::Transport, format!("USB context: {err}"))
    })?;
    let devices = context.devices().map_err(|err| {
        AppCoreError::usb(
            DeviceErrorKind::Transport,
            format!("USB enumeration: {err}"),
        )
    })?;
    for device in devices.iter() {
        if device.bus_number() == target.bus_number && device.address() == target.device_address {
            return open_matching_transport(&device, target);
        }
    }
    Err(AppCoreError::usb(
        DeviceErrorKind::Offline,
        "MTP device disconnected before directory open",
    ))
}

fn open_matching_transport(
    device: &rusb::Device<rusb::Context>,
    target: &super::MtpDeviceInfo,
) -> Result<RusbTransport, AppCoreError> {
    let descriptor = device.device_descriptor().map_err(|err| {
        AppCoreError::usb(DeviceErrorKind::Transport, format!("USB descriptor: {err}"))
    })?;
    if descriptor.vendor_id() != target.vendor_id || descriptor.product_id() != target.product_id {
        return Err(invalid("MTP device identity changed before directory open"));
    }
    let endpoints = inspect_device_endpoints(device, &descriptor)?
        .ok_or_else(|| invalid("MTP interface is no longer available"))?;
    RusbTransport::open_directory(device, &descriptor, endpoints, &target.serial_number)
}

/// Lists a directory through a live session; no metadata failure is silently discarded.
pub fn list_session_directory<T: UsbBulkTransport>(
    session: &mut MtpSession<T>,
    path: &str,
) -> Result<Vec<DeviceEntry>, AppCoreError> {
    let storage_ids = session.get_storage_ids()?;
    let storage = match storage_ids.as_slice() {
        [storage] => *storage,
        [] => return Err(invalid("MTP device reported no accessible storage")),
        _ => {
            return Err(invalid(
                "MTP device has multiple storages; an explicit storage selection is required",
            ))
        }
    };
    let parent = resolve_parent(session, storage, path)?;
    let handles = session.get_object_handles(storage, 0, parent)?;
    let prefix = if path.starts_with('/') {
        path.trim_end_matches('/')
    } else {
        ""
    };
    // Do not reserve a whole UI entry vector from an untrusted device-reported handle count.
    let mut entries = Vec::new();
    for handle in handles {
        let info = session.get_object_info(handle)?;
        validate_child(&info, storage, parent)?;
        entries.push(DeviceEntry {
            path: format!("{prefix}/{}", info.filename),
            name: info.filename.clone(),
            size: info.file_size(),
            is_dir: info.is_directory(),
            mtime: 0,
            object_id: Some(handle),
            posix_mode: None,
        });
    }
    Ok(entries)
}

fn resolve_parent<T: UsbBulkTransport>(
    session: &mut MtpSession<T>,
    storage: u32,
    path: &str,
) -> Result<u32, AppCoreError> {
    if path == "/" || path.is_empty() {
        return Ok(MTP_OBJECT_HANDLE_ROOT);
    }
    if !path.starts_with('/') {
        let handle = path
            .parse::<u32>()
            .map_err(|_| invalid("MTP path must be absolute or an explicit numeric handle"))?;
        if handle == 0 || handle == u32::MAX {
            return Err(invalid("Invalid explicit MTP directory handle"));
        }
        let info = session.get_object_info(handle)?;
        if !info.is_directory() || info.storage_id != storage {
            return Err(invalid(
                "MTP directory handle does not belong to the selected storage",
            ));
        }
        return Ok(handle);
    }
    let mut parent = MTP_OBJECT_HANDLE_ROOT;
    for segment in path.trim_matches('/').split('/') {
        if !valid_filename(segment) {
            return Err(invalid("Invalid MTP directory path component"));
        }
        parent = find_named_directory(session, storage, parent, segment)?;
    }
    Ok(parent)
}

fn find_named_directory<T: UsbBulkTransport>(
    session: &mut MtpSession<T>,
    storage: u32,
    parent: u32,
    name: &str,
) -> Result<u32, AppCoreError> {
    let mut matched = None;
    for handle in session.get_object_handles(storage, 0, parent)? {
        let info = session.get_object_info(handle)?;
        validate_child(&info, storage, parent)?;
        if info.is_directory() && info.filename == name {
            if matched.is_some() {
                return Err(invalid("Ambiguous MTP directory name"));
            }
            matched = Some(handle);
        }
    }
    matched.ok_or_else(|| invalid("MTP directory was not found; root fallback is prohibited"))
}

fn valid_filename(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\0'])
}

fn validate_child(info: &MtpObjectInfo, storage: u32, parent: u32) -> Result<(), AppCoreError> {
    let parent_matches = info.parent_handle == parent
        || (parent == MTP_OBJECT_HANDLE_ROOT && info.parent_handle == u32::MAX);
    if info.storage_id != storage || !parent_matches || !valid_filename(&info.filename) {
        return Err(invalid(
            "MTP metadata does not match the requested storage and directory",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "directory_tests.rs"]
mod tests;
