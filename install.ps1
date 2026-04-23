# Void Stack installer para Windows — https://void-stack.dev
# Usage:
#   irm https://void-stack.dev/install.ps1 | iex
#   irm https://void-stack.dev/install.ps1 | iex; Install-VoidStack -Bin void
#   irm https://void-stack.dev/install.ps1 | iex; Install-VoidStack -InstallDir "C:\tools"

param(
    [string]$Bin         = "",           # Binario específico (vacío = todos)
    [string]$InstallDir  = ""            # Dir de instalación (vacío = auto)
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$REPO = "Mague/void-stack"
$ALL_BINS = @("void", "void-stack-tui", "void-stack-mcp", "void-stack-daemon")

# ── Helpers de color ──────────────────────────────────────────────────────────
function Write-Info    { param($msg) Write-Host "info  $msg"  -ForegroundColor Cyan }
function Write-Ok      { param($msg) Write-Host "✓     $msg"  -ForegroundColor Green }
function Write-Warn    { param($msg) Write-Host "warn  $msg"  -ForegroundColor Yellow }
function Write-Fail    { param($msg) Write-Host "error $msg"  -ForegroundColor Red; exit 1 }

# ── Resolver directorio de instalación ───────────────────────────────────────
if ($InstallDir -eq "") {
    $InstallDir = $env:VOID_INSTALL_DIR
    if (-not $InstallDir) {
        $InstallDir = Join-Path $env:LOCALAPPDATA "void-stack\bin"
    }
}

$BINARIES = if ($Bin -ne "") { @($Bin) } else { $ALL_BINS }

# ── Detectar arquitectura ─────────────────────────────────────────────────────
$arch = $env:PROCESSOR_ARCHITECTURE
$target = switch ($arch) {
    "AMD64" { "x86_64-pc-windows-msvc" }
    "ARM64" { "aarch64-pc-windows-msvc" }
    default { Write-Fail "Arquitectura no soportada: $arch" }
}

# ── Obtener última versión ────────────────────────────────────────────────────
Write-Info "Buscando última versión..."
try {
    $release = Invoke-RestMethod "https://api.github.com/repos/$REPO/releases/latest"
    $version = $release.tag_name
} catch {
    Write-Fail "No se pudo obtener la versión desde GitHub: $_"
}

Write-Info "Instalando Void Stack $version → $InstallDir"

# ── Descargar ─────────────────────────────────────────────────────────────────
$asset    = "void-stack-${version}-${target}.zip"
$url      = "https://github.com/$REPO/releases/download/$version/$asset"
$tmpDir   = Join-Path $env:TEMP "void-stack-install-$(Get-Random)"
$zipPath  = Join-Path $tmpDir $asset

New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

Write-Info "Descargando $asset..."
try {
    Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
} catch {
    Write-Fail "Descarga fallida desde $url`n$_"
}

# ── Extraer ───────────────────────────────────────────────────────────────────
Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force
$extractedDir = Join-Path $tmpDir "void-stack-${version}-${target}"

# ── Instalar binarios ─────────────────────────────────────────────────────────
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

foreach ($bin in $BINARIES) {
    $src = Join-Path $extractedDir "${bin}.exe"
    if (Test-Path $src) {
        $dest = Join-Path $InstallDir "${bin}.exe"
        Copy-Item $src $dest -Force
        Write-Ok "Instalado: $dest"
    } else {
        Write-Warn "$bin no encontrado en el release"
    }
}

# ── Limpiar temp ─────────────────────────────────────────────────────────────
Remove-Item $tmpDir -Recurse -Force

# ── Agregar al PATH del usuario si no está ───────────────────────────────────
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$userPath;$InstallDir", "User")
    Write-Warn "$InstallDir agregado al PATH. Reinicia tu terminal para aplicar."
} else {
    Write-Ok "$InstallDir ya está en el PATH."
}

# ── Resumen ───────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "¡Listo! " -ForegroundColor Green -NoNewline
Write-Host "Void Stack $version instalado en $InstallDir"
Write-Host ""
Write-Host "  Empieza con:" -ForegroundColor DarkGray
Write-Host "  void add mi-proyecto C:\ruta\al\proyecto"
Write-Host "  void start mi-proyecto"
Write-Host ""