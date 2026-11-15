// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

const SWIFT_BINDINGS_SRC: &str =
    include_str!("../../Sources/TTZipAppCore/Generated/ttzip_app_core.swift");
const C_HEADER_SRC: &str = include_str!("../../Sources/CTTZipAppCoreBridge/include/ttzip_app_coreFFI.h");
const EXPECTED_UNIFFI_CONTRACT_VERSION: u32 = 26;
const EXPECTED_CHECKSUM_SYMBOL_COUNT: usize = 49;

unsafe extern "C" {
    fn ffi_ttzip_app_core_uniffi_contract_version() -> u32;

    fn uniffi_ttzip_app_core_checksum_func_compute_treemap_layout() -> u16;
    fn uniffi_ttzip_app_core_checksum_func_media_normalize_rotation() -> u16;
    fn uniffi_ttzip_app_core_checksum_func_media_resume_position() -> u16;
    fn uniffi_ttzip_app_core_checksum_func_mtp_delete_remote_object() -> u16;
    fn uniffi_ttzip_app_core_checksum_func_mtp_pull_file_to_local_staged() -> u16;
    fn uniffi_ttzip_app_core_checksum_func_mtp_push_local_file_verified() -> u16;
    fn uniffi_ttzip_app_core_checksum_func_start_storage_scan() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_add_favorite() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_add_workspace_tab() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_cancel_transfer() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_close_workspace_session() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_close_workspace_tab() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_connect_wireless_adb() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_create_workspace_session() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_finalize_transfer() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_get_active_throughput_mb_s() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_get_transfer_progress() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_get_workspace_session() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_list_adb_devices() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_list_device_directory() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_list_favorites() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_list_transfers() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_list_workspace_sessions() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_tab_back() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_tab_forward() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_workspace_tab() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_pause_transfer() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_record_transfer_chunk() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_remove_favorite() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_resume_transfer() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_scan_mtp_devices() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_submit_transfer() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_switch_workspace_tab() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_appcoreengine_write_transfer_chunk() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_readerlibrary_delete_annotation() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_readerlibrary_export_annotations() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_readerlibrary_has_document() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_readerlibrary_load_document() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_readerlibrary_save_position() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_readerlibrary_save_preferences() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_readerlibrary_upsert_annotation() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_storagescansessionhandle_cancel() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_storagescansessionhandle_is_cancelled() -> u16;
    fn uniffi_ttzip_app_core_checksum_constructor_appcoreengine_new() -> u16;
    fn uniffi_ttzip_app_core_checksum_constructor_readerlibrary_new() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_mtptransferprogresscallback_on_progress() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_storagescancallback_on_progress() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_storagescancallback_on_completed() -> u16;
    fn uniffi_ttzip_app_core_checksum_method_storagescancallback_on_error() -> u16;
}

struct ChecksumSymbol {
    name: &'static str,
    func: unsafe extern "C" fn() -> u16,
}

const LIVE_CHECKSUM_SYMBOLS: [ChecksumSymbol; EXPECTED_CHECKSUM_SYMBOL_COUNT] = [
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_func_compute_treemap_layout",
        func: uniffi_ttzip_app_core_checksum_func_compute_treemap_layout,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_func_media_normalize_rotation",
        func: uniffi_ttzip_app_core_checksum_func_media_normalize_rotation,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_func_media_resume_position",
        func: uniffi_ttzip_app_core_checksum_func_media_resume_position,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_func_mtp_delete_remote_object",
        func: uniffi_ttzip_app_core_checksum_func_mtp_delete_remote_object,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_func_mtp_pull_file_to_local_staged",
        func: uniffi_ttzip_app_core_checksum_func_mtp_pull_file_to_local_staged,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_func_mtp_push_local_file_verified",
        func: uniffi_ttzip_app_core_checksum_func_mtp_push_local_file_verified,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_func_start_storage_scan",
        func: uniffi_ttzip_app_core_checksum_func_start_storage_scan,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_add_favorite",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_add_favorite,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_add_workspace_tab",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_add_workspace_tab,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_cancel_transfer",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_cancel_transfer,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_close_workspace_session",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_close_workspace_session,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_close_workspace_tab",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_close_workspace_tab,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_connect_wireless_adb",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_connect_wireless_adb,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_create_workspace_session",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_create_workspace_session,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_finalize_transfer",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_finalize_transfer,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_get_active_throughput_mb_s",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_get_active_throughput_mb_s,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_get_transfer_progress",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_get_transfer_progress,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_get_workspace_session",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_get_workspace_session,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_list_adb_devices",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_list_adb_devices,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_list_device_directory",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_list_device_directory,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_list_favorites",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_list_favorites,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_list_transfers",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_list_transfers,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_list_workspace_sessions",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_list_workspace_sessions,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_tab_back",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_tab_back,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_tab_forward",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_tab_forward,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_workspace_tab",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_navigate_workspace_tab,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_pause_transfer",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_pause_transfer,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_record_transfer_chunk",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_record_transfer_chunk,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_remove_favorite",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_remove_favorite,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_resume_transfer",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_resume_transfer,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_scan_mtp_devices",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_scan_mtp_devices,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_submit_transfer",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_submit_transfer,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_switch_workspace_tab",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_switch_workspace_tab,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_appcoreengine_write_transfer_chunk",
        func: uniffi_ttzip_app_core_checksum_method_appcoreengine_write_transfer_chunk,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_readerlibrary_delete_annotation",
        func: uniffi_ttzip_app_core_checksum_method_readerlibrary_delete_annotation,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_readerlibrary_export_annotations",
        func: uniffi_ttzip_app_core_checksum_method_readerlibrary_export_annotations,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_readerlibrary_has_document",
        func: uniffi_ttzip_app_core_checksum_method_readerlibrary_has_document,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_readerlibrary_load_document",
        func: uniffi_ttzip_app_core_checksum_method_readerlibrary_load_document,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_readerlibrary_save_position",
        func: uniffi_ttzip_app_core_checksum_method_readerlibrary_save_position,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_readerlibrary_save_preferences",
        func: uniffi_ttzip_app_core_checksum_method_readerlibrary_save_preferences,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_readerlibrary_upsert_annotation",
        func: uniffi_ttzip_app_core_checksum_method_readerlibrary_upsert_annotation,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_storagescansessionhandle_cancel",
        func: uniffi_ttzip_app_core_checksum_method_storagescansessionhandle_cancel,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_storagescansessionhandle_is_cancelled",
        func: uniffi_ttzip_app_core_checksum_method_storagescansessionhandle_is_cancelled,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_constructor_appcoreengine_new",
        func: uniffi_ttzip_app_core_checksum_constructor_appcoreengine_new,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_constructor_readerlibrary_new",
        func: uniffi_ttzip_app_core_checksum_constructor_readerlibrary_new,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_mtptransferprogresscallback_on_progress",
        func: uniffi_ttzip_app_core_checksum_method_mtptransferprogresscallback_on_progress,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_storagescancallback_on_progress",
        func: uniffi_ttzip_app_core_checksum_method_storagescancallback_on_progress,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_storagescancallback_on_completed",
        func: uniffi_ttzip_app_core_checksum_method_storagescancallback_on_completed,
    },
    ChecksumSymbol {
        name: "uniffi_ttzip_app_core_checksum_method_storagescancallback_on_error",
        func: uniffi_ttzip_app_core_checksum_method_storagescancallback_on_error,
    },
];

fn parse_swift_checksum_guards(src: &str) -> BTreeMap<&str, u16> {
    let mut guards = BTreeMap::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("uniffi_ttzip_app_core_checksum_") {
            continue;
        }
        let inner = trimmed
            .strip_prefix("if (")
            .and_then(|s| s.strip_suffix(") {"))
            .unwrap_or_else(|| panic!("Malformed Swift checksum guard line: {trimmed}"));
        let (call_expr, expected_str) = inner
            .split_once(" != ")
            .unwrap_or_else(|| panic!("Missing '!=' in Swift checksum guard: {trimmed}"));
        let symbol = call_expr
            .strip_suffix("()")
            .unwrap_or_else(|| panic!("Expected zero-arg call in Swift checksum guard: {trimmed}"));
        let expected = expected_str
            .parse::<u16>()
            .unwrap_or_else(|e| panic!("Invalid u16 checksum in Swift guard '{trimmed}': {e}"));
        assert!(
            guards.insert(symbol, expected).is_none(),
            "Duplicate Swift checksum guard for symbol {symbol}"
        );
    }
    guards
}

fn parse_header_checksum_declarations(src: &str) -> BTreeSet<&str> {
    let mut symbols = BTreeSet::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("uniffi_ttzip_app_core_checksum_") {
            continue;
        }
        let rest = trimmed
            .strip_prefix("uint16_t ")
            .unwrap_or_else(|| panic!("Unexpected C header checksum line format: {trimmed}"));
        let (symbol, _) = rest
            .split_once('(')
            .unwrap_or_else(|| panic!("Missing '(' in C header checksum declaration: {trimmed}"));
        assert!(
            symbols.insert(symbol),
            "Duplicate C header checksum declaration for symbol {symbol}"
        );
    }
    symbols
}

#[test]
fn test_uniffi_contract_version_and_full_49_symbol_checksum_parity() {
    let live_version = unsafe { ffi_ttzip_app_core_uniffi_contract_version() };
    assert_eq!(live_version, EXPECTED_UNIFFI_CONTRACT_VERSION);

    let mut live_checksums = BTreeMap::new();
    for entry in &LIVE_CHECKSUM_SYMBOLS {
        let value = unsafe { (entry.func)() };
        assert!(
            live_checksums.insert(entry.name, value).is_none(),
            "Duplicate symbol in LIVE_CHECKSUM_SYMBOLS: {}",
            entry.name
        );
    }
    assert_eq!(live_checksums.len(), EXPECTED_CHECKSUM_SYMBOL_COUNT);

    let swift_checksums = parse_swift_checksum_guards(SWIFT_BINDINGS_SRC);
    assert_eq!(swift_checksums.len(), EXPECTED_CHECKSUM_SYMBOL_COUNT);

    let header_symbols = parse_header_checksum_declarations(C_HEADER_SRC);
    assert_eq!(header_symbols.len(), EXPECTED_CHECKSUM_SYMBOL_COUNT);

    let live_symbols: BTreeSet<&str> = live_checksums.keys().copied().collect();
    let swift_symbols: BTreeSet<&str> = swift_checksums.keys().copied().collect();
    assert_eq!(live_symbols, header_symbols);
    assert_eq!(live_symbols, swift_symbols);

    for (symbol, live_value) in &live_checksums {
        let swift_value = swift_checksums.get(symbol).copied().unwrap();
        assert_eq!(
            *live_value, swift_value,
            "UniFFI checksum drift for {symbol}: Rust C-ABI={live_value}, Swift={swift_value}"
        );
    }
}
