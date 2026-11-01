#!/usr/bin/env bash
# Copyright 2026 Witt Kung and TTZip Contributors.
# SPDX-License-Identifier: MIT OR Apache-2.0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_CORE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
UNIFFI_BINDGEN_VERSION="0.28.3"
BUILD_MODE="debug"
CARGO_FLAGS=""
BINDGEN_BIN=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --release) BUILD_MODE="release"; CARGO_FLAGS="--release"; shift ;;
        --debug) BUILD_MODE="debug"; CARGO_FLAGS=""; shift ;;
        --bindgen) BINDGEN_BIN="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--debug|--release] [--bindgen <PATH>]"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 2 ;;
    esac
done

if ! command -v rustc >/dev/null 2>&1; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

if [ -z "${BINDGEN_BIN}" ]; then
    BINDGEN_BIN="$(command -v uniffi-bindgen 2>/dev/null || true)"
fi
if [ -z "${BINDGEN_BIN}" ] || [ ! -x "${BINDGEN_BIN}" ]; then
    echo "Error: uniffi-bindgen not found. Install it with:" >&2
    echo "  cargo install uniffi-bindgen --version ${UNIFFI_BINDGEN_VERSION}" >&2
    exit 1
fi

echo "--> Building ttzip_app_core cdylib via Cargo (${BUILD_MODE})..."
(cd "${APP_CORE_DIR}" && cargo build ${CARGO_FLAGS})

TARGET_DIR="${CARGO_TARGET_DIR:-${APP_CORE_DIR}/target}"
DYLIB="${TARGET_DIR}/${BUILD_MODE}/libttzip_app_core.dylib"
if [ ! -f "${DYLIB}" ]; then
    echo "Error: built cdylib not found at ${DYLIB}" >&2
    exit 1
fi

TMP_DIR="$(mktemp -d /tmp/ttzip_app_core_uniffi_gen.XXXXXX)"
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "--> Generating UniFFI Swift bindings and C headers..."
(cd "${APP_CORE_DIR}" && "${BINDGEN_BIN}" generate \
    --library "${DYLIB}" \
    --language swift \
    --out-dir "${TMP_DIR}" \
    --no-format \
    --metadata-no-deps)

if [ -f "${TMP_DIR}/ttzip_app_core.swift" ]; then
    python3 "${APP_CORE_DIR}/scripts/postprocess_uniffi_swift.py" "${TMP_DIR}/ttzip_app_core.swift"
    mkdir -p "${APP_CORE_DIR}/Sources/TTZipAppCore/Generated"
    cp -f "${TMP_DIR}/ttzip_app_core.swift" "${APP_CORE_DIR}/Sources/TTZipAppCore/Generated/ttzip_app_core.swift"
fi

if [ -f "${TMP_DIR}/ttzip_app_coreFFI.h" ]; then
    mkdir -p "${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include"
    cp -f "${TMP_DIR}/ttzip_app_coreFFI.h" "${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include/ttzip_app_coreFFI.h"
fi

if [ -f "${TMP_DIR}/ttzip_app_coreFFI.modulemap" ]; then
    mkdir -p "${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include"
    cp -f "${TMP_DIR}/ttzip_app_coreFFI.modulemap" "${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include/module.modulemap"
fi

echo "UniFFI Swift bindings and C headers generated and synchronized successfully."
