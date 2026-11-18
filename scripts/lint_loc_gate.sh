#!/usr/bin/env bash
# Copyright 2026 Witt Kung and TTZip Contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

python3 "${SCRIPT_DIR}/lint_loc_gate.py" "$@"
