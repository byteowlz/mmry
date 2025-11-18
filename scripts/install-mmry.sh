#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Select an ORT feature to apply to both mmry binaries:"
echo "  1) none (default)"
echo "  2) ort-coreml"
echo "  3) ort-cuda"
echo "  4) ort-directml"
echo "  5) ort-openvino"
echo "  6) ort-tensorrt"
echo "  7) ort-rocm"
echo "  8) ort-nnapi"
echo "  9) ort-xnnpack"
echo " 10) ort-load-dynamic"
read -rp "Enter choice [1-10]: " choice

FEATURE_FLAG=()
case "${choice:-1}" in
2) FEATURE_FLAG=(--features ort-coreml) ;;
3) FEATURE_FLAG=(--features ort-cuda) ;;
4) FEATURE_FLAG=(--features ort-directml) ;;
5) FEATURE_FLAG=(--features ort-openvino) ;;
6) FEATURE_FLAG=(--features ort-tensorrt) ;;
7) FEATURE_FLAG=(--features ort-rocm) ;;
8) FEATURE_FLAG=(--features ort-nnapi) ;;
9) FEATURE_FLAG=(--features ort-xnnpack) ;;
10) FEATURE_FLAG=(--features ort-load-dynamic) ;;
*) FEATURE_FLAG=() ;;
esac

echo "Building workspace..."
cargo build --release "${FEATURE_FLAG[@]}"

echo "Installing mmry service..."
cargo install --path "${ROOT_DIR}/crates/mmry-service" --force "${FEATURE_FLAG[@]}"

echo "Installing mmry CLI..."
cargo install --path "${ROOT_DIR}/crates/mmry-cli" --force "${FEATURE_FLAG[@]}"

echo "Installing mmry TUI..."
cargo install --path "${ROOT_DIR}/crates/mmry-tui" --force "${FEATURE_FLAG[@]}"

echo "Installation complete. Binaries are available in \$HOME/.cargo/bin."
