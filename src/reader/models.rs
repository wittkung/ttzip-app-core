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
        Self {
            latin_font: "serif".into(),
            cjk_font: "serif".into(),
            font_size: 17,
            theme: "light".into(),
            layout: "paginated".into(),
        }
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
    if !anchor.chapter_ratio.is_finite() || !(0.0..=1.0).contains(&anchor.chapter_ratio) {
        return Err(ReaderError::invalid(
            "Chapter ratio must be finite and within 0...1",
        ));
    }
    Ok(())
}

pub(super) const MAX_READER_ANNOTATIONS: usize = 10_000;
pub(super) const MAX_ANNOTATION_QUOTE_BYTES: usize = 64 * 1024;
pub(super) const MAX_ANNOTATION_NOTE_BYTES: usize = 256 * 1024;
pub(super) const MAX_ANNOTATION_ID_BYTES: usize = 16 * 1024;
pub(super) const MAX_ANNOTATION_COLOR_BYTES: usize = 4 * 1024;

pub(super) fn validate_preferences(preferences: &ReaderPreferences) -> Result<(), ReaderError> {
    if !(10..=48).contains(&preferences.font_size)
        || !["light", "dark", "sepia", "transparent"].contains(&preferences.theme.as_str())
        || !["paginated", "scrolling"].contains(&preferences.layout.as_str())
        || preferences.latin_font.len() > 256
        || preferences.cjk_font.len() > 256
    {
        return Err(ReaderError::invalid("Unsupported reader preferences"));
    }
    Ok(())
}

/// Validates upsert content while allowing the API to allocate an initially empty ID.
pub(super) fn validate_annotation_content(
    annotation: &ReaderAnnotation,
) -> Result<(), ReaderError> {
    validate_anchor(&annotation.anchor)?;
    if annotation.quote.trim().is_empty()
        || annotation.quote.len() > MAX_ANNOTATION_QUOTE_BYTES
        || annotation.note.len() > MAX_ANNOTATION_NOTE_BYTES
    {
        return Err(ReaderError::invalid("Empty or oversized annotation"));
    }
    if annotation.id.len() > MAX_ANNOTATION_ID_BYTES
        || (!annotation.id.is_empty() && annotation.id.trim().is_empty())
    {
        return Err(ReaderError::invalid("Invalid annotation ID"));
    }
    if annotation.color.trim().is_empty() || annotation.color.len() > MAX_ANNOTATION_COLOR_BYTES {
        return Err(ReaderError::invalid("Invalid annotation color"));
    }
    Ok(())
}

pub(super) fn validate_document(document: &ReaderDocument) -> Result<(), ReaderError> {
    validate_anchor(&document.position)?;
    validate_preferences(&document.preferences)?;
    if document.annotations.len() > MAX_READER_ANNOTATIONS {
        return Err(ReaderError::invalid("Annotation limit reached"));
    }
    let mut ids = std::collections::HashSet::new();
    for annotation in &document.annotations {
        validate_annotation_content(annotation)?;
        if annotation.id.is_empty() || !ids.insert(annotation.id.as_str()) {
            return Err(ReaderError::invalid("Missing or duplicate annotation ID"));
        }
    }
    Ok(())
}
