// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Directory-scoped transport budget; upload/download transport semantics are unchanged.

use super::{AppCoreError, DeviceErrorKind, UsbBulkTransport};
use std::time::{Duration, Instant};

pub(super) trait DirectoryClock: Send + Sync {
    fn elapsed(&self) -> Duration;
}

pub(super) struct WallClock(Instant);
impl DirectoryClock for WallClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}

pub(super) struct DirectoryTransport<T, C = WallClock> {
    inner: T,
    clock: C,
    budget: Duration,
}

impl<T: UsbBulkTransport> DirectoryTransport<T> {
    pub(super) fn new(inner: T, budget: Duration) -> Self {
        Self::with_clock(inner, budget, WallClock(Instant::now()))
    }
}

impl<T: UsbBulkTransport, C: DirectoryClock> DirectoryTransport<T, C> {
    pub(super) fn with_clock(inner: T, budget: Duration, clock: C) -> Self {
        Self {
            inner,
            clock,
            budget,
        }
    }

    fn remaining_timeout(&self, requested: Duration) -> Result<Duration, AppCoreError> {
        let remaining = self.budget.saturating_sub(self.clock.elapsed());
        if remaining.is_zero() {
            return Err(AppCoreError::usb(
                DeviceErrorKind::Timeout,
                "MTP directory protocol budget exhausted; retry the directory",
            ));
        }
        Ok(requested.min(remaining))
    }
}

impl<T: UsbBulkTransport, C: DirectoryClock> UsbBulkTransport for DirectoryTransport<T, C> {
    fn write_bulk(&mut self, data: &[u8], timeout: Duration) -> Result<usize, AppCoreError> {
        let timeout = self.remaining_timeout(timeout)?;
        let written = self.inner.write_bulk(data, timeout)?;
        self.remaining_timeout(timeout)?;
        Ok(written)
    }

    fn read_bulk(&mut self, data: &mut [u8], timeout: Duration) -> Result<usize, AppCoreError> {
        let timeout = self.remaining_timeout(timeout)?;
        let read = self.inner.read_bulk(data, timeout)?;
        self.remaining_timeout(timeout)?;
        Ok(read)
    }

    fn endpoint_in(&self) -> u8 {
        self.inner.endpoint_in()
    }
    fn endpoint_out(&self) -> u8 {
        self.inner.endpoint_out()
    }
}
