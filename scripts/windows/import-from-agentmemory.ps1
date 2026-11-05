# Porta memórias consolidadas (e opcionalmente observações) do agentmemory para ai-memory.
param(
    [string]$AgentMemoryUrl = "http://127.0.0.1:3111",
    [string]$AiMemoryUrl = "http://127.0.0.1:49374",
    [string]$Workspace = "default",
    [switch]$IncludeObservations,
    [switch]$DryRun,
    [switch]$StartServer
)

$ErrorActionPreference = "Stop"
$RepoRoot = if ($env:AI_MEMORY_REPO) { $env:AI_MEMORY_REPO } else { "C:\GIT\ai-memory" }
. (Join-Path $RepoRoot "scripts\windows\Import-AiMemoryEnv.ps1")
Import-AiMemoryEnv -RepoRoot $RepoRoot

function Sanitize-ProjectName {
    param([string]$Raw)
    if ([string]::IsNullOrWhiteSpace($Raw)) { return "global" }
    $name = [System.IO.Path]::GetFileName($Raw.TrimEnd('\', '/'))
    if ([string]::IsNullOrWhiteSpace($name)) { return "global" }
    $name = $name -replace '[^\w\-\.]', '-'
    if ($name.Length -gt 64) { $name = $name.Substring(0, 64) }
    if ([string]::IsNullOrWhiteSpace($name)) { return "global" }
    return $name.ToLowerInvariant()
}

function Resolve-ProjectFromMemory {
    param($Memory)
    if ($Memory.project) { return (Sanitize-ProjectName $Memory.project) }
    foreach ($f in @($Memory.files)) {
        if ($f -match 'consisanet') { return "consisanet" }
        if ($f -match 'agentmemory') { return "agentmemory" }
        if ($f -match 'Garimpo_memorias') { return "garimpo" }
    }
    foreach ($c in @($Memory.concepts)) {
        if ($c -match 'consisanet|darf|delphi') { return "consisanet" }
        if ($c -match 'agentmemory|cursor') { return "agentmemory" }
    }
    return "global"
}

function Map-MemoryTier {
    param([string]$Type)
    switch ($Type) {
        "workflow" { "procedural" }
        "pattern" { "procedural" }
        default { "semantic" }
    }
}

function Slug-Path {
    param([string]$Id, [string]$Prefix)
    $safe = ($Id -replace '[^a-zA-Z0-9_-]', '-').ToLowerInvariant()
    return "$Prefix/$safe.md"
}

function Build-MemoryBody {
    param($Memory)
    $lines = @()
    if ($Memory.title -and $Memory.title -ne $Memory.content) {
        $lines += "# $($Memory.title)"
        $lines += ""
    }
    $lines += $Memory.content
    if (@($Memory.concepts).Count -gt 0) {
        $lines += ""
        $lines += "## Conceitos"
        foreach ($c in $Memory.concepts) { $lines += "- $c" }
    }
    if (@($Memory.files).Count -gt 0) {
        $lines += ""
        $lines += "## Arquivos"
        foreach ($f in $Memory.files) { $lines += "- $f" }
    }
    $lines += ""
    $lines += "---"
    $lines += "_Migrado do agentmemory ``$($Memory.id)`` em $(Get-Date -Format 'yyyy-MM-dd')._"
    return ($lines -join "`n")
}

function Build-LessonBody {
    param($Lesson)
    $lines = @("# Lição: $($Lesson.id)", "")
    if ($Lesson.context) {
        $lines += "**Quando aplicar:** $($Lesson.context)"
        $lines += ""
    }
    $lines += $Lesson.content
    $lines += ""
    $lines += "---"
    $lines += "_Migrado do agentmemory lesson ``$($Lesson.id)``._"
    return ($lines -join "`n")
}

function Build-ObservationSessionBody {
    param($SessionId, $Observations)
    $lines = @(
        "# Sessão $SessionId (observações migradas)",
        "",
        "Total: $(@($Observations).Count) observações do agentmemory.",
        ""
    )
    foreach ($o in ($Observations | Sort-Object { $_.timestamp })) {
        $lines += "## $($o.title)"
        $lines += "- **Tipo:** $($o.type) | **Importância:** $($o.importance) | **Quando:** $($o.timestamp)"
        if (@($o.facts).Count -gt 0) {
            foreach ($f in $o.facts) { $lines += "- $f" }
        }
        if ($o.narrative) {
            $lines += ""
            $lines += $o.narrative
        }
        $lines += ""
    }
    $lines += "---"
    $lines += "_Migrado do agentmemory (sessão ``$SessionId``)._"
    return ($lines -join "`n")
}

function Test-AiMemoryUp {
    try {
        $r = Invoke-WebRequest -Uri "$AiMemoryUrl/admin/status" -UseBasicParsing -TimeoutSec 3
        return $r.StatusCode -eq 200
    } catch { return $false }
}

function Start-AiMemoryServe {
    $launcher = Join-Path $env:USERPROFILE ".local\bin\ai-memory.ps1"
    if (-not (Test-Path $launcher)) {
        $launcher = Join-Path $env:USERPROFILE ".local\bin\ai_memory.ps1"
    }
    if (Test-Path $launcher) {
        Start-Process -FilePath "pwsh" -ArgumentList @(
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $launcher, "serve",
            "--transport", "http", "--bind", "127.0.0.1:49374"
        ) -WindowStyle Hidden
    } else {
        $bin = Join-Path $RepoRoot "target\release\ai-memory.exe"
        Start-Process -FilePath $bin -ArgumentList @("serve", "--transport", "http", "--bind", "127.0.0.1:49374") `
            -WindowStyle Hidden
    }
}

function Write-AiMemoryPage {
    param(
        [string]$Project,
        [string]$Path,
        [string]$Body,
        [string]$Title,
        [string]$Tier,
        [string[]]$Tags,
        [bool]$Pinned
    )
    $payload = @{
        workspace = $Workspace
        project   = $Project
        path      = $Path
        body      = $Body
        title     = $Title
        tier      = $Tier
        tags      = $Tags
        pinned    = $Pinned
    }
    if ($DryRun) {
        Write-Host "[dry-run] $Project :: $Path ($Tier)" -ForegroundColor DarkGray
        return
    }
    $null = Invoke-RestMethod -Method Post -Uri "$AiMemoryUrl/admin/write-page" `
        -ContentType "application/json" -Body ($payload | ConvertTo-Json -Depth 6) -TimeoutSec 120
    Write-Host "  OK $Project/$Path" -ForegroundColor Green
}

# --- agentmemory deve estar no ar ---
try {
    $null = Invoke-WebRequest -Uri "$AgentMemoryUrl/agentmemory/livez" -UseBasicParsing -TimeoutSec 5
} catch {
    Write-Error "agentmemory não responde em $AgentMemoryUrl — inicie com: npx @agentmemory/agentmemory (ou agentmemory-start.bat)"
}

# --- ai-memory serve ---
if (-not (Test-AiMemoryUp)) {
    if ($StartServer) {
        Write-Host "Subindo ai-memory serve em background..." -ForegroundColor Yellow
        Start-AiMemoryServe
        $deadline = (Get-Date).AddSeconds(60)
        while ((Get-Date) -lt $deadline) {
            if (Test-AiMemoryUp) { break }
            Start-Sleep -Seconds 1
        }
    }
    if (-not (Test-AiMemoryUp)) {
        Write-Error "ai-memory não responde em $AiMemoryUrl — rode: ai-memory serve (ou -StartServer)"
    }
}

$stats = @{ memories = 0; lessons = 0; sessions = 0; errors = 0 }

Write-Host "Importando memórias (latest)..." -ForegroundColor Cyan
$memResp = Invoke-RestMethod -Uri "$AgentMemoryUrl/agentmemory/memories?latest=true" -TimeoutSec 120
foreach ($m in @($memResp.memories)) {
    if (-not $m.isLatest) { continue }
    $project = Resolve-ProjectFromMemory $m
    $tier = Map-MemoryTier $m.type
    $tags = @("migrated", "agentmemory", $m.type) + @($m.concepts)
    $pinned = ($m.strength -ge 8) -or ($m.type -eq "preference")
    try {
        Write-AiMemoryPage -Project $project -Path (Slug-Path $m.id "imported/memories") `
            -Body (Build-MemoryBody $m) -Title $m.title -Tier $tier -Tags $tags -Pinned:$pinned
        $stats.memories++
    } catch {
        $stats.errors++
        Write-Host "  ERRO mem $($m.id): $($_.Exception.Message)" -ForegroundColor Red
    }
}

Write-Host "Importando lições..." -ForegroundColor Cyan
try {
    $lesResp = Invoke-RestMethod -Uri "$AgentMemoryUrl/agentmemory/lessons" -TimeoutSec 60
    foreach ($l in @($lesResp.lessons)) {
        $project = if ($l.project) { Sanitize-ProjectName $l.project } else { "global" }
        $tags = @("migrated", "agentmemory", "lesson") + @($l.tags)
        try {
            Write-AiMemoryPage -Project $project -Path (Slug-Path $l.id "imported/lessons") `
                -Body (Build-LessonBody $l) -Title "Lição $($l.id)" -Tier "semantic" -Tags $tags -Pinned:$true
            $stats.lessons++
        } catch {
            $stats.errors++
            Write-Host "  ERRO lesson $($l.id): $($_.Exception.Message)" -ForegroundColor Red
        }
    }
} catch {
    Write-Host "  Aviso: lessons indisponível — $($_.Exception.Message)" -ForegroundColor Yellow
}

if ($IncludeObservations) {
    Write-Host "Importando observações por sessão..." -ForegroundColor Cyan
    $kvDir = Join-Path $env:USERPROFILE ".agentmemory\data\state_store.db"
    $obsKeys = Get-ChildItem -Path $kvDir -Filter "mem%3Aobs%3A*.bin" -ErrorAction SilentlyContinue
    foreach ($f in $obsKeys) {
        if ($f.Name -match 'mem%3Aobs%3A([^%]+)\.bin') {
            $sessionId = [System.Uri]::UnescapeDataString($Matches[1])
            try {
                $obsResp = Invoke-RestMethod -Uri "$AgentMemoryUrl/agentmemory/observations?sessionId=$sessionId" -TimeoutSec 180
                $obs = @($obsResp.observations)
                if ($obs.Count -eq 0) { continue }
                $sess = Invoke-RestMethod -Uri "$AgentMemoryUrl/agentmemory/sessions" -TimeoutSec 30
                $match = $sess.sessions | Where-Object { $_.id -eq $sessionId } | Select-Object -First 1
                $project = if ($match -and $match.project) { Sanitize-ProjectName $match.project } else { "global" }
                $body = Build-ObservationSessionBody $sessionId $obs
                Write-AiMemoryPage -Project $project -Path (Slug-Path $sessionId "imported/sessions") `
                    -Body $body -Title "Sessão $sessionId" -Tier "episodic" `
                    -Tags @("migrated", "agentmemory", "observations") -Pinned:$false
                $stats.sessions++
            } catch {
                $stats.errors++
                Write-Host "  ERRO obs $sessionId : $($_.Exception.Message)" -ForegroundColor Red
            }
        }
    }
}

Write-Host ""
Write-Host "Resumo:" -ForegroundColor Cyan
Write-Host "  Memórias: $($stats.memories)"
Write-Host "  Lições:   $($stats.lessons)"
Write-Host "  Sessões:  $($stats.sessions) (observações)"
Write-Host "  Erros:    $($stats.errors)"
if (-not $DryRun -and $stats.errors -eq 0) {
    Write-Host ""
    Write-Host "Próximo passo (embeddings):" -ForegroundColor Yellow
    Write-Host "  ai-memory embed"
}
