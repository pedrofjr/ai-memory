# Helpers HTTP + MCP para scripts windows (ai-memory serve em :49374).

function Get-AiMemoryRepoRoot {
    if ($env:AI_MEMORY_REPO) { return $env:AI_MEMORY_REPO.TrimEnd('\', '/') }
    return "C:\GIT\ai-memory"
}

function Import-AiMemoryEnvForScripts {
    param([string]$RepoRoot = (Get-AiMemoryRepoRoot))
    $import = Join-Path $RepoRoot "scripts\windows\Import-AiMemoryEnv.ps1"
    if (-not (Test-Path $import)) { throw "Missing $import" }
    . $import
    Import-AiMemoryEnv -RepoRoot $RepoRoot
    if (-not $env:AI_MEMORY_DATA_DIR) {
        $env:AI_MEMORY_DATA_DIR = Join-Path $env:USERPROFILE ".ai-memory"
    }
}

function Get-AiMemoryBaseUrl {
    param([string]$Override)
    if ($Override) { return $Override.TrimEnd('/') }
    if ($env:AI_MEMORY_SERVER_URL) { return $env:AI_MEMORY_SERVER_URL.TrimEnd('/') }
    return "http://127.0.0.1:49374"
}

function Get-AiMemoryAuthHeaders {
    $h = @{ Accept = "application/json" }
    if ($env:AI_MEMORY_AUTH_TOKEN) {
        $h["Authorization"] = "Bearer $($env:AI_MEMORY_AUTH_TOKEN)"
    }
    return $h
}

function Test-AiMemoryServerUp {
    param([string]$BaseUrl = (Get-AiMemoryBaseUrl))
    try {
        $r = Invoke-WebRequest -Uri "$BaseUrl/admin/status" -Headers (Get-AiMemoryAuthHeaders) `
            -UseBasicParsing -TimeoutSec 5
        return ($r.StatusCode -eq 200)
    } catch { return $false }
}

function Start-AiMemoryServeBackground {
    param([string]$RepoRoot = (Get-AiMemoryRepoRoot))
    $launcher = Join-Path $env:USERPROFILE ".local\bin\ai-memory.ps1"
    if (-not (Test-Path $launcher)) {
        $launcher = Join-Path $env:USERPROFILE ".local\bin\ai_memory.ps1"
    }
    if (Test-Path $launcher) {
        Start-Process -FilePath "pwsh" -ArgumentList @(
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $launcher, "serve",
            "--transport", "http", "--bind", "127.0.0.1:49374"
        ) -WindowStyle Hidden | Out-Null
        return
    }
    $bin = Join-Path $RepoRoot "target\release\ai-memory.exe"
    if (-not (Test-Path $bin)) {
        $bin = Join-Path $RepoRoot "target\debug\ai-memory.exe"
    }
    if (-not (Test-Path $bin)) {
        throw "Binary not found. Run scripts\windows\build.ps1 first."
    }
    Start-Process -FilePath $bin -ArgumentList @(
        "serve", "--transport", "http", "--bind", "127.0.0.1:49374"
    ) -WindowStyle Hidden | Out-Null
}

function Wait-AiMemoryServer {
    param(
        [string]$BaseUrl = (Get-AiMemoryBaseUrl),
        [int]$TimeoutSec = 60
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-AiMemoryServerUp -BaseUrl $BaseUrl) { return $true }
        Start-Sleep -Seconds 1
    }
    return $false
}

function Invoke-AiMemoryAdminPost {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$Body,
        [string]$BaseUrl = (Get-AiMemoryBaseUrl)
    )
    $uri = "$BaseUrl$Path"
    $json = if ($Body -is [string]) { $Body } else { $Body | ConvertTo-Json -Depth 12 -Compress }
    return Invoke-RestMethod -Method Post -Uri $uri -ContentType "application/json" `
        -Headers (Get-AiMemoryAuthHeaders) -Body $json -TimeoutSec 300
}

function Invoke-AiMemoryMcpTool {
    param(
        [Parameter(Mandatory)][string]$ToolName,
        [hashtable]$Arguments = @{},
        [string]$BaseUrl = (Get-AiMemoryBaseUrl),
        [int]$Id = 2
    )
    $payload = @{
        jsonrpc = "2.0"
        id      = $Id
        method  = "tools/call"
        params  = @{
            name      = $ToolName
            arguments = $Arguments
        }
    } | ConvertTo-Json -Depth 8 -Compress

    $headers = Get-AiMemoryAuthHeaders
    $headers["Content-Type"] = "application/json"
    $headers["Accept"] = "application/json, text/event-stream"

    $resp = Invoke-WebRequest -Method Post -Uri "$BaseUrl/mcp" -Headers $headers `
        -Body $payload -UseBasicParsing -TimeoutSec 600
    $text = $resp.Content
    if ($text -match '(?s)\{.*\}') { $text = $Matches[0] }
    $rpc = $text | ConvertFrom-Json
    if ($rpc.error) {
        throw "MCP error: $($rpc.error.message)"
    }
    return $rpc.result
}

function Get-AiMemoryBinary {
    param([string]$RepoRoot = (Get-AiMemoryRepoRoot))
    $launcher = Join-Path $env:USERPROFILE ".local\bin\ai-memory.ps1"
    if (Test-Path $launcher) { return @{ Type = "launcher"; Path = $launcher } }
    $exe = Join-Path $RepoRoot "target\release\ai-memory.exe"
    if (-not (Test-Path $exe)) { $exe = Join-Path $RepoRoot "target\debug\ai-memory.exe" }
    if (-not (Test-Path $exe)) { throw "ai-memory binary not found. Run build.ps1." }
    return @{ Type = "exe"; Path = $exe }
}

function Find-WikiPageFile {
    param(
        [Parameter(Mandatory)][string]$RelativePath
    )
    $wiki = Join-Path $env:AI_MEMORY_DATA_DIR "wiki"
    $leaf = Split-Path $RelativePath -Leaf
    $needle = $RelativePath.Replace('/', '\')
    $hits = Get-ChildItem -Path $wiki -Recurse -Filter $leaf -File -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName.Replace('/', '\').EndsWith($needle) }
    if ($hits.Count -eq 0) { return $null }
    if ($hits.Count -gt 1) {
        Write-Warning "Multiple wiki files for $RelativePath; using first."
    }
    return $hits[0].FullName
}

function Extract-JsonFromLlmText {
    param([string]$Text)
    if ($Text -match '(?s)```(?:json)?\s*(\{.*?\})\s*```') {
        return $Matches[1]
    }
    $start = $Text.IndexOf('{')
    $end = $Text.LastIndexOf('}')
    if ($start -ge 0 -and $end -gt $start) {
        return $Text.Substring($start, $end - $start + 1)
    }
    throw "No JSON object found in LLM output."
}
