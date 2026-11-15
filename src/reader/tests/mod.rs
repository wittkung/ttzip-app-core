// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use ttzip_reader_storage::ReaderStore;

#[test]
fn legacy_bridge_and_storage_leaf_share_existing_documents() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let identity = "file:///reader-bridge.epub";
    let bridge =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("bridge");
    let leaf = ReaderStore::new(directory.path()).expect("leaf");

    assert!(!bridge.has_document(identity.into()).expect("new document"));
    bridge
        .save_position(identity.into(), ReaderAnchor::default())
        .expect("save zero anchor");
    let pin = bridge
        .upsert_annotation(
            identity.into(),
            ReaderAnnotation {
                id: "legacy-note".into(),
                anchor: ReaderAnchor {
                    chapter_index: 2,
                    block_index: 3,
                    character_offset: 4,
                    chapter_ratio: 0.5,
                },
                quote: "A😀中文".into(),
                note: "Reader annotation".into(),
                color: "yellow".into(),
            },
        )
        .expect("bridge writes annotation");

    assert!(leaf
        .has_document(identity.into())
        .expect("zero anchor is present"));
    let document = leaf
        .load_document(identity.into())
        .expect("leaf reopens bridge record");
    assert_eq!(document.position.chapter_ratio, 0.0);
    assert_eq!(document.annotations[0].id, "legacy-note");
    assert_eq!(document.annotations[0].anchor.character_offset, 4);
    assert_eq!(document.annotations[0].quote, "A😀中文");
    assert_eq!(pin.id, document.annotations[0].id);

    let path = std::fs::read_dir(directory.path())
        .expect("records")
        .next()
        .expect("record")
        .expect("file")
        .path();
    assert_eq!(
        std::fs::read_to_string(&path).expect("persisted state"),
        concat!(
            r#"{"identity":"file:///reader-bridge.epub","position":{"chapter_index":0,"block_index":0,"character_offset":0,"chapter_ratio":0.0},"preferences":{"latin_font":"serif","cjk_font":"serif","font_size":17,"theme":"light","layout":"paginated"},"annotations":["#,
            r#"{"id":"legacy-note","anchor":{"chapter_index":2,"block_index":3,"character_offset":4,"chapter_ratio":0.5},"quote":"A😀中文","note":"Reader annotation","color":"yellow"}]}"#,
        )
    );
    drop(bridge);

    leaf.save_preferences(
        identity.into(),
        ttzip_reader_storage::ReaderPreferences {
            font_size: 24,
            ..document.preferences
        },
    )
    .expect("leaf writes preferences");
    let bridge =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("reopen bridge");
    assert_eq!(
        bridge
            .load_document(identity.into())
            .expect("bridge reads leaf record")
            .preferences
            .font_size,
        24
    );
    bridge
        .delete_annotation(identity.into(), pin.id)
        .expect("bridge deletes annotation");
    assert!(leaf
        .load_document(identity.into())
        .expect("leaf rereads record")
        .annotations
        .is_empty());
}

#[test]
fn legacy_bridge_preserves_storage_and_validation_errors() {
    let directory = tempfile::tempdir().expect("temporary reader directory");
    let identity = "file:///corrupt-reader-bridge.epub";
    let bridge =
        ReaderLibrary::new(directory.path().to_string_lossy().into_owned()).expect("bridge");
    bridge
        .save_position(identity.into(), ReaderAnchor::default())
        .expect("create record");
    let path = std::fs::read_dir(directory.path())
        .expect("records")
        .next()
        .expect("record")
        .expect("file")
        .path();

    std::fs::write(&path, b"{").expect("invalid historical data");
    assert!(matches!(
        bridge.load_document(identity.into()),
        Err(ReaderError::Storage { .. })
    ));
    assert!(matches!(
        bridge.save_position(
            identity.into(),
            ReaderAnchor {
                chapter_ratio: f64::NAN,
                ..ReaderAnchor::default()
            }
        ),
        Err(ReaderError::InvalidState { .. })
    ));
    assert_eq!(std::fs::read(&path).expect("original corrupt state"), b"{");
}
