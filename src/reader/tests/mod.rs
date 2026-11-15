// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

#[test]
fn annotations_and_position_survive_reopen_and_export() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let path = directory.path().to_string_lossy().into_owned();
    let reader = ReaderLibrary::new(path.clone()).expect("reader");
    let identity = "ttzip-vfs://archive-stable/OEBPS/book.epub".to_string();
    let anchor = ReaderAnchor {
        chapter_index: 3,
        block_index: 4,
        character_offset: 9,
        chapter_ratio: 0.4,
    };
    reader
        .save_position(identity.clone(), anchor.clone())
        .expect("position");
    let annotation = reader
        .upsert_annotation(
            identity.clone(),
            ReaderAnnotation {
                id: String::new(),
                anchor,
                quote: "中文 passage".into(),
                note: "Reader note".into(),
                color: "yellow".into(),
            },
        )
        .expect("annotation");
    drop(reader);
    let reader = ReaderLibrary::new(path).expect("reopen");
    let document = reader.load_document(identity.clone()).expect("load");
    assert_eq!(document.position.character_offset, 9);
    assert_eq!(document.annotations[0].id, annotation.id);
    let json = reader
        .export_annotations(identity.clone(), "json".into())
        .expect("json");
    let exported: ReaderDocument = serde_json::from_str(&json).expect("document JSON");
    assert_eq!(exported.annotations[0].quote, "中文 passage");
    let markdown = reader
        .export_annotations(identity.clone(), "markdown".into())
        .expect("markdown");
    assert!(markdown.contains("> 中文 passage"));
    assert!(markdown.contains("Reader note"));
    reader
        .delete_annotation(identity.clone(), annotation.id)
        .expect("delete");
    assert!(reader
        .load_document(identity)
        .expect("after delete")
        .annotations
        .is_empty());
}

#[test]
fn invalid_mutation_preserves_existing_document() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let reader =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("reader");
    let identity = "file:///books/book.epub".to_string();
    reader
        .save_position(identity.clone(), ReaderAnchor::default())
        .expect("initial");
    let anchor = ReaderAnchor {
        chapter_ratio: f64::NAN,
        ..ReaderAnchor::default()
    };
    assert!(reader.save_position(identity.clone(), anchor).is_err());
    assert_eq!(
        reader
            .load_document(identity)
            .expect("unchanged")
            .position
            .chapter_ratio,
        0.0
    );
}

#[test]
fn same_filename_in_different_locations_is_isolated() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let reader =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("reader");
    let first = "file:///first/book.epub".to_string();
    let second = "file:///second/book.epub".to_string();
    let anchor = ReaderAnchor {
        chapter_index: 5,
        ..ReaderAnchor::default()
    };
    reader.save_position(first.clone(), anchor).expect("first");
    assert_eq!(
        reader
            .load_document(first)
            .expect("first load")
            .position
            .chapter_index,
        5
    );
    assert_eq!(
        reader
            .load_document(second)
            .expect("second load")
            .position
            .chapter_index,
        0
    );
}

#[test]
fn stored_zero_position_is_distinguished_from_new_document_and_corruption() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let reader =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("reader");
    let identity = "file:///book.epub".to_string();
    assert!(!reader.has_document(identity.clone()).expect("new document"));
    reader
        .save_position(identity.clone(), ReaderAnchor::default())
        .expect("explicit beginning");
    assert!(reader
        .has_document(identity.clone())
        .expect("stored beginning"));
    let entry = std::fs::read_dir(directory.path())
        .expect("entries")
        .next()
        .expect("entry")
        .expect("file");
    std::fs::write(entry.path(), b"invalid JSON").expect("corrupt fixture");
    assert!(reader.has_document(identity.clone()).is_err());
    assert!(reader.load_document(identity).is_err());
}

fn valid_semantic_document(identity: &str) -> ReaderDocument {
    ReaderDocument {
        identity: identity.into(),
        position: ReaderAnchor {
            chapter_index: 2,
            block_index: 4,
            character_offset: 6,
            chapter_ratio: 0.4,
        },
        preferences: ReaderPreferences::default(),
        annotations: vec![ReaderAnnotation {
            id: "custom-id/章节#1".into(),
            anchor: ReaderAnchor::default(),
            quote: "A😀中文 quotation".into(),
            note: "Preserved note".into(),
            color: "rgb(250, 190, 40)".into(),
        }],
    }
}

fn semantically_invalid_documents(identity: &str) -> Vec<(&'static str, ReaderDocument)> {
    let base = valid_semantic_document(identity);
    let mut cases = Vec::new();
    let mut document = base.clone();
    document.preferences.font_size = 200;
    cases.push(("oversized font", document));
    let mut document = base.clone();
    document.preferences.font_size = 0;
    cases.push(("zero font", document));
    let mut document = base.clone();
    document.preferences.theme = "invalid".into();
    cases.push(("invalid theme", document));
    let mut document = base.clone();
    document.preferences.layout = "invalid".into();
    cases.push(("invalid layout", document));
    let mut document = base.clone();
    document.preferences.cjk_font = "x".repeat(257);
    cases.push(("oversized font name", document));
    let mut document = base.clone();
    document.position.chapter_ratio = 1.1;
    cases.push(("invalid position ratio", document));
    let mut document = base.clone();
    document.annotations[0].anchor.chapter_ratio = -0.1;
    cases.push(("invalid annotation ratio", document));
    let mut document = base.clone();
    document.annotations[0].id.clear();
    cases.push(("missing ID", document));
    let mut document = base.clone();
    document.annotations[0].id = " ".into();
    cases.push(("blank ID", document));
    let mut document = base.clone();
    document.annotations[0].id = "x".repeat(models::MAX_ANNOTATION_ID_BYTES + 1);
    cases.push(("oversized ID", document));
    let mut document = base.clone();
    document.annotations.push(document.annotations[0].clone());
    cases.push(("duplicate ID", document));
    let mut document = base.clone();
    document.annotations[0].color.clear();
    cases.push(("missing color", document));
    let mut document = base.clone();
    document.annotations[0].color = "x".repeat(models::MAX_ANNOTATION_COLOR_BYTES + 1);
    cases.push(("oversized color", document));
    let mut document = base.clone();
    document.annotations[0].quote = " \n\t".into();
    cases.push(("blank quote", document));
    let mut document = base.clone();
    document.annotations[0].quote = "x".repeat(models::MAX_ANNOTATION_QUOTE_BYTES + 1);
    cases.push(("oversized quote", document));
    let mut document = base.clone();
    document.annotations[0].note = "x".repeat(models::MAX_ANNOTATION_NOTE_BYTES + 1);
    cases.push(("oversized note", document));
    let mut document = base;
    document.annotations = (0..=models::MAX_READER_ANNOTATIONS)
        .map(|index| {
            let mut annotation = document.annotations[0].clone();
            annotation.id = index.to_string();
            annotation
        })
        .collect();
    cases.push(("annotation count", document));
    cases
}

#[test]
fn semantic_corruption_is_rejected_by_reads_and_mutations_without_overwriting_source() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let reader =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("reader");
    let identity = "file:///semantic-book.epub".to_string();
    let path = reader.document_path(&identity).expect("document path");
    for (label, document) in semantically_invalid_documents(&identity) {
        let bytes = serde_json::to_vec(&document).expect("syntactically valid JSON");
        std::fs::write(&path, &bytes).expect("semantic corruption fixture");
        assert!(
            reader.load_document(identity.clone()).is_err(),
            "load accepted {label}"
        );
        assert!(
            reader.has_document(identity.clone()).is_err(),
            "has_document accepted {label}"
        );
        assert!(
            reader
                .save_position(identity.clone(), ReaderAnchor::default())
                .is_err(),
            "save_position accepted {label}"
        );
        assert!(
            reader
                .save_preferences(identity.clone(), ReaderPreferences::default())
                .is_err(),
            "save_preferences accepted {label}"
        );
        let annotation = valid_semantic_document(&identity).annotations.remove(0);
        assert!(
            reader
                .upsert_annotation(identity.clone(), annotation)
                .is_err(),
            "upsert accepted {label}"
        );
        assert!(
            reader
                .delete_annotation(identity.clone(), "custom-id/章节#1".into())
                .is_err(),
            "delete accepted {label}"
        );
        assert!(
            reader
                .export_annotations(identity.clone(), "json".into())
                .is_err(),
            "export accepted {label}"
        );
        assert_eq!(
            std::fs::read(&path).expect("source bytes"),
            bytes,
            "source changed for {label}"
        );
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("entries")
                .count(),
            1,
            "temporary file leaked for {label}"
        );
    }
}

#[test]
fn full_document_writes_share_semantic_validation_and_preserve_existing_bytes() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let reader =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("reader");
    let identity = "file:///write-validation.epub";
    let valid = valid_semantic_document(identity);
    reader
        .write_document(&valid)
        .expect("initial valid document");
    let path = reader.document_path(identity).expect("path");
    let bytes = std::fs::read(&path).expect("baseline");
    for (label, document) in semantically_invalid_documents(identity) {
        assert!(
            reader.write_document(&document).is_err(),
            "write accepted {label}"
        );
        assert_eq!(
            std::fs::read(&path).expect("source"),
            bytes,
            "source changed for {label}"
        );
    }
}

#[test]
fn custom_ids_colors_and_utf16_anchor_values_remain_compatible() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let reader =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("reader");
    let identity = "file:///compatible-book.epub";
    let document = valid_semantic_document(identity);
    reader
        .write_document(&document)
        .expect("write legacy-compatible custom values");
    let loaded = reader.load_document(identity.into()).expect("load");
    assert_eq!(loaded.position.character_offset, 6);
    assert_eq!(loaded.annotations[0].id, "custom-id/章节#1");
    assert_eq!(loaded.annotations[0].color, "rgb(250, 190, 40)");
    assert_eq!(loaded.annotations[0].quote, "A😀中文 quotation");
}

#[test]
fn inclusive_field_budgets_and_u32_utf16_offsets_remain_valid() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let reader =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("reader");
    let identity = "file:///boundary-book.epub";
    for font_size in [10, 48] {
        let mut document = valid_semantic_document(identity);
        document.preferences.font_size = font_size;
        document.preferences.latin_font = "x".repeat(256);
        document.preferences.cjk_font = "y".repeat(256);
        document.position.character_offset = u32::MAX;
        let annotation = &mut document.annotations[0];
        annotation.id = "i".repeat(models::MAX_ANNOTATION_ID_BYTES);
        annotation.color = "c".repeat(models::MAX_ANNOTATION_COLOR_BYTES);
        annotation.quote = "q".repeat(models::MAX_ANNOTATION_QUOTE_BYTES);
        annotation.note = "n".repeat(models::MAX_ANNOTATION_NOTE_BYTES);
        reader
            .write_document(&document)
            .expect("inclusive field budgets");
        let loaded = reader
            .load_document(identity.into())
            .expect("load boundary values");
        assert_eq!(loaded.preferences.font_size, font_size);
        assert_eq!(loaded.position.character_offset, u32::MAX);
        assert_eq!(
            loaded.annotations[0].note.len(),
            models::MAX_ANNOTATION_NOTE_BYTES
        );
    }
    let mut document = valid_semantic_document(identity);
    let annotation = document.annotations[0].clone();
    document.annotations = (0..models::MAX_READER_ANNOTATIONS)
        .map(|index| {
            let mut item = annotation.clone();
            item.id = index.to_string();
            item
        })
        .collect();
    reader
        .write_document(&document)
        .expect("inclusive annotation count");
    assert_eq!(
        reader
            .load_document(identity.into())
            .expect("load annotation boundary")
            .annotations
            .len(),
        models::MAX_READER_ANNOTATIONS
    );
}
