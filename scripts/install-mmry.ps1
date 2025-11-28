# Install script for mmry on Windows
# Run with: powershell -ExecutionPolicy Bypass -File scripts\install-mmry.ps1

$ErrorActionPreference = "Stop"

$ROOT_DIR = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

Write-Host "Select an ORT feature to apply to both mmry binaries:"
Write-Host "  1) none (default)"
Write-Host "  2) ort-coreml"
Write-Host "  3) ort-cuda"
Write-Host "  4) ort-directml"
Write-Host "  5) ort-openvino"
Write-Host "  6) ort-tensorrt"
Write-Host "  7) ort-rocm"
Write-Host "  8) ort-nnapi"
Write-Host "  9) ort-xnnpack"
Write-Host " 10) ort-load-dynamic"
$choice = Read-Host "Enter choice [1-10]"

$FEATURE_FLAG = @()
switch ($choice) {
    "2" { $FEATURE_FLAG = @("--features", "ort-coreml") }
    "3" { $FEATURE_FLAG = @("--features", "ort-cuda") }
    "4" { $FEATURE_FLAG = @("--features", "ort-directml") }
    "5" { $FEATURE_FLAG = @("--features", "ort-openvino") }
    "6" { $FEATURE_FLAG = @("--features", "ort-tensorrt") }
    "7" { $FEATURE_FLAG = @("--features", "ort-rocm") }
    "8" { $FEATURE_FLAG = @("--features", "ort-nnapi") }
    "9" { $FEATURE_FLAG = @("--features", "ort-xnnpack") }
    "10" { $FEATURE_FLAG = @("--features", "ort-load-dynamic") }
    default { $FEATURE_FLAG = @() }
}

Write-Host "Building workspace..."
cargo build --release @FEATURE_FLAG
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installing mmry service..."
cargo install --path "$ROOT_DIR\crates\mmry-service" --force @FEATURE_FLAG
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installing mmry CLI..."
cargo install --path "$ROOT_DIR\crates\mmry-cli" --force @FEATURE_FLAG
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installing mmry TUI..."
cargo install --path "$ROOT_DIR\crates\mmry-tui" --force @FEATURE_FLAG
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Installation complete. Binaries are available in $env:USERPROFILE\.cargo\bin."
