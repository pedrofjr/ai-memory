# Piloto: reescreve 2 páginas imported/ via LLM + POST /admin/write-page.
# Não apaga o imported/ original (graveyard manual depois da revisão).
param(
    [string]$BaseUrl,
    [string]$RunDir = (Join-Path $env:USERPROFILE ".ai-memory\runs\consolidate-inventory-20260527-104637"),
    [string]$ManifestImported = "",
    [string[]]$Paths = @(
        "imported/memories/mem_mpfka37d_4ef6e02c862d.md",
        "imported/lessons/lsn_b5a224f20cb216c5.md"
    ),
    [string]$Workspace = "default",
    [switch]$Apply,
    [switch]$StartServer
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "lib\Invoke-AiMemoryApi.ps1")
Import-AiMemoryEnvForScripts
if ($BaseUrl) { $env:AI_MEMORY_SERVER_URL = $BaseUrl }
$url = Get-AiMemoryBaseUrl

if (-not $env:AI_MEMORY_LLM_PROVIDER) {
    throw "Defina AI_MEMORY_LLM_PROVIDER em .env / .env.local"
}
if (-not $env:AI_MEMORY_LLM_MODEL -and $env:CURSOR_MODEL) {
    $env:AI_MEMORY_LLM_MODEL = $env:CURSOR_MODEL
}
if (-not $env:AI_MEMORY_LLM_MODEL) {
    throw "Defina AI_MEMORY_LLM_MODEL ou CURSOR_MODEL"
}

if (-not (Test-AiMemoryServerUp -BaseUrl $url)) {
    if ($StartServer) {
        Write-Host "Subindo ai-memory serve..." -ForegroundColor Yellow
        Start-AiMemoryServeBackground
        if (-not (Wait-AiMemoryServer -BaseUrl $url)) {
            throw "ai-memory não responde em $url"
        }
    } else {
        throw "ai-memory offline em $url. Use -StartServer."
    }
}

$dryRun = -not $Apply
$logPath = Join-Path $RunDir "pilot-imported-$(Get-Date -Format 'yyyyMMdd-HHmmss').jsonl"
New-Item -ItemType Directory -Path $RunDir -Force | Out-Null

# Resolver project a partir do manifest-imported.json quando Paths omitidos.
if ($ManifestImported -eq "") {
    $ManifestImported = Join-Path $RunDir "manifest-imported.json"
}
$projectByPath = @{}
if (Test-Path $ManifestImported) {
    $manifest = Get-Content $ManifestImported -Raw -Encoding utf8 | ConvertFrom-Json
    foreach ($row in $manifest.pending) {
        $projectByPath[$row.path] = $row.project
    }
}

$repo = Get-AiMemoryRepoRoot
$exe = Join-Path $repo "target\release\ai-memory.exe"
if (-not (Test-Path $exe)) { $exe = Join-Path $repo "target\debug\ai-memory.exe" }
$provider = ($env:AI_MEMORY_LLM_PROVIDER -replace '_', '-').ToLowerInvariant()
$model = $env:AI_MEMORY_LLM_MODEL

function Invoke-LlmRewrite {
    param([string]$SourceBody, [string]$SourcePath, [string]$ImportKind)
    $prompt = @"
Você limpa páginas wiki migradas do agentmemory para o formato ai-memory.

Regras:
- Remova rodapé "_Migrado do agentmemory..._" e seções vazias/redundantes (## Conceitos / ## Arquivos) se o corpo já estiver claro.
- Português brasileiro.
- Responda APENAS com um JSON válido (sem markdown ao redor):
{"title":"...","body_markdown":"...","tags":["migrated-clean"],"suggested_path":"facts/slug-curto.md","tier":"semantic","kind":"fact"}
- kind: fact | decision | gotcha | rule (rule só se for convenção permanente do projeto).
- suggested_path: relativo, sem imported/; use facts/, decisions/, gotchas/ ou _rules/ conforme kind.
- body_markdown: sem frontmatter YAML; só markdown.

Origem ($ImportKind): $SourcePath

--- CONTEÚDO ---
$SourceBody
"@
    $out = & $exe @(
        "llm-test", "--provider", $provider, "--model", $model, "--prompt", $prompt
    ) 2>&1 | Out-String
    $jsonText = Extract-JsonFromLlmText -Text $out
    return $jsonText | ConvertFrom-Json
}

Write-Host "Piloto imported (dry_run=$dryRun) provider=$provider model=$model" -ForegroundColor Cyan
Write-Host "Log: $logPath"

foreach ($relPath in $Paths) {
    Write-Host "`n--- $relPath ---" -ForegroundColor Cyan
    $project = $projectByPath[$relPath]
    if (-not $project) {
        Write-Warning "project não encontrado no manifest; use global"
        $project = "global"
    }

    $file = Find-WikiPageFile -RelativePath $relPath
    if (-not $file) {
        Write-Host "ERRO: arquivo não encontrado no wiki" -ForegroundColor Red
        continue
    }
    $sourceBody = Get-Content $file -Raw -Encoding utf8
    $kind = if ($relPath -match '/lessons/') { "lesson" } else { "memory" }

    try {
        $rewritten = Invoke-LlmRewrite -SourceBody $sourceBody -SourcePath $relPath -ImportKind $kind
        $targetPath = $rewritten.suggested_path
        if (-not $targetPath) {
            $slug = (Split-Path $relPath -Leaf) -replace '\.md$', ''
            $targetPath = "facts/$slug-clean.md"
        }

        $record = @{
            ts           = (Get-Date).ToUniversalTime().ToString("o")
            source_path  = $relPath
            project      = $project
            target_path  = $targetPath
            title        = $rewritten.title
            dry_run      = $dryRun
            ok           = $true
            tags         = $rewritten.tags
        }

        if ($dryRun) {
            Write-Host "[dry-run] $($project)::$targetPath" -ForegroundColor DarkGray
            Write-Host $rewritten.body_markdown.Substring(0, [Math]::Min(400, $rewritten.body_markdown.Length))
            if ($rewritten.body_markdown.Length -gt 400) { Write-Host "..." }
        } else {
            $tags = @($rewritten.tags) + @("migrated-clean", "from-imported")
            $resp = Invoke-AiMemoryAdminPost -Path "/admin/write-page" -Body @{
                workspace = $Workspace
                project   = $project
                path      = $targetPath
                body      = $rewritten.body_markdown
                title     = $rewritten.title
                tier      = $rewritten.tier
                tags      = $tags
                pinned    = $false
            } -BaseUrl $url
            $record.page_id = $resp.page_id
            Write-Host "OK wrote $($resp.path) page_id=$($resp.page_id)" -ForegroundColor Green
        }

        ($record | ConvertTo-Json -Depth 8 -Compress) | Add-Content -Path $logPath -Encoding utf8
    } catch {
        Write-Host "ERRO: $($_.Exception.Message)" -ForegroundColor Red
        @{
            ts = (Get-Date).ToUniversalTime().ToString("o")
            source_path = $relPath
            ok = $false
            error = $_.Exception.Message
        } | ConvertTo-Json -Compress | Add-Content -Path $logPath -Encoding utf8
    }
}

if ($dryRun) {
    Write-Host "`nDry-run: nada gravado. imported/ original intacto. Para gravar: -Apply" -ForegroundColor Yellow
} else {
    Write-Host "`nPáginas novas gravadas. Revise no /web antes de mover imported/ para graveyard." -ForegroundColor Yellow
}
