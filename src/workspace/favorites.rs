// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Universal JSON favorites and bookmark model with atomic filesystem persistence.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppCoreError;
use crate::workspace::types::{current_epoch_ms, normalize_uri, FavoriteItem};

#[derive(Debug, Clone, Deserialize)]
struct FavoritesFileModel {
    version: u32,
    favorites: Vec<FavoriteItem>,
}

#[derive(Serialize)]
struct FavoritesSerializeModel<'a> {
    version: u32,
    favorites: &'a [FavoriteItem],
}

/// Thread-safe registry maintaining user favorites across local, MTP, ADB, and archive URIs.
pub struct FavoritesRegistry {
    storage_path: Option<PathBuf>,
    items: RwLock<Vec<FavoriteItem>>,
}

impl Default for FavoritesRegistry {
    fn default() -> Self {
        Self::new_in_memory()
    }
}

impl FavoritesRegistry {
    /// Constructs a new in-memory favorites registry.
    pub fn new() -> Self {
        Self::new_in_memory()
    }

    /// In-memory registry without disk binding (useful for tests or default profiles).
    pub fn new_in_memory() -> Self {
        Self {
            storage_path: None,
            items: RwLock::new(Vec::new()),
        }
    }

    /// Loads or initializes the registry from the specified JSON file path.
    pub fn load_or_create<P: AsRef<Path>>(file_path: P) -> Result<Self, AppCoreError> {
        let path = file_path.as_ref().to_path_buf();
        if path.exists() {
            let data = fs::read(&path).map_err(|e| AppCoreError::Io {
                reason: format!("Failed to read favorites file '{}': {}", path.display(), e),
            })?;
            let model: FavoritesFileModel = serde_json::from_slice(&data).map_err(|e| {
                AppCoreError::Workspace {
                    reason: format!("Corrupted favorites JSON in '{}': {}", path.display(), e),
                }
            })?;
            if model.version == 0 {
                return Err(AppCoreError::Workspace {
                    reason: format!("Unsupported favorites schema version 0 in '{}'", path.display()),
                });
            }
            Ok(Self {
                storage_path: Some(path),
                items: RwLock::new(model.favorites),
            })
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| AppCoreError::Io {
                    reason: format!("Failed to create parent dir for favorites: {}", e),
                })?;
            }
            let reg = Self {
                storage_path: Some(path),
                items: RwLock::new(Vec::new()),
            };
            reg.persist_to_disk()?;
            Ok(reg)
        }
    }

    /// Adds a new favorite item with normalized URI and 64-bit creation timestamp, persisting to disk.
    pub fn add_favorite(
        &self,
        name: &str,
        uri: &str,
        icon_hint: &str,
        is_pinned: bool,
        tags: Vec<String>,
    ) -> Result<FavoriteItem, AppCoreError> {
        let mut items = self.items.write();
        let next_index = items.len() as u32;

        let item = FavoriteItem {
            id: Uuid::new_v4(),
            name: name.to_string(),
            uri: normalize_uri(uri),
            icon_hint: icon_hint.to_string(),
            is_pinned,
            tags,
            order_index: next_index,
            created_at_epoch_ms: current_epoch_ms(),
        };

        let result = item.clone();
        items.push(item);
        drop(items);

        let _ = self.persist_to_disk();
        Ok(result)
    }

    /// Removes a favorite item by ID and persists to disk.
    pub fn remove_favorite(&self, item_id: Uuid) -> Result<bool, AppCoreError> {
        let mut items = self.items.write();
        let original_len = items.len();
        items.retain(|item| item.id != item_id);
        let removed = items.len() != original_len;

        for (idx, item) in items.iter_mut().enumerate() {
            item.order_index = idx as u32;
        }
        drop(items);

        if removed {
            let _ = self.persist_to_disk();
        }
        Ok(removed)
    }

    /// Returns all current favorites sorted by order index.
    pub fn list_favorites(&self) -> Vec<FavoriteItem> {
        let mut list = self.items.read().clone();
        list.sort_by_key(|item| item.order_index);
        list
    }

    /// Returns only pinned favorites.
    pub fn list_pinned(&self) -> Vec<FavoriteItem> {
        let mut pinned: Vec<FavoriteItem> = self
            .items
            .read()
            .iter()
            .filter(|item| item.is_pinned)
            .cloned()
            .collect();
        pinned.sort_by_key(|item| item.order_index);
        pinned
    }

    /// Atomically persists items to disk with temporary file and rename.
    pub fn persist_to_disk(&self) -> Result<(), AppCoreError> {
        let path = match &self.storage_path {
            Some(p) => p,
            None => return Ok(()),
        };

        let items = self.items.read();
        let model = FavoritesSerializeModel {
            version: 1,
            favorites: &items[..],
        };

        let data = serde_json::to_vec_pretty(&model).map_err(|e| AppCoreError::Workspace {
            reason: format!("Failed to serialize favorites: {}", e),
        })?;

        let tmp_path = path.with_extension("tmp");
        {
            let mut file = File::create(&tmp_path).map_err(|e| AppCoreError::Io {
                reason: format!("Failed to create favorites tmp file: {}", e),
            })?;
            file.write_all(&data)?;
            file.sync_all()?;
        }

        fs::rename(&tmp_path, path).map_err(|e| AppCoreError::Io {
            reason: format!("Atomic rename for favorites file failed: {}", e),
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_favorites_registry_in_memory() {
        let reg = FavoritesRegistry::new();
        assert!(reg.list_favorites().is_empty());

        let fav = reg
            .add_favorite(
                "Docs",
                "file:///Users/docs/../docs",
                "folder",
                true,
                vec!["pinned".to_string()],
            )
            .unwrap();

        assert_eq!(fav.name, "Docs");
        assert_eq!(fav.uri, "file:///Users/docs");
        assert!(fav.is_pinned);
        assert_eq!(reg.list_favorites().len(), 1);
        assert_eq!(reg.list_pinned().len(), 1);

        assert!(reg.remove_favorite(fav.id).unwrap());
        assert!(reg.list_favorites().is_empty());
        assert!(!reg.remove_favorite(fav.id).unwrap());
    }

    #[test]
    fn test_favorites_registry_persistence() {
        let dir = tempdir().expect("tempdir failed");
        let json_path = dir.path().join("favorites.json");

        let reg = FavoritesRegistry::load_or_create(&json_path).expect("load failed");
        let fav1 = reg
            .add_favorite(
                "Documents",
                "file:///Users/home/Documents",
                "folder",
                true,
                vec!["work".to_string()],
            )
            .expect("add fav failed");

        assert_eq!(fav1.name, "Documents");
        assert!(fav1.is_pinned);

        // Reload from disk
        let reg2 = FavoritesRegistry::load_or_create(&json_path).expect("reload failed");
        let loaded = reg2.list_favorites();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Documents");
        assert_eq!(loaded[0].uri, "file:///Users/home/Documents");
        assert_eq!(loaded[0].created_at_epoch_ms, fav1.created_at_epoch_ms);
    }
}
