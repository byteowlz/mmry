$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
cargo install --path "$RootDir\crates\mmry-cli" --force
Write-Host "Installed mmry"
