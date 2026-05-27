# Registra MCP + hooks do Cursor (requer binário compilado).
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Bin = Join-Path $RepoRoot "target\release\ai-memory.exe"
$Server = "http://127.0.0.1:49374"
# Destino dos scripts após stage (data_local_dir/ai-memory/hooks/cursor)
$StagedHooks = Join-Path $env:LOCALAPPDATA "ai-memory\hooks\cursor"

if (-not (Test-Path $Bin)) {
    & (Join-Path $PSScriptRoot "build.ps1")
}

# Sem --hooks-dir: usa hooks/ do repo (detectado via target\release\ai-memory.exe)
if ($env:AI_MEMORY_AUTH_TOKEN) {
    & $Bin install-mcp --client cursor --apply `
        --server-url "$Server/mcp" `
        --auth-token $env:AI_MEMORY_AUTH_TOKEN
    & $Bin install-hooks --agent cursor --apply `
        --server-url $Server `
        --auth-token $env:AI_MEMORY_AUTH_TOKEN
} else {
    & $Bin install-mcp --client cursor --apply --server-url "$Server/mcp"
    & $Bin install-hooks --agent cursor --apply --server-url $Server
}

Write-Host "Cursor: reinicie o IDE ou toggle MCP em Settings." -ForegroundColor Green
Write-Host "Hooks staged em: $StagedHooks" -ForegroundColor DarkGray
