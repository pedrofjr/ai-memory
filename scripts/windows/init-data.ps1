# Inicializa data dir (wiki, db, raw, config.toml).
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Bin = Join-Path $RepoRoot "target\release\ai-memory.exe"
if (-not (Test-Path $Bin)) {
    & (Join-Path $PSScriptRoot "build.ps1")
}

if (-not $env:AI_MEMORY_DATA_DIR) {
    $env:AI_MEMORY_DATA_DIR = Join-Path $env:LOCALAPPDATA "ai-memory"
}
Write-Host "Data dir: $env:AI_MEMORY_DATA_DIR" -ForegroundColor Cyan
& $Bin init
Write-Host "Init OK. Run serve.ps1 then: ai-memory status (needs server)." -ForegroundColor Green
