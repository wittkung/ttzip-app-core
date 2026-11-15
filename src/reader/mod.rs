// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

mod conversion;
mod models;
#[cfg(test)]
mod tests;

pub use models::*;
use std::sync::Arc;
use ttzip_reader_storage::ReaderStore;

#[derive(uniffi::Object)]
pub struct ReaderLibrary {
    store: ReaderStore,
}

#[uniffi::export]
impl ReaderLibrary {
    #[uniffi::constructor]
    pub fn new(directory: String) -> Result<Arc<Self>, ReaderError> {
        Ok(Arc::new(Self {
            store: ReaderStore::new(directory)?,
        }))
    }

    pub fn has_document(&self, identity: String) -> Result<bool, ReaderError> {
        self.store.has_document(identity).map_err(Into::into)
    }

    pub fn load_document(&self, identity: String) -> Result<ReaderDocument, ReaderError> {
        self.store
            .load_document(identity)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_position(
        &self,
        identity: String,
        position: ReaderAnchor,
    ) -> Result<(), ReaderError> {
        self.store
            .save_position(identity, (&position).into())
            .map_err(Into::into)
    }

    pub fn save_preferences(
        &self,
        identity: String,
        preferences: ReaderPreferences,
    ) -> Result<(), ReaderError> {
        self.store
            .save_preferences(identity, preferences.into())
            .map_err(Into::into)
    }

    pub fn upsert_annotation(
        &self,
        identity: String,
        annotation: ReaderAnnotation,
    ) -> Result<ReaderAnnotation, ReaderError> {
        self.store
            .upsert_annotation(identity, annotation.into())
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn delete_annotation(
        &self,
        identity: String,
        annotation_id: String,
    ) -> Result<(), ReaderError> {
        self.store
            .delete_annotation(identity, annotation_id)
            .map_err(Into::into)
    }

    pub fn export_annotations(
        &self,
        identity: String,
        format: String,
    ) -> Result<String, ReaderError> {
        self.store
            .export_annotations(identity, format)
            .map_err(Into::into)
    }
}
