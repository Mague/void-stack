$ErrorActionPreference = "Stop"

$RootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$FrontendDir = Join-Path $RootDir "crates\void-stack-desktop\frontend"

Write-Host "==> Building frontend..."
Set-Location $FrontendDir
npm install --silent
npm run build

Write-Host "==> Running clippy..."
Set-Location $RootDir
cargo clippy --workspace -- -D warnings

Write-Host "==> Building Rust workspace..."
cargo build @args

Write-Host "==> Build complete!"
Write-Host "    Binary: $RootDir\target\debug\void-stack-desktop.exe"
