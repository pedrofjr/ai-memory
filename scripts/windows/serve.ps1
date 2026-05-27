# Sobe o servidor HTTP (MCP + hooks) em loopback :49374.
# Carrega .env.local na raiz do repo, se existir.
param(
    [switch]$Web,
    [string]$Bind = "127.0.0.1:49374"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Bin = Join-Path $RepoRoot "target\release\ai-memory.exe"
if (-not (Test-Path $Bin)) {
    & (Join-Path $PSScriptRoot "build.ps1")
}

. (Join-Path $PSScriptRoot "Import-AiMemoryEnv.ps1")
Import-AiMemoryEnv -RepoRoot $RepoRoot
Write-Host "Env loaded from $RepoRoot (.env + .env.local)" -ForegroundColor DarkGray

$configDir = $env:AI_MEMORY_DATA_DIR
if (-not (Test-Path (Join-Path $configDir "config.toml"))) {
    Write-Host "Data dir not initialized. Running init-data.ps1..." -ForegroundColor Yellow
    & (Join-Path $PSScriptRoot "init-data.ps1")
}

$args = @("serve", "--transport", "http", "--bind", $Bind)
if ($Web) { $args += "--enable-web" }

Write-Host "ai-memory serve -> http://$($Bind -replace ':.*',''):$($Bind -split ':' | Select-Object -Last 1)" -ForegroundColor Green
Write-Host "Data: $env:AI_MEMORY_DATA_DIR" -ForegroundColor DarkGray
& $Bin @args
