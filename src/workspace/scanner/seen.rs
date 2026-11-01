// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! High-concurrency Inode deduplication engine.
//!
//! Utilizes 256 shards of `parking_lot::RwLock<FxHashSet<(u64, u64)>>` for high-throughput
//! Inode deduplication across APFS 64-bit sparse Inodes, and incorporates POSIX `st_nlink`
//! fast-path pruning for regular files with `nlink <= 1`.

use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hasher};

/// Number of partitions for the sharded hash table.
pub const NUM_SHARDS: usize = 256;

/// Golden ratio hash multiplier for distributing Inodes evenly across shards.
const GOLDEN_RATIO_64: u64 = 0x9E37_79B9_7F4A_7C15;

/// High-throughput zero-dependency FxHasher implementation.
#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

impl Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            let val = u64::from_ne_bytes(bytes[..8].try_into().expect("slice length >= 8"));
            self.hash = self.hash.rotate_left(5) ^ val;
            self.hash = self.hash.wrapping_mul(0x517c_c1b7_2722_0a95);
            bytes = &bytes[8..];
        }
        if !bytes.is_empty() {
            let mut buf = [0u8; 8];
            buf[..bytes.len()].copy_from_slice(bytes);
            let val = u64::from_ne_bytes(buf);
            self.hash = self.hash.rotate_left(5) ^ val;
            self.hash = self.hash.wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.hash = self.hash.rotate_left(5) ^ i;
        self.hash = self.hash.wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

pub type FxBuildHasher = BuildHasherDefault<FxHasher>;
pub type FxHashSet<T> = HashSet<T, FxBuildHasher>;

type ShardSet = parking_lot::RwLock<FxHashSet<(u64, u64)>>;

/// High-performance 256-shard Inode deduplicator.
pub struct SeenDeduplicator {
    shards: Box<[ShardSet; NUM_SHARDS]>,
}

impl SeenDeduplicator {
    /// Allocates a new deduplicator with 256 independent lock shards.
    pub fn new() -> Self {
        let shards_array: [ShardSet; NUM_SHARDS] =
            std::array::from_fn(|_| parking_lot::RwLock::new(FxHashSet::default()));

        Self {
            shards: Box::new(shards_array),
        }
    }

    /// Computes the target shard index using a golden-ratio multiplicative hash.
    #[inline]
    fn shard_index(&self, dev: u64, inode: u64) -> usize {
        let h = (inode ^ dev.rotate_left(32)).wrapping_mul(GOLDEN_RATIO_64);
        ((h >> 56) & ((NUM_SHARDS - 1) as u64)) as usize
    }

    /// Evaluates whether an entry requires Inode deduplication under the POSIX `st_nlink` invariant.
    ///
    /// Regular files with `nlink <= 1` physically cannot have additional hard links on POSIX filesystems,
    /// allowing callers to safely prune them from Inode tracking. Directories must always be tracked
    /// to guard against directory recursion loops.
    #[inline]
    pub fn requires_deduplication(is_dir: bool, nlink: u64) -> bool {
        is_dir || nlink > 1
    }

    /// Tests and sets an Inode identifier for the given device.
    ///
    /// Returns `true` if this `(dev, inode)` pair is newly discovered (not previously seen).
    /// Returns `false` if it was already marked as seen.
    #[inline]
    pub fn test_and_set(&self, dev: u64, inode: u64) -> bool {
        let idx = self.shard_index(dev, inode);
        let mut guard = self.shards[idx].write();
        guard.insert((dev, inode))
    }

    /// Tests and sets an Inode record while leveraging the POSIX `st_nlink` invariant.
    ///
    /// When `!is_dir && nlink <= 1`, returns `true` immediately without touching shard locks or
    /// allocating memory in the hash set.
    #[inline]
    pub fn test_and_set_with_nlink(&self, dev: u64, inode: u64, is_dir: bool, nlink: u64) -> bool {
        if !Self::requires_deduplication(is_dir, nlink) {
            return true;
        }
        self.test_and_set(dev, inode)
    }

    /// Queries whether an Inode identifier has already been recorded without modifying state.
    #[inline]
    pub fn is_seen(&self, dev: u64, inode: u64) -> bool {
        let idx = self.shard_index(dev, inode);
        let guard = self.shards[idx].read();
        guard.contains(&(dev, inode))
    }
}

impl Default for SeenDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}

// Compile-time assertion of Send + Sync thread safety
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    let _ = assert_send_sync::<SeenDeduplicator>;
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_sharded_deduplication_basic() {
        let seen = SeenDeduplicator::new();
        // First insert succeeds
        assert!(seen.test_and_set(1, 0));
        assert!(seen.test_and_set(1, 1));
        assert!(seen.test_and_set(1, 63));
        assert!(seen.test_and_set(1, 64));
        assert!(seen.test_and_set(1, 1_000_000));
        assert!(seen.test_and_set(1, 100_000_000));

        // Queries confirm existence
        assert!(seen.is_seen(1, 0));
        assert!(seen.is_seen(1, 1));
        assert!(seen.is_seen(1, 63));
        assert!(seen.is_seen(1, 64));
        assert!(seen.is_seen(1, 1_000_000));
        assert!(seen.is_seen(1, 100_000_000));

        // Second insert returns false
        assert!(!seen.test_and_set(1, 0));
        assert!(!seen.test_and_set(1, 1));
        assert!(!seen.test_and_set(1, 63));
        assert!(!seen.test_and_set(1, 64));
        assert!(!seen.test_and_set(1, 1_000_000));
        assert!(!seen.test_and_set(1, 100_000_000));

        // Unseen inodes return false
        assert!(!seen.is_seen(1, 2));
        assert!(!seen.is_seen(1, 65));
    }

    #[test]
    fn test_sparse_apfs_inodes_and_multiple_devices() {
        let seen = SeenDeduplicator::new();
        let sparse_apfs_inode = 0x8000_0000_1234_5678u64;
        let huge_inode = u64::MAX - 10;

        assert!(!seen.is_seen(1, sparse_apfs_inode));
        assert!(seen.test_and_set(1, sparse_apfs_inode));
        assert!(seen.is_seen(1, sparse_apfs_inode));
        assert!(!seen.test_and_set(1, sparse_apfs_inode));

        // Different device with same high inode is treated independently
        assert!(seen.test_and_set(2, sparse_apfs_inode));
        assert!(!seen.test_and_set(2, sparse_apfs_inode));

        assert!(seen.test_and_set(1, huge_inode));
        assert!(!seen.test_and_set(1, huge_inode));
    }

    #[test]
    fn test_posix_nlink_invariant_pruning() {
        let seen = SeenDeduplicator::new();

        // Regular file with nlink <= 1 should bypass deduplication tracking
        assert!(SeenDeduplicator::requires_deduplication(true, 1));
        assert!(!SeenDeduplicator::requires_deduplication(false, 1));
        assert!(!SeenDeduplicator::requires_deduplication(false, 0));
        assert!(SeenDeduplicator::requires_deduplication(false, 2));

        // Regular file with nlink = 1 is always reported as new and not recorded
        assert!(seen.test_and_set_with_nlink(1, 555, false, 1));
        assert!(seen.test_and_set_with_nlink(1, 555, false, 1));
        assert!(!seen.is_seen(1, 555));

        // Regular file with nlink = 2 is tracked and deduplicated
        assert!(seen.test_and_set_with_nlink(1, 777, false, 2));
        assert!(seen.is_seen(1, 777));
        assert!(!seen.test_and_set_with_nlink(1, 777, false, 2));

        // Directory is always tracked regardless of nlink
        assert!(seen.test_and_set_with_nlink(1, 888, true, 1));
        assert!(seen.is_seen(1, 888));
        assert!(!seen.test_and_set_with_nlink(1, 888, true, 1));
    }

    #[test]
    fn test_multithreaded_concurrency() {
        let seen = Arc::new(SeenDeduplicator::new());
        let mut handles = Vec::new();

        for thread_id in 0..16u64 {
            let seen_clone = Arc::clone(&seen);
            handles.push(thread::spawn(move || {
                let mut local_inserted = 0;
                for i in 0..5000u64 {
                    let inode = (thread_id * 10_000) + i;
                    if seen_clone.test_and_set(1, inode) {
                        local_inserted += 1;
                    }
                }
                local_inserted
            }));
        }

        let mut total_inserted = 0;
        for h in handles {
            total_inserted += h.join().expect("thread panicked");
        }

        assert_eq!(total_inserted, 16 * 5000);
    }
}
