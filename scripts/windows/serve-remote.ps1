# Sobe uma SEGUNDA instancia do ai-memory, dedicada a acesso remoto via tunnel.
#
# Por que uma instancia separada:
#   - o servidor local (porta 49374) continua SEM auth, entao hooks e clientes
#     MCP da maquina nao precisam de mudanca nenhuma;
#   - esta instancia exige Bearer token, entao o tunnel nao expoe a memoria
#     para quem descobrir a URL.
#
# --no-watcher: o servidor local ja observa o wiki/ deste data dir. Um segundo
# watcher aqui duplicaria a ingestao de arquivos. As duas instancias dividem o
# mesmo SQLite: o WAL serializa as escritas, mas cada processo tem seu proprio
# scheduler de consolidacao. Trate esta instancia como leitura + Web UI e deixe
# as escritas pesadas para a local.
#
# Host: o servidor so aceita um Host presente em AI_MEMORY_ALLOWED_HOSTS (default
# localhost, 127.0.0.1, ::1) e responde 403 forbidden host ao resto. Tuneis que
# preservam o Host original (cloudflared, ngrok) precisam do hostname publico em
# -AllowedHosts; sem isso toda requisicao e recusada.
param(
    [int]$Port = 49380,
    [string]$TokenFile = "$env:USERPROFILE\.ai-memory\remote-token.txt",
    [string[]]$AllowedHosts = @()
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Bin = Join-Path $RepoRoot "target\release\ai-memory.exe"
if (-not (Test-Path $Bin)) { throw "binario nao encontrado: $Bin (rode scripts\windows\build.ps1)" }

. (Join-Path $PSScriptRoot "Import-AiMemoryEnv.ps1")
Import-AiMemoryEnv -RepoRoot $RepoRoot

if (-not (Test-Path $TokenFile)) {
    $raw = & $Bin generate-auth-token 2>&1
    $tok = ($raw | Where-Object { $_ -is [string] -and $_ -match '^[0-9a-f]{64}$' } | Select-Object -First 1)
    if (-not $tok) { throw "nao consegui gerar token" }
    Set-Content -Path $TokenFile -Value $tok -NoNewline -Encoding ascii
    Write-Host "[..] token novo gerado em $TokenFile" -ForegroundColor DarkGray
}

# Depois do Import-AiMemoryEnv: o .env/.env.local nao pode sobrescrever o token
# desta instancia (que e diferente do token local, que nem existe).
$env:AI_MEMORY_AUTH_TOKEN = (Get-Content $TokenFile -Raw).Trim()

if ($AllowedHosts.Count -gt 0) {
    $env:AI_MEMORY_ALLOWED_HOSTS = ($AllowedHosts -join ',')
    Write-Host "Hosts aceitos: $env:AI_MEMORY_ALLOWED_HOSTS" -ForegroundColor DarkGray
} else {
    Write-Warning "sem -AllowedHosts: um tunnel que preserva o Host publico responde 403 forbidden host."
}

Write-Host "ai-memory REMOTE -> http://127.0.0.1:$Port (auth: bearer obrigatorio)" -ForegroundColor Green
Write-Host "Data: $env:AI_MEMORY_DATA_DIR" -ForegroundColor DarkGray

& $Bin serve --transport http --bind "127.0.0.1:$Port" --enable-web --no-watcher
