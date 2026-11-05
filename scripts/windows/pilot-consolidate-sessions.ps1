# Piloto: memory_consolidate (dry-run por padrão) em 2 sessões pequenas.
param(
    [string]$BaseUrl,
    [string]$RunDir = (Join-Path $env:USERPROFILE ".ai-memory\runs\consolidate-inventory-20260527-104637"),
    [string[]]$SessionIds = @(
        "019e6046-c648-7711-8f78-748f0e06391e",
        "019e652c-fd57-7495-9ef3-d3c093c3425f"
    ),
    [switch]$Apply,
    [switch]$StartServer
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "lib\Invoke-AiMemoryApi.ps1")
Import-AiMemoryEnvForScripts
if ($BaseUrl) { $env:AI_MEMORY_SERVER_URL = $BaseUrl }
$url = Get-AiMemoryBaseUrl

if (-not (Test-AiMemoryServerUp -BaseUrl $url)) {
    if ($StartServer) {
        Write-Host "Subindo ai-memory serve..." -ForegroundColor Yellow
        Start-AiMemoryServeBackground
        if (-not (Wait-AiMemoryServer -BaseUrl $url)) {
            throw "ai-memory não responde em $url"
        }
    } else {
        throw "ai-memory offline em $url. Use -StartServer ou rode serve.ps1."
    }
}

$dryRun = -not $Apply
$logPath = Join-Path $RunDir "pilot-sessions-$(Get-Date -Format 'yyyyMMdd-HHmmss').jsonl"
New-Item -ItemType Directory -Path $RunDir -Force | Out-Null

Write-Host "Piloto sessões (dry_run=$dryRun)" -ForegroundColor Cyan
Write-Host "Log: $logPath"

foreach ($sid in $SessionIds) {
    Write-Host "`n--- session $sid ---" -ForegroundColor Cyan
    try {
        $result = Invoke-AiMemoryMcpTool -ToolName "memory_consolidate" -Arguments @{
            session_id = $sid
            dry_run    = $dryRun
            multi_page = $false
        } -BaseUrl $url

        $line = @{
            ts         = (Get-Date).ToUniversalTime().ToString("o")
            session_id = $sid
            dry_run    = $dryRun
            ok         = $true
            result     = $result
        }
        ($line | ConvertTo-Json -Depth 12 -Compress) | Add-Content -Path $logPath -Encoding utf8

        $content = $result.content
        if ($content -is [array]) {
            foreach ($c in $content) {
                if ($c.text) { Write-Host $c.text }
            }
        } else {
            Write-Host ($result | ConvertTo-Json -Depth 8)
        }
    } catch {
        Write-Host "ERRO: $($_.Exception.Message)" -ForegroundColor Red
        $line = @{
            ts         = (Get-Date).ToUniversalTime().ToString("o")
            session_id = $sid
            dry_run    = $dryRun
            ok         = $false
            error      = $_.Exception.Message
        }
        ($line | ConvertTo-Json -Compress) | Add-Content -Path $logPath -Encoding utf8
    }
}

if ($dryRun) {
    Write-Host "`nDry-run apenas. Para gravar: -Apply" -ForegroundColor Yellow
}
