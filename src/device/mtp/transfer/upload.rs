// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Upload planning and new-handle verification before replacing an existing MTP object.

use super::{
    allocate_non_colliding_filename, resolve_directory_handle_by_path, resolve_storage_id,
    MtpConflictPolicy, MtpUploadOutcome,
};
use crate::device::mtp::{driver::UsbBulkTransport, session::MtpSession};
use crate::error::AppCoreError;
use sha2::{Digest, Sha256};
use std::io::Read;

struct UploadDestination {
    storage_id: u32,
    parent_handle: u32,
    existing_names: Vec<(u32, String)>,
    existing_same_name_handle: Option<u32>,
    target_filename: String,
    conflict_policy: MtpConflictPolicy,
}

enum UploadPlan {
    Skip(MtpUploadOutcome),
    Send(UploadDestination),
}

fn prepare_upload_destination<T: UsbBulkTransport>(
    session: &mut MtpSession<T>,
    storage_id: u32,
    parent_handle: u32,
    filename: &str,
    conflict_policy: MtpConflictPolicy,
    max_keep_both_candidates: u32,
) -> Result<UploadPlan, AppCoreError> {
    // Strictly enumerate existing objects in parent folder; any probe error aborts before writing.
    let existing_handles = session.get_object_handles(storage_id, 0, parent_handle)?;
    let mut existing_names: Vec<(u32, String)> = Vec::with_capacity(existing_handles.len());
    for h in existing_handles {
        let info = session.get_object_info(h)?;
        existing_names.push((h, info.filename));
    }

    let existing_same_name_handle = existing_names
        .iter()
        .find(|(_, name)| name == filename)
        .map(|(h, _)| *h);

    let target_filename = match (existing_same_name_handle, conflict_policy) {
        (Some(existing_handle), MtpConflictPolicy::Skip) => {
            return Ok(UploadPlan::Skip(MtpUploadOutcome {
                created_handle: existing_handle,
                final_filename: filename.to_string(),
                bytes_transferred: 0,
                sha256_hex: String::new(),
                source_stream_sha256_verified: false,
                remote_size_verified: false,
                remote_readback_sha256_verified: false,
                skipped: true,
            }));
        }
        (Some(_), MtpConflictPolicy::KeepBoth) => {
            allocate_non_colliding_filename(filename, &existing_names, max_keep_both_candidates)?
        }
        _ => filename.to_string(),
    };

    Ok(UploadPlan::Send(UploadDestination {
        storage_id,
        parent_handle,
        existing_names,
        existing_same_name_handle,
        target_filename,
        conflict_policy,
    }))
}

impl UploadDestination {
    fn verify_uploaded_object<T: UsbBulkTransport>(
        &self,
        session: &mut MtpSession<T>,
        created_handle: u32,
        total_size: u64,
        actual_streamed_sha256_hex: &str,
        expected_sha256_hex: Option<&str>,
    ) -> Result<(), AppCoreError> {
        // 1. Verify actual streamed source SHA-256 against expected source SHA-256 BEFORE deleting any old handle.
        if let Some(expected_hex) = expected_sha256_hex.map(str::trim).filter(|s| !s.is_empty()) {
            if !actual_streamed_sha256_hex.eq_ignore_ascii_case(expected_hex) {
                let _ = session.delete_object(created_handle);
                return Err(AppCoreError::Transfer {
                reason: format!(
                    "MTP source stream SHA-256 mismatch before commit for '{}': expected {}, actual streamed {}",
                    self.target_filename, expected_hex, actual_streamed_sha256_hex
                ),
            });
            }
        }

        // 2. Verify the newly created handle's exact byte size on the device BEFORE touching any pre-existing handle.
        let verified_info = match session.get_object_info(created_handle) {
            Ok(info) => info,
            Err(err) => {
                let _ = session.delete_object(created_handle);
                return Err(err);
            }
        };

        if verified_info.file_size() != total_size {
            let _ = session.delete_object(created_handle);
            return Err(AppCoreError::Mtp {
            code: 0,
            message: format!(
                "MTP post-upload size verification failed for '{}' (handle 0x{:08X}): expected {} bytes, device reported {} bytes",
                self.target_filename,
                created_handle,
                total_size,
                verified_info.file_size()
            ),
        });
        }

        Ok(())
    }

    fn verify_parent_after_upload<T: UsbBulkTransport>(
        &self,
        session: &mut MtpSession<T>,
        created_handle: u32,
    ) -> Result<(), AppCoreError> {
        // 3. Re-enumerate parent directory to detect any concurrent same-name object created during upload.
        // ISO 15740 has no atomic non-overwrite lock; if another handle with `self.target_filename` appeared
        // during upload, roll back `created_handle` immediately without touching other handles.
        let post_handles = match session.get_object_handles(self.storage_id, 0, self.parent_handle)
        {
            Ok(handles) => handles,
            Err(err) => {
                let _ = session.delete_object(created_handle);
                return Err(err);
            }
        };
        for h in post_handles {
            if h == created_handle {
                continue;
            }
            let h_name = if let Some((_, known_name)) =
                self.existing_names.iter().find(|(eh, _)| *eh == h)
            {
                known_name.clone()
            } else {
                match session.get_object_info(h) {
                    Ok(info) => info.filename,
                    Err(err) => {
                        let _ = session.delete_object(created_handle);
                        return Err(err);
                    }
                }
            };
            if h_name == self.target_filename {
                let is_expected_overwrite_target = self.conflict_policy
                    == MtpConflictPolicy::Overwrite
                    && self.existing_same_name_handle == Some(h);
                if !is_expected_overwrite_target {
                    let _ = session.delete_object(created_handle);
                    return Err(AppCoreError::Mtp {
                    code: 0,
                    message: format!(
                        "Concurrent same-name MTP object '{}' (handle 0x{:08X}) appeared during upload; rolled back new handle 0x{:08X} without modifying concurrent object",
                        self.target_filename, h, created_handle
                    ),
                });
                }
            }
        }

        Ok(())
    }

    fn commit_overwrite<T: UsbBulkTransport>(
        &self,
        session: &mut MtpSession<T>,
        created_handle: u32,
    ) -> Result<(), AppCoreError> {
        // 4. Only after source stream SHA-256, remote object size, and post-upload directory state are verified,
        // commit Overwrite by deleting the pre-existing handle.
        if self.conflict_policy == MtpConflictPolicy::Overwrite {
            if let Some(old_handle) = self.existing_same_name_handle {
                if old_handle != created_handle {
                    session.delete_object(old_handle)?;
                }
            }
        }

        Ok(())
    }
}

fn validate_upload_request(
    filename: &str,
    total_size: u64,
    max_payload: u64,
) -> Result<(), AppCoreError> {
    if filename.trim().is_empty() {
        return Err(AppCoreError::Mtp {
            code: 0,
            message: "MTP upload filename must not be empty".to_string(),
        });
    }
    if total_size > max_payload {
        return Err(AppCoreError::Mtp {
            code: crate::device::mtp::codes::resp::PARAMETER_NOT_SUPPORTED,
            message: format!(
                "MTP upload size {} bytes exceeds 32-bit PTP SendObjectInfo limit ({} bytes); rejected before creating remote object",
                total_size,
                max_payload
            ),
        });
    }

    Ok(())
}

/// Streams a bounded `Read` source to an MTP parent directory with strict conflict probing,
/// pre-commit source stream SHA-256 verification, and post-verify target handle protection.
#[allow(
    clippy::too_many_arguments,
    reason = "Internal session upload entrypoint with explicit verification and candidate limit parameters"
)]
pub fn push_reader_with_session<T, R, F>(
    session: &mut MtpSession<T>,
    requested_storage_id: u32,
    parent_dir_or_handle: &str,
    filename: &str,
    total_size: u64,
    reader: &mut R,
    expected_sha256_hex: Option<&str>,
    conflict_policy: MtpConflictPolicy,
    max_keep_both_candidates: u32,
    mut on_progress: F,
) -> Result<MtpUploadOutcome, AppCoreError>
where
    T: UsbBulkTransport,
    R: Read,
    F: FnMut(u64, u64) -> Result<(), AppCoreError>,
{
    validate_upload_request(
        filename,
        total_size,
        MtpSession::<T>::MAX_MTP_SEND_OBJECT_BYTES,
    )?;

    let storage_id = resolve_storage_id(session, requested_storage_id)?;
    let parent_handle =
        resolve_directory_handle_by_path(session, storage_id, parent_dir_or_handle)?;
    let destination = match prepare_upload_destination(
        session,
        storage_id,
        parent_handle,
        filename,
        conflict_policy,
        max_keep_both_candidates,
    )? {
        UploadPlan::Skip(outcome) => return Ok(outcome),
        UploadPlan::Send(destination) => destination,
    };
    let mut stream_hasher = Sha256::new();
    let created_handle = session.send_object_from_reader(
        storage_id,
        parent_handle,
        &destination.target_filename,
        total_size,
        reader,
        |chunk, sent| {
            stream_hasher.update(chunk);
            on_progress(sent, total_size)
        },
    )?;
    let actual_streamed_sha256_hex = format!("{:x}", stream_hasher.finalize());

    destination.verify_uploaded_object(
        session,
        created_handle,
        total_size,
        &actual_streamed_sha256_hex,
        expected_sha256_hex,
    )?;
    destination.verify_parent_after_upload(session, created_handle)?;
    destination.commit_overwrite(session, created_handle)?;

    Ok(MtpUploadOutcome {
        created_handle,
        final_filename: destination.target_filename,
        bytes_transferred: total_size,
        sha256_hex: actual_streamed_sha256_hex,
        source_stream_sha256_verified: true,
        remote_size_verified: true,
        remote_readback_sha256_verified: false,
        skipped: false,
    })
}
