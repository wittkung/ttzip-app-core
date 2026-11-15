// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{ReaderAnchor, ReaderAnnotation, ReaderDocument, ReaderError, ReaderPreferences};
use ttzip_reader_storage as storage;

impl From<&ReaderAnchor> for storage::ReaderAnchor {
    fn from(anchor: &ReaderAnchor) -> Self {
        Self {
            chapter_index: anchor.chapter_index,
            block_index: anchor.block_index,
            character_offset: anchor.character_offset,
            chapter_ratio: anchor.chapter_ratio,
        }
    }
}

impl From<storage::ReaderAnchor> for ReaderAnchor {
    fn from(anchor: storage::ReaderAnchor) -> Self {
        Self {
            chapter_index: anchor.chapter_index,
            block_index: anchor.block_index,
            character_offset: anchor.character_offset,
            chapter_ratio: anchor.chapter_ratio,
        }
    }
}

impl From<ReaderPreferences> for storage::ReaderPreferences {
    fn from(preferences: ReaderPreferences) -> Self {
        Self {
            latin_font: preferences.latin_font,
            cjk_font: preferences.cjk_font,
            font_size: preferences.font_size,
            theme: preferences.theme,
            layout: preferences.layout,
        }
    }
}

impl From<storage::ReaderPreferences> for ReaderPreferences {
    fn from(preferences: storage::ReaderPreferences) -> Self {
        Self {
            latin_font: preferences.latin_font,
            cjk_font: preferences.cjk_font,
            font_size: preferences.font_size,
            theme: preferences.theme,
            layout: preferences.layout,
        }
    }
}

impl From<ReaderAnnotation> for storage::ReaderAnnotation {
    fn from(annotation: ReaderAnnotation) -> Self {
        Self {
            id: annotation.id,
            anchor: (&annotation.anchor).into(),
            quote: annotation.quote,
            note: annotation.note,
            color: annotation.color,
        }
    }
}

impl From<storage::ReaderAnnotation> for ReaderAnnotation {
    fn from(annotation: storage::ReaderAnnotation) -> Self {
        Self {
            id: annotation.id,
            anchor: annotation.anchor.into(),
            quote: annotation.quote,
            note: annotation.note,
            color: annotation.color,
        }
    }
}

impl From<storage::ReaderDocument> for ReaderDocument {
    fn from(document: storage::ReaderDocument) -> Self {
        Self {
            identity: document.identity,
            position: document.position.into(),
            preferences: document.preferences.into(),
            annotations: document.annotations.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<storage::ReaderError> for ReaderError {
    fn from(error: storage::ReaderError) -> Self {
        match error {
            storage::ReaderError::InvalidState { message } => Self::InvalidState { message },
            storage::ReaderError::Storage { message } => Self::Storage { message },
        }
    }
}
