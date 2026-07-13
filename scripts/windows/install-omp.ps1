# Configura OMP (~/.omp/agent) para ai-memory + snapcompact com modelos Cursor.
# Requer: ai-memory compilado, omp instalado (bun install -g @oh-my-pi/pi-coding-agent).
param(
    [string]$ServerUrl = "http://127.0.0.1:49374",
    [string]$OmpAgentDir = $(Join-Path $env:USERPROFILE ".omp\agent"),
    [string]$CursorSkillsDir = $(Join-Path $env:USERPROFILE ".cursor\skills"),
    [int]$StreamIdleTimeoutMs = 600000,
    [switch]$SkipSkillsLink,
    [switch]$SkipModelOverrides,
    [switch]$SkipStreamTimeout
)

$ErrorActionPreference = "Stop"
$RepoRoot = if ($env:AI_MEMORY_REPO) { $env:AI_MEMORY_REPO } else { "C:\GIT\ai-memory" }
$Bin = Join-Path $RepoRoot "target\release\ai-memory.exe"

. (Join-Path $RepoRoot "scripts\windows\Import-AiMemoryEnv.ps1")
Import-AiMemoryEnv -RepoRoot $RepoRoot

if (-not (Test-Path $Bin)) {
    & (Join-Path $RepoRoot "scripts\windows\build.ps1")
}

New-Item -ItemType Directory -Force -Path $OmpAgentDir | Out-Null

function Set-OmpCursorComposerVisionOverrides {
    param([string]$AgentDir)

    $modelsPath = Join-Path $AgentDir "models.json"
    $composerIds = @("composer-1", "composer-1.5", "composer-2.5", "composer-2.5-fast")

    $root = [ordered]@{ providers = [ordered]@{} }
    if (Test-Path $modelsPath) {
        $raw = Get-Content $modelsPath -Raw -Encoding UTF8
        if ($raw.Trim().Length -gt 0) {
            $parsed = $raw | ConvertFrom-Json
            if ($null -ne $parsed) {
                $root = $parsed
            }
        }
    }

    if ($null -eq $root.providers) {
        $root | Add-Member -NotePropertyName providers -NotePropertyValue ([pscustomobject]@{}) -Force
    }
    if ($null -eq $root.providers.cursor) {
        $root.providers | Add-Member -NotePropertyName cursor -NotePropertyValue ([pscustomobject]@{}) -Force
    }
    if ($null -eq $root.providers.cursor.modelOverrides) {
        $root.providers.cursor | Add-Member -NotePropertyName modelOverrides -NotePropertyValue ([pscustomobject]@{}) -Force
    }

    foreach ($id in $composerIds) {
        $root.providers.cursor.modelOverrides | Add-Member -NotePropertyName $id -NotePropertyValue ([pscustomobject]@{
            input = @("text", "image")
        }) -Force
    }

    ($root | ConvertTo-Json -Depth 10) | Set-Content $modelsPath -Encoding UTF8
    Write-Host "models.json: cursor/composer modelOverrides -> text+image" -ForegroundColor Green
    Write-Host "  (persistente: OMP reaplica no boot; nao edite models.db)" -ForegroundColor DarkGray
}

function Remove-OmpContextFullCompactionWorkaround {
    param([string]$AgentDir)

    $configPath = Join-Path $AgentDir "config.yml"
    if (-not (Test-Path $configPath)) { return }

    $lines = Get-Content $configPath -Encoding UTF8
    $filtered = $lines | Where-Object { $_ -notmatch '^\s*compaction\.strategy:\s*context-full\s*$' }
    if ($filtered.Count -eq $lines.Count) { return }

    Set-Content $configPath ($filtered -join "`n") -Encoding UTF8
    Write-Host "config.yml: removido compaction.strategy context-full" -ForegroundColor Green
}

function Set-OmpStreamTimeoutEnv {
    param(
        [string]$AgentDir,
        [int]$TimeoutMs = 600000
    )

    $envPath = Join-Path $AgentDir ".env"
    $line = "PI_STREAM_IDLE_TIMEOUT_MS=$TimeoutMs"

    if (Test-Path $envPath) {
        $lines = Get-Content $envPath -Encoding UTF8
        $found = $false
        $newLines = foreach ($l in $lines) {
            if ($l -match '^\s*PI_STREAM_IDLE_TIMEOUT_MS\s*=') {
                $found = $true
                $line
            } else {
                $l
            }
        }
        if (-not $found) {
            $newLines += $line
        }
        Set-Content $envPath ($newLines -join "`n") -Encoding UTF8
    } else {
        @(
            "# OMP agent env (carregado automaticamente no startup)",
            "# Evita cortes em sessoes longas com modelos Cursor (default OMP: 120s)",
            $line
        ) | Set-Content $envPath -Encoding UTF8
    }

    Write-Host ".env: $line" -ForegroundColor Green
}

function Repair-OmpAgentsAiMemoryFooter {
    param([string]$AgentsPath)

    if (-not (Test-Path $AgentsPath)) { return }

    $content = Get-Content $AgentsPath -Raw -Encoding UTF8
    $end = "<!-- ai-memory:end -->"
    $idx = $content.IndexOf($end)
    if ($idx -lt 0) { return }

    $trimmed = $content.Substring(0, $idx + $end.Length).TrimEnd()
    if ($trimmed.Length -eq $content.TrimEnd().Length) { return }

    Set-Content $AgentsPath ($trimmed + "`n") -Encoding UTF8 -NoNewline
    Write-Host "AGENTS.md: removido lixo apos <!-- ai-memory:end -->" -ForegroundColor Green
}

function To-ForwardSlashPath {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) { return $Path }
    return ($Path -replace '\\', '/')
}

function Repair-OmpFirebirdMcpSpawn {
    param(
        [string]$McpPath,
        [string]$DatabasePath = "C:\consisanet\banco\consisanet.fdb",
        [string]$HostName = "localhost",
        [int]$Port = 5050,
        [string]$User = "SYSDBA",
        [string]$Password = "masterkey"
    )

    if (-not (Test-Path $McpPath)) { return }

    $nodeCmd = Get-Command node -ErrorAction SilentlyContinue
    if (-not $nodeCmd) {
        Write-Host "firebird MCP: node nao encontrado no PATH - mantendo mcp.json" -ForegroundColor Yellow
        return
    }

    $npmRoot = (& npm root -g 2>$null).Trim()
    $cliPath = Join-Path $npmRoot "mcp-firebird\dist\cli.js"
    if (-not (Test-Path $cliPath)) {
        Write-Host "firebird MCP: instalando mcp-firebird@2.6.0 global..." -ForegroundColor Cyan
        npm install -g mcp-firebird@2.6.0 | Out-Null
        $npmRoot = (& npm root -g 2>$null).Trim()
        $cliPath = Join-Path $npmRoot "mcp-firebird\dist\cli.js"
    }
    if (-not (Test-Path $cliPath)) {
        Write-Host "firebird MCP: cli.js nao encontrado apos npm install - mantendo mcp.json" -ForegroundColor Yellow
        return
    }

    $raw = Get-Content $McpPath -Raw -Encoding UTF8
    $root = $raw | ConvertFrom-Json
    if ($null -eq $root.mcpServers) {
        $root | Add-Member -NotePropertyName mcpServers -NotePropertyValue ([pscustomobject]@{}) -Force
    }
    if ($null -eq $root.mcpServers.firebird) {
        $root.mcpServers | Add-Member -NotePropertyName firebird -NotePropertyValue ([pscustomobject]@{}) -Force
    }

    $fb = $root.mcpServers.firebird
    $fb | Add-Member -NotePropertyName type -NotePropertyValue "stdio" -Force
    $fb | Add-Member -NotePropertyName command -NotePropertyValue "node" -Force
    $fb | Add-Member -NotePropertyName enabled -NotePropertyValue $true -Force
    $fb | Add-Member -NotePropertyName args -NotePropertyValue @(
        (To-ForwardSlashPath $cliPath),
        "--host", $HostName,
        "--port", "$Port",
        "--database", (To-ForwardSlashPath $DatabasePath),
        "--user", $User,
        "--password", $Password
    ) -Force

    ($root | ConvertTo-Json -Depth 10) | Set-Content $McpPath -Encoding UTF8
    Write-Host "mcp.json: firebird via node (contorna bug npx/cmd no OMP Windows)" -ForegroundColor Green
    Write-Host "  cli: $(To-ForwardSlashPath $cliPath)" -ForegroundColor DarkGray
}

function Ensure-OmpAgentRulesSection {
    param([string]$AgentsPath)

    if (-not (Test-Path $AgentsPath)) { return }

    $rulesBlock = @'
<!-- omp-agent-rules:start -->
## Ferramentas OMP (grep / glob)

- **Nunca** chame `grep` sem `pattern` (regex nao vazio). `grep` busca **texto dentro** de arquivos.
- Para **listar** ou **encontrar arquivos por nome**, use `glob` (`path` com glob, ex. `src/**/*.ts`).
- Erro `Pattern must not be empty` = chamada invalida de `grep`; corrija com `glob` ou um `pattern` concreto.

## Stream do provider (sessoes longas)

Turnos com muitas ferramentas podem abortar com `Provider stream stalled while waiting for the next event` (timeout padrao 120s).

- `~/.omp/agent/.env` define `PI_STREAM_IDLE_TIMEOUT_MS=600000` (10 min).
- Reinicie o OMP apos alterar. Use `0` para desabilitar o watchdog.
<!-- omp-agent-rules:end -->
'@

    $content = Get-Content $AgentsPath -Raw -Encoding UTF8
    if ($content -match '<!-- omp-agent-rules:start -->') {
        $content = [regex]::Replace(
            $content,
            '(?s)<!-- omp-agent-rules:start -->.*?<!-- omp-agent-rules:end -->',
            $rulesBlock
        )
    } else {
        $needle = 'Reinicie o `omp` após alterar extensões, MCP ou `AGENTS.md`.'
        if ($content -match [regex]::Escape($needle)) {
            $content = $content -replace ([regex]::Escape($needle)), ($needle + "`n`n" + $rulesBlock)
        } elseif ($content -match '<!-- ai-memory:start -->') {
            $content = $content -replace '(<!-- ai-memory:start -->)', ($rulesBlock + "`n`n`$1")
        } else {
            $content = ($content.TrimEnd() + "`n`n" + $rulesBlock + "`n")
        }
    }

    Set-Content $AgentsPath $content.TrimEnd() -Encoding UTF8 -NoNewline
    Write-Host "AGENTS.md: bloco omp-agent-rules aplicado" -ForegroundColor Green
}

if (-not $SkipModelOverrides) {
    Set-OmpCursorComposerVisionOverrides -AgentDir $OmpAgentDir
    Remove-OmpContextFullCompactionWorkaround -AgentDir $OmpAgentDir
}

if (-not $SkipStreamTimeout) {
    Set-OmpStreamTimeoutEnv -AgentDir $OmpAgentDir -TimeoutMs $StreamIdleTimeoutMs
}

$extensionsDir = Join-Path $OmpAgentDir "extensions"
New-Item -ItemType Directory -Force -Path $extensionsDir | Out-Null
$extPath = Join-Path $extensionsDir "ai-memory.ts"
$hookArgs = @(
    "install-hooks", "--agent", "omp", "--apply",
    "--server-url", $ServerUrl,
    "--config-file", $extPath
)
if ($env:AI_MEMORY_AUTH_TOKEN) {
    $hookArgs += @("--auth-token", $env:AI_MEMORY_AUTH_TOKEN)
}
& $Bin @hookArgs
Write-Host "ai-memory extension: $extPath" -ForegroundColor Green

$mcpArgs = @("install-mcp", "--client", "pi", "--apply", "--server-url", "$ServerUrl/mcp")
if ($env:AI_MEMORY_AUTH_TOKEN) {
    $mcpArgs += @("--auth-token", $env:AI_MEMORY_AUTH_TOKEN)
}
& $Bin @mcpArgs
Repair-OmpFirebirdMcpSpawn -McpPath (Join-Path $OmpAgentDir "mcp.json")
Write-Host "MCP: $OmpAgentDir\mcp.json" -ForegroundColor Green

$skillsDir = Join-Path $OmpAgentDir "skills"
if (-not $SkipSkillsLink) {
    if (Test-Path $skillsDir) {
        $item = Get-Item $skillsDir
        if ($item.LinkType -and $item.Target -eq $CursorSkillsDir) {
            Write-Host "skills junction ja aponta para $CursorSkillsDir" -ForegroundColor DarkGray
        } else {
            Write-Host "skills dir existe sem junction - nao sobrescrevi: $skillsDir" -ForegroundColor Yellow
        }
    } elseif (Test-Path $CursorSkillsDir) {
        New-Item -ItemType Junction -Path $skillsDir -Target $CursorSkillsDir | Out-Null
        Write-Host "skills junction: $skillsDir -> $CursorSkillsDir" -ForegroundColor Green
    }
}

$agentsPath = Join-Path $OmpAgentDir "AGENTS.md"
if (-not (Test-Path $agentsPath)) {
    @"
# Instrucoes globais (OMP + ai-memory)

Responda sempre em **portugues brasileiro**.

## Ambiente (Windows)

- OS: Windows 11, shell: **PowerShell 7**
- Trabalhar apenas em **C:\**
- Nao use sintaxe bash. Use PowerShell e equivalentes Windows.

## Memoria (ai-memory)

Servidor local: $ServerUrl - rode ai-memory serve --transport http antes de sessoes longas.

Captura automatica: extensao ~/.omp/agent/extensions/ai-memory.ts

Consulta: ferramentas MCP ai-memory (~/.omp/agent/mcp.json).

"@ | Set-Content $agentsPath -Encoding UTF8
}
& $Bin install-instructions --target $agentsPath
Repair-OmpAgentsAiMemoryFooter -AgentsPath $agentsPath
Ensure-OmpAgentRulesSection -AgentsPath $agentsPath
Write-Host "AGENTS.md: $agentsPath" -ForegroundColor Green

Write-Host ""
Write-Host "Proximos passos:" -ForegroundColor Yellow
Write-Host "  1. ai-memory serve --transport http --enable-web"
Write-Host "  2. Reinicie o OMP (extensoes + models.json + .env carregam no startup)"
Write-Host "  3. omp --model cursor/composer-2.5-fast"
Write-Host "  4. Teste /compact (snapcompact)"
Write-Host "  5. Confirme MCP firebird (nao deve aparecer Transport closed no log)"
Write-Host ""
Write-Host "Reaplique apos upgrade OMP:" -ForegroundColor Cyan
Write-Host "  cd C:\GIT\ai-memory; .\scripts\windows\install-omp.ps1"
