# Compila ai-memory no Windows (sem Docker).
# TAILWIND_SKIP=1: usa static/tailwind.css versionado (evita download + checksum só-linux).
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $RepoRoot

# $env:TAILWIND_SKIP = "1"
$env:TAILWIND_SKIP = "0"
$BridgeDir = Join-Path $RepoRoot "scripts\cursor-bridge"
if (-not (Test-Path "$BridgeDir\node_modules\@cursor\sdk")) {
    Write-Host "Installing cursor-bridge npm deps (first time)..." -ForegroundColor Yellow
    & (Join-Path $PSScriptRoot "install-cursor-bridge.ps1")
}

# Write-Host "Building ai-memory (TAILWIND_SKIP=1)..." -ForegroundColor Cyan
Write-Host "Building ai-memory..." -ForegroundColor Cyan
cargo build --release --workspace
if (-not (Test-Path "$RepoRoot\target\release\ai-memory.exe")) {
    throw "Build failed: target\release\ai-memory.exe not found"
}
Write-Host "OK: $RepoRoot\target\release\ai-memory.exe" -ForegroundColor Green

# Remove artifacts de debug (release build não precisa de target\debug).
$DebugDir = Join-Path $RepoRoot "target\debug"
if (Test-Path $DebugDir) {
    $SizeMB = [Math]::Round((Get-ChildItem $DebugDir -Recurse -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum / 1MB, 1)
    Remove-Item $DebugDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "Limpou target\debug (${SizeMB} MB)" -ForegroundColor DarkGray
}
