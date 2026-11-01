// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Data models for multi-tab workspaces, view layouts, navigation history, and favorites.

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Helper returning current timestamp in milliseconds since UNIX epoch as a 64-bit unsigned integer.
#[inline]
pub fn current_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Helper converting a 64-bit milliseconds timestamp since UNIX epoch into a standard `SystemTime`.
#[inline]
pub fn epoch_ms_to_system_time(epoch_ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(epoch_ms)
}

/// Normalizes a path string, lexically resolving `.` and `..` components and collapsing duplicate slashes.
pub fn normalize_path_str(path: &str) -> String {
    let is_absolute = path.starts_with('/') || (cfg!(windows) && path.len() >= 2 && &path[1..2] == ":");
    let mut components: Vec<&str> = Vec::new();

    for segment in path.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                if let Some(last) = components.last() {
                    if *last != ".." {
                        components.pop();
                        continue;
                    }
                }
                if !is_absolute {
                    components.push("..");
                }
            }
            comp => components.push(comp),
        }
    }

    let joined = components.join("/");
    if is_absolute {
        format!("/{}", joined)
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// Lexically normalizes a filesystem path or URI reference.
///
/// Eliminates redundant slashes, resolves relative `.` and `..` components,
/// and maintains canonical formatting across local file paths and URI schemes.
pub fn normalize_uri(raw_uri: &str) -> String {
    let trimmed = raw_uri.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some((scheme, rest)) = trimmed.split_once("://") {
        if scheme.eq_ignore_ascii_case("file") {
            let norm_path = normalize_path_str(rest);
            format!("file://{}", norm_path)
        } else if let Some((authority, path_part)) = rest.split_once('/') {
            let norm_path = normalize_path_str(&format!("/{}", path_part));
            format!("{}://{}{}", scheme, authority, norm_path)
        } else {
            format!("{}://{}", scheme, rest)
        }
    } else {
        normalize_path_str(trimmed)
    }
}

/// Normalizes a `Path` reference into a canonical `PathBuf` without symlink resolution or disk I/O.
pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            c => normalized.push(c.as_os_str()),
        }
    }
    normalized
}

/// Extracts and normalizes local filesystem path from a URI string if applicable.
pub fn uri_to_normalized_path(uri: &str) -> Option<PathBuf> {
    if let Some(rest) = uri.strip_prefix("file://") {
        Some(normalize_path(Path::new(rest)))
    } else if !uri.contains("://") && (uri.starts_with('/') || (cfg!(windows) && uri.len() >= 2 && &uri[1..2] == ":")) {
        Some(normalize_path(Path::new(uri)))
    } else {
        None
    }
}

/// Screen layout arrangements for multi-pane workspaces.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceLayout {
    Single,
    DualPaneHorizontal,
    DualPaneVertical,
}

/// View presentation mode inside a tab.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabViewMode {
    MillerColumns,
    IconGrid,
    DetailList,
    Treemap,
}

/// Canonical 32-byte squarified treemap tile layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreemapTile {
    pub x: f32,          // 0..4:   Normalized [0.0..1.0] or pixel X coordinate
    pub y: f32,          // 4..8:   Normalized [0.0..1.0] or pixel Y coordinate
    pub width: f32,      // 8..12:  Tile width
    pub height: f32,     // 12..16: Tile height
    pub node_id: u32,    // 16..20: VFS CompactNode ID (0xFFFFFFFF = Others aggregate)
    pub depth: u16,      // 20..22: Relative directory depth from current root
    pub category: u8,    // 22..23: TileCategory discriminant
    pub flags: u8,       // 23..24: TileFlags bitmask
    pub bytes: u64,      // 24..32: Total uncompressed / physical size in bytes
}

pub const TREEMAP_FLAG_IS_DIR: u8 = 1 << 0;         // 0x01: Entry is a directory container
pub const TREEMAP_FLAG_IS_LEAF: u8 = 1 << 1;        // 0x02: Entry is a leaf node / standalone file
pub const TREEMAP_FLAG_TAIL_COLLAPSED: u8 = 1 << 2; // 0x04: Entry is an Others synthetic aggregate
pub const TREEMAP_FLAG_RECLAIMABLE: u8 = 1 << 3;    // 0x08: Marked as developer-reclaimable space
pub const TREEMAP_FLAG_CLOUD_STUB: u8 = 1 << 4;     // 0x10: Evicted dataless cloud file (iCloud / OneDrive)
pub const TREEMAP_FLAG_SELECTED: u8 = 1 << 5;       // 0x20: Currently highlighted by user cursor/tap
pub const TREEMAP_FLAG_FIRMLINK: u8 = 1 << 6;       // 0x40: APFS synthetic firmlink boundary

pub const TREEMAP_OTHERS_NODE_ID: u32 = 0xFFFF_FFFF;

/// Semantic category classifications for treemap visual presentation.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TileCategory {
    GeneralCode = 0,
    MediaImage = 1,
    MediaVideo = 2,
    MediaAudio = 3,
    CompressedArchive = 4,
    DocumentText = 5,
    ExecutableBinary = 6,
    ReclaimCache = 7,
    SystemMeta = 8,
    Others = 9,
}

impl TileCategory {
    #[inline]
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    #[inline]
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::GeneralCode,
            1 => Self::MediaImage,
            2 => Self::MediaVideo,
            3 => Self::MediaAudio,
            4 => Self::CompressedArchive,
            5 => Self::DocumentText,
            6 => Self::ExecutableBinary,
            7 => Self::ReclaimCache,
            8 => Self::SystemMeta,
            _ => Self::Others,
        }
    }
}

impl From<TileCategory> for u8 {
    #[inline]
    fn from(cat: TileCategory) -> Self {
        cat as u8
    }
}

impl From<u8> for TileCategory {
    #[inline]
    fn from(val: u8) -> Self {
        Self::from_u8(val)
    }
}

/// Sort criteria applied to folder contents inside a tab.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabSortOrder {
    NameAsc,
    NameDesc,
    SizeAsc,
    SizeDesc,
    DateModifiedAsc,
    DateModifiedDesc,
    Kind,
}

/// Canonical navigation history entry recording URI, timestamp, and optional title.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub uri: String,
    pub title: Option<String>,
    pub visited_at_epoch_ms: u64,
}

impl HistoryEntry {
    /// Creates a new navigation history entry with normalized URI and current 64-bit epoch timestamp.
    pub fn new(uri: String, title: Option<String>) -> Self {
        Self {
            uri: normalize_uri(&uri),
            title,
            visited_at_epoch_ms: current_epoch_ms(),
        }
    }

    /// Creates a navigation history entry with an explicit 64-bit timestamp.
    pub fn with_timestamp(uri: String, title: Option<String>, visited_at_epoch_ms: u64) -> Self {
        Self {
            uri: normalize_uri(&uri),
            title,
            visited_at_epoch_ms,
        }
    }

    /// Returns the visit timestamp as a standard `SystemTime`.
    pub fn visited_at(&self) -> SystemTime {
        epoch_ms_to_system_time(self.visited_at_epoch_ms)
    }

    /// Resolves local normalized filesystem path if this history entry points to a local file or directory.
    pub fn normalized_path(&self) -> Option<PathBuf> {
        uri_to_normalized_path(&self.uri)
    }
}

/// Description of an individual workspace tab with navigation history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceTab {
    pub id: Uuid,
    pub title: String,
    pub current_uri: String,
    pub history_back: Vec<String>,
    pub history_forward: Vec<String>,
    pub view_mode: TabViewMode,
    pub sort_order: TabSortOrder,
    pub is_pinned: bool,
    #[serde(default)]
    pub created_at_epoch_ms: u64,
    #[serde(default)]
    pub last_accessed_epoch_ms: u64,
    #[serde(default)]
    pub history_entries: Vec<HistoryEntry>,
}

impl WorkspaceTab {
    /// Creates a new tab with normalized URI, default presentation options, and 64-bit timestamps.
    pub fn new(title: String, initial_uri: String) -> Self {
        let norm_uri = normalize_uri(&initial_uri);
        let now_ms = current_epoch_ms();
        let initial_history = HistoryEntry::with_timestamp(norm_uri.clone(), Some(title.clone()), now_ms);

        Self {
            id: Uuid::new_v4(),
            title,
            current_uri: norm_uri,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            view_mode: TabViewMode::MillerColumns,
            sort_order: TabSortOrder::NameAsc,
            is_pinned: false,
            created_at_epoch_ms: now_ms,
            last_accessed_epoch_ms: now_ms,
            history_entries: vec![initial_history],
        }
    }

    /// Returns tab creation time as a `SystemTime`.
    pub fn created_at(&self) -> SystemTime {
        epoch_ms_to_system_time(self.created_at_epoch_ms)
    }

    /// Returns tab last access time as a `SystemTime`.
    pub fn last_accessed_at(&self) -> SystemTime {
        epoch_ms_to_system_time(self.last_accessed_epoch_ms)
    }

    /// Resolves local normalized filesystem path if this tab displays a local file or directory.
    pub fn normalized_path(&self) -> Option<PathBuf> {
        uri_to_normalized_path(&self.current_uri)
    }
}

/// Complete multi-tab workspace session state with 64-bit timestamps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSession {
    pub id: Uuid,
    pub name: String,
    pub active_tab_id: Uuid,
    pub tabs: Vec<WorkspaceTab>,
    pub layout: WorkspaceLayout,
    pub created_at_epoch_ms: u64,
    pub updated_at_epoch_ms: u64,
}

impl WorkspaceSession {
    /// Returns session creation time as a `SystemTime`.
    pub fn created_at(&self) -> SystemTime {
        epoch_ms_to_system_time(self.created_at_epoch_ms)
    }

    /// Returns session last modification time as a `SystemTime`.
    pub fn updated_at(&self) -> SystemTime {
        epoch_ms_to_system_time(self.updated_at_epoch_ms)
    }
}

/// Standardized cross-platform bookmark entry serialized in universal JSON format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FavoriteItem {
    pub id: Uuid,
    pub name: String,
    pub uri: String,
    pub icon_hint: String,
    pub is_pinned: bool,
    pub tags: Vec<String>,
    pub order_index: u32,
    #[serde(default)]
    pub created_at_epoch_ms: u64,
}

impl FavoriteItem {
    /// Returns favorite creation time as a `SystemTime`.
    pub fn created_at(&self) -> SystemTime {
        epoch_ms_to_system_time(self.created_at_epoch_ms)
    }

    /// Resolves local normalized filesystem path if this favorite points to a local file or directory.
    pub fn normalized_path(&self) -> Option<PathBuf> {
        uri_to_normalized_path(&self.uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_uri_and_path() {
        assert_eq!(
            normalize_uri("file:///Users/home/Downloads/../Documents/"),
            "file:///Users/home/Documents"
        );
        assert_eq!(
            normalize_uri("file:///Users//john///Downloads/"),
            "file:///Users/john/Downloads"
        );
        assert_eq!(
            normalize_uri("/var/log/../../etc/passwd"),
            "/etc/passwd"
        );
        assert_eq!(
            normalize_uri("mtp://device1/Internal/DCIM/../Pictures"),
            "mtp://device1/Internal/Pictures"
        );
        assert_eq!(
            normalize_uri("adb://192.168.1.100:5555/sdcard/Download/"),
            "adb://192.168.1.100:5555/sdcard/Download"
        );
    }

    #[test]
    fn test_uri_to_normalized_path() {
        let p1 = uri_to_normalized_path("file:///Users/home/Documents/report.txt").unwrap();
        assert_eq!(p1, PathBuf::from("/Users/home/Documents/report.txt"));

        let p2 = uri_to_normalized_path("/var/log/audit.log").unwrap();
        assert_eq!(p2, PathBuf::from("/var/log/audit.log"));

        assert!(uri_to_normalized_path("mtp://04E8:6860/Storage").is_none());
        assert!(uri_to_normalized_path("adb://device/sdcard").is_none());
    }

    #[test]
    fn test_workspace_tab_and_history_precision() {
        let tab = WorkspaceTab::new("Downloads".to_string(), "file:///Users/home/Downloads/..".to_string());
        assert_eq!(tab.current_uri, "file:///Users/home");
        assert!(tab.created_at_epoch_ms > 0);
        assert_eq!(tab.created_at_epoch_ms, tab.last_accessed_epoch_ms);
        assert_eq!(tab.history_entries.len(), 1);
        assert_eq!(tab.history_entries[0].uri, "file:///Users/home");
        assert!(tab.created_at() <= SystemTime::now());
    }

    #[test]
    fn test_favorite_item_precision_and_normalization() {
        let item = FavoriteItem {
            id: Uuid::new_v4(),
            name: "Home".to_string(),
            uri: "file:///Users/home/../home".to_string(),
            icon_hint: "folder".to_string(),
            is_pinned: true,
            tags: vec!["quick".to_string()],
            order_index: 0,
            created_at_epoch_ms: 1_700_000_000_000,
        };

        assert_eq!(item.created_at_epoch_ms, 1_700_000_000_000);
        let path = item.normalized_path().unwrap();
        assert_eq!(path, PathBuf::from("/Users/home"));
    }

    #[test]
    fn test_treemap_tile_layout() {
        assert_eq!(std::mem::size_of::<TreemapTile>(), 32);
        assert_eq!(std::mem::align_of::<TreemapTile>(), 8);
        assert_eq!(std::mem::size_of::<TileCategory>(), 1);
        assert_eq!(TileCategory::from_u8(0), TileCategory::GeneralCode);
        assert_eq!(TileCategory::from_u8(7), TileCategory::ReclaimCache);
        assert_eq!(TileCategory::from_u8(99), TileCategory::Others);
    }
}
