#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cargo clean --manifest-path "$SCRIPT_DIR/Cargo.toml"
rm -rf "$SCRIPT_DIR/build"
