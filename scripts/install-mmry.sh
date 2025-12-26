#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

echo "Detected platform: ${OS} (${ARCH})"
echo ""

# GPU acceleration options
echo "Select GPU acceleration (improves embedding/reranking performance):"
echo ""
echo "  1) None (CPU only) - works everywhere"
echo "  2) CUDA (NVIDIA GPU) - requires CUDA toolkit"
echo "  3) CoreML (Apple Silicon) - macOS only, uses Neural Engine"
echo "  4) DirectML (Windows GPU) - Windows only"
echo ""

# Default based on platform
if [[ "${OS}" == "Darwin" && "${ARCH}" == "arm64" ]]; then
    DEFAULT_CHOICE="3"
    echo "Recommended for Apple Silicon: CoreML (3)"
elif [[ "${OS}" == "Linux" ]] && command -v nvidia-smi &> /dev/null; then
    DEFAULT_CHOICE="2"
    echo "NVIDIA GPU detected, recommended: CUDA (2)"
else
    DEFAULT_CHOICE="1"
    echo "Recommended: CPU only (1)"
fi

echo ""
read -p "Enter choice [${DEFAULT_CHOICE}]: " CHOICE
CHOICE="${CHOICE:-$DEFAULT_CHOICE}"

# Set ORT environment variable based on choice
case "${CHOICE}" in
    1)
        echo "Building with CPU-only support..."
        unset ORT_USE_CUDA
        unset ORT_USE_COREML
        unset ORT_USE_DIRECTML
        ;;
    2)
        echo "Building with CUDA support..."
        export ORT_USE_CUDA=1
        ;;
    3)
        if [[ "${OS}" != "Darwin" ]]; then
            echo "Warning: CoreML is only available on macOS. Falling back to CPU."
        else
            echo "Building with CoreML support..."
            export ORT_USE_COREML=1
        fi
        ;;
    4)
        echo "Building with DirectML support..."
        export ORT_USE_DIRECTML=1
        ;;
    *)
        echo "Invalid choice. Using CPU-only."
        ;;
esac

echo ""
read -p "Install benchmark runner (mmry bench)? [y/N]: " BENCH_CHOICE
BENCH_CHOICE="${BENCH_CHOICE:-N}"

CLI_FEATURES=()
if [[ "${BENCH_CHOICE}" =~ ^[Yy]$ ]]; then
    CLI_FEATURES+=("--features" "bench")
fi

echo ""
echo "Building workspace..."
cargo build --release

echo ""
echo "Installing mmry service..."
cargo install --path "${ROOT_DIR}/crates/mmry-service" --force

echo ""
echo "Installing mmry CLI..."
cargo install --path "${ROOT_DIR}/crates/mmry-cli" --force "${CLI_FEATURES[@]}"

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
