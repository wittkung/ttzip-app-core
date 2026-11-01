// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! TTZip Universal Cross-Platform Headless Application Core.
//!
//! Provides genuine USB MTP 1.1 protocol communication, wireless/daemon ADB transport,
//! sliding-window prioritized file transfer coordination with WAL checkpoints,
//! multi-tab workspace sessions with JSON bookmarks,
//! squarified treemap calculation with tail merging,
//! high-concurrency Darwin local storage scanning,
//! and safe UniFFI multi-language bindings.

uniffi::setup_scaffolding!("ttzip_app_core");

pub mod cabi;
pub mod device;
pub mod error;
pub mod media_preferences;
pub mod reader;
pub mod transfer;
pub mod uniffi_api;
pub mod workspace;

pub use cabi::*;

/// Unified top-level prelude exporting canonical entry points across all app-core subsystems.
pub mod prelude {
    pub use crate::device::adb::prelude::*;
    pub use crate::device::mtp::prelude::*;
    pub use crate::error::AppCoreError;
    pub use crate::transfer::prelude::*;
    pub use crate::workspace::prelude::*;
    pub use ttzip_types as types;
}

/// Shared data-type re-export from ttzip-types.
pub use ttzip_types as types;

pub use device::adb::{
    AdbClient, AdbDevice, AdbDeviceState, AdbError, AdbResult,
    AdbSyncEntry, AdbSyncStream, AdbTransportError,
};
pub use device::mtp::{
    codes as mtp_codes, ContainerType as MtpContainerType, MtpClient, MtpDeviceInfo, MtpError,
    MtpObjectHandle, MtpObjectInfo, MtpResult, MtpSession, MtpSessionState, MtpStorageInfo,
};
pub use error::AppCoreError;
pub use transfer::{
    SlidingWindowThroughput, TransferCoordinator, TransferJob,
    TransferPriority, TransferProgress, TransferState,
};
pub use uniffi_api::AppCoreEngine;
pub use uniffi_api::treemap::{
    compute_treemap_layout, start_storage_scan, StorageScanCallback, StorageScanError,
    StorageScanSessionHandle, TreemapItemInput, TreemapTileDto, UniFFIStorageScanProgress,
};
pub use uniffi_api::types::{
    UniFFIAppCoreError, UniFFIDeviceDescriptor, UniFFIFavoriteItem,
    UniFFITransferJobInfo, UniFFIWorkspaceSession, UniFFIWorkspaceTab,
};
pub use workspace::{
    compute_hierarchical_treemap, compute_squarified_treemap, DirectoryScanner, FavoriteItem,
    FavoritesRegistry, HistoryEntry, ReclaimCategory, ScannedEntry, SeenDeduplicator,
    SessionManager, StorageScanProgressDto, TabSortOrder, TabViewMode, TileCategory, TreemapTile,
    WorkspaceLayout, WorkspaceSession, WorkspaceTab, TREEMAP_FLAG_CLOUD_STUB, TREEMAP_FLAG_FIRMLINK,
    TREEMAP_FLAG_IS_DIR, TREEMAP_FLAG_IS_LEAF, TREEMAP_FLAG_RECLAIMABLE, TREEMAP_FLAG_SELECTED,
    TREEMAP_FLAG_TAIL_COLLAPSED, TREEMAP_OTHERS_NODE_ID,
};
