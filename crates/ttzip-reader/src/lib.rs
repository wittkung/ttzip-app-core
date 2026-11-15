// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

uniffi::setup_scaffolding!("ttzip_reader");

#[path = "../../../src/reader/mod.rs"]
pub mod reader;
