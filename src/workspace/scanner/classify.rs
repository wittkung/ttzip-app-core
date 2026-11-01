// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Semantic classification and developer cache identification.
//!
//! Categorizes files and directories into `TileCategory` values and detects
//! reclaimable development artifacts (e.g. `target/`, `node_modules/`, `DerivedData`).

use crate::workspace::types::TileCategory;

/// Granular categorization of reclaimable developer caches and temporary artifacts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReclaimCategory {
    /// Rust Cargo compilation artifacts (`target/`).
    CargoTarget,
    /// Node.js package dependencies (`node_modules/`).
    NodeModules,
    /// Apple Xcode intermediate build outputs (`DerivedData/`).
    XcodeDerivedData,
    /// Java/Gradle/Maven package caches (`.gradle/`, `.m2/`).
    PackageCache,
    /// Docker container virtual machine disk layers.
    DockerStorage,
    /// Python virtual environments and bytecode (`.venv/`, `venv/`, `__pycache__/`).
    PythonEnv,
    /// Git repositories or internal loose objects (`.git/`).
    GitGarbage,
    /// Generic build outputs (`.build/`, `dist/`, `build/`, `out/`).
    BuildOutput,
}

/// Checks whether a directory name matches known reclaimable cache signatures.
pub fn is_reclaimable_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "target"
            | "node_modules"
            | "deriveddata"
            | ".gradle"
            | ".m2"
            | "__pycache__"
            | ".pytest_cache"
            | ".venv"
            | "venv"
            | ".build"
            | "build"
            | "dist"
            | "out"
            | ".next"
            | ".nuxt"
            | ".turbo"
            | "pods"
            | ".cargo-cache"
    )
}

fn match_name_reclaim(name: &str) -> Option<ReclaimCategory> {
    match name {
        "target" => Some(ReclaimCategory::CargoTarget),
        "node_modules" => Some(ReclaimCategory::NodeModules),
        "deriveddata" => Some(ReclaimCategory::XcodeDerivedData),
        ".gradle" | ".m2" | "pods" => Some(ReclaimCategory::PackageCache),
        "docker" => Some(ReclaimCategory::DockerStorage),
        "__pycache__" | ".pytest_cache" | ".venv" | "venv" => Some(ReclaimCategory::PythonEnv),
        ".git" => Some(ReclaimCategory::GitGarbage),
        ".build" | "build" | "dist" | "out" | ".next" | ".nuxt" => {
            Some(ReclaimCategory::BuildOutput)
        }
        _ => None,
    }
}

/// Detects the granular `ReclaimCategory` for a given directory name and path.
pub fn detect_reclaim_category(name: &str, path: &str) -> Option<ReclaimCategory> {
    let lower_name = name.to_ascii_lowercase();
    if let Some(cat) = match_name_reclaim(lower_name.as_str()) {
        return Some(cat);
    }
    let lower_path = path.to_ascii_lowercase();
    if lower_path.contains("/developer/xcode/deriveddata") {
        Some(ReclaimCategory::XcodeDerivedData)
    } else if lower_path.contains("com.docker.docker") {
        Some(ReclaimCategory::DockerStorage)
    } else {
        None
    }
}

/// Classifies a path or filename into a canonical `TileCategory`.
pub fn classify_tile_category(name: &str, is_dir: bool) -> TileCategory {
    if is_dir {
        if is_reclaimable_dir(name) {
            return TileCategory::ReclaimCache;
        }
        if name.starts_with('.') || name == ".DS_Store" || name == ".localized" {
            return TileCategory::SystemMeta;
        }
        return TileCategory::GeneralCode;
    }

    if name == ".DS_Store" || name == ".localized" || name.starts_with("._") {
        return TileCategory::SystemMeta;
    }

    if let Some((_, ext)) = name.rsplit_once('.') {
        classify_extension(ext)
    } else {
        TileCategory::Others
    }
}

/// Classifies a file extension into a `TileCategory`.
pub fn classify_extension(ext: &str) -> TileCategory {
    let lower = ext.to_ascii_lowercase();
    match lower.as_str() {
        // General source code & web standards
        "rs" | "swift" | "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" | "m" | "mm" | "go" | "py"
        | "ts" | "js" | "tsx" | "jsx" | "java" | "kt" | "kts" | "rb" | "php" | "cs" | "html"
        | "htm" | "css" | "scss" | "sass" | "less" | "json" | "yaml" | "yml" | "toml" | "xml"
        | "sql" | "sh" | "bash" | "zsh" | "lua" | "vim" => TileCategory::GeneralCode,

        // Visual media / raster and vector graphics
        "png" | "jpg" | "jpeg" | "heic" | "heif" | "webp" | "svg" | "gif" | "bmp" | "tiff"
        | "tif" | "ico" | "raw" | "cr2" | "nef" | "psd" | "ai" => TileCategory::MediaImage,

        // Video containers and streams
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "flv" | "wmv" | "m4v" | "m2ts" | "mts" | "mpg"
        | "mpeg" | "vob" => TileCategory::MediaVideo,

        // Audio containers and streams
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "aiff" | "alac" | "opus" => {
            TileCategory::MediaAudio
        }

        // Compressed archives
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "bz2" | "tbz2" | "xz" | "txz" | "zst"
        | "lzma" | "cab" | "iso" | "dmg" => TileCategory::CompressedArchive,

        // Textual and productivity documents
        "pdf" | "docx" | "doc" | "rtf" | "txt" | "md" | "markdown" | "csv" | "xlsx" | "xls"
        | "pptx" | "ppt" | "pages" | "numbers" | "keynote" | "epub" => TileCategory::DocumentText,

        // Executable binaries and dynamic libraries
        "dylib" | "so" | "dll" | "framework" | "a" | "o" | "obj" | "exe" | "app" | "bin" => {
            TileCategory::ExecutableBinary
        }

        _ => TileCategory::Others,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reclaimable_dir_classification() {
        assert!(is_reclaimable_dir("target"));
        assert!(is_reclaimable_dir("Target"));
        assert!(is_reclaimable_dir("node_modules"));
        assert!(is_reclaimable_dir("DerivedData"));
        assert!(is_reclaimable_dir(".gradle"));
        assert!(is_reclaimable_dir("__pycache__"));
        assert!(is_reclaimable_dir(".venv"));
        assert!(is_reclaimable_dir("build"));
        assert!(is_reclaimable_dir(".build"));
        assert!(!is_reclaimable_dir("src"));
        assert!(!is_reclaimable_dir("docs"));
    }

    #[test]
    fn test_detect_reclaim_category() {
        assert_eq!(
            detect_reclaim_category("target", "/project/target"),
            Some(ReclaimCategory::CargoTarget)
        );
        assert_eq!(
            detect_reclaim_category("node_modules", "/app/node_modules"),
            Some(ReclaimCategory::NodeModules)
        );
        assert_eq!(
            detect_reclaim_category("DerivedData", "/Library/Developer/Xcode/DerivedData"),
            Some(ReclaimCategory::XcodeDerivedData)
        );
        assert_eq!(
            detect_reclaim_category(".gradle", "/home/.gradle"),
            Some(ReclaimCategory::PackageCache)
        );
    }

    #[test]
    fn test_tile_category_classification() {
        assert_eq!(
            classify_tile_category("target", true),
            TileCategory::ReclaimCache
        );
        assert_eq!(
            classify_tile_category(".git", true),
            TileCategory::SystemMeta
        );
        assert_eq!(
            classify_tile_category(".DS_Store", false),
            TileCategory::SystemMeta
        );
        assert_eq!(
            classify_tile_category("main.rs", false),
            TileCategory::GeneralCode
        );
        assert_eq!(
            classify_tile_category("photo.png", false),
            TileCategory::MediaImage
        );
        assert_eq!(
            classify_tile_category("movie.mov", false),
            TileCategory::MediaVideo
        );
        assert_eq!(
            classify_tile_category("song.flac", false),
            TileCategory::MediaAudio
        );
        assert_eq!(
            classify_tile_category("backup.tar.gz", false),
            TileCategory::CompressedArchive
        );
        assert_eq!(
            classify_tile_category("spec.md", false),
            TileCategory::DocumentText
        );
        assert_eq!(
            classify_tile_category("libttzip.dylib", false),
            TileCategory::ExecutableBinary
        );
    }
}
