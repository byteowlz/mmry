#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo install --path "${ROOT_DIR}/crates/mmry-cli" --force
echo "Installed mmry to ${CARGO_HOME:-$HOME/.cargo}/bin/mmry"
