// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{models::validate_document, ReaderDocument, ReaderError, ReaderLibrary};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
use std::path::PathBuf;

impl ReaderLibrary {
    pub(super) fn document_path(&self, identity: &str) -> Result<PathBuf, ReaderError> {
        if identity.trim().is_empty() || identity.len() > 16 * 1024 {
            return Err(ReaderError::invalid("Invalid document identity"));
        }
        let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
        Ok(self.directory.join(format!("{digest}.json")))
    }

    pub(super) fn read_document(&self, identity: &str) -> Result<ReaderDocument, ReaderError> {
        let path = self.document_path(identity)?;
        match std::fs::File::open(&path) {
            Ok(file) => {
                let mut data = Vec::new();
                file.take(MAX_DOCUMENT_BYTES + 1)
                    .read_to_end(&mut data)
                    .map_err(ReaderError::storage)?;
                if data.len() as u64 > MAX_DOCUMENT_BYTES {
                    return Err(ReaderError::invalid(
                        "Reader document exceeds storage budget",
                    ));
                }
                let document: ReaderDocument =
                    serde_json::from_slice(&data).map_err(ReaderError::storage)?;
                if document.identity != identity {
                    return Err(ReaderError::invalid("Identity mismatch"));
                }
                validate_document(&document)?;
                Ok(document)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ReaderDocument {
                identity: identity.into(),
                ..ReaderDocument::default()
            }),
            Err(error) => Err(ReaderError::storage(error)),
        }
    }

    pub(super) fn write_document(&self, document: &ReaderDocument) -> Result<(), ReaderError> {
        validate_document(document)?;
        let path = self.document_path(&document.identity)?;
        let temporary = self
            .directory
            .join(format!(".reader-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(ReaderError::storage)?;
            let data = serde_json::to_vec(document).map_err(ReaderError::storage)?;
            if data.len() as u64 > MAX_DOCUMENT_BYTES {
                return Err(ReaderError::invalid(
                    "Reader document exceeds storage budget",
                ));
            }
            file.write_all(&data).map_err(ReaderError::storage)?;
            file.sync_all().map_err(ReaderError::storage)?;
            std::fs::rename(&temporary, path).map_err(ReaderError::storage)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
}

pub(super) fn export_markdown(document: &ReaderDocument) -> String {
    let mut output = format!(
        "# Reading notes\n\nDocument: {}\n",
        document.identity.replace('\n', " ")
    );
    for annotation in &document.annotations {
        output.push_str(&format!(
            "\n## Chapter {}\n\n",
            u64::from(annotation.anchor.chapter_index) + 1
        ));
        for line in annotation.quote.lines() {
            output.push_str(&format!("> {line}\n"));
        }
        if !annotation.note.is_empty() {
            output.push_str(&format!("\n{}\n", annotation.note));
        }
        output.push_str(&format!(
            "\nAnnotation: `{}`\n",
            annotation.id.replace('`', "")
        ));
    }
    output
}
