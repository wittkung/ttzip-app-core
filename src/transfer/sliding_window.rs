// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! 500-millisecond physical sliding-window throughput rate calculator.

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use parking_lot::Mutex;

pub const SLIDING_WINDOW_DURATION: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
struct ByteSample {
    timestamp: Instant,
    bytes: u64,
}

/// Thread-safe sliding-window throughput calculator measuring physical bytes over 500ms.
#[derive(Debug)]
pub struct SlidingWindowThroughput {
    window_duration: Duration,
    samples: Mutex<VecDeque<ByteSample>>,
}

impl Default for SlidingWindowThroughput {
    fn default() -> Self {
        Self::new(SLIDING_WINDOW_DURATION)
    }
}

impl SlidingWindowThroughput {
    /// Creates a new calculator with the specified sliding window duration (default 500ms).
    pub fn new(window_duration: Duration) -> Self {
        Self {
            window_duration,
            samples: Mutex::new(VecDeque::with_capacity(128)),
        }
    }

    /// Records newly transferred physical bytes at current instant.
    pub fn record_bytes(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let now = Instant::now();
        let mut samples = self.samples.lock();
        samples.push_back(ByteSample {
            timestamp: now,
            bytes,
        });
        self.prune_expired(&mut samples, now);
    }

    /// Calculates current physical transfer throughput in Megabytes per second (MB/s).
    /// Pure physical calculation based strictly on recorded byte samples; no fake fallbacks.
    pub fn current_throughput_mb_s(&self) -> f64 {
        let now = Instant::now();
        let mut samples = self.samples.lock();
        self.prune_expired(&mut samples, now);

        if samples.is_empty() {
            return 0.0;
        }

        let total_bytes: u64 = samples.iter().map(|s| s.bytes).sum();
        let oldest = match samples.front() {
            Some(sample) => sample.timestamp,
            None => return 0.0,
        };
        let elapsed_secs = (now - oldest).as_secs_f64().max(0.001);

        // Throughput in MB/s (1 MB = 1,048,576 bytes)
        (total_bytes as f64) / (1_048_576.0 * elapsed_secs)
    }

    /// Clears all recorded samples, resetting rate to zero.
    pub fn reset(&self) {
        let mut samples = self.samples.lock();
        samples.clear();
    }

    fn prune_expired(&self, samples: &mut VecDeque<ByteSample>, now: Instant) {
        while let Some(front) = samples.front() {
            if now.duration_since(front.timestamp) > self.window_duration {
                samples.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_window_throughput() {
        let throughput = SlidingWindowThroughput::default();
        assert_eq!(throughput.current_throughput_mb_s(), 0.0);
    }

    #[test]
    fn test_record_zero_bytes_noop() {
        let throughput = SlidingWindowThroughput::default();
        throughput.record_bytes(0);
        assert_eq!(throughput.current_throughput_mb_s(), 0.0);
    }

    #[test]
    fn test_large_bytes_64bit_throughput() {
        let throughput = SlidingWindowThroughput::default();
        // 8 GiB (> 4 GB 32-bit boundary)
        let large_bytes: u64 = 8 * 1024 * 1024 * 1024;
        throughput.record_bytes(large_bytes);
        let speed = throughput.current_throughput_mb_s();
        assert!(speed > 0.0, "Expected non-zero throughput for large sample");
    }

    #[test]
    fn test_reset_clears_throughput() {
        let throughput = SlidingWindowThroughput::default();
        throughput.record_bytes(1_048_576);
        assert!(throughput.current_throughput_mb_s() > 0.0);
        throughput.reset();
        assert_eq!(throughput.current_throughput_mb_s(), 0.0);
    }
}
