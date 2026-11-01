// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! UniFFI exports for squarified treemap calculation and local storage scanning.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::workspace::scanner::DirectoryScanner;
use crate::workspace::treemap::{compute_squarified_treemap, TreemapInputNode};
use crate::workspace::types::{TileCategory, TreemapTile, TREEMAP_OTHERS_NODE_ID};

/// Foreign language DTO representing an individual visual treemap tile.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TreemapTileDto {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub node_id: u32,
    pub depth: u16,
    pub category: u8,
    pub flags: u8,
    pub bytes: u64,
    pub name: String,
    pub relative_path: String,
}

impl From<TreemapTile> for TreemapTileDto {
    fn from(tile: TreemapTile) -> Self {
        Self {
            x: tile.x,
            y: tile.y,
            width: tile.width,
            height: tile.height,
            node_id: tile.node_id,
            depth: tile.depth,
            category: tile.category,
            flags: tile.flags,
            bytes: tile.bytes,
            name: String::new(),
            relative_path: String::new(),
        }
    }
}

/// Input element passed from foreign languages for treemap geometry layout.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct TreemapItemInput {
    pub node_id: u32,
    pub bytes: u64,
    pub is_dir: bool,
    pub category: u8,
    pub name: String,
    pub relative_path: String,
}

/// Real-time progress report emitted across the FFI boundary during storage scanning.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniFFIStorageScanProgress {
    pub total_scanned_files: u64,
    pub total_scanned_dirs: u64,
    pub total_scanned_bytes: u64,
    pub total_reclaimable_bytes: u64,
    pub is_complete: bool,
    pub current_directory_path: String,
}

/// Errors raised across the FFI boundary during directory scanning.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum StorageScanError {
    #[error("Path not found: {path}")]
    PathNotFound { path: String },
    #[error("Permission denied: {path}")]
    PermissionDenied { path: String },
    #[error("I/O error: {reason}")]
    IoError { reason: String },
    #[error("Operation cancelled")]
    Cancelled,
}

/// Callback interface implemented in Swift/foreign language to receive scan updates.
#[uniffi::export(callback_interface)]
pub trait StorageScanCallback: Send + Sync {
    fn on_progress(&self, progress: UniFFIStorageScanProgress);
    fn on_completed(&self, session_id: String);
    fn on_error(&self, error_message: String);
}

/// Controller handle allowing cancellation and inspection of an in-flight scan.
#[derive(uniffi::Object)]
pub struct StorageScanSessionHandle {
    is_cancelled: Arc<AtomicBool>,
}

#[uniffi::export]
impl StorageScanSessionHandle {
    /// Signals the background scan worker to stop traversing directories immediately.
    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
    }

    /// Queries whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::SeqCst)
    }
}

/// Computes a squarified treemap layout for a slice of items within pixel dimensions.
#[uniffi::export]
pub fn compute_treemap_layout(
    items: Vec<TreemapItemInput>,
    width_px: f32,
    height_px: f32,
) -> Vec<TreemapTileDto> {
    if width_px <= 0.0 || height_px <= 0.0 || items.is_empty() {
        return Vec::new();
    }

    let mut meta_map = HashMap::with_capacity(items.len());
    let mut inputs = Vec::with_capacity(items.len());

    for item in items {
        meta_map.insert(item.node_id, (item.name, item.relative_path));
        inputs.push(TreemapInputNode::new(
            item.node_id,
            item.bytes,
            item.is_dir,
            TileCategory::from_u8(item.category),
        ));
    }

    let tiles = compute_squarified_treemap(&inputs, 0.0, 0.0, width_px, height_px, 1);

    tiles
        .into_iter()
        .map(|tile| {
            let (name, relative_path) = if tile.node_id == TREEMAP_OTHERS_NODE_ID {
                ("Others".to_string(), String::new())
            } else if let Some((n, p)) = meta_map.get(&tile.node_id) {
                (n.clone(), p.clone())
            } else {
                (String::new(), String::new())
            };

            TreemapTileDto {
                x: tile.x,
                y: tile.y,
                width: tile.width,
                height: tile.height,
                node_id: tile.node_id,
                depth: tile.depth,
                category: tile.category,
                flags: tile.flags,
                bytes: tile.bytes,
                name,
                relative_path,
            }
        })
        .collect()
}

/// Initiates an asynchronous background directory scan rooted at `root_path`.
#[uniffi::export]
pub fn start_storage_scan(
    root_path: String,
    progress_callback: Box<dyn StorageScanCallback>,
) -> Result<Arc<StorageScanSessionHandle>, StorageScanError> {
    let root = Path::new(&root_path);
    if !root.exists() {
        return Err(StorageScanError::PathNotFound { path: root_path });
    }

    let cancel_token = Arc::new(AtomicBool::new(false));
    let handle = Arc::new(StorageScanSessionHandle {
        is_cancelled: Arc::clone(&cancel_token),
    });

    let scanner = Arc::new(DirectoryScanner::new());
    let session_id = uuid::Uuid::new_v4().to_string();
    let root_path_clone = root_path.clone();
    let session_id_clone = session_id.clone();

    std::thread::spawn(move || {
        let res = scanner.scan_tree(&root_path_clone, &cancel_token, |p| {
            progress_callback.on_progress(UniFFIStorageScanProgress {
                total_scanned_files: p.total_scanned_files,
                total_scanned_dirs: p.total_scanned_dirs,
                total_scanned_bytes: p.total_scanned_bytes,
                total_reclaimable_bytes: p.total_reclaimable_bytes,
                is_complete: p.is_complete,
                current_directory_path: p.current_directory_path.clone(),
            });
        });

        match res {
            Ok(_) => {
                progress_callback.on_completed(session_id_clone);
            }
            Err(e) => {
                progress_callback.on_error(e.to_string());
            }
        }
    });

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockScanCallback {
        progresses: std::sync::Mutex<Vec<UniFFIStorageScanProgress>>,
        completed: std::sync::Mutex<Option<String>>,
    }

    impl MockScanCallback {
        fn new() -> Self {
            Self {
                progresses: std::sync::Mutex::new(Vec::new()),
                completed: std::sync::Mutex::new(None),
            }
        }
    }

    impl StorageScanCallback for MockScanCallback {
        fn on_progress(&self, progress: UniFFIStorageScanProgress) {
            self.progresses.lock().unwrap().push(progress);
        }

        fn on_completed(&self, session_id: String) {
            *self.completed.lock().unwrap() = Some(session_id);
        }

        fn on_error(&self, _error_message: String) {}
    }

    #[test]
    fn test_compute_treemap_layout_uniffi() {
        let items = vec![
            TreemapItemInput {
                node_id: 1,
                bytes: 700_000,
                is_dir: false,
                category: 0,
                name: "main.rs".to_string(),
                relative_path: "src/main.rs".to_string(),
            },
            TreemapItemInput {
                node_id: 2,
                bytes: 300_000,
                is_dir: false,
                category: 1,
                name: "logo.png".to_string(),
                relative_path: "assets/logo.png".to_string(),
            },
        ];

        let tiles = compute_treemap_layout(items, 1000.0, 500.0);
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].name, "main.rs");
        assert_eq!(tiles[0].relative_path, "src/main.rs");
        assert_eq!(tiles[1].name, "logo.png");
        assert_eq!(tiles[1].relative_path, "assets/logo.png");
    }

    #[test]
    fn test_start_storage_scan_smoke() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let cb = Box::new(MockScanCallback::new());
        let handle = start_storage_scan(temp_dir.path().to_str().unwrap().to_string(), cb)
            .expect("start scan");
        assert!(!handle.is_cancelled());
        handle.cancel();
        assert!(handle.is_cancelled());
    }
}
