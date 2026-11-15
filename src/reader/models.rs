// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, uniffi::Record)]
pub struct ReaderAnchor {
    pub chapter_index: u32,
    pub block_index: u32,
    pub character_offset: u32,
    pub chapter_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, uniffi::Record)]
pub struct ReaderPreferences {
    pub latin_font: String,
    pub cjk_font: String,
    pub font_size: u32,
    pub theme: String,
    pub layout: String,
}

impl Default for ReaderPreferences {
    fn default() -> Self {
        ttzip_reader_storage::ReaderPreferences::default().into()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, uniffi::Record)]
pub struct ReaderAnnotation {
    pub id: String,
    pub anchor: ReaderAnchor,
    pub quote: String,
    pub note: String,
    pub color: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, uniffi::Record)]
pub struct ReaderDocument {
    pub identity: String,
    pub position: ReaderAnchor,
    pub preferences: ReaderPreferences,
    pub annotations: Vec<ReaderAnnotation>,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ReaderError {
    #[error("Invalid reader state: {message}")]
    InvalidState { message: String },
    #[error("Reader storage failed: {message}")]
    Storage { message: String },
}

impl ReaderError {
    pub fn invalid(message: &str) -> Self {
        Self::InvalidState {
            message: message.into(),
        }
    }
    pub fn storage(error: impl std::fmt::Display) -> Self {
        Self::Storage {
            message: error.to_string(),
        }
    }
}

pub fn validate_anchor(anchor: &ReaderAnchor) -> Result<(), ReaderError> {
    ttzip_reader_storage::validate_anchor(&anchor.into()).map_err(Into::into)
}
