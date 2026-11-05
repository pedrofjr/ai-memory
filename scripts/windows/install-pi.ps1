# Configura pi (~/.pi/agent) para ai-memory + porte de contexto do Cursor.
# Requer: ai-memory compilado, pi + pi-cursor-sdk instalados.
param(
    [string]$ServerUrl = "http://127.0.0.1:49374",
    [string]$PiAgentDir = $(Join-Path $env:USERPROFILE ".pi\agent"),
    [string]$CursorRulesDir = $(Join-Path $env:USERPROFILE ".cursor\rules"),
    [string]$CursorSkillsDir = $(Join-Path $env:USERPROFILE ".cursor\skills"),
    [switch]$SkipSkillsLink
)

$ErrorActionPreference = "Stop"
$RepoRoot = if ($env:AI_MEMORY_REPO) { $env:AI_MEMORY_REPO } else { "C:\GIT\ai-memory" }
$Bin = Join-Path $RepoRoot "target\release\ai-memory.exe"

. (Join-Path $RepoRoot "scripts\windows\Import-AiMemoryEnv.ps1")
Import-AiMemoryEnv -RepoRoot $RepoRoot

if (-not (Test-Path $Bin)) {
    & (Join-Path $RepoRoot "scripts\windows\build.ps1")
}

$extensionsDir = Join-Path $PiAgentDir "extensions"
$skillsDir = Join-Path $PiAgentDir "skills"
New-Item -ItemType Directory -Force -Path $extensionsDir | Out-Null

# --- 1) Extensão ai-memory (captura + handoff) em ~/.pi/agent/extensions ---
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
if (Test-Path $extPath) {
    (Get-Content $extPath -Raw) -replace 'const AGENT = "omp"', 'const AGENT = "pi"' | Set-Content $extPath -Encoding UTF8
}
Write-Host "ai-memory hooks: $extPath" -ForegroundColor Green

# --- 2) Extensão: regras do Cursor (.mdc) no system prompt ---
$bridgePath = Join-Path $extensionsDir "cursor-context-bridge.ts"
@'
/**
 * Expõe ~/.cursor/rules/*.mdc no system prompt do pi (conteúdo após frontmatter YAML).
 * Complementa AGENTS.md; não substitui /skill: do pi.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const RULES_DIR = process.env.PI_CURSOR_RULES_DIR
  ? path.resolve(process.env.PI_CURSOR_RULES_DIR)
  : path.join(process.env.USERPROFILE ?? process.env.HOME ?? "", ".cursor", "rules");

function stripFrontmatter(raw: string): string {
  const trimmed = raw.trimStart();
  if (!trimmed.startsWith("---")) return raw.trim();
  const end = trimmed.indexOf("---", 3);
  if (end === -1) return raw.trim();
  return trimmed.slice(end + 3).trim();
}

function loadRules(): { name: string; body: string }[] {
  if (!fs.existsSync(RULES_DIR)) return [];
  return fs
    .readdirSync(RULES_DIR)
    .filter((f) => f.endsWith(".mdc"))
    .sort()
    .map((f) => ({
      name: f.replace(/\.mdc$/, ""),
      body: stripFrontmatter(fs.readFileSync(path.join(RULES_DIR, f), "utf8")),
    }))
    .filter((r) => r.body.length > 0);
}

export default function cursorContextBridge(pi: ExtensionAPI) {
  let rules: { name: string; body: string }[] = [];

  pi.on("session_start", async (_event, ctx) => {
    rules = loadRules();
    if (rules.length > 0) {
      ctx.ui.notify(`Cursor rules: ${rules.length} arquivo(s) em ${RULES_DIR}`, "info");
    }
  });

  pi.on("before_agent_start", async (event) => {
    if (rules.length === 0) return;
    const blocks = rules.map((r) => `### Regra Cursor: ${r.name}\n\n${r.body}`).join("\n\n---\n\n");
    return {
      systemPrompt:
        event.systemPrompt +
        `\n\n## Regras importadas de ${RULES_DIR}\n\n` +
        blocks,
    };
  });
}
'@ | Set-Content $bridgePath -Encoding UTF8
Write-Host "cursor-context-bridge: $bridgePath" -ForegroundColor Green

# --- 3) Skills: junction ~/.pi/agent/skills -> ~/.cursor/skills ---
if (-not $SkipSkillsLink) {
    if (Test-Path $skillsDir) {
        $item = Get-Item $skillsDir
        if ($item.LinkType -and $item.Target -eq $CursorSkillsDir) {
            Write-Host "skills junction já aponta para $CursorSkillsDir" -ForegroundColor DarkGray
        } else {
            Write-Host "skills dir existe sem junction — não sobrescrevi: $skillsDir" -ForegroundColor Yellow
        }
    } elseif (Test-Path $CursorSkillsDir) {
        New-Item -ItemType Junction -Path $skillsDir -Target $CursorSkillsDir | Out-Null
        Write-Host "skills junction: $skillsDir -> $CursorSkillsDir" -ForegroundColor Green
    }
}

# --- 4) AGENTS.md global do pi ---
$agentsPath = Join-Path $PiAgentDir "AGENTS.md"
$header = @'
# Instruções globais (pi + Cursor SDK)

Responda sempre em **português brasileiro**.

## Ambiente (Windows)

- OS: Windows 11, shell: **PowerShell 7**
- Trabalhar apenas em **C:\**
- Não use sintaxe bash (&&, ||, /tmp). Use PowerShell e equivalentes Windows.
- Comandos shell externos: prefixe com **rtk** quando aplicável (rtk git status, etc.).

## pi + pi-cursor-sdk (modelos Cursor)

- Provider padrão: **cursor** (composer-2.5 em ~/.pi/agent/settings.json).
- O **Cursor SDK** (via pi-cursor-sdk) carrega automaticamente:
  - MCP de ~/.cursor/mcp.json (ai-memory, firebird, etc.)
  - Regras/plugins/settings do Cursor (PI_CURSOR_SETTING_SOURCES=all é o padrão da extensão)
- **Não** desative o bridge de ferramentas pi sem motivo: PI_CURSOR_PI_TOOL_BRIDGE=0 remove tools pi do agente Cursor.
- Chave: CURSOR_API_KEY ou /login no pi (gravado em ~/.pi/agent/auth.json).

## Delphi 6 (Consisanet)

Quando o trabalho for Delphi 6 / Consisanet / Firebird ERP:

1. Use /skill:delphi-orchestrator (ou leia ~/.cursor/skills/delphi-orchestrator/SKILL.md).
2. Carregue delphi-context antes de editar código.
3. Tarefas não triviais: planejar com subagentes (rubber-duck / delphi-qa) conforme a skill.

## Memória (ai-memory)

Servidor local: SERVER_URL_PLACEHOLDER — rode ai-memory serve --transport http em outro terminal antes de sessões longas.

Captura automática: extensão ~/.pi/agent/extensions/ai-memory.ts (hooks HTTP).

Consulta: ferramentas MCP **ai-memory** (carregadas pelo Cursor SDK a partir de ~/.cursor/mcp.json).

'@ -replace 'SERVER_URL_PLACEHOLDER', $ServerUrl
if (-not (Test-Path $agentsPath)) {
    Set-Content $agentsPath $header -Encoding UTF8
} elseif ((Get-Content $agentsPath -Raw) -notmatch 'pi \+ Cursor SDK') {
    Add-Content $agentsPath "`n$header" -Encoding UTF8
}
& $Bin install-instructions --target $agentsPath
Write-Host "AGENTS.md: $agentsPath" -ForegroundColor Green

# --- 5) Cursor MCP já em ~/.cursor/mcp.json (SDK carrega) ---
Write-Host ""
Write-Host "MCP para pi+cursor:" -ForegroundColor Cyan
Write-Host "  O pi nativo nao usa ~/.omp/agent/mcp.json — o pi-cursor-sdk le ~/.cursor/mcp.json via Cursor SDK."
Write-Host "  Confirme: $env:USERPROFILE\.cursor\mcp.json contem ai-memory -> $ServerUrl/mcp"
Write-Host ""
Write-Host "Proximos passos:" -ForegroundColor Yellow
Write-Host "  1. ai-memory serve --transport http --enable-web"
Write-Host "  2. Reinicie o pi (extensoes carregam no startup)"
Write-Host "  3. pi --model cursor/composer-2.5"
Write-Host "  4. No projeto: cd C:\GIT\consisanet (ou seu repo) antes de abrir o pi"
