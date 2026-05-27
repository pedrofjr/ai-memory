# Consolida sessões pendentes (memory_consolidate) a partir do manifest-before.json.
param(
    [string]$RunDir = (Join-Path $env:USERPROFILE ".ai-memory\runs\consolidate-inventory-20260527-104637"),
    [string[]]$SkipSessionIds = @(),
    [string[]]$OnlySessionIds = @(),
    [int]$ThrottleSec = 45,
    [switch]$Apply,
    [switch]$StartServer
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "lib\Invoke-AiMemoryApi.ps1")
Import-AiMemoryEnvForScripts
$url = Get-AiMemoryBaseUrl

if (-not (Test-AiMemoryServerUp -BaseUrl $url)) {
    if ($StartServer) {
        Write-Host "Subindo serve..." -ForegroundColor Yellow
        Start-AiMemoryServeBackground
        if (-not (Wait-AiMemoryServer -BaseUrl $url)) { throw "serve offline" }
    } else { throw "serve offline; use -StartServer" }
}

$manifestPath = Join-Path $RunDir "manifest-before.json"
if (-not (Test-Path $manifestPath)) { throw "Missing $manifestPath" }
$manifest = Get-Content $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json

$queue = @($manifest.pending_sessions)
if ($OnlySessionIds.Count -gt 0) {
    $queue = $queue | Where-Object { $OnlySessionIds -contains $_.session_id }
}
if ($SkipSessionIds.Count -gt 0) {
    $queue = $queue | Where-Object { $SkipSessionIds -notcontains $_.session_id }
}
$queue = $queue | Sort-Object { $_.obs_count }, { $_.body_bytes }

$dryRun = -not $Apply
$logPath = Join-Path $RunDir "batch-sessions-$(Get-Date -Format 'yyyyMMdd-HHmmss').jsonl"
Write-Host "Batch sessões: $($queue.Count) itens, dry_run=$dryRun, throttle=${ThrottleSec}s" -ForegroundColor Cyan
Write-Host "Log: $logPath"

$ok = 0; $fail = 0
foreach ($item in $queue) {
    $sid = $item.session_id
    Write-Host "`n[$($ok+$fail+1)/$($queue.Count)] $sid ($($item.project), obs=$($item.obs_count))" -ForegroundColor Cyan
    try {
        $result = Invoke-AiMemoryMcpTool -ToolName "memory_consolidate" -Arguments @{
            session_id = $sid
            dry_run    = $dryRun
            multi_page = $false
        } -BaseUrl $url
        $text = ""
        if ($result.content -is [array]) {
            foreach ($c in $result.content) { if ($c.text) { $text += $c.text } }
        }
        $line = @{ ts = (Get-Date).ToUniversalTime().ToString("o"); session_id = $sid; ok = $true; dry_run = $dryRun; preview = $text.Substring(0, [Math]::Min(200, $text.Length)) }
        ($line | ConvertTo-Json -Compress) | Add-Content $logPath -Encoding utf8
        $ok++
        if ($text) { Write-Host $text.Substring(0, [Math]::Min(500, $text.Length)) }
    } catch {
        Write-Host "ERRO: $($_.Exception.Message)" -ForegroundColor Red
        @{ ts = (Get-Date).ToUniversalTime().ToString("o"); session_id = $sid; ok = $false; error = $_.Exception.Message } |
            ConvertTo-Json -Compress | Add-Content $logPath -Encoding utf8
        $fail++
    }
    if ($ThrottleSec -gt 0 -and ($ok + $fail) -lt $queue.Count) { Start-Sleep -Seconds $ThrottleSec }
}
Write-Host "`nFim: ok=$ok fail=$fail" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
