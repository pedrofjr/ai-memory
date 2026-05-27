# ai-memory no Windows (sem Docker)

## Pré-requisitos

- [Rust 1.95+](https://rustup.rs/) (o repo traz `rust-toolchain.toml`)
- Git (wiki versionada)

## Setup rápido

```powershell
cd C:\GIT\ai-memory

# 1. Compilar (instala @cursor/sdk no bridge na primeira vez)
.\scripts\windows\build.ps1
# Ou só o bridge: .\scripts\windows\install-cursor-bridge.ps1

# 2. Inicializar dados (%LOCALAPPDATA%\ai-memory)
.\scripts\windows\init-data.ps1

# 3. (Opcional) LLM — copie e edite chaves
Copy-Item .env.local.example .env.local
# Comentários em português (fork): Copy-Item .env.local.example-ptbr .env.local

# 4. Subir servidor
.\scripts\windows\serve.ps1
# Com UI web:
.\scripts\windows\serve.ps1 -Web

# 5. Integrar Cursor (outro terminal, com serve rodando)
.\scripts\windows\install-cursor.ps1

# 6. Comando global no PATH (como agentmemory)
.\scripts\windows\install-global.ps1 -AddToPath
# Novo terminal: ai-memory
```

## Comando global `ai-memory`

```powershell
.\scripts\windows\install-global.ps1 -AddToPath
```

Instala em `%USERPROFILE%\.local\bin\` e carrega `C:\GIT\ai-memory\.env` (+ `.env.local` se existir).

| Comando | Ação |
|---------|------|
| `ai-memory` | Sobe o servidor HTTP em :49374 |
| `ai-memory serve --enable-web` | HTTP + UI em `/web` (o launcher injeta `--transport http` se faltar) |
| `ai-memory status --json` | Status via HTTP |
| `ai-memory search "termo"` | Busca FTS/híbrida |

Override do repo: `$env:AI_MEMORY_REPO = "D:\outro\ai-memory"`

## Migrar do agentmemory

Com o **agentmemory** rodando em `:3111` e o **ai-memory** em `:49374`:

```powershell
.\scripts\windows\import-from-agentmemory.ps1 -StartServer
# Incluir log de observações por sessão (opcional, mais lento):
.\scripts\windows\import-from-agentmemory.ps1 -StartServer -IncludeObservations
ai-memory embed
```

Origem: `%USERPROFILE%\.agentmemory` (API REST). Destino: páginas em `imported/memories/` e `imported/lessons/` por projeto (`consisanet`, `agentmemory`, `global`, etc.).

## Piloto de consolidação (inventário)

Após gerar o inventário em `%USERPROFILE%\.ai-memory\runs\consolidate-inventory-*\`:

```powershell
cd C:\GIT\ai-memory
# Servidor + LLM em .env.local; por padrão só dry-run (não grava)

# A) Sessões heurísticas → memory_consolidate (MCP)
.\scripts\windows\pilot-consolidate-sessions.ps1 -StartServer

# B) Imported agentmemory → LLM + write-page (não usa memory_consolidate)
.\scripts\windows\pilot-rewrite-imported.ps1 -StartServer

# Gravar de verdade (depois de revisar a saída):
.\scripts\windows\pilot-consolidate-sessions.ps1 -StartServer -Apply
.\scripts\windows\pilot-rewrite-imported.ps1 -StartServer -Apply
```

Logs JSONL na mesma pasta do inventário (`pilot-sessions-*.jsonl`, `pilot-imported-*.jsonl`). O piloto imported **não apaga** `imported/` — só cria páginas novas em `facts/` etc.

### Lote + correção de acentos (mojibake)

O `batch-rewrite-imported.ps1` pode gravar texto com `├º` / `ÔÇ` se o stdout do `llm-test` passar pelo `ConvertFrom-Json` do PowerShell. Para corrigir **sem novo LLM** (texto do backup do inventário):

```powershell
# Servidor em :49374
python .\scripts\windows\repair-encoding-from-backup.py --apply
```

Reescrita com LLM em UTF-8 seguro (quando `llm-test --provider cursor` estiver OK):

```powershell
python .\scripts\windows\repair-rewrite-encoding.py --apply --throttle 45
```

Detecção local apenas: `python .\scripts\windows\fix-wiki-encoding.py`

## Portas

| Serviço | URL |
|---------|-----|
| MCP + hooks | `http://127.0.0.1:49374` |
| MCP endpoint | `http://127.0.0.1:49374/mcp` |
| Web UI (`-Web`) | `http://127.0.0.1:49374/web` |

## Provider Cursor (Composer local)

No `.env.local`:

```powershell
AI_MEMORY_LLM_PROVIDER=cursor
CURSOR_API_KEY=...
CURSOR_MODEL=composer-2.5
CURSOR_AGENT_CWD=C:\GIT\seu-projeto
```

Requer `scripts/cursor-bridge` com `npm install` (o `build.ps1` faz isso na primeira vez).

Teste: `.\target\release\ai-memory.exe llm-test --provider cursor --model composer-2.5 --prompt "ping"`

## Embeddings Google (Gemini)

```powershell
AI_MEMORY_EMBEDDING_PROVIDER=google
AI_MEMORY_EMBEDDING_MODEL=gemini-embedding-001
AI_MEMORY_EMBEDDING_DIM=768
GEMINI_API_KEY=...
```

Ver: https://ai.google.dev/gemini-api/docs/embeddings

## Build note

No Windows o crate `ai-memory-web` usa `TAILWIND_SKIP=1` (CSS já em `crates/ai-memory-web/static/tailwind.css`). Sem isso o build tenta baixar o CLI Tailwind e falha na verificação de checksum.

## pi + pi-cursor-sdk

O **pi** usa `~/.pi/agent/` (não `~/.omp/`). Com modelos **cursor** via `pi-cursor-sdk`, o Cursor SDK carrega MCP e settings do Cursor automaticamente.

### O que cada camada faz

| Camada | Faz sozinho | Você precisa configurar |
|--------|-------------|-------------------------|
| **pi-cursor-sdk** | Modelos Cursor (`@cursor/sdk`), MCP de `~/.cursor/mcp.json`, settings Cursor (`PI_CURSOR_SETTING_SOURCES=all`) | `CURSOR_API_KEY` ou `/login` no pi |
| **ai-memory (MCP)** | Ferramentas `memory_query`, `memory_recent`, … via MCP HTTP | `ai-memory serve --transport http` rodando; entrada em `~/.cursor/mcp.json` |
| **ai-memory (captura)** | Grava prompts/tools via hooks HTTP | Extensão `~/.pi/agent/extensions/ai-memory.ts` |
| **pi (skills)** | `/skill:nome` a partir de `~/.pi/agent/skills` | Junction/cópia das skills do Cursor |
| **cursor-context-bridge** | Injeta `~/.cursor/rules/*.mdc` no system prompt | Gerado por `install-pi.ps1` |

O pi **não** executa `~/.cursor/hooks.json` — a captura no terminal depende da extensão `ai-memory.ts`.

### Instalar

```powershell
cd C:\GIT\ai-memory
.\scripts\windows\install-pi.ps1
```

Cria/atualiza:

- `~/.pi/agent/extensions/ai-memory.ts` (agent=`pi`)
- `~/.pi/agent/extensions/cursor-context-bridge.ts`
- `~/.pi/agent/skills` → junction para `~/.cursor/skills`
- `~/.pi/agent/AGENTS.md` (português, RTK, Delphi, bloco ai-memory)

### Uso diário

```powershell
# Terminal 1
ai-memory serve --transport http

# Terminal 2 — no diretório do projeto
cd C:\GIT\consisanet
pi --model cursor/composer-2.5
```

Reinicie o pi após mudar extensões. Para UI web: `ai-memory serve --transport http --enable-web`.

## CLI direto

```powershell
$env:TAILWIND_SKIP = "1"
.\target\release\ai-memory.exe status --json
.\target\release\ai-memory.exe search "karpathy"
```
