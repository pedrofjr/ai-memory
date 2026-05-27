# Reescreve imported/ pendentes (manifest-imported.json) via llm-test + write-page.
param(
    [string]$RunDir = (Join-Path $env:USERPROFILE ".ai-memory\runs\consolidate-inventory-20260527-104637"),
    [string[]]$SkipPaths = @(
        "imported/memories/mem_mpfka37d_4ef6e02c862d.md",
        "imported/lessons/lsn_b5a224f20cb216c5.md"
    ),
    [string[]]$OnlyPaths = @(),
    [int]$ThrottleSec = 60,
    [switch]$Apply,
    [switch]$StartServer
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "lib\Invoke-AiMemoryApi.ps1")
Import-AiMemoryEnvForScripts
$url = Get-AiMemoryBaseUrl

if (-not $env:AI_MEMORY_LLM_PROVIDER) { throw "AI_MEMORY_LLM_PROVIDER missing" }
if (-not $env:AI_MEMORY_LLM_MODEL -and $env:CURSOR_MODEL) { $env:AI_MEMORY_LLM_MODEL = $env:CURSOR_MODEL }

if (-not (Test-AiMemoryServerUp -BaseUrl $url)) {
    if ($StartServer) {
        Start-AiMemoryServeBackground
        if (-not (Wait-AiMemoryServer -BaseUrl $url)) { throw "serve offline" }
    } else { throw "serve offline" }
}

$repo = Get-AiMemoryRepoRoot
$exe = Join-Path $repo "target\release\ai-memory.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $repo "target\debug\ai-memory.exe" }
$provider = ($env:AI_MEMORY_LLM_PROVIDER -replace '_', '-').ToLowerInvariant()
$model = $env:AI_MEMORY_LLM_MODEL

$manifestPath = Join-Path $RunDir "manifest-imported.json"
$manifest = Get-Content $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
$queue = @($manifest.pending)
if ($OnlyPaths.Count -gt 0) { $queue = $queue | Where-Object { $OnlyPaths -contains $_.path } }
if ($SkipPaths.Count -gt 0) { $queue = $queue | Where-Object { $SkipPaths -notcontains $_.path } }
$queue = $queue | Sort-Object { $_.body_bytes }

$dryRun = -not $Apply
$logPath = Join-Path $RunDir "batch-imported-$(Get-Date -Format 'yyyyMMdd-HHmmss').jsonl"
Write-Host "Batch imported: $($queue.Count), dry_run=$dryRun" -ForegroundColor Cyan

function Invoke-LlmRewrite {
    param([string]$SourceBody, [string]$SourcePath, [string]$ImportKind)
    $prompt = @"
Limpe esta página migrada do agentmemory para wiki ai-memory.
Responda só JSON: {"title":"...","body_markdown":"...","tags":["migrated-clean"],"suggested_path":"facts/slug.md","tier":"semantic","kind":"fact"}
kind: fact|decision|gotcha|rule. suggested_path sem imported/. body_markdown sem YAML.
Origem ($ImportKind): $SourcePath
--- CONTEÚDO ---
$SourceBody
"@
    $out = & $exe @("llm-test", "--provider", $provider, "--model", $model, "--prompt", $prompt) 2>&1 | Out-String
    $jsonText = Extract-JsonFromLlmText -Text $out
    return $jsonText | ConvertFrom-Json
}

$ok = 0; $fail = 0
foreach ($row in $queue) {
    $rel = $row.path
    Write-Host "`n[$($ok+$fail+1)/$($queue.Count)] $rel" -ForegroundColor Cyan
    try {
        $file = Find-WikiPageFile -RelativePath $rel
        if (-not $file) { throw "wiki file not found" }
        $body = Get-Content $file -Raw -Encoding utf8
        $kind = if ($rel -match '/lessons/') { "lesson" } else { "memory" }
        $rewritten = Invoke-LlmRewrite -SourceBody $body -SourcePath $rel -ImportKind $kind
        $target = $rewritten.suggested_path
        if (-not $target) { $target = "facts/$(Split-Path $rel -Leaf)" }
        if ($dryRun) {
            Write-Host "[dry-run] $($row.project)::$target" -ForegroundColor DarkGray
            $ok++
        } else {
            $tags = @($rewritten.tags) + @("migrated-clean", "from-imported")
            $resp = Invoke-AiMemoryAdminPost -Path "/admin/write-page" -Body @{
                workspace = "default"; project = $row.project; path = $target
                body = $rewritten.body_markdown; title = $rewritten.title
                tier = $rewritten.tier; tags = $tags; pinned = $false
            } -BaseUrl $url
            Write-Host "OK $($resp.path)" -ForegroundColor Green
            @{ ts = (Get-Date).ToUniversalTime().ToString("o"); path = $rel; target = $target; ok = $true } |
                ConvertTo-Json -Compress | Add-Content $logPath -Encoding utf8
            $ok++
        }
    } catch {
        Write-Host "ERRO: $($_.Exception.Message)" -ForegroundColor Red
        @{ ts = (Get-Date).ToUniversalTime().ToString("o"); path = $rel; ok = $false; error = $_.Exception.Message } |
            ConvertTo-Json -Compress | Add-Content $logPath -Encoding utf8
        $fail++
    }
    if ($ThrottleSec -gt 0) { Start-Sleep -Seconds $ThrottleSec }
}
Write-Host "`nFim: ok=$ok fail=$fail imported/ originais intactos" -ForegroundColor Green
