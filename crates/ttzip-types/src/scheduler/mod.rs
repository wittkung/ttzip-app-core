// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Local work-stealing thread pool and hardware-conscious compute scheduler.

pub mod qos;
pub mod work_stealing;

pub use qos::apply_thread_compute_qos;
pub use work_stealing::{LocalWorkStealingPool, Scope};
