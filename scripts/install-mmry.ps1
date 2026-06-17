# Install script for mmry on Windows
# Run with: powershell -ExecutionPolicy Bypass -File scripts\install-mmry.ps1

$ErrorActionPreference = "Stop"

$ROOT_DIR = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

# Embeddings/reranking run out-of-process in vqtrs-api, so mmry itself has no
# GPU/ONNX build options — nothing to select here.

Write-Host "Building workspace..."
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installing mmry service..."
cargo install --path "$ROOT_DIR\crates\mmry-service" --force
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installing mmry CLI..."
cargo install --path "$ROOT_DIR\crates\mmry-cli" --force
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installing mmry TUI..."
cargo install --path "$ROOT_DIR\crates\mmry-tui" --force
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installation complete. Binaries are available in $env:USERPROFILE\.cargo\bin."
