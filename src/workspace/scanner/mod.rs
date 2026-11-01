// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! High-throughput local storage scanner and developer artifact inspector.
//!
//! Provides recursive directory scanning with Darwin `getattrlistbulk(2)` acceleration,
//! lock-free Inode deduplication (`SeenDeduplicator`), and real-time streaming progress.

pub mod classify;
pub mod darwin;
pub mod seen;

pub use classify::{classify_extension, classify_tile_category, detect_reclaim_category, is_reclaimable_dir, ReclaimCategory};
pub use darwin::{disable_dataless_materialization, scan_directory_entries, ScannedEntry, SCAN_BUFFER_SIZE};
pub use seen::SeenDeduplicator;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use crate::workspace::types::TREEMAP_FLAG_RECLAIMABLE;

/// Real-time progress report emitted during storage tree inspection.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageScanProgressDto {
    pub total_scanned_files: u64,
    pub total_scanned_dirs: u64,
    pub total_scanned_bytes: u64,
    pub total_reclaimable_bytes: u64,
    pub is_complete: bool,
    pub current_directory_path: String,
}

impl StorageScanProgressDto {
    /// Creates an empty initial progress record.
    pub fn new(current_path: String) -> Self {
        Self {
            total_scanned_files: 0,
            total_scanned_dirs: 0,
            total_scanned_bytes: 0,
            total_reclaimable_bytes: 0,
            is_complete: false,
            current_directory_path: current_path,
        }
    }
}

/// Rate-limiting throttler enforcing a minimum emission interval (16ms / 60Hz debounce)
/// and batch threshold (2048 entries) for storage scan progress reports.
#[derive(Debug)]
pub struct ProgressThrottler {
    last_emit: Instant,
    min_interval: Duration,
    items_since_emit: u64,
    batch_threshold: u64,
}

impl ProgressThrottler {
    /// Constructs a progress throttler with the specified rate limit and batch threshold.
    pub fn new(min_interval: Duration, batch_threshold: u64) -> Self {
        Self {
            last_emit: Instant::now(),
            min_interval,
            items_since_emit: 0,
            batch_threshold,
        }
    }

    /// Evaluates whether progress should be emitted to callers.
    ///
    /// If `force` is true, resets internal counters and unconditionally returns `true`.
    /// Otherwise, increments the entry counter and returns `true` if either the time
    /// elapsed exceeds `min_interval` or the item count reaches `batch_threshold`.
    #[inline]
    pub fn should_emit(&mut self, force: bool) -> bool {
        if force {
            self.last_emit = Instant::now();
            self.items_since_emit = 0;
            return true;
        }

        self.items_since_emit = self.items_since_emit.saturating_add(1);
        if self.items_since_emit >= self.batch_threshold || self.last_emit.elapsed() >= self.min_interval {
            self.last_emit = Instant::now();
            self.items_since_emit = 0;
            true
        } else {
            false
        }
    }
}

impl Default for ProgressThrottler {
    fn default() -> Self {
        Self::new(Duration::from_millis(16), 2048)
    }
}

/// Callback interface for receiving real-time storage scan notifications.
pub trait StorageScanCallback: Send + Sync {
    fn on_progress(&self, progress: StorageScanProgressDto);
    fn on_completed(&self, session_id: String);
    fn on_error(&self, error_message: String);
}

/// Recursive directory scanner coordinating deduplication and progress events.
pub struct DirectoryScanner {
    seen: Arc<SeenDeduplicator>,
}

impl DirectoryScanner {
    /// Creates a new directory scanner with an empty deduplicator.
    pub fn new() -> Self {
        Self {
            seen: Arc::new(SeenDeduplicator::new()),
        }
    }

    /// Creates a directory scanner sharing an existing deduplicator instance.
    pub fn with_deduplicator(seen: Arc<SeenDeduplicator>) -> Self {
        Self { seen }
    }

    /// Recursively scans a root directory, tracking Inodes and invoking progress hooks.
    pub fn scan_tree<F>(
        &self,
        root_path: &str,
        cancel_token: &AtomicBool,
        mut progress_hook: F,
    ) -> std::io::Result<StorageScanProgressDto>
    where
        F: FnMut(&StorageScanProgressDto),
    {
        disable_dataless_materialization();

        let mut throttler = ProgressThrottler::default();
        let mut progress = StorageScanProgressDto::new(root_path.to_string());
        let mut dir_queue = std::collections::VecDeque::new();
        dir_queue.push_back((root_path.to_string(), false));

        let is_root_scan = root_path == "/" || root_path.is_empty();

        while let Some((dir, is_reclaim_tree)) = dir_queue.pop_front() {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }

            progress.current_directory_path = dir.clone();
            if throttler.should_emit(false) {
                progress_hook(&progress);
            }

            let entries = match scan_directory_entries(&dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for entry in entries {
                if cancel_token.load(Ordering::Relaxed) {
                    break;
                }

                self.process_entry(&entry, is_reclaim_tree, is_root_scan, &mut progress, &mut dir_queue);

                if throttler.should_emit(false) {
                    progress_hook(&progress);
                }
            }
        }

        progress.is_complete = !cancel_token.load(Ordering::Relaxed);
        if throttler.should_emit(true) {
            progress_hook(&progress);
        }
        Ok(progress)
    }

    #[inline]
    fn process_entry(
        &self,
        entry: &ScannedEntry,
        is_reclaim_tree: bool,
        is_root_scan: bool,
        progress: &mut StorageScanProgressDto,
        dir_queue: &mut std::collections::VecDeque<(String, bool)>,
    ) {
        let is_entry_reclaim = is_reclaim_tree || (entry.flags & TREEMAP_FLAG_RECLAIMABLE) != 0;

        if entry.is_dir {
            if is_root_scan && entry.path == "/System/Volumes/Data" {
                return;
            }
            if self.seen.test_and_set(entry.dev, entry.inode) {
                progress.total_scanned_dirs += 1;
                dir_queue.push_back((entry.path.clone(), is_entry_reclaim));
            }
        } else if self
            .seen
            .test_and_set_with_nlink(entry.dev, entry.inode, false, u64::from(entry.nlink))
        {
            progress.total_scanned_files += 1;
            progress.total_scanned_bytes = progress.total_scanned_bytes.saturating_add(entry.size);
            if is_entry_reclaim {
                progress.total_reclaimable_bytes = progress.total_reclaimable_bytes.saturating_add(entry.size);
            }
        }
    }
}

impl Default for DirectoryScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_progress_throttler_rate_limiting_and_batching() {
        let mut throttler = ProgressThrottler::new(Duration::from_millis(50), 10);

        // First 9 calls without delay should not emit (rate-limited and under batch threshold)
        for _ in 0..9 {
            assert!(!throttler.should_emit(false));
        }

        // 10th item reaches batch threshold and triggers emission
        assert!(throttler.should_emit(false));

        // After emission, counters are reset; next call under threshold should not emit
        assert!(!throttler.should_emit(false));

        // Sleeping past min_interval triggers emission via elapsed time
        std::thread::sleep(Duration::from_millis(55));
        assert!(throttler.should_emit(false));

        // Forced emit always triggers immediately
        assert!(throttler.should_emit(true));
    }

    #[test]
    fn test_directory_scanner_recursive_progress() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let root = temp_dir.path();

        let target_dir = root.join("target");
        fs::create_dir(&target_dir).expect("create target");

        let src_dir = root.join("src");
        fs::create_dir(&src_dir).expect("create src");

        let mut f1 = File::create(src_dir.join("lib.rs")).expect("create file");
        f1.write_all(b"pub fn hello() {}").expect("write");

        let mut f2 = File::create(target_dir.join("build.log")).expect("create file");
        f2.write_all(&[0u8; 1024]).expect("write");

        let scanner = DirectoryScanner::new();
        let cancel = AtomicBool::new(false);
        let mut updates = Vec::new();

        let final_progress = scanner
            .scan_tree(root.to_str().unwrap(), &cancel, |p| {
                updates.push(p.clone());
            })
            .expect("scan tree");

        assert!(final_progress.is_complete);
        assert_eq!(final_progress.total_scanned_files, 2);
        assert_eq!(final_progress.total_scanned_dirs, 2);
        assert!(final_progress.total_reclaimable_bytes >= 1024);
        assert!(final_progress.total_scanned_bytes >= 1024);
        assert!(!updates.is_empty());
    }

    #[test]
    fn test_directory_scanner_hardlink_deduplication_and_single_link_pruning() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let root = temp_dir.path();

        let single_path = root.join("single.txt");
        File::create(&single_path)
            .expect("create single")
            .write_all(b"single")
            .expect("write single");

        let hard1 = root.join("hard1.bin");
        let hard2 = root.join("hard2.bin");
        File::create(&hard1)
            .expect("create hard1")
            .write_all(&[0xAB; 4096])
            .expect("write hard1");
        fs::hard_link(&hard1, &hard2).expect("hard_link");

        let seen = Arc::new(SeenDeduplicator::new());
        let scanner = DirectoryScanner::with_deduplicator(Arc::clone(&seen));
        let cancel = AtomicBool::new(false);

        let progress = scanner
            .scan_tree(root.to_str().unwrap(), &cancel, |_| {})
            .expect("scan tree");

        // 1 single file + 1 unique hardlinked file = 2 counted files
        assert_eq!(progress.total_scanned_files, 2);

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let single_meta = fs::metadata(&single_path).expect("single meta");
            let hard_meta = fs::metadata(&hard1).expect("hard meta");
            // Single-link file (nlink == 1) must be pruned from hash set insertion
            assert!(!seen.is_seen(single_meta.dev(), single_meta.ino()));
            // Multi-link file (nlink == 2) must be recorded in the hash set
            assert!(seen.is_seen(hard_meta.dev(), hard_meta.ino()));
        }
    }

    #[test]
    fn test_directory_scanner_cancellation() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let root = temp_dir.path();

        for i in 0..10 {
            let d = root.join(format!("dir_{}", i));
            fs::create_dir(&d).expect("dir");
            let mut f = File::create(d.join("file.bin")).expect("file");
            f.write_all(b"data").expect("write");
        }

        let scanner = DirectoryScanner::new();
        let cancel = AtomicBool::new(true); // cancelled immediately

        let res = scanner
            .scan_tree(root.to_str().unwrap(), &cancel, |_| {})
            .expect("scan");

        assert!(!res.is_complete);
    }
}
