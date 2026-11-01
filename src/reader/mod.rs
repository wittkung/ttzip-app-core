// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

mod models;
mod storage;
#[cfg(test)]
mod tests;

pub use models::*;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(uniffi::Object)]
pub struct ReaderLibrary {
    directory: PathBuf,
    mutation_lock: Mutex<()>,
}

#[uniffi::export]
impl ReaderLibrary {
    #[uniffi::constructor]
    pub fn new(directory: String) -> Result<Arc<Self>, ReaderError> {
        let directory = PathBuf::from(directory);
        if !directory.is_absolute() {
            return Err(ReaderError::invalid("Storage path must be absolute"));
        }
        std::fs::create_dir_all(&directory).map_err(ReaderError::storage)?;
        Ok(Arc::new(Self {
            directory,
            mutation_lock: Mutex::new(()),
        }))
    }

    pub fn has_document(&self, identity: String) -> Result<bool, ReaderError> {
        let _guard = self.mutation_lock.lock();
        let path = self.document_path(&identity)?;
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => {
                self.read_document(&identity)?;
                Ok(true)
            }
            Ok(_) => Err(ReaderError::invalid(
                "Reader document is not a regular file",
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(ReaderError::storage(error)),
        }
    }

    pub fn load_document(&self, identity: String) -> Result<ReaderDocument, ReaderError> {
        let _guard = self.mutation_lock.lock();
        self.read_document(&identity)
    }

    pub fn save_position(
        &self,
        identity: String,
        position: ReaderAnchor,
    ) -> Result<(), ReaderError> {
        validate_anchor(&position)?;
        let _guard = self.mutation_lock.lock();
        let mut document = self.read_document(&identity)?;
        document.position = position;
        self.write_document(&document)
    }

    pub fn save_preferences(
        &self,
        identity: String,
        preferences: ReaderPreferences,
    ) -> Result<(), ReaderError> {
        models::validate_preferences(&preferences)?;
        let _guard = self.mutation_lock.lock();
        let mut document = self.read_document(&identity)?;
        document.preferences = preferences;
        self.write_document(&document)
    }

    pub fn upsert_annotation(
        &self,
        identity: String,
        mut annotation: ReaderAnnotation,
    ) -> Result<ReaderAnnotation, ReaderError> {
        models::validate_annotation_content(&annotation)?;
        let _guard = self.mutation_lock.lock();
        let mut document = self.read_document(&identity)?;
        if annotation.id.is_empty() {
            annotation.id = uuid::Uuid::new_v4().to_string();
        }
        if let Some(existing) = document
            .annotations
            .iter_mut()
            .find(|item| item.id == annotation.id)
        {
            *existing = annotation.clone();
        } else {
            if document.annotations.len() >= models::MAX_READER_ANNOTATIONS {
                return Err(ReaderError::invalid("Annotation limit reached"));
            }
            document.annotations.push(annotation.clone());
        }
        self.write_document(&document)?;
        Ok(annotation)
    }

    pub fn delete_annotation(
        &self,
        identity: String,
        annotation_id: String,
    ) -> Result<(), ReaderError> {
        let _guard = self.mutation_lock.lock();
        let mut document = self.read_document(&identity)?;
        document.annotations.retain(|item| item.id != annotation_id);
        self.write_document(&document)
    }

    pub fn export_annotations(
        &self,
        identity: String,
        format: String,
    ) -> Result<String, ReaderError> {
        let document = self.load_document(identity)?;
        match format.as_str() {
            "json" => serde_json::to_string_pretty(&document).map_err(ReaderError::storage),
            "markdown" => Ok(storage::export_markdown(&document)),
            _ => Err(ReaderError::invalid(
                "Export format must be json or markdown",
            )),
        }
    }
}
