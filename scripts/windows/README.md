# ai-memory no Windows (sem Docker)

Scripts de apoio para operar o ai-memory a partir de um checkout no Windows.
Nada aqui faz parte do build do produto: o binário sai do `cargo` e o `/web` já
vem embutido.

## Pré-requisitos

- Rust 1.95+ (o repo traz `rust-toolchain.toml`)
- Git (a wiki é versionada)

## Setup rápido

```powershell
cd <raiz do clone>

# 1. Compilar o binário
.\scripts\windows\build.ps1

# 2. Subir o servidor (HTTP + MCP em :49374)
.\scripts\windows\serve.ps1
# Com UI web em /web:
.\scripts\windows\serve.ps1 -Web

# 3. Comando global no PATH
.\scripts\windows\install-global.ps1 -AddToPath
# Novo terminal: ai-memory
```

`build.ps1` compila só o binário (`cargo build --release --bin ai-memory`) e
limpa `target\debug` no fim. O CSS do Tailwind é vendorizado no repositório,
então o build não baixa nada.

## Carregamento de ambiente

`Import-AiMemoryEnv.ps1` lê `.env` e `.env.local` da raiz do clone e exporta as
variáveis para o processo atual; `serve.ps1` e os demais scripts o carregam antes
de chamar o binário. Sem `AI_MEMORY_DATA_DIR` definido, o padrão é
`%LOCALAPPDATA%\ai-memory`.

Se o data dir ainda não tem `config.toml`, `serve.ps1` roda `ai-memory init`
antes de subir.

## Comando global `ai-memory`

`install-global.ps1` grava `ai-memory.ps1` e `ai-memory.cmd` em
`%USERPROFILE%\.local\bin`. A raiz do repo é resolvida a partir da localização do
próprio script (sem caminho fixo); `-AddToPath` acrescenta a pasta ao PATH do
usuário.

| Comando | Ação |
|---------|------|
| `ai-memory` | Sobe o servidor HTTP em :49374 |
| `ai-memory serve --enable-web` | HTTP + UI em `/web` (o launcher injeta `--transport http` se faltar) |
| `ai-memory status --json` | Status via HTTP |
| `ai-memory search "termo"` | Busca FTS/híbrida |

Override do repo: `$env:AI_MEMORY_REPO = "D:\outro\ai-memory"`.

## Servidor no logon (tarefa agendada)

```powershell
.\scripts\windows\install-serve-autostart.ps1
```

Registra a tarefa `ai-memory-serve` com gatilho "ao fazer logon", roda
`serve.ps1 -Web` em janela oculta e já dispara a tarefa no fim. Reexecutar
re-registra.

O Agendador de Tarefas não consegue iniciar o alias MSIX de
`%LOCALAPPDATA%\Microsoft\WindowsApps` (é um stub/reparse point): o
`CreateProcess` falha com `0x80070002` e a tarefa nunca sobe. O script resolve o
`pwsh.exe` real dentro do pacote e, na falta dele, cai para o Windows PowerShell
5.1.

Remover: `Unregister-ScheduledTask -TaskName ai-memory-serve -Confirm:$false`.

## Acesso remoto por túnel (instância separada)

```powershell
.\scripts\windows\serve-remote.ps1 -AllowedHosts memory.exemplo.com
# tunnel apontando para 127.0.0.1:49380
```

Sobe uma segunda instância em `:49380` com `--enable-web --no-watcher` e bearer
token obrigatório, gerado em `~/.ai-memory/remote-token.txt`. A instância local
continua sem auth, então hooks e clientes MCP da máquina não mudam de
configuração.

Dois cuidados:

- **Host.** O servidor só aceita um `Host` presente em `AI_MEMORY_ALLOWED_HOSTS`
  (default `localhost`, `127.0.0.1`, `::1`) e responde `403 forbidden host` ao
  resto. Túneis que preservam o Host original (cloudflared, ngrok) precisam do
  hostname público em `-AllowedHosts`.
- **Dois processos, um data dir.** `--no-watcher` evita ingestão dupla do wiki,
  mas cada processo tem seu próprio scheduler de consolidação e writer actor
  sobre o mesmo SQLite. Use a instância remota para leitura e Web UI; escritas
  pesadas ficam na local.

## Backup do wiki

```powershell
.\scripts\windows\backup-push-wiki.ps1
```

Faz commit e push do repositório git do wiki (default `~/.ai-memory/wiki`). Cuida
de execuções sobrepostas (mutex), `index.lock` órfão e retry curto quando o
servidor está commitando ao mesmo tempo. O backup do banco não é este script: é
o `ai-memory backup`.

## Integração com OMP (snapcompact + modelos Cursor)

```powershell
.\scripts\windows\install-omp.ps1
```

Configura `~/.omp/agent`: extensão de captura, MCP, junction de skills, bloco no
`AGENTS.md`, `providers.cursor.modelOverrides` em `models.json` (visão para o
snapcompact) e `.env` com timeout de stream. Reaplique após upgrade do OMP.

## Cobertura de embeddings

```powershell
python .\scripts\windows\check-embed-coverage.py
```

Abre `~/.ai-memory/db/memory.sqlite` em modo somente-leitura e reporta a
cobertura por workspace comparando com o triple `provider/model/dim` esperado
(constantes no topo do arquivo). Não escreve nada.

## Rodar a suíte de testes

```powershell
cargo test --workspace --all-targets
```

No Windows, alguns testes de paridade dos hooks invocam `sh` para fazer source de
`hooks/_lib.sh`. Sem o `sh` do Git no PATH eles falham com `program not found`
(o resto da suíte passa). Para esta máquina:

```powershell
$env:PATH = "C:\Program Files\Git\bin;" + $env:PATH
```

## Issues para upstream

`docs/issues/` guarda os drafts de issue que o fork mantém para propor mudanças
ao upstream. A convenção aqui é proposta como issue, não PR imediato.
