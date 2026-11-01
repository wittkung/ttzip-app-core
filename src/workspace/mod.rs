// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Multi-tab workspace sessions, navigation stacks, and unified JSON bookmarks.
//!
//! # Quickstart
//!
//! ```no_run
//! use ttzip_app_core::workspace::prelude::*;
//!
//! // 1. Instantiate the multi-session coordinator
//! let session_mgr = SessionManager::new();
//! let session_id = session_mgr.create_session("Main Workspace", "file:///Users/username/Documents");
//!
//! // 2. Add an auxiliary tab and navigate across paths
//! let tab_id = session_mgr
//!     .add_tab(session_id, "Downloads", "file:///Users/username/Downloads")
//!     .expect("failed to add tab");
//! session_mgr
//!     .navigate_tab(session_id, tab_id, "file:///Users/username/Downloads/archives")
//!     .expect("navigation failed");
//!
//! // 3. Manage fast bookmarks using FavoritesRegistry
//! let favorites = FavoritesRegistry::new_in_memory();
//! favorites
//!     .add_favorite(
//!         "Projects",
//!         "file:///Users/username/Projects",
//!         "folder",
//!         true,
//!         vec!["dev".to_string()],
//!     )
//!     .expect("failed to add favorite");
//! ```

#![deny(unsafe_op_in_unsafe_fn)]

pub mod favorites;
pub mod scanner;
pub mod session;
pub mod treemap;
pub mod types;

/// Unified domain-specific error enumeration across the workspace subsystem.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    /// Referenced workspace session was not found in the manager.
    #[error("Workspace session '{0}' not found")]
    SessionNotFound(uuid::Uuid),

    /// Referenced workspace tab was not found in the session.
    #[error("Workspace tab '{tab_id}' not found in session '{session_id}'")]
    TabNotFound {
        session_id: uuid::Uuid,
        tab_id: uuid::Uuid,
    },

    /// Cannot close the sole remaining active tab in a session.
    #[error("Cannot close the last remaining tab in workspace session '{0}'")]
    EmptySession(uuid::Uuid),

    /// History stack is empty during back or forward traversal.
    #[error("Navigation history stack is empty: cannot navigate {0}")]
    EmptyHistory(&'static str),

    /// Duplicate favorite item with identical URI already registered.
    #[error("Favorite item with URI '{0}' already exists")]
    DuplicateFavorite(String),

    /// Referenced favorite bookmark was not found.
    #[error("Favorite item '{0}' not found")]
    FavoriteNotFound(uuid::Uuid),

    /// General workspace operation or schema version error.
    #[error("Workspace general error: {0}")]
    General(String),

    /// Standard I/O filesystem error.
    #[error("Workspace I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<WorkspaceError> for std::io::Error {
    #[inline]
    fn from(err: WorkspaceError) -> Self {
        match err {
            WorkspaceError::Io(io_err) => io_err,
            err @ (WorkspaceError::SessionNotFound(_)
            | WorkspaceError::TabNotFound { .. }
            | WorkspaceError::FavoriteNotFound(_)) => {
                std::io::Error::new(std::io::ErrorKind::NotFound, err)
            }
            err @ WorkspaceError::DuplicateFavorite(_) => {
                std::io::Error::new(std::io::ErrorKind::AlreadyExists, err)
            }
            err @ (WorkspaceError::EmptySession(_) | WorkspaceError::EmptyHistory(_)) => {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, err)
            }
            err @ WorkspaceError::General(_) => std::io::Error::other(err),
        }
    }
}

impl From<WorkspaceError> for crate::error::AppCoreError {
    #[inline]
    fn from(err: WorkspaceError) -> Self {
        match err {
            WorkspaceError::Io(io_err) => {
                crate::error::AppCoreError::Io {
                    reason: io_err.to_string(),
                }
            }
            other => crate::error::AppCoreError::Workspace {
                reason: other.to_string(),
            },
        }
    }
}

/// Unified Result type for workspace subsystem operations.
pub type WorkspaceResult<T> = std::result::Result<T, WorkspaceError>;

/// Standard prelude exporting commonly used workspace session and bookmark types.
pub mod prelude {
    pub use super::favorites::FavoritesRegistry;
    pub use super::scanner::{
        classify_extension, classify_tile_category, detect_reclaim_category,
        disable_dataless_materialization, is_reclaimable_dir, scan_directory_entries,
        DirectoryScanner, ReclaimCategory, ScannedEntry, SeenDeduplicator, StorageScanCallback,
        StorageScanProgressDto,
    };
    pub use super::session::SessionManager;
    pub use super::treemap::{
        compute_hierarchical_treemap, compute_squarified_treemap, TreemapHierarchyNode,
        TreemapInputNode, MAX_VIEWPORT_TILES, MIN_CONTAINER_RATIO, MIN_TILE_AREA_PX,
    };
    pub use super::types::{
        FavoriteItem, HistoryEntry, TabSortOrder, TabViewMode, TileCategory, TreemapTile,
        WorkspaceLayout, WorkspaceSession, WorkspaceTab,
    };
    pub use super::{WorkspaceError, WorkspaceResult};
}

pub use favorites::FavoritesRegistry;
pub use scanner::{
    classify_extension, classify_tile_category, detect_reclaim_category,
    disable_dataless_materialization, is_reclaimable_dir, scan_directory_entries, DirectoryScanner,
    ReclaimCategory, ScannedEntry, SeenDeduplicator, StorageScanCallback, StorageScanProgressDto,
};
pub use session::SessionManager;
pub use treemap::{
    compute_hierarchical_treemap, compute_squarified_treemap, TreemapHierarchyNode,
    TreemapInputNode, MAX_VIEWPORT_TILES, MIN_CONTAINER_RATIO, MIN_TILE_AREA_PX,
};
pub use types::{
    current_epoch_ms, epoch_ms_to_system_time, normalize_path, normalize_uri, FavoriteItem,
    HistoryEntry, TabSortOrder, TabViewMode, TileCategory, TreemapTile, WorkspaceLayout,
    WorkspaceSession, WorkspaceTab, TREEMAP_FLAG_CLOUD_STUB, TREEMAP_FLAG_FIRMLINK,
    TREEMAP_FLAG_IS_DIR, TREEMAP_FLAG_IS_LEAF, TREEMAP_FLAG_RECLAIMABLE, TREEMAP_FLAG_SELECTED,
    TREEMAP_FLAG_TAIL_COLLAPSED, TREEMAP_OTHERS_NODE_ID,
};

// Static compile-time barrier enforcing thread-safety concurrency contracts
const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    const fn assert_std_error<T: std::error::Error + Send + Sync + 'static>() {}
    const fn assert_from<T, U>()
    where
        U: From<T>,
    {
    }

    assert_send_sync::<WorkspaceError>();
    assert_send_sync::<WorkspaceSession>();
    assert_send_sync::<WorkspaceTab>();
    assert_send_sync::<FavoriteItem>();
    assert_send_sync::<HistoryEntry>();
    assert_send_sync::<SessionManager>();
    assert_send_sync::<FavoritesRegistry>();

    assert_std_error::<WorkspaceError>();
    assert_from::<std::io::Error, WorkspaceError>();
    assert_from::<WorkspaceError, std::io::Error>();
    assert_from::<WorkspaceError, crate::error::AppCoreError>();

    // Memory layout guarantee: ensure error type fits within a single 64-byte CPU cache line
    assert!(std::mem::size_of::<WorkspaceError>() <= 64);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_facade_integration_and_lifecycle() {
        let mgr = SessionManager::new();
        let session_id = mgr.create_session("Default", "file:///Users/home/..");

        let session = mgr.get_session(session_id).expect("session must exist");
        assert_eq!(session.tabs.len(), 1);
        assert_eq!(session.tabs[0].current_uri, "file:///Users");

        let tab2 = mgr
            .add_tab(session_id, "Downloads", "file:///Users/home/Downloads")
            .expect("tab2 creation failed");

        mgr.navigate_tab(session_id, tab2, "file:///Users/home/Downloads/archive.zip")
            .expect("navigation failed");

        let back_uri = mgr.navigate_back(session_id, tab2).expect("navigate back");
        assert_eq!(back_uri, "file:///Users/home/Downloads");

        let fwd_uri = mgr.navigate_forward(session_id, tab2).expect("navigate forward");
        assert_eq!(fwd_uri, "file:///Users/home/Downloads/archive.zip");

        // Favorites integration
        let fav_reg = FavoritesRegistry::new();
        let fav = fav_reg
            .add_favorite("Downloads", "file:///Users/home/Downloads", "folder", true, vec![])
            .expect("add favorite");
        assert_eq!(fav.name, "Downloads");
        assert_eq!(fav.uri, "file:///Users/home/Downloads");
        assert!(fav.created_at_epoch_ms > 0);

        // Close second tab
        mgr.close_tab(session_id, tab2).expect("close tab");
        let final_session = mgr.get_session(session_id).unwrap();
        assert_eq!(final_session.tabs.len(), 1);
        assert_eq!(final_session.active_tab_id, session.tabs[0].id);
    }

    #[test]
    fn test_workspace_error_downcast_and_elevation() {
        let sid = uuid::Uuid::new_v4();
        let tid = uuid::Uuid::new_v4();
        let err = WorkspaceError::TabNotFound {
            session_id: sid,
            tab_id: tid,
        };
        let io_err: std::io::Error = err.into();
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
        let downcasted = io_err.get_ref().and_then(|e| e.downcast_ref::<WorkspaceError>());
        assert!(matches!(downcasted, Some(WorkspaceError::TabNotFound { .. })));

        let dup = WorkspaceError::DuplicateFavorite("file:///test".to_string());
        let io_dup: std::io::Error = dup.into();
        assert_eq!(io_dup.kind(), std::io::ErrorKind::AlreadyExists);

        let app_err: crate::error::AppCoreError = WorkspaceError::EmptyHistory("back").into();
        assert!(matches!(app_err, crate::error::AppCoreError::Workspace { .. }));
    }
}
