// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! High-level USB MTP 1.1 streaming file transfer and target-file protection routines.
//!
//! Bridges UI/Coordinator requests (`deviceId`, `storageId`, `objectHandle`, `remotePath`)
//! to `MtpSession` streaming reads and writes while enforcing handle-level target protection:
//! - Overwrite uploads keep the existing target handle untouched until the newly created handle
//!   completes streaming and passes `GetObjectInfo` byte-length verification.
//! - Mid-transfer cancellation or USB disconnect attempts `DeleteObject(created_handle)` and
//!   never deletes the pre-existing object handle.

use sha2::{Digest, Sha256};
use std::io::Write;

use crate::device::mtp::driver::{
    inspect_device_endpoints, scan_physical_mtp_devices, select_unique_mtp_device, RusbTransport,
    UsbBulkTransport,
};
use crate::device::mtp::session::MtpSession;
use crate::device::mtp::types::{MtpObjectInfo, MTP_OBJECT_HANDLE_ROOT};
use crate::error::{AppCoreError, DeviceErrorKind};

mod upload;
pub use upload::push_reader_with_session;

/// Canonical cancellation sentinel used by streaming callbacks.
pub const MTP_TRANSFER_CANCELLED_REASON: &str = "MTP transfer cancelled by caller";

/// Default maximum number of `(N)` candidate names probed under `MtpConflictPolicy::KeepBoth`.
pub const DEFAULT_MAX_KEEP_BOTH_CANDIDATES: u32 = 10_000;

/// Returns whether an `AppCoreError` represents a caller-initiated transfer cancellation.
#[inline]
pub fn is_mtp_transfer_cancelled(err: &AppCoreError) -> bool {
    match err {
        AppCoreError::Transfer { reason } => reason == MTP_TRANSFER_CANCELLED_REASON,
        _ => false,
    }
}

/// Conflict resolution strategy when an object with the same filename already exists in the target parent folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
#[repr(u8)]
pub enum MtpConflictPolicy {
    /// Upload new object first, verify source stream SHA-256 and `GetObjectInfo` size, then delete the previous handle.
    Overwrite = 0,
    /// Preserve the existing object handle and skip uploading.
    Skip = 1,
    /// Allocate a non-colliding filename (`name (1).ext`) in the same parent folder; fail if candidates are exhausted.
    KeepBoth = 2,
}

impl MtpConflictPolicy {
    /// Converts a raw byte code into an `MtpConflictPolicy`.
    #[inline]
    pub fn from_u8(raw: u8) -> Self {
        match raw {
            1 => Self::Skip,
            2 => Self::KeepBoth,
            _ => Self::Overwrite,
        }
    }
}

/// Verified outcome of an MTP upload operation, explicitly separating verification tiers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MtpUploadOutcome {
    /// Newly created (or preserved when skipped) MTP object handle.
    pub created_handle: u32,
    /// Final committed filename inside the target parent directory.
    pub final_filename: String,
    /// Total payload bytes transferred and verified.
    pub bytes_transferred: u64,
    /// Lowercase hex-encoded SHA-256 digest of the actual streamed source bytes.
    pub sha256_hex: String,
    /// True when the actual streamed bytes matched the caller's expected source SHA-256 before commit.
    pub source_stream_sha256_verified: bool,
    /// True when `GetObjectInfo` on the created handle matched the exact uploaded byte length.
    pub remote_size_verified: bool,
    /// True only when a full remote readback (`GetObject`) was performed and matched `sha256_hex`.
    pub remote_readback_sha256_verified: bool,
    /// Whether the upload was skipped due to `MtpConflictPolicy::Skip`.
    pub skipped: bool,
}

/// Verified outcome of an MTP download operation.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MtpDownloadOutcome {
    /// Total payload bytes streamed and verified against `GetObjectInfo`.
    pub bytes_transferred: u64,
    /// Lowercase hex-encoded SHA-256 digest of the downloaded payload written to the staged file.
    pub sha256_hex: String,
}

/// UniFFI callback interface for reporting streaming MTP transfer progress and supporting cancellation.
#[uniffi::export(callback_interface)]
pub trait MtpTransferProgressCallback: Send + Sync {
    /// Returns `true` to continue streaming or `false` to cancel and clean up any incomplete object.
    fn on_progress(&self, transferred_bytes: u64, total_bytes: u64) -> bool;
}

/// Resolves an active storage ID on the session, falling back to the primary storage partition when `0` is passed.
pub fn resolve_storage_id<T: UsbBulkTransport>(
    session: &mut MtpSession<T>,
    requested_storage_id: u32,
) -> Result<u32, AppCoreError> {
    if requested_storage_id != 0 {
        return Ok(requested_storage_id);
    }
    let storages = session.get_storage_ids()?;
    storages.first().copied().ok_or_else(|| AppCoreError::Mtp {
        code: 0,
        message: "No storage partitions found on MTP device".to_string(),
    })
}

/// Resolves a directory path or numeric handle string into an MTP parent object handle.
pub fn resolve_directory_handle_by_path<T: UsbBulkTransport>(
    session: &mut MtpSession<T>,
    storage_id: u32,
    remote_dir_path: &str,
) -> Result<u32, AppCoreError> {
    let trimmed = remote_dir_path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(MTP_OBJECT_HANDLE_ROOT);
    }
    if let Ok(explicit_handle) = trimmed.parse::<u32>() {
        if explicit_handle != 0 {
            return Ok(explicit_handle);
        }
    }

    let mut current_parent = MTP_OBJECT_HANDLE_ROOT;
    for segment in trimmed.split('/').filter(|s| !s.is_empty()) {
        let handles = session.get_object_handles(storage_id, 0, current_parent)?;
        let mut matched_dir = None;
        for handle in handles {
            let info = session.get_object_info(handle)?;
            if info.is_directory() && info.filename == segment {
                matched_dir = Some(handle);
                break;
            }
        }
        current_parent = matched_dir.ok_or_else(|| AppCoreError::Mtp {
            code: 0,
            message: format!(
                "MTP directory segment '{}' in path '{}' not found on storage 0x{:08X}",
                segment, remote_dir_path, storage_id
            ),
        })?;
    }
    Ok(current_parent)
}

fn parse_explicit_file_handle(path: &str) -> Option<u32> {
    path.parse::<u32>()
        .ok()
        .filter(|&handle| handle != 0 && handle != MTP_OBJECT_HANDLE_ROOT)
}

/// Resolves a file object handle and its `MtpObjectInfo` from either an explicit handle or a VFS path.
pub fn resolve_file_handle<T: UsbBulkTransport>(
    session: &mut MtpSession<T>,
    storage_id: u32,
    explicit_handle: u32,
    remote_path: &str,
) -> Result<(u32, MtpObjectInfo), AppCoreError> {
    if explicit_handle != 0 && explicit_handle != MTP_OBJECT_HANDLE_ROOT {
        let info = session.get_object_info(explicit_handle)?;
        return Ok((explicit_handle, info));
    }

    let trimmed = remote_path.trim_matches('/');
    if trimmed.is_empty() {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: "Cannot resolve root directory '/' as an MTP file object".to_string(),
        });
    }
    if let Some(parsed_handle) = parse_explicit_file_handle(trimmed) {
        let info = session.get_object_info(parsed_handle)?;
        return Ok((parsed_handle, info));
    }

    let (parent_dir, file_name) = match trimmed.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", trimmed),
    };
    let parent_handle = resolve_directory_handle_by_path(session, storage_id, parent_dir)?;
    let handles = session.get_object_handles(storage_id, 0, parent_handle)?;
    for handle in handles {
        let info = session.get_object_info(handle)?;
        if info.filename == file_name {
            return Ok((handle, info));
        }
    }

    Err(AppCoreError::Mtp {
        code: 0,
        message: format!(
            "MTP file '{}' not found in parent handle 0x{:08X}",
            remote_path, parent_handle
        ),
    })
}

/// Streams an MTP file object via an active `MtpSession`, verifying total length and computing its SHA-256 digest.
pub fn pull_object_streaming_with_session<T, F>(
    session: &mut MtpSession<T>,
    requested_storage_id: u32,
    explicit_handle: u32,
    remote_path: &str,
    mut on_chunk: F,
) -> Result<(u64, String), AppCoreError>
where
    T: UsbBulkTransport,
    F: FnMut(&[u8], u64, u64) -> Result<(), AppCoreError>,
{
    let storage_id = if explicit_handle != 0 && explicit_handle != MTP_OBJECT_HANDLE_ROOT {
        requested_storage_id
    } else {
        resolve_storage_id(session, requested_storage_id)?
    };
    let (handle, info) = resolve_file_handle(session, storage_id, explicit_handle, remote_path)?;
    if info.is_directory() {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: format!(
                "MTP handle 0x{:08X} ('{}') is a directory, not a regular file",
                handle, info.filename
            ),
        });
    }

    let expected_size = if info.is_large_file_overflow() {
        let prop_size = session.get_object_size(handle)?;
        if prop_size > MtpSession::<T>::MAX_MTP_SEND_OBJECT_BYTES {
            return Err(AppCoreError::Mtp {
                code: crate::device::mtp::codes::resp::PARAMETER_NOT_SUPPORTED,
                message: format!(
                    "MTP object '{}' (handle 0x{:08X}) size {} exceeds 32-bit PTP GetObject container limit",
                    info.filename, handle, prop_size
                ),
            });
        }
        prop_size
    } else {
        info.file_size()
    };

    let mut hasher = Sha256::new();
    let mut cumulative_bytes = 0u64;
    let bytes_read = session.get_object_streaming(handle, |chunk| {
        hasher.update(chunk);
        cumulative_bytes += chunk.len() as u64;
        on_chunk(chunk, cumulative_bytes, expected_size)
    })?;

    if bytes_read != expected_size {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: format!(
                "MTP download size mismatch for '{}': expected {} bytes from GetObjectInfo, read {} bytes",
                info.filename, expected_size, bytes_read
            ),
        });
    }

    let sha256_hex = format!("{:x}", hasher.finalize());
    Ok((bytes_read, sha256_hex))
}

/// Uploads an in-memory payload slice to an MTP parent directory with conflict resolution and target handle protection.
pub fn push_object_with_session<T, F>(
    session: &mut MtpSession<T>,
    requested_storage_id: u32,
    parent_dir_or_handle: &str,
    filename: &str,
    data: &[u8],
    conflict_policy: MtpConflictPolicy,
    on_progress: F,
) -> Result<MtpUploadOutcome, AppCoreError>
where
    T: UsbBulkTransport,
    F: FnMut(u64, u64) -> Result<(), AppCoreError>,
{
    let mut cursor = std::io::Cursor::new(data);
    push_reader_with_session(
        session,
        requested_storage_id,
        parent_dir_or_handle,
        filename,
        data.len() as u64,
        &mut cursor,
        None,
        conflict_policy,
        DEFAULT_MAX_KEEP_BOTH_CANDIDATES,
        on_progress,
    )
}

/// Allocates a non-colliding filename (`stem (idx).ext`) up to `max_candidates`, returning an explicit
/// error if all candidates are occupied instead of falling back to the original filename.
pub fn allocate_non_colliding_filename(
    original: &str,
    existing: &[(u32, String)],
    max_candidates: u32,
) -> Result<String, AppCoreError> {
    let (stem, ext) = match original.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s, format!(".{}", e)),
        _ => (original, String::new()),
    };
    for idx in 1..=max_candidates {
        let candidate = format!("{} ({}){}", stem, idx, ext);
        if !existing.iter().any(|(_, name)| name == &candidate) {
            return Ok(candidate);
        }
    }
    Err(AppCoreError::Mtp {
        code: 0,
        message: format!(
            "MTP KeepBoth candidate names exhausted (limit {}) for '{}'",
            max_candidates, original
        ),
    })
}

/// Opens a non-preemptive hardware `MtpSession<RusbTransport>` bound unambiguously to `serial_or_id`.
///
/// Rejects ambiguous selectors (such as `VID:PID` when multiple devices of the same model are connected),
/// binds to the exact USB `(bus_number, device_address)`, and re-verifies the hardware serial number
/// from the opened USB device handle before claiming the interface or opening an MTP session.
pub fn open_hardware_mtp_session(
    serial_or_id: &str,
) -> Result<MtpSession<RusbTransport>, AppCoreError> {
    use rusb::UsbContext;
    let discovered = scan_physical_mtp_devices()?;
    let target = select_unique_mtp_device(&discovered, serial_or_id)?;

    let context = rusb::Context::new().map_err(|e| {
        AppCoreError::usb(
            DeviceErrorKind::Transport,
            format!("Failed to initialize libusb context: {}", e),
        )
    })?;

    let devices = context.devices().map_err(|e| {
        AppCoreError::usb(
            DeviceErrorKind::Transport,
            format!("Failed to enumerate USB bus: {}", e),
        )
    })?;

    for dev in devices.iter() {
        if dev.bus_number() != target.bus_number || dev.address() != target.device_address {
            continue;
        }
        let Ok(desc) = dev.device_descriptor() else {
            continue;
        };
        if desc.vendor_id() != target.vendor_id || desc.product_id() != target.product_id {
            continue;
        }
        let Ok(Some(endpoints)) = inspect_device_endpoints(&dev, &desc) else {
            continue;
        };
        let transport =
            RusbTransport::open_verified(&dev, &desc, endpoints, &target.serial_number)?;
        let mut session = MtpSession::new(transport);
        session.open_session(1)?;
        return Ok(session);
    }

    Err(AppCoreError::usb(
        DeviceErrorKind::Offline,
        format!(
            "MTP hardware device '{}' (bus {}, address {}) detached before session open",
            target.serial_number, target.bus_number, target.device_address
        ),
    ))
}

/// Opens a hardware MTP session and streams a remote object via `on_chunk`.
pub fn pull_mtp_file_streaming<F>(
    serial_or_id: &str,
    storage_id: u32,
    object_handle: u32,
    remote_path: &str,
    on_chunk: F,
) -> Result<(u64, String), AppCoreError>
where
    F: FnMut(&[u8], u64, u64) -> Result<(), AppCoreError>,
{
    let mut session = open_hardware_mtp_session(serial_or_id)?;
    let result = pull_object_streaming_with_session(
        &mut session,
        storage_id,
        object_handle,
        remote_path,
        on_chunk,
    );
    let _ = session.close_session();
    result
}

/// Opens a hardware MTP session and streams a local file from disk in bounded 64 KiB chunks.
#[allow(
    clippy::too_many_arguments,
    reason = "Hardware MTP file upload entrypoint with explicit source path and expected SHA-256"
)]
pub fn push_local_file_path_to_mtp<F>(
    serial_or_id: &str,
    storage_id: u32,
    parent_dir_or_handle: &str,
    filename: &str,
    local_source_path: &std::path::Path,
    expected_sha256_hex: Option<&str>,
    conflict_policy: MtpConflictPolicy,
    max_keep_both_candidates: u32,
    on_progress: F,
) -> Result<MtpUploadOutcome, AppCoreError>
where
    F: FnMut(u64, u64) -> Result<(), AppCoreError>,
{
    let mut file = std::fs::File::open(local_source_path).map_err(|e| AppCoreError::Io {
        reason: format!(
            "Failed to open local source file '{}' for MTP upload: {}",
            local_source_path.display(),
            e
        ),
    })?;
    let total_size = file
        .metadata()
        .map_err(|e| AppCoreError::Io {
            reason: format!(
                "Failed to stat local source file '{}' for MTP upload: {}",
                local_source_path.display(),
                e
            ),
        })?
        .len();

    let mut session = open_hardware_mtp_session(serial_or_id)?;
    let result = push_reader_with_session(
        &mut session,
        storage_id,
        parent_dir_or_handle,
        filename,
        total_size,
        &mut file,
        expected_sha256_hex,
        conflict_policy,
        max_keep_both_candidates.max(1),
        on_progress,
    );
    let _ = session.close_session();
    result
}

/// Opens a hardware MTP session and uploads a local file buffer to the target parent directory.
pub fn push_local_file_to_mtp<F>(
    serial_or_id: &str,
    storage_id: u32,
    parent_dir_or_handle: &str,
    filename: &str,
    data: &[u8],
    conflict_policy: MtpConflictPolicy,
    on_progress: F,
) -> Result<MtpUploadOutcome, AppCoreError>
where
    F: FnMut(u64, u64) -> Result<(), AppCoreError>,
{
    let mut session = open_hardware_mtp_session(serial_or_id)?;
    let result = push_object_with_session(
        &mut session,
        storage_id,
        parent_dir_or_handle,
        filename,
        data,
        conflict_policy,
        on_progress,
    );
    let _ = session.close_session();
    result
}

/// Opens a hardware MTP session and deletes a remote object resolved by handle or path.
pub fn delete_mtp_object(
    serial_or_id: &str,
    storage_id: u32,
    object_handle: u32,
    remote_path: &str,
) -> Result<(), AppCoreError> {
    let mut session = open_hardware_mtp_session(serial_or_id)?;
    let resolved_sid = resolve_storage_id(&mut session, storage_id)?;
    let result = resolve_file_handle(&mut session, resolved_sid, object_handle, remote_path)
        .and_then(|(handle, _)| session.delete_object(handle));
    let _ = session.close_session();
    result
}

/// UniFFI-exported MTP streaming download to a staged local file path.
#[uniffi::export]
pub fn mtp_pull_file_to_local_staged(
    serial_or_id: String,
    storage_id: u32,
    object_handle: u32,
    remote_path: String,
    staged_local_path: String,
    progress: Box<dyn MtpTransferProgressCallback>,
) -> Result<MtpDownloadOutcome, crate::uniffi_api::UniFFIAppCoreError> {
    let staged_path = std::path::PathBuf::from(&staged_local_path);
    let mut out_file = std::fs::File::create(&staged_path).map_err(|e| AppCoreError::Io {
        reason: format!(
            "Failed to create staged local file '{}': {}",
            staged_local_path, e
        ),
    })?;

    let pull_res = pull_mtp_file_streaming(
        &serial_or_id,
        storage_id,
        object_handle,
        &remote_path,
        |chunk, received, total| {
            out_file.write_all(chunk).map_err(|e| AppCoreError::Io {
                reason: format!(
                    "Failed to write chunk to staged file '{}': {}",
                    staged_local_path, e
                ),
            })?;
            if !progress.on_progress(received, total) {
                return Err(AppCoreError::Transfer {
                    reason: MTP_TRANSFER_CANCELLED_REASON.to_string(),
                });
            }
            Ok(())
        },
    );

    match pull_res {
        Ok((bytes_transferred, sha256_hex)) => {
            out_file.sync_all().map_err(|e| AppCoreError::Io {
                reason: format!("Failed to fsync staged file '{}': {}", staged_local_path, e),
            })?;
            Ok(MtpDownloadOutcome {
                bytes_transferred,
                sha256_hex,
            })
        }
        Err(err) => {
            drop(out_file);
            let _ = std::fs::remove_file(&staged_path);
            Err(err.into())
        }
    }
}

/// UniFFI-exported MTP bounded 64 KiB file upload with pre-commit source stream SHA-256 verification.
#[uniffi::export]
#[allow(
    clippy::too_many_arguments,
    reason = "UniFFI exported MTP upload bridge with explicit source verification parameters"
)]
pub fn mtp_push_local_file_verified(
    serial_or_id: String,
    storage_id: u32,
    parent_dir_or_handle: String,
    filename: String,
    local_source_path: String,
    expected_sha256_hex: String,
    conflict_policy: MtpConflictPolicy,
    max_keep_both_candidates: u32,
    progress: Box<dyn MtpTransferProgressCallback>,
) -> Result<MtpUploadOutcome, crate::uniffi_api::UniFFIAppCoreError> {
    push_local_file_path_to_mtp(
        &serial_or_id,
        storage_id,
        &parent_dir_or_handle,
        &filename,
        std::path::Path::new(&local_source_path),
        Some(expected_sha256_hex.as_str()),
        conflict_policy,
        max_keep_both_candidates,
        |sent, total| {
            if !progress.on_progress(sent, total) {
                return Err(AppCoreError::Transfer {
                    reason: MTP_TRANSFER_CANCELLED_REASON.to_string(),
                });
            }
            Ok(())
        },
    )
    .map_err(Into::into)
}

/// UniFFI-exported MTP remote object deletion.
#[uniffi::export]
pub fn mtp_delete_remote_object(
    serial_or_id: String,
    storage_id: u32,
    object_handle: u32,
    remote_path: String,
) -> Result<(), crate::uniffi_api::UniFFIAppCoreError> {
    delete_mtp_object(&serial_or_id, storage_id, object_handle, &remote_path).map_err(Into::into)
}
