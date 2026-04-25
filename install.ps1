# Void Stack installer para Windows — https://www.void-stack.dev
# irm https://www.void-stack.dev/install.ps1 | iex
# $env:VOID_NO_MCP = "1" para saltar MCP config

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$REPO     = "Mague/void-stack"
$ALL_BINS = @("void", "void-stack-tui", "void-stack-mcp", "void-stack-daemon")
$Bin        = if ($env:VOID_BIN)         { $env:VOID_BIN }         else { "" }
$InstallDir = if ($env:VOID_INSTALL_DIR) { $env:VOID_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "void-stack\bin" }
$NoMcp      = ($env:VOID_NO_MCP -eq "1")
$BINARIES   = if ($Bin -ne "") { @($Bin) } else { $ALL_BINS }

function Write-Info  { param($m) Write-Host "info  $m" -ForegroundColor Cyan }
function Write-Ok    { param($m) Write-Host "v     $m" -ForegroundColor Green }
function Write-Warn  { param($m) Write-Host "warn  $m" -ForegroundColor Yellow }
function Write-Step  { param($m) Write-Host "`n$m" -ForegroundColor White }
function Write-Fail  { param($m) Write-Host "error $m" -ForegroundColor Red; exit 1 }

$target = switch ($env:PROCESSOR_ARCHITECTURE) {
  "AMD64" { "x86_64-pc-windows-msvc" }
  "ARM64" { "aarch64-pc-windows-msvc" }
  default { Write-Fail "Unsupported arch"; exit 1 }
}

Write-Step "Void Stack Installer"
Write-Info "Fetching latest version..."
try {
  $version = (Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases/latest" -UseBasicParsing).tag_name
} catch { Write-Fail "Could not fetch version: $_" }
Write-Info "Installing $version to $InstallDir"

$asset  = "void-stack-${version}-${target}.zip"
$tmpDir = Join-Path $env:TEMP "void-stack-$(Get-Random)"
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
Write-Info "Downloading $asset..."
try { Invoke-WebRequest -Uri "https://github.com/$REPO/releases/download/$version/$asset" -OutFile (Join-Path $tmpDir $asset) -UseBasicParsing }
catch { Write-Fail "Download failed: $_" }
Expand-Archive -Path (Join-Path $tmpDir $asset) -DestinationPath $tmpDir -Force
$extracted = Join-Path $tmpDir "void-stack-${version}-${target}"

Write-Step "Installing binaries"
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
$McpBin = ""
foreach ($bin in $BINARIES) {
  $src = Join-Path $extracted "${bin}.exe"
  if (Test-Path $src) {
    Copy-Item $src (Join-Path $InstallDir "${bin}.exe") -Force
    Write-Ok "Installed: $InstallDir\${bin}.exe"
    if ($bin -eq "void-stack-mcp") { $McpBin = Join-Path $InstallDir "void-stack-mcp.exe" }
  } else { Write-Warn "$bin not found in release" }
}
Remove-Item $tmpDir -Recurse -Force

$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$InstallDir*") {
  [Environment]::SetEnvironmentVariable("PATH", "$userPath;$InstallDir", "User")
  Write-Warn "Added $InstallDir to PATH. Restart terminal to apply."
}

function Merge-Config {
  param([string]$File, [string]$Format)
  if (-not (Test-Path $File)) { return $false }
  Copy-Item $File "$File.void-stack-backup" -Force
  try { $d = Get-Content $File -Raw -Encoding UTF8 | ConvertFrom-Json }
  catch { $d = [PSCustomObject]@{} }
  $h = @{}; $d.PSObject.Properties | ForEach-Object { $h[$_.Name] = $_.Value }

  switch ($Format) {
    "standard" {
      if (-not $h["mcpServers"]) { $h["mcpServers"] = @{} }
      $s = @{}; if ($h["mcpServers"] -isnot [hashtable]) {
        $h["mcpServers"].PSObject.Properties | ForEach-Object { $s[$_.Name] = $_.Value }
      } else { $s = $h["mcpServers"] }
      if ($s["void-stack"]) { return $false }
      $s["void-stack"] = @{ command = $McpBin }; $h["mcpServers"] = $s
    }
    "opencode" {
      if (-not $h["mcp"]) { $h["mcp"] = @{} }
      $m = @{}; if ($h["mcp"] -isnot [hashtable]) {
        $h["mcp"].PSObject.Properties | ForEach-Object { $m[$_.Name] = $_.Value }
      } else { $m = $h["mcp"] }
      if ($m["void-stack"]) { return $false }
      $m["void-stack"] = @{ type="local"; command=@($McpBin); enabled=$true }; $h["mcp"] = $m
    }
    "zed" {
      if (-not $h["context_servers"]) { $h["context_servers"] = @{} }
      $cs = @{}; if ($h["context_servers"] -isnot [hashtable]) {
        $h["context_servers"].PSObject.Properties | ForEach-Object { $cs[$_.Name] = $_.Value }
      } else { $cs = $h["context_servers"] }
      if ($cs["void-stack"]) { return $false }
      $cs["void-stack"] = @{ command=@{ path=$McpBin; args=@() } }; $h["context_servers"] = $cs
    }
  }
  $h | ConvertTo-Json -Depth 10 | Set-Content $File -Encoding UTF8
  return $true
}

if (-not $NoMcp -and $McpBin -ne "") {
  Write-Step "Detecting MCP-compatible tools..."
  $tools = @(
    @{N="Claude Desktop"; F="$env:APPDATA\Claude\claude_desktop_config.json";                                                              T="standard"},
    @{N="Cursor";         F="$env:USERPROFILE\.cursor\mcp.json";                                                                           T="standard"},
    @{N="Windsurf";       F="$env:USERPROFILE\.codeium\windsurf\mcp_server_config.json";                                                   T="standard"},
    @{N="OpenCode";       F="$env:USERPROFILE\.config\opencode\opencode.json";                                                             T="opencode"},
    @{N="Cline";          F="$env:APPDATA\Code\User\globalStorage\saoudrizwan.claude-dev\settings\cline_mcp_settings.json";               T="standard"},
    @{N="Continue.dev";   F="$env:USERPROFILE\.continue\config.json";                                                                      T="standard"},
    @{N="Zed";            F="$env:APPDATA\Zed\settings.json";                                                                             T="zed"}
  )
  $found = $tools | Where-Object { Test-Path $_.F }
  $hasClaude = $null -ne (Get-Command "claude" -ErrorAction SilentlyContinue)

  if ($found.Count -eq 0 -and -not $hasClaude) {
    Write-Info "No MCP tools detected."
  } else {
    Write-Host ""
    Write-Host "MCP tools detected:" -ForegroundColor White
    foreach ($t in $found) { Write-Host "  v $($t.N) ($($t.F))" -ForegroundColor Green }
    if ($hasClaude) { Write-Host "  v Claude Code (claude mcp add)" -ForegroundColor Green }
    Write-Host ""
    $confirm = Read-Host "Auto-configure void-stack-mcp in all? [Y/n]"
    if ($confirm -eq "" -or $confirm -match "^[Yy]") {
      Write-Step "Configuring..."
      foreach ($t in $found) {
        if (Merge-Config -File $t.F -Format $t.T) { Write-Ok "$($t.N) configured" }
        else { Write-Info "$($t.N): already configured" }
      }
      if ($hasClaude) {
        if ((& claude mcp list 2>$null) -match "void-stack") { Write-Info "Claude Code: already configured" }
        else { try { & claude mcp add void-stack $McpBin 2>$null; Write-Ok "Claude Code configured" } catch { Write-Warn "Claude Code: configure manually" } }
      }
      Write-Host ""; Write-Warn "Restart apps to load the new MCP server."
    }
  }
}

Write-Host ""; Write-Host "Done! Void Stack $version installed." -ForegroundColor Green
Write-Host ""; Write-Host "  void add my-project C:\projects\my-app"
Write-Host "  void start my-project"; Write-Host ""