#!/usr/bin/env bash
# Copyright 2026 Witt Kung and TTZip Contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_CORE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
CORE_DIR="${APP_CORE_DIR}/../core"
if [ ! -d "${CORE_DIR}" ]; then
    CORE_DIR=""
fi
CORE_RUST_DIR="${CORE_DIR:+${CORE_DIR}/rust}"
FRAMEWORKS_DIR="${APP_CORE_DIR}/Frameworks"
XCFRAMEWORK_DIR="${FRAMEWORKS_DIR}/TTZipAppCoreVendor.xcframework"
CORE_FRAMEWORKS_DIR="${CORE_DIR:+${CORE_DIR}/Frameworks}"
CORE_XCFRAMEWORK_DIR="${CORE_FRAMEWORKS_DIR:+${CORE_FRAMEWORKS_DIR}/TTZipVendor.xcframework}"

BUILD_MODE="release"
CARGO_FLAGS="--release"
BUILD_TARGET=""
UNIVERSAL=0
OFFLINE_FLAG=""
FORCE_REBUILD=0
SKIP_UNIFFI=0

usage() {
    echo "Usage: $0 [OPTIONS]"
    echo "Options:"
    echo "  --release        Build in release mode with the pinned rustup toolchain (default)"
    echo "  --debug          Build in debug mode"
    echo "  --universal      Build universal fat binary (arm64 + x86_64)"
    echo "  --target <TRGT>  Build specific target architecture (e.g. aarch64-apple-darwin)"
    echo "  --skip-uniffi    Skip UniFFI bindings generation"
    echo "  --force, -f      Force clean rebuild of cargo targets and xcframework"
    echo "  --offline        Build offline without network access"
    echo "  --help, -h       Show this help message"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --release)
            BUILD_MODE="release"
            CARGO_FLAGS="--release"
            shift
            ;;
        --debug)
            BUILD_MODE="debug"
            CARGO_FLAGS=""
            shift
            ;;
        --universal)
            UNIVERSAL=1
            shift
            ;;
        --target)
            BUILD_TARGET="$2"
            shift 2
            ;;
        --skip-uniffi)
            SKIP_UNIFFI=1
            shift
            ;;
        --force|-f)
            FORCE_REBUILD=1
            shift
            ;;
        --offline)
            OFFLINE_FLAG="--offline"
            shift
            ;;
        --help|-h)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

# Ensure Rust toolchain is in PATH if not already available
if ! command -v rustc >/dev/null 2>&1; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi
export MACOSX_DEPLOYMENT_TARGET="14.0"
export CFLAGS="-mmacosx-version-min=14.0 ${CFLAGS:-}"
export CXXFLAGS="-mmacosx-version-min=14.0 ${CXXFLAGS:-}"
export RUSTFLAGS="-C link-arg=-mmacosx-version-min=14.0 ${RUSTFLAGS:-}"

# Enforce static vendored libusb compilation for hermetic staticlib packaging
export LIBUSB_STATIC=1

EFFECTIVE_APP_TARGET_DIR="${CARGO_TARGET_DIR:-${APP_CORE_DIR}/target}"
if [ -n "${CORE_RUST_DIR}" ]; then
    EFFECTIVE_CORE_TARGET_DIR="${CARGO_TARGET_DIR:-${CORE_RUST_DIR}/target}"
else
    EFFECTIVE_CORE_TARGET_DIR="${CARGO_TARGET_DIR:-${APP_CORE_DIR}/target}"
fi
HOST_ARCH="$(uname -m)"
TARGETS=()

if [ "${UNIVERSAL}" -eq 1 ]; then
    TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin")
elif [ -n "${BUILD_TARGET}" ]; then
    TARGETS+=("${BUILD_TARGET}")
else
    if [ "${HOST_ARCH}" = "arm64" ]; then
        TARGETS+=("aarch64-apple-darwin")
    else
        TARGETS+=("x86_64-apple-darwin")
    fi
fi

if [ "${BUILD_MODE}" = release ]; then
    release_codec_helper="${SCRIPT_DIR}/release_codec_stage.sh"
    if [ ! -f "${release_codec_helper}" ] && [ -n "${CORE_DIR:-}" ]; then
        release_codec_helper="${CORE_DIR}/scripts/release_codec_stage.sh"
    fi
    source "${release_codec_helper}"
    prepare_release_codec_stages "${TARGETS[@]}"
fi

CARGO=(cargo)
if [ "${BUILD_MODE}" = "release" ]; then
    # Use the rustup sysroot with a macOS 14-compatible floor and macOS 27 strip support.
    RUST_VERSION="1.99.1"
    RUSTUP="$(command -v rustup || printf '%s\n' "${CARGO_HOME:-${HOME}/.cargo}/bin/rustup")"
    if ! RUSTC="$("${RUSTUP}" which --toolchain "${RUST_VERSION}" rustc)"; then
        echo "error: install the release Rust toolchain with: rustup toolchain install ${RUST_VERSION}" >&2
        exit 1
    fi
    export RUSTC RUSTC_WRAPPER="" RUSTC_WORKSPACE_WRAPPER=""
    CARGO=("${RUSTUP}" run "${RUST_VERSION}" cargo)
    "${RUSTC}" --version
    for target in "${TARGETS[@]}"; do
        target_libdir="$("${RUSTC}" --print target-libdir --target "${target}")"
        if [ ! -d "${target_libdir}" ]; then
            echo "error: install the release target with: rustup target add --toolchain ${RUST_VERSION} ${target}" >&2
            exit 1
        fi
        echo "--> Rust target libraries: ${target_libdir}"
    done
fi

if [ "${FORCE_REBUILD}" -eq 1 ]; then
    echo "--> [INFO] Forcing full clean rebuild..."
    "${CARGO[@]}" clean --manifest-path "${APP_CORE_DIR}/Cargo.toml"
    if [ -n "${CORE_RUST_DIR}" ] && [ -f "${CORE_RUST_DIR}/Cargo.toml" ]; then
        "${CARGO[@]}" clean --manifest-path "${CORE_RUST_DIR}/Cargo.toml"
    fi
    rm -rf "${XCFRAMEWORK_DIR}" "${FRAMEWORKS_DIR}/libttzip_desktop.a"
fi

echo "=========================================="
echo "📦 Building TTZip Unified Desktop Core (${BUILD_MODE})"
echo "--> Targets: ${TARGETS[*]}"
echo "=========================================="

# Resolve candidate C library search directories dynamically
CANDIDATE_LIB_DIRS=()
if [ -n "${TTZIP_SYS_LIB_DIR:-}" ] && [ -d "${TTZIP_SYS_LIB_DIR}" ]; then
    CANDIDATE_LIB_DIRS+=("${TTZIP_SYS_LIB_DIR}")
fi
if command -v brew >/dev/null 2>&1; then
    BREW_PREFIX="$(brew --prefix 2>/dev/null || true)"
    if [ -n "${BREW_PREFIX}" ] && [ -d "${BREW_PREFIX}/lib" ]; then
        CANDIDATE_LIB_DIRS+=("${BREW_PREFIX}/lib")
    fi
fi
if [ -d "/opt/homebrew/lib" ]; then
    CANDIDATE_LIB_DIRS+=("/opt/homebrew/lib")
fi
if [ -d "/usr/local/lib" ]; then
    CANDIDATE_LIB_DIRS+=("/usr/local/lib")
fi

IFS=":"
RESOLVED_LIB_PATH="${CANDIDATE_LIB_DIRS[*]:-}"
unset IFS
export LIBRARY_PATH="${RESOLVED_LIB_PATH:+${RESOLVED_LIB_PATH}:}${LIBRARY_PATH:-}"

PER_TARGET_MERGED=()
for index in "${!TARGETS[@]}"; do
    target="${TARGETS[index]}"
    release_stage=""
    usb_features=()
    if [ "${BUILD_MODE}" = release ]; then
        release_stage="${RELEASE_CODEC_STAGES[index]}"
        usb_features=(--features rusb/vendored)
    fi
    echo "--> [INFO] Compiling ttzip_app_core staticlib for ${target} (${BUILD_MODE})..."
    (cd "${APP_CORE_DIR}" && TTZIP_RELEASE_CODEC_STAGE="${release_stage}" "${CARGO[@]}" build \
        --target "${target}" \
        ${usb_features[@]+"${usb_features[@]}"} \
        ${CARGO_FLAGS} \
        ${OFFLINE_FLAG})

    APP_TARGET_LIB="${EFFECTIVE_APP_TARGET_DIR}/${target}/${BUILD_MODE}/libttzip_app_core.a"
    if [ ! -f "${APP_TARGET_LIB}" ]; then
        echo "❌ Error: Expected static library not found at ${APP_TARGET_LIB}"
        exit 1
    fi

    TARGET_PARTS=("${APP_TARGET_LIB}")

    # Build or link ttzip-engine from core repository if available
    if [ -n "${CORE_RUST_DIR}" ] && [ -f "${CORE_RUST_DIR}/Cargo.toml" ]; then
        echo "--> [INFO] Compiling ttzip-engine core staticlib for ${target} (${BUILD_MODE})..."
        TTZIP_RELEASE_CODEC_STAGE="${release_stage}" "${CARGO[@]}" build \
            --manifest-path "${CORE_RUST_DIR}/Cargo.toml" \
            --package ttzip-engine \
            --features full \
            --target "${target}" \
            ${CARGO_FLAGS} \
            ${OFFLINE_FLAG} || [ "${BUILD_MODE}" = debug ]

        CORE_TARGET_LIB="${EFFECTIVE_CORE_TARGET_DIR}/${target}/${BUILD_MODE}/libttzip_engine.a"
        if [ -f "${CORE_TARGET_LIB}" ]; then
            TARGET_PARTS+=("${CORE_TARGET_LIB}")
        fi

        # Locate native codecs library from core build output
        CODEC_LIB="$(find "${EFFECTIVE_CORE_TARGET_DIR}/${target}" -name "libttzip_native_codecs.a" -exec ls -t {} + 2>/dev/null | head -n 1 || true)"
        if [ -n "${CODEC_LIB}" ] && [ -f "${CODEC_LIB}" ]; then
            TARGET_PARTS+=("${CODEC_LIB}")
        fi
    fi

    if [ "${BUILD_MODE}" = release ]; then
        for libname in "${RELEASE_CODEC_ARCHIVES[@]}"; do
            TARGET_PARTS+=("${release_stage}/${libname}")
        done
    else
        # Locate vendored libusb archive produced by libusb1-sys build script
        LIBUSB_STATIC="$(find "${EFFECTIVE_APP_TARGET_DIR}/${target}" -name "libusb-vendored.a" 2>/dev/null | head -n 1 || true)"
        if [ -n "${LIBUSB_STATIC}" ] && [ -f "${LIBUSB_STATIC}" ]; then
            TARGET_PARTS+=("${LIBUSB_STATIC}")
        fi

        # Locate static external compression libraries (deflate, lz4, zstd)
        for libname in libdeflate.a liblz4.a libzstd.a; do
            FOUND_LIB=""
            pkg_base="${libname#lib}"
            pkg_base="${pkg_base%.a}"
            if command -v pkg-config >/dev/null 2>&1 && pkg-config --exists "${pkg_base}" 2>/dev/null; then
                PKG_LIB_DIR="$(pkg-config --variable=libdir "${pkg_base}" 2>/dev/null || true)"
                if [ -n "${PKG_LIB_DIR}" ] && [ -f "${PKG_LIB_DIR}/${libname}" ]; then
                    FOUND_LIB="${PKG_LIB_DIR}/${libname}"
                fi
            fi
            if [ -z "${FOUND_LIB}" ]; then
                for candidate in "${CANDIDATE_LIB_DIRS[@]}"; do
                    if [ -f "${candidate}/${libname}" ]; then
                        FOUND_LIB="${candidate}/${libname}"
                        break
                    fi
                done
            fi
            if [ -n "${FOUND_LIB}" ]; then
                TARGET_PARTS+=("${FOUND_LIB}")
            fi
        done
    fi

    # Merge all target components into single monolithic slice via libtool
    PER_TARGET_OUT="$(mktemp /tmp/ttzip_desktop_${target}_XXXXXX.a)"
    if [ ${#TARGET_PARTS[@]} -gt 1 ]; then
        echo "--> [INFO] Merging ${#TARGET_PARTS[@]} static components into monolithic libttzip_desktop slice for ${target}..."
        libtool -static -no_warning_for_no_symbols -o "${PER_TARGET_OUT}" "${TARGET_PARTS[@]}"
    else
        cp -f "${TARGET_PARTS[0]}" "${PER_TARGET_OUT}"
    fi

    PER_TARGET_MERGED+=("${PER_TARGET_OUT}")
done

# ------------------------------------------------------------------------------
# Duplicate member removal
# ------------------------------------------------------------------------------
#
# A Rust static library bundles its upstream crates, so merging two of them
# carries the same object file twice. A linker that loads every member of the
# archive reports duplicate symbols. Members that define the same symbols are
# the same object file and are removed; members that define different symbols
# are different build variants and are kept.

# ------------------------------------------------------------------------------
# Embedded bitcode removal
# ------------------------------------------------------------------------------
#
# A Rust release build embeds the LLVM bitcode of every object so that a
# consumer can run link time optimization on the archive. The shipped
# framework does not need it, and leaving it in triples the size of the
# archive.
#
# The bitcode has to be removed with `llvm-objcopy`. Apple's `strip` drops the
# `.llvm.<hash>` suffix that Rust puts on the internal symbols it instantiates
# in more than one codegen unit, and without that suffix two members define
# the same symbol, which a linker that loads every member of the archive
# reports as a duplicate symbol.

resolve_archive_deduplicator() {
    if [ -f "${SCRIPT_DIR}/deduplicate_archive.py" ]; then
        printf '%s\n' "${SCRIPT_DIR}/deduplicate_archive.py"
        return 0
    fi
    if [ -n "${CORE_DIR:-}" ] && [ -f "${CORE_DIR}/scripts/deduplicate_archive.py" ]; then
        printf '%s\n' "${CORE_DIR}/scripts/deduplicate_archive.py"
        return 0
    fi
    return 1
}

strip_embedded_bitcode() {
    local archive="$1"
    local tool
    if tool="$(resolve_archive_deduplicator)"; then
        python3 "${tool}" strip-bitcode "${archive}"
    fi
}

deduplicate_archive() {
    local archive="$1"
    local tool
    if tool="$(resolve_archive_deduplicator)"; then
        python3 "${tool}" deduplicate "${archive}"
    else
        ranlib -s "${archive}" >/dev/null 2>&1 || true
    fi
}

# Prepare final single or universal static library
FINAL_STATIC_LIB_DIR="$(mktemp -d /tmp/ttzip_desktop_lib_XXXXXX)"
FINAL_STATIC_LIB="${FINAL_STATIC_LIB_DIR}/libTTZipAppCoreVendor.a"

if [ ${#PER_TARGET_MERGED[@]} -gt 1 ]; then
    echo "--> [INFO] Combining slices into universal fat binary via lipo..."
    lipo -create "${PER_TARGET_MERGED[@]}" -output "${FINAL_STATIC_LIB}"
else
    cp -f "${PER_TARGET_MERGED[0]}" "${FINAL_STATIC_LIB}"
fi
rm -f "${PER_TARGET_MERGED[@]}"

strip_embedded_bitcode "${FINAL_STATIC_LIB}"

deduplicate_archive "${FINAL_STATIC_LIB}"


# Deploy monolithic libttzip_desktop.a artifact
mkdir -p "${FRAMEWORKS_DIR}"
cp -f "${FINAL_STATIC_LIB}" "${FRAMEWORKS_DIR}/libttzip_desktop.a"

# Optional UniFFI bindings generation and Swift 6 postprocessing
if [ "${SKIP_UNIFFI}" -eq 0 ]; then
    echo "--> [INFO] Verifying and synchronizing UniFFI bindings..."
    FIRST_TARGET="${TARGETS[0]}"
    FIRST_DYLIB="${EFFECTIVE_APP_TARGET_DIR}/${FIRST_TARGET}/${BUILD_MODE}/libttzip_app_core.dylib"
    if [ ! -f "${FIRST_DYLIB}" ]; then
        FIRST_DYLIB="${EFFECTIVE_APP_TARGET_DIR}/${BUILD_MODE}/libttzip_app_core.dylib"
    fi

    if [ -f "${FIRST_DYLIB}" ]; then
        TMP_UNIFFI_DIR="$(mktemp -d /tmp/ttzip_uniffi_out_XXXXXX)"
        BINDGEN_BIN="$(which uniffi-bindgen 2>/dev/null || true)"
        if [ -n "${BINDGEN_BIN}" ]; then
            "${BINDGEN_BIN}" generate \
                --library "${FIRST_DYLIB}" \
                --language swift \
                --out-dir "${TMP_UNIFFI_DIR}" \
                --no-format \
                --metadata-no-deps || true
        fi

        if [ -f "${TMP_UNIFFI_DIR}/ttzip_app_core.swift" ]; then
            if [ -f "${APP_CORE_DIR}/scripts/postprocess_uniffi_swift.py" ]; then
                python3 "${APP_CORE_DIR}/scripts/postprocess_uniffi_swift.py" "${TMP_UNIFFI_DIR}/ttzip_app_core.swift"
            fi
            mkdir -p "${APP_CORE_DIR}/Sources/TTZipAppCore/Generated"
            cp -f "${TMP_UNIFFI_DIR}/ttzip_app_core.swift" "${APP_CORE_DIR}/Sources/TTZipAppCore/Generated/ttzip_app_core.swift"
        fi

        if [ -f "${TMP_UNIFFI_DIR}/ttzip_app_coreFFI.h" ]; then
            mkdir -p "${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include"
            cp -f "${TMP_UNIFFI_DIR}/ttzip_app_coreFFI.h" "${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include/ttzip_app_coreFFI.h"
        fi

        if [ -f "${TMP_UNIFFI_DIR}/ttzip_app_coreFFI.modulemap" ]; then
            mkdir -p "${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include"
            cp -f "${TMP_UNIFFI_DIR}/ttzip_app_coreFFI.modulemap" "${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include/module.modulemap"
        fi

        rm -rf "${TMP_UNIFFI_DIR}"
    fi
fi

# Prepare public C-ABI headers directory for XCFramework packaging
HEADERS_DIR="$(mktemp -d /tmp/ttzip_desktop_headers_XXXXXX)"
HEADER_SRC="${APP_CORE_DIR}/Sources/CTTZipAppCoreBridge/include/ttzip_app_coreFFI.h"
if [ ! -f "${HEADER_SRC}" ]; then
    echo "❌ Error: CTTZipAppCoreBridge public header not found at ${HEADER_SRC}"
    rm -rf "${FINAL_STATIC_LIB_DIR}" "${HEADERS_DIR}"
    exit 1
fi
cp -f "${HEADER_SRC}" "${HEADERS_DIR}/"

# Package into XCFramework via xcodebuild
echo "--> [INFO] Packaging monolithic XCFramework via xcodebuild..."
rm -rf "${XCFRAMEWORK_DIR}"

xcodebuild -create-xcframework \
    -library "${FINAL_STATIC_LIB}" \
    -headers "${HEADERS_DIR}" \
    -output "${XCFRAMEWORK_DIR}"
rm -rf "${FINAL_STATIC_LIB_DIR}" "${HEADERS_DIR}"

if [ -d "${XCFRAMEWORK_DIR}" ] && [ -f "${XCFRAMEWORK_DIR}/Info.plist" ]; then
    echo "=========================================="
    echo "✅ [SUCCESS] Unified TTZip desktop XCFramework created successfully."
    echo "📍 Primary path: ${XCFRAMEWORK_DIR}"
    echo "📍 Monolithic archive: ${FRAMEWORKS_DIR}/libttzip_desktop.a"
    echo "=========================================="
else
    echo "❌ Error: Failed to generate ${XCFRAMEWORK_DIR}"
    exit 1
fi
