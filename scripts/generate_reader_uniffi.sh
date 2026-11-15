#!/usr/bin/env bash
# Copyright 2026 Witt Kung and TTZip Contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

set -euo pipefail

if [[ $# != 2 ]]; then
    echo "Usage: $0 <uniffi-bindgen> <libttzip_reader.dylib>" >&2
    exit 2
fi
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_CORE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

(cd "${APP_CORE_DIR}" && "$1" generate --library "$2" --language swift --out-dir "${TMP_DIR}" --no-format --metadata-no-deps)
python3 "${SCRIPT_DIR}/postprocess_uniffi_swift.py" "${TMP_DIR}/ttzip_reader.swift"
python3 - "${TMP_DIR}" <<'PYTHON'
from pathlib import Path
import sys

for path in Path(sys.argv[1]).iterdir():
    lines = (line.rstrip() for line in path.read_text().splitlines())
    path.write_text("\n".join(lines).rstrip() + "\n")
PYTHON
cp "${TMP_DIR}/ttzip_reader.swift" "${APP_CORE_DIR}/Sources/TTZipReader/Generated/"
cp "${TMP_DIR}/ttzip_readerFFI.h" "${APP_CORE_DIR}/Sources/CTTZipReaderBridge/include/"
cp "${TMP_DIR}/ttzip_readerFFI.modulemap" "${APP_CORE_DIR}/Sources/CTTZipReaderBridge/include/module.modulemap"
