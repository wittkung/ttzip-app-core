// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Thread-safe priority transfer queue coordinator managing sliding-window speed and .ttpart staging.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;
use uuid::Uuid;

use crate::error::AppCoreError;
use crate::transfer::job::{TransferJob, TransferPriority, TransferProgress, TransferState};
use crate::transfer::part_writer::PartWriter;
use crate::transfer::sliding_window::SlidingWindowThroughput;

/// Central coordinator managing queued and in-flight file transfers via `.ttpart` staging files.
pub struct TransferCoordinator {
    jobs: RwLock<HashMap<Uuid, TransferJob>>,
    writers: RwLock<HashMap<Uuid, PartWriter>>,
    throughput: Arc<SlidingWindowThroughput>,
}

impl TransferCoordinator {
    /// Creates a new transfer coordinator with the given sliding-window throughput calculator.
    pub fn new(throughput: Arc<SlidingWindowThroughput>) -> Self {
        Self {
            jobs: RwLock::new(HashMap::new()),
            writers: RwLock::new(HashMap::new()),
            throughput,
        }
    }

    /// Calculates write buffer alignment based on configured job chunk size.
    #[inline]
    fn calculate_alignment(chunk_size: u64) -> usize {
        if chunk_size >= 1024 * 1024 {
            1024 * 1024
        } else if chunk_size > 0 {
            chunk_size as usize
        } else {
            64 * 1024
        }
    }

    /// Submits a new transfer job and queues it with the requested priority.
    pub fn submit_job(
        &self,
        priority: TransferPriority,
        source_uri: String,
        destination_uri: String,
        total_bytes: u64,
    ) -> Result<Uuid, AppCoreError> {
        let job = TransferJob::new(priority, source_uri, destination_uri, total_bytes);
        let job_id = job.id;

        // If destination refers to a local filesystem path, initialize an aligned PartWriter
        if let Some(dest_path) = job.local_destination_path() {
            let alignment = Self::calculate_alignment(job.chunk_size);
            if let Ok(writer) = PartWriter::open(&dest_path, alignment) {
                self.writers.write().insert(job_id, writer);
            }
        }

        self.jobs.write().insert(job_id, job);
        Ok(job_id)
    }

    /// Updates physical transferred bytes for a job and updates the sliding window throughput.
    pub fn record_chunk_transferred(
        &self,
        job_id: Uuid,
        chunk_bytes: u64,
    ) -> Result<TransferProgress, AppCoreError> {
        self.throughput.record_bytes(chunk_bytes);
        let speed = self.throughput.current_throughput_mb_s();

        let mut jobs = self.jobs.write();
        let job = jobs.get_mut(&job_id).ok_or_else(|| AppCoreError::Transfer {
            reason: format!("Transfer job '{}' not found", job_id),
        })?;

        if !job.is_active() {
            return Err(AppCoreError::InvalidState {
                reason: format!("Cannot transfer bytes for job in state {:?}", job.state),
            });
        }

        job.state = TransferState::Running;
        job.transferred_bytes = job
            .transferred_bytes
            .saturating_add(chunk_bytes)
            .min(job.total_bytes);

        if job.transferred_bytes >= job.total_bytes {
            job.state = TransferState::Completed;
        }

        Ok(TransferProgress::from_job(job, speed))
    }

    /// Writes a chunk of data to the active `PartWriter` and advances transferred bytes.
    pub fn write_chunk(&self, job_id: Uuid, chunk: &[u8]) -> std::io::Result<u64> {
        let written = {
            let mut writers = self.writers.write();
            if let Some(writer) = writers.get_mut(&job_id) {
                writer.append_chunk(chunk)?
            } else {
                let job_opt = self.jobs.read().get(&job_id).cloned();
                if let Some(job) = job_opt {
                    if let Some(dest_path) = job.local_destination_path() {
                        let alignment = Self::calculate_alignment(job.chunk_size);
                        let mut writer = PartWriter::open(&dest_path, alignment)?;
                        let bytes = writer.append_chunk(chunk)?;
                        writers.insert(job_id, writer);
                        bytes
                    } else {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("Transfer job '{}' does not target a local filesystem path", job_id),
                        ));
                    }
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Transfer job '{}' not found", job_id),
                    ));
                }
            }
        };

        self.record_chunk_transferred(job_id, chunk.len() as u64)
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        Ok(written)
    }

    /// Finalizes an active transfer job by syncing buffers, atomically renaming the `.ttpart`
    /// staging file to its final destination path, removing the writer, and marking the job completed.
    pub fn finalize_job(&self, job_id: Uuid) -> std::io::Result<PathBuf> {
        let writer = self
            .writers
            .write()
            .remove(&job_id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("No active PartWriter found for transfer job '{}'", job_id),
                )
            })?;

        let final_path = match writer.finalize() {
            Ok(p) => p,
            Err(e) => {
                let _ = self.fail_job(job_id, format!("Atomic finalization failed: {}", e));
                return Err(e);
            }
        };

        {
            let mut jobs = self.jobs.write();
            if let Some(job) = jobs.get_mut(&job_id) {
                job.state = TransferState::Completed;
                if job.total_bytes > 0 {
                    job.transferred_bytes = job.total_bytes;
                }
            }
        }

        Ok(final_path)
    }

    /// Pauses an active transfer job, syncing buffered chunks to disk.
    pub fn pause_job(&self, job_id: Uuid) -> Result<(), AppCoreError> {
        if let Some(writer) = self.writers.write().get_mut(&job_id) {
            let _ = writer.sync();
        }

        let mut jobs = self.jobs.write();
        let job = jobs.get_mut(&job_id).ok_or_else(|| AppCoreError::Transfer {
            reason: format!("Job '{}' not found", job_id),
        })?;

        if !job.is_active() {
            return Err(AppCoreError::InvalidState {
                reason: format!("Cannot pause job in state {:?}", job.state),
            });
        }

        job.state = TransferState::Paused;
        Ok(())
    }

    /// Resumes a paused transfer job directly from the physical written bytes of the `.ttpart` file.
    pub fn resume_job(&self, job_id: Uuid) -> Result<u64, AppCoreError> {
        let mut jobs = self.jobs.write();
        let job = jobs.get_mut(&job_id).ok_or_else(|| AppCoreError::Transfer {
            reason: format!("Job '{}' not found", job_id),
        })?;

        if !job.can_resume() {
            return Err(AppCoreError::InvalidState {
                reason: format!("Cannot resume job in state {:?}", job.state),
            });
        }

        let mut writers = self.writers.write();
        if let Some(writer) = writers.get(&job_id) {
            job.transferred_bytes = writer.written_bytes();
        } else if let Some(dest_path) = job.local_destination_path() {
            let alignment = Self::calculate_alignment(job.chunk_size);
            if let Ok(writer) = PartWriter::open(&dest_path, alignment) {
                job.transferred_bytes = writer.written_bytes();
                writers.insert(job_id, writer);
            }
        }

        job.state = TransferState::Running;
        Ok(job.transferred_bytes)
    }

    /// Cancels a transfer job and removes its active staging writer.
    pub fn cancel_job(&self, job_id: Uuid) -> Result<(), AppCoreError> {
        self.writers.write().remove(&job_id);

        let mut jobs = self.jobs.write();
        let job = jobs.get_mut(&job_id).ok_or_else(|| AppCoreError::Transfer {
            reason: format!("Job '{}' not found", job_id),
        })?;

        if job.is_terminal() {
            return Err(AppCoreError::InvalidState {
                reason: format!("Cannot cancel job already in terminal state {:?}", job.state),
            });
        }

        job.state = TransferState::Cancelled;
        Ok(())
    }

    /// Marks an active transfer job as failed with an explicit error reason.
    pub fn fail_job(&self, job_id: Uuid, reason: String) -> Result<(), AppCoreError> {
        self.writers.write().remove(&job_id);

        let mut jobs = self.jobs.write();
        let job = jobs.get_mut(&job_id).ok_or_else(|| AppCoreError::Transfer {
            reason: format!("Job '{}' not found", job_id),
        })?;

        if job.is_terminal() {
            return Err(AppCoreError::InvalidState {
                reason: format!("Cannot fail job already in terminal state {:?}", job.state),
            });
        }

        job.state = TransferState::Failed { reason };
        Ok(())
    }

    /// Handles a transfer timeout event by terminating the job with dedicated diagnostic error mapping.
    pub fn handle_timeout(
        &self,
        job_id: Uuid,
        timeout: std::time::Duration,
    ) -> Result<(), AppCoreError> {
        let timeout_msg = format!(
            "Transfer job '{}' timed out after {}ms without physical progress",
            job_id,
            timeout.as_millis()
        );
        self.fail_job(job_id, timeout_msg.clone())?;
        Err(AppCoreError::Transfer { reason: timeout_msg })
    }

    /// Retrieves real-time snapshot for a specific transfer job.
    pub fn get_job_progress(&self, job_id: Uuid) -> Option<TransferProgress> {
        let jobs = self.jobs.read();
        let job = jobs.get(&job_id)?;
        let speed = if job.state == TransferState::Running {
            self.throughput.current_throughput_mb_s()
        } else {
            0.0
        };
        Some(TransferProgress::from_job(job, speed))
    }

    /// Returns current global throughput rate in MB/s across all active transfers.
    pub fn active_throughput_mb_s(&self) -> f64 {
        self.throughput.current_throughput_mb_s()
    }

    /// Lists all registered transfer jobs sorted by priority and submission time.
    pub fn list_jobs(&self) -> Vec<TransferProgress> {
        let jobs = self.jobs.read();
        let speed = self.throughput.current_throughput_mb_s();

        let mut list: Vec<&TransferJob> = Vec::with_capacity(jobs.len());
        list.extend(jobs.values());
        list.sort_unstable_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.created_at_epoch_ms.cmp(&b.created_at_epoch_ms))
        });

        list.into_iter()
            .map(|j| {
                let job_speed = if j.state == TransferState::Running {
                    speed
                } else {
                    0.0
                };
                TransferProgress::from_job(j, job_speed)
            })
            .collect()
    }
}

impl Default for TransferCoordinator {
    fn default() -> Self {
        Self::new(Arc::new(SlidingWindowThroughput::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_test_coordinator() -> TransferCoordinator {
        let throughput = Arc::new(SlidingWindowThroughput::default());
        TransferCoordinator::new(throughput)
    }

    #[test]
    fn test_priority_queue_scheduling_order() {
        let coord = make_test_coordinator();

        let id_low = coord
            .submit_job(
                TransferPriority::Low,
                "s_low".to_string(),
                "d_low".to_string(),
                100,
            )
            .unwrap();

        let id_crit = coord
            .submit_job(
                TransferPriority::Critical,
                "s_crit".to_string(),
                "d_crit".to_string(),
                200,
            )
            .unwrap();

        let id_norm = coord
            .submit_job(
                TransferPriority::Normal,
                "s_norm".to_string(),
                "d_norm".to_string(),
                300,
            )
            .unwrap();

        let id_high = coord
            .submit_job(
                TransferPriority::High,
                "s_high".to_string(),
                "d_high".to_string(),
                400,
            )
            .unwrap();

        let jobs = coord.list_jobs();
        assert_eq!(jobs.len(), 4);
        assert_eq!(jobs[0].job_id, id_crit);
        assert_eq!(jobs[1].job_id, id_high);
        assert_eq!(jobs[2].job_id, id_norm);
        assert_eq!(jobs[3].job_id, id_low);
    }

    #[test]
    fn test_sliding_window_throughput_integration() {
        let coord = make_test_coordinator();
        assert_eq!(coord.active_throughput_mb_s(), 0.0);

        let job_id = coord
            .submit_job(
                TransferPriority::Normal,
                "s".to_string(),
                "d".to_string(),
                10_000_000,
            )
            .unwrap();

        let progress = coord.record_chunk_transferred(job_id, 2_097_152).unwrap();
        assert!(progress.current_speed_mb_s > 0.0);
        assert!(coord.active_throughput_mb_s() > 0.0);
    }

    #[test]
    fn test_large_file_64bit_transfer() {
        let coord = make_test_coordinator();

        let total_bytes: u64 = 8 * 1024 * 1024 * 1024;
        let job_id = coord
            .submit_job(
                TransferPriority::High,
                "file:///large.bin".to_string(),
                "mtp://dest.bin".to_string(),
                total_bytes,
            )
            .unwrap();

        let chunk1: u64 = 3 * 1024 * 1024 * 1024;
        let p1 = coord.record_chunk_transferred(job_id, chunk1).unwrap();
        assert_eq!(p1.transferred_bytes, chunk1);
        assert_eq!(p1.file_size(), total_bytes);
        assert_eq!(p1.state, TransferState::Running);

        let chunk2: u64 = 5 * 1024 * 1024 * 1024;
        let p2 = coord.record_chunk_transferred(job_id, chunk2).unwrap();
        assert_eq!(p2.transferred_bytes, total_bytes);
        assert_eq!(p2.state, TransferState::Completed);
        assert_eq!(p2.progress_ratio, 1.0);
    }

    #[test]
    fn test_invalid_state_guards() {
        let coord = make_test_coordinator();

        let job_id = coord
            .submit_job(
                TransferPriority::Normal,
                "s".to_string(),
                "d".to_string(),
                1000,
            )
            .unwrap();

        coord.pause_job(job_id).unwrap();

        let err_transfer = coord.record_chunk_transferred(job_id, 100);
        assert!(err_transfer.is_err());

        let err_pause = coord.pause_job(job_id);
        assert!(err_pause.is_err());

        coord.resume_job(job_id).unwrap();

        let err_resume = coord.resume_job(job_id);
        assert!(err_resume.is_err());

        coord.record_chunk_transferred(job_id, 1000).unwrap();

        let err_cancel = coord.cancel_job(job_id);
        assert!(err_cancel.is_err());
    }

    #[test]
    fn test_timeout_handling() {
        let coord = make_test_coordinator();

        let job_id = coord
            .submit_job(
                TransferPriority::Critical,
                "src".to_string(),
                "dst".to_string(),
                5000,
            )
            .unwrap();

        let err = coord
            .handle_timeout(job_id, crate::transfer::DEFAULT_TRANSFER_TIMEOUT)
            .unwrap_err();

        match err {
            AppCoreError::Transfer { reason } => {
                assert!(reason.contains(&format!(
                    "timed out after {}ms",
                    crate::transfer::DEFAULT_TRANSFER_TIMEOUT.as_millis()
                )));
            }
            other => panic!("Unexpected error variant: {:?}", other),
        }

        let progress = coord.get_job_progress(job_id).unwrap();
        assert!(progress.is_terminal());
        assert!(!progress.is_active());
        assert!(!progress.can_resume());
    }

    #[test]
    fn test_pause_and_resume_lifecycle() {
        let dir = tempdir().expect("tempdir failed");
        let coord = TransferCoordinator::default();

        let dest_file = dir.path().join("pause_resume.bin");
        let dest_uri = format!("file://{}", dest_file.to_str().unwrap());
        let total_bytes: u64 = 64 * 1024;

        let job_id = coord
            .submit_job(
                TransferPriority::High,
                "src".to_string(),
                dest_uri,
                total_bytes,
            )
            .unwrap();

        let chunk = vec![0xEE; 16 * 1024];
        coord.write_chunk(job_id, &chunk).unwrap();
        coord.pause_job(job_id).unwrap();

        let resumed_bytes = coord.resume_job(job_id).unwrap();
        assert_eq!(resumed_bytes, 16 * 1024);
    }

    #[test]
    fn test_write_chunk_and_finalize_job_lifecycle() {
        let dir = tempdir().expect("tempdir failed");
        let coord = TransferCoordinator::default();

        let dest_file = dir.path().join("downloaded.bin");
        let dest_uri = format!("file://{}", dest_file.to_str().unwrap());
        let total_bytes: u64 = 128 * 1024;

        let job_id = coord
            .submit_job(
                TransferPriority::High,
                "mtp://camera/photo.raw".to_string(),
                dest_uri,
                total_bytes,
            )
            .expect("submit_job failed");

        let chunk1 = vec![0xAB; 64 * 1024];
        let written1 = coord.write_chunk(job_id, &chunk1).expect("write_chunk 1 failed");
        assert_eq!(written1, 64 * 1024);

        let part_file = dir.path().join("downloaded.bin.ttpart");
        assert!(part_file.exists());
        assert_eq!(std::fs::metadata(&part_file).unwrap().len(), 64 * 1024);

        let progress = coord.get_job_progress(job_id).expect("job progress missing");
        assert_eq!(progress.transferred_bytes, 64 * 1024);
        assert_eq!(progress.state, TransferState::Running);

        let chunk2 = vec![0xCD; 64 * 1024];
        let written2 = coord.write_chunk(job_id, &chunk2).expect("write_chunk 2 failed");
        assert_eq!(written2, 128 * 1024);

        let final_path = coord.finalize_job(job_id).expect("finalize_job failed");
        assert_eq!(final_path, dest_file);
        assert!(dest_file.exists());
        assert!(!part_file.exists());
        assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 128 * 1024);

        let final_progress = coord.get_job_progress(job_id).expect("final progress missing");
        assert_eq!(final_progress.state, TransferState::Completed);
        assert_eq!(final_progress.transferred_bytes, total_bytes);

        assert!(coord.finalize_job(job_id).is_err());
    }

    #[test]
    fn test_write_chunk_nonexistent_job_error() {
        let coord = make_test_coordinator();
        let non_existent_id = Uuid::new_v4();

        let err = coord.write_chunk(non_existent_id, &[1, 2, 3]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

        let err2 = coord.finalize_job(non_existent_id).unwrap_err();
        assert_eq!(err2.kind(), std::io::ErrorKind::NotFound);
    }
}
