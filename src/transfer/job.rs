// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Transfer job entities, priorities, operational states, and progress snapshots.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Priority level for queuing and scheduling file transfer jobs.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TransferPriority {
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
}

/// Operational execution state of a transfer job.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferState {
    Pending,
    Running,
    Paused,
    Completed,
    Failed { reason: String },
    Cancelled,
}

impl TransferState {
    /// Returns true if the job is actively executing or queued for execution.
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }

    /// Returns true if the job has reached an immutable terminal state.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed { .. } | Self::Cancelled)
    }

    /// Returns true if the job can be resumed from a previous pause or pending point.
    #[inline]
    pub fn can_resume(&self) -> bool {
        matches!(self, Self::Paused | Self::Pending)
    }

    /// Returns true if the job is currently paused.
    #[inline]
    pub fn is_paused(&self) -> bool {
        matches!(self, Self::Paused)
    }
}

/// In-flight file transfer task metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferJob {
    pub id: Uuid,
    pub priority: TransferPriority,
    pub source_uri: String,
    pub destination_uri: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub state: TransferState,
    pub chunk_size: u64,
    pub created_at_epoch_ms: i64,
}

impl TransferJob {
    pub fn new(
        priority: TransferPriority,
        source_uri: String,
        destination_uri: String,
        total_bytes: u64,
    ) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        Self {
            id: Uuid::new_v4(),
            priority,
            source_uri,
            destination_uri,
            total_bytes,
            transferred_bytes: 0,
            state: TransferState::Pending,
            chunk_size: 65536, // 64 KB default chunk size
            created_at_epoch_ms: now_ms,
        }
    }

    /// Configures custom chunk size in bytes (guaranteed 64-bit precision).
    #[inline]
    pub fn with_chunk_size(mut self, chunk_size: u64) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Alias for `total_bytes` ensuring uniform 64-bit cursor precision across engines.
    #[inline]
    pub fn file_size(&self) -> u64 {
        self.total_bytes
    }

    /// Returns true if the job is actively executing or pending.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    /// Returns true if the job is in a terminal state (Completed, Failed, Cancelled).
    #[inline]
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Returns true if the job is eligible for resumption.
    #[inline]
    pub fn can_resume(&self) -> bool {
        self.state.can_resume()
    }

    /// Returns true if the job is currently paused.
    #[inline]
    pub fn is_paused(&self) -> bool {
        self.state.is_paused()
    }

    /// Returns the local destination filesystem path if `destination_uri` targets local disk storage.
    #[inline]
    pub fn local_destination_path(&self) -> Option<PathBuf> {
        Self::parse_local_path(&self.destination_uri)
    }

    /// Resolves a URI string or filesystem path into a canonical local `PathBuf`.
    ///
    /// Accepts `file://` URLs (with or without host) as well as absolute filesystem paths.
    /// Remote protocols (such as `mtp://`, `adb://`, `http://`) return `None`.
    pub fn parse_local_path(uri: &str) -> Option<PathBuf> {
        if let Some(stripped) = uri.strip_prefix("file://") {
            let path_str = if let Some(local) = stripped.strip_prefix("localhost/") {
                format!("/{}", local)
            } else if stripped.starts_with('/') {
                stripped.to_string()
            } else {
                format!("/{}", stripped)
            };
            Some(PathBuf::from(path_str))
        } else if uri.starts_with('/') {
            Some(PathBuf::from(uri))
        } else {
            None
        }
    }
}

/// Real-time progress snapshot returned to client presentation layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub job_id: Uuid,
    pub source_uri: String,
    pub destination_uri: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub state: TransferState,
    pub current_speed_mb_s: f64,
    pub progress_ratio: f64,
}

impl TransferProgress {
    /// Creates a new progress snapshot with full encapsulation.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: Uuid,
        source_uri: String,
        destination_uri: String,
        total_bytes: u64,
        transferred_bytes: u64,
        state: TransferState,
        current_speed_mb_s: f64,
        progress_ratio: f64,
    ) -> Self {
        Self {
            job_id,
            source_uri,
            destination_uri,
            total_bytes,
            transferred_bytes,
            state,
            current_speed_mb_s,
            progress_ratio: progress_ratio.clamp(0.0, 1.0),
        }
    }

    pub fn from_job(job: &TransferJob, speed_mb_s: f64) -> Self {
        let ratio = if job.total_bytes > 0 {
            (job.transferred_bytes as f64) / (job.total_bytes as f64)
        } else {
            0.0
        };
        Self {
            job_id: job.id,
            source_uri: job.source_uri.clone(),
            destination_uri: job.destination_uri.clone(),
            total_bytes: job.total_bytes,
            transferred_bytes: job.transferred_bytes,
            state: job.state.clone(),
            current_speed_mb_s: speed_mb_s,
            progress_ratio: ratio.clamp(0.0, 1.0),
        }
    }

    /// Alias for `total_bytes` ensuring 64-bit precision.
    #[inline]
    pub fn file_size(&self) -> u64 {
        self.total_bytes
    }

    /// Returns true if the transfer progress reflects an active job.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    /// Returns true if the transfer progress reflects a terminated job.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Returns true if the transfer progress reflects a resumable job.
    #[inline]
    pub fn can_resume(&self) -> bool {
        self.state.can_resume()
    }

    /// Returns true if the transfer progress reflects a paused job.
    #[inline]
    pub fn is_paused(&self) -> bool {
        self.state.is_paused()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_state_predicates() {
        assert!(TransferState::Pending.is_active());
        assert!(!TransferState::Pending.is_terminal());
        assert!(TransferState::Pending.can_resume());
        assert!(!TransferState::Pending.is_paused());

        assert!(TransferState::Running.is_active());
        assert!(!TransferState::Running.is_terminal());
        assert!(!TransferState::Running.can_resume());
        assert!(!TransferState::Running.is_paused());

        assert!(!TransferState::Paused.is_active());
        assert!(!TransferState::Paused.is_terminal());
        assert!(TransferState::Paused.can_resume());
        assert!(TransferState::Paused.is_paused());

        assert!(!TransferState::Completed.is_active());
        assert!(TransferState::Completed.is_terminal());
        assert!(!TransferState::Completed.can_resume());
        assert!(!TransferState::Completed.is_paused());

        let failed = TransferState::Failed {
            reason: "I/O error".to_string(),
        };
        assert!(!failed.is_active());
        assert!(failed.is_terminal());
        assert!(!failed.can_resume());
        assert!(!failed.is_paused());

        assert!(!TransferState::Cancelled.is_active());
        assert!(TransferState::Cancelled.is_terminal());
        assert!(!TransferState::Cancelled.can_resume());
        assert!(!TransferState::Cancelled.is_paused());
    }

    #[test]
    fn test_large_file_64bit_precision() {
        // 50 GiB file (> 4 GB 32-bit boundary)
        let total_bytes = 50 * 1024 * 1024 * 1024_u64;
        let job = TransferJob::new(
            TransferPriority::High,
            "source".to_string(),
            "dest".to_string(),
            total_bytes,
        )
        .with_chunk_size(4 * 1024 * 1024);

        assert_eq!(job.file_size(), total_bytes);
        assert_eq!(job.chunk_size, 4 * 1024 * 1024);
        assert!(job.is_active());
        assert!(!job.is_terminal());

        let progress = TransferProgress::from_job(&job, 125.5);
        assert_eq!(progress.file_size(), total_bytes);
        assert_eq!(progress.progress_ratio, 0.0);
        assert!(progress.is_active());
    }

    #[test]
    fn test_parse_local_path_resolution() {
        assert_eq!(
            TransferJob::parse_local_path("file:///Users/dev/archive.zip"),
            Some(PathBuf::from("/Users/dev/archive.zip"))
        );
        assert_eq!(
            TransferJob::parse_local_path("file://localhost/tmp/data.bin"),
            Some(PathBuf::from("/tmp/data.bin"))
        );
        assert_eq!(
            TransferJob::parse_local_path("/var/log/system.log"),
            Some(PathBuf::from("/var/log/system.log"))
        );
        assert_eq!(TransferJob::parse_local_path("mtp://0001:0002/DCIM/photo.jpg"), None);
        assert_eq!(TransferJob::parse_local_path("adb://emulator-5554/sdcard/test.txt"), None);
        assert_eq!(TransferJob::parse_local_path("relative_name"), None);
    }
}
