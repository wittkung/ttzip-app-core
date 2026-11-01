// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![forbid(unsafe_code)]

//! Prioritized file transfer coordinator, 500ms sliding window throughput calculator, and .ttpart staging.
//!
//! # Quickstart
//!
//! ```no_run
//! use std::sync::Arc;
//! use ttzip_app_core::transfer::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let throughput = Arc::new(SlidingWindowThroughput::default());
//!     let coordinator = TransferCoordinator::new(throughput);
//!
//!     // Submit large file transfer job with guaranteed 64-bit precision (> 4 GB)
//!     let job_id = coordinator.submit_job(
//!         TransferPriority::Normal,
//!         "file:///path/to/archive.tar".to_string(),
//!         "mtp://storage/archive.tar".to_string(),
//!         10 * 1024 * 1024 * 1024_u64, // 10 GiB
//!     )?;
//!
//!     // Record chunk progression and check state predicates
//!     let progress = coordinator.record_chunk_transferred(job_id, 64 * 1024 * 1024)?;
//!     assert!(progress.is_active());
//!     assert!(!progress.is_terminal());
//!     Ok(())
//! }
//! ```

pub mod coordinator;
pub mod job;
pub mod part_writer;
pub mod sliding_window;

pub use coordinator::TransferCoordinator;
pub use job::{TransferJob, TransferPriority, TransferProgress, TransferState};
pub use part_writer::PartWriter;
pub use sliding_window::{SlidingWindowThroughput, SLIDING_WINDOW_DURATION};


/// Canonical timeout duration for stalled transfers without physical progress.
pub const DEFAULT_TRANSFER_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

/// Unified domain-specific error enumeration across the transfer subsystem.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    /// Referenced transfer job ID was not found in the coordinator registry.
    #[error("Transfer job '{0}' not found")]
    JobNotFound(uuid::Uuid),

    /// Invalid job operational state transition.
    #[error("Invalid transfer state: {0}")]
    InvalidState(String),

    /// Transfer operation timed out without physical chunk progress.
    #[error("Transfer job '{job_id}' timed out after {timeout:?}: {reason}")]
    Timeout {
        job_id: uuid::Uuid,
        timeout: std::time::Duration,
        reason: String,
    },

    /// Standard I/O filesystem error.
    #[error("Transfer I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<TransferError> for std::io::Error {
    #[inline]
    fn from(err: TransferError) -> Self {
        match err {
            TransferError::Io(io_err) => io_err,
            err @ TransferError::Timeout { .. } => {
                std::io::Error::new(std::io::ErrorKind::TimedOut, err)
            }
            err @ TransferError::JobNotFound(_) => {
                std::io::Error::new(std::io::ErrorKind::NotFound, err)
            }
            err @ TransferError::InvalidState(_) => {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, err)
            }
        }
    }
}

impl From<TransferError> for crate::error::AppCoreError {
    #[inline]
    fn from(err: TransferError) -> Self {
        match err {
            TransferError::InvalidState(reason) => {
                crate::error::AppCoreError::InvalidState { reason }
            }
            TransferError::Io(io_err) => {
                crate::error::AppCoreError::Io {
                    reason: io_err.to_string(),
                }
            }
            TransferError::Timeout { reason, .. } => {
                crate::error::AppCoreError::Transfer { reason }
            }
            other => crate::error::AppCoreError::Transfer {
                reason: other.to_string(),
            },
        }
    }
}

/// Unified Result type for transfer subsystem operations.
pub type TransferResult<T> = std::result::Result<T, TransferError>;

/// Standard prelude exporting commonly used file transfer types and coordinator.
pub mod prelude {
    pub use super::coordinator::TransferCoordinator;
    pub use super::job::{TransferJob, TransferPriority, TransferProgress, TransferState};
    pub use super::part_writer::PartWriter;
    pub use super::sliding_window::SlidingWindowThroughput;
    pub use super::{TransferError, TransferResult, DEFAULT_TRANSFER_TIMEOUT};
}

/// Formats a dedicated diagnostic error for physical transfer timeouts.
#[inline]
pub fn format_transfer_timeout_error(
    job_id: uuid::Uuid,
    timeout: std::time::Duration,
) -> crate::error::AppCoreError {
    crate::error::AppCoreError::Transfer {
        reason: format!(
            "Transfer job '{}' timed out after {}ms without physical progress",
            job_id,
            timeout.as_millis()
        ),
    }
}

// Compile-time concurrency contract assertions ensuring all types are Send + Sync + 'static.
const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    const fn assert_unpin<T: Unpin>() {}
    const fn assert_std_error<T: std::error::Error + Send + Sync + 'static>() {}
    const fn assert_from<T, U>()
    where
        U: From<T>,
    {
    }

    assert_send_sync::<TransferError>();
    assert_send_sync::<TransferJob>();
    assert_send_sync::<TransferProgress>();
    assert_send_sync::<TransferState>();
    assert_send_sync::<TransferPriority>();
    assert_send_sync::<TransferCoordinator>();
    assert_send_sync::<SlidingWindowThroughput>();
    assert_send_sync::<PartWriter>();

    assert_unpin::<TransferCoordinator>();
    assert_unpin::<SlidingWindowThroughput>();
    assert_unpin::<PartWriter>();

    assert_std_error::<TransferError>();
    assert_from::<std::io::Error, TransferError>();
    assert_from::<TransferError, std::io::Error>();
    assert_from::<TransferError, crate::error::AppCoreError>();

    // Memory layout guarantee: ensure error type fits within a single 64-byte CPU cache line
    assert!(std::mem::size_of::<TransferError>() <= 64);
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_transfer_coordinator_lifecycle() {
        let throughput = Arc::new(SlidingWindowThroughput::default());
        let coord = TransferCoordinator::new(throughput);

        // 10 GiB large file transfer (> 4 GB 32-bit boundary)
        let total_bytes: u64 = 10 * 1024 * 1024 * 1024;
        let chunk1: u64 = 5 * 1024 * 1024 * 1024;
        let chunk2: u64 = 5 * 1024 * 1024 * 1024;

        let job_id = coord
            .submit_job(
                TransferPriority::High,
                "file:///large/source.bin".to_string(),
                "mtp://device/target.bin".to_string(),
                total_bytes,
            )
            .expect("submit failed");

        // Verify initial progress & state queries
        let p1 = coord.get_job_progress(job_id).expect("job not found");
        assert_eq!(p1.transferred_bytes, 0);
        assert_eq!(p1.file_size(), total_bytes);
        assert_eq!(p1.state, TransferState::Pending);
        assert!(p1.is_active());
        assert!(!p1.is_terminal());
        assert!(p1.can_resume());
        assert!(!p1.is_paused());

        // Transfer 5 GiB (> 4 GB)
        let p2 = coord
            .record_chunk_transferred(job_id, chunk1)
            .expect("record chunk failed");
        assert_eq!(p2.transferred_bytes, chunk1);
        assert_eq!(p2.state, TransferState::Running);
        assert!(p2.is_active());
        assert!(!p2.is_terminal());

        // Pause job
        coord.pause_job(job_id).expect("pause failed");
        let p3 = coord.get_job_progress(job_id).unwrap();
        assert_eq!(p3.state, TransferState::Paused);
        assert_eq!(p3.transferred_bytes, chunk1);
        assert!(!p3.is_active());
        assert!(p3.is_paused());
        assert!(p3.can_resume());
        assert!(!p3.is_terminal());

        // Resume job
        let resumed_offset = coord.resume_job(job_id).expect("resume failed");
        assert_eq!(resumed_offset, chunk1);
        let p_resumed = coord.get_job_progress(job_id).unwrap();
        assert_eq!(p_resumed.state, TransferState::Running);
        assert_eq!(p_resumed.transferred_bytes, chunk1);
        assert!(p_resumed.is_active());

        // Complete remaining 5 GiB
        let p4 = coord
            .record_chunk_transferred(job_id, chunk2)
            .expect("second chunk failed");
        assert_eq!(p4.transferred_bytes, total_bytes);
        assert_eq!(p4.state, TransferState::Completed);
        assert!(!p4.is_active());
        assert!(p4.is_terminal());
        assert!(!p4.can_resume());
        assert!(!p4.is_paused());

        // Cancelling a completed job must fail
        assert!(coord.cancel_job(job_id).is_err());
    }

    #[test]
    fn test_transfer_error_downcast_and_elevation() {
        let err = TransferError::Timeout {
            job_id: uuid::Uuid::nil(),
            timeout: DEFAULT_TRANSFER_TIMEOUT,
            reason: "stalled pipe".to_string(),
        };
        let io_err: std::io::Error = err.into();
        assert_eq!(io_err.kind(), std::io::ErrorKind::TimedOut);
        let downcasted = io_err.get_ref().and_then(|e| e.downcast_ref::<TransferError>());
        assert!(matches!(downcasted, Some(TransferError::Timeout { .. })));

        let not_found = TransferError::JobNotFound(uuid::Uuid::nil());
        let io_nf: std::io::Error = not_found.into();
        assert_eq!(io_nf.kind(), std::io::ErrorKind::NotFound);

        let app_err: crate::error::AppCoreError = TransferError::InvalidState("bad state".to_string()).into();
        assert!(matches!(app_err, crate::error::AppCoreError::InvalidState { .. }));
    }
}
