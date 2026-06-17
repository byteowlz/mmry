#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

echo "Detected platform: ${OS} (${ARCH})"
echo ""
# Embeddings/reranking run out-of-process in vqtrs-api, so mmry itself has no
# GPU build options — there is nothing to select here.

echo "Building workspace..."
cargo build --release

echo ""
echo "Installing mmry service..."
cargo install --path "${ROOT_DIR}/crates/mmry-service" --force

echo ""
echo "Installing mmry CLI..."
cargo install --path "${ROOT_DIR}/crates/mmry-cli" --force

echo ""
echo "Installing mmry TUI..."
cargo install --path "${ROOT_DIR}/crates/mmry-tui" --force

echo ""
echo "Installation complete!"
echo "Binaries are available in \$HOME/.cargo/bin"
echo ""
echo "Quick start:"
echo "  mmry init              # Initialize config and database"
echo "  mmry add 'memory'      # Add a memory"
echo "  mmry search 'query'    # Search memories"
echo "  mmry-tui               # Launch the TUI"
