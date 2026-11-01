// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Safe Mozilla UniFFI 0.28 bindings for Swift, C#, and C++.

pub mod engine;
pub mod treemap;
pub mod types;

pub use engine::AppCoreEngine;
pub use treemap::{
    compute_treemap_layout, start_storage_scan, StorageScanCallback, StorageScanError,
    StorageScanSessionHandle, TreemapItemInput, TreemapTileDto, UniFFIStorageScanProgress,
};
pub use types::{
    UniFFIAppCoreError, UniFFIDeviceDescriptor, UniFFIFavoriteItem,
    UniFFITransferJobInfo, UniFFIWorkspaceSession, UniFFIWorkspaceTab,
};
