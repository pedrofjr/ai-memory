# Compila ai-memory no Windows (sem Docker).
# O CSS do Tailwind é vendorizado no repo, então o build não baixa nada.
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $RepoRoot

Write-Host "Building ai-memory..." -ForegroundColor Cyan
cargo build --release --bin ai-memory
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
