# Pull requests para o upstream (akitaonrails/ai-memory)

Este documento descreve como publicar, a partir deste fork, as alterações que
fazem sentido no repositório original — separadas do fluxo Windows/pi pessoal.

## Branches locais

| Branch | Base | Conteúdo | Prioridade |
|---|---|---|---|
| `pr/fix-store-mcp-handoff-fts` | `upstream/main` | V07 `omp`, handoff `effective_ids`, FTS5 `:` | **1 — enviar primeiro** |
| `pr/feat-bootstrap-chunks` | `upstream/main` | Prune antes do POST + chunks LLM no bootstrap | **2** |
| `pr/fix-embed-stale-purge` | `upstream/main` | Re-embed all projects + purge stale `page_embeddings` | **3** |
| `pr/fix-llm-embed-input` | `upstream/main` | Truncamento + OpenAI-compat/OpenRouter + retry 429 | **4** |
| `pr/feat-google-embeddings` | `upstream/main` | `GoogleEmbedder` (`embedContent`) — sem Cursor/scripts | opcional (upstream pode já ter) |

Commits de referência no fork (após rebase em 4 commits):

- `a2c65ab` → branch ①
- `9772e9f` → branch ②
- `d5da983` (parcial) → branch ③

## O que **não** vai para upstream (ficar no fork)

- `e9d22fa` — `install-pi.ps1`, `import-from-agentmemory.ps1`, `HANDOFF.md`
- `scripts/cursor-bridge/`, `cursor.rs` (provider LLM via Node bridge)
- `scripts/windows/*` além do que o upstream já documenta em `docs/windows.md`

---

## PR ① — `fix(store,mcp): omp, handoff namespace, FTS5 colon queries`

### Problema

1. **OMP/pi hooks** gravam `agent_kind = 'omp'`, mas o CHECK do V01 só aceita
   `claude-code|codex|open-code|other` → WARN em todo `POST /hook`.
2. **Handoff MCP** usava `self.project_id` fixo do `serve --project`, enquanto
   leituras usam `effective_ids()` (ActiveProject) → `pending_handoff_count: 0`
   com handoff aberto em outro namespace.
3. **`memory_query`** com texto tipo `pick: handoff` → FTS5 interpreta `pick:` como
   coluna → `no such column: pick`.

### Arquivos

- `crates/ai-memory-store/migrations/V07__agent_kind_omp.sql`
- `crates/ai-memory-store/src/fts_query.rs`
- `crates/ai-memory-store/src/reader.rs`, `lib.rs`, `ops.rs`
- `crates/ai-memory-mcp/src/server.rs`

### Testes

```powershell
$env:TAILWIND_SKIP = '1'
rtk cargo test -p ai-memory-store search_colon -p ai-memory-mcp handoff_begin_pending
```

### Criar branch e push

```powershell
cd C:\GIT\ai-memory
rtk git fetch upstream
rtk git checkout -B pr/fix-store-mcp-handoff-fts upstream/main
rtk git cherry-pick a2c65ab
# resolver conflitos se houver; depois:
rtk git push -u origin pr/fix-store-mcp-handoff-fts
rtk gh pr create --repo akitaonrails/ai-memory --head pedrofjr:pr/fix-store-mcp-handoff-fts --base main --title "fix(store,mcp): omp agent_kind, handoff effective_ids, FTS5 colon queries" --body-file docs/pr-bodies/fix-store-mcp.md
```

---

## PR ② — `feat(bootstrap): chunk large bundles and prune before POST`

### Problema

Repositórios com dezenas de milhares de commits (ex. Consisanet) geram:

- **413** — corpo JSON > 10 MiB antes do prune no servidor
- **502** — uma única chamada LLM com ~150k tokens estoura o provider / Cursor bridge

### Solução

- CLI aplica `prune_sources_to_budget` **antes** do POST
- Servidor divide fontes em chunks sequenciais (`--chunk-input-tokens`, default 24k)
- Rotas `/admin/*` aceitam até 32 MiB; hooks/MCP permanecem em 10 MiB

### Arquivos principais

- `crates/ai-memory-consolidate/src/bootstrap.rs`
- `crates/ai-memory-cli/src/commands/bootstrap.rs`, `cli.rs`
- `crates/ai-memory-cli/src/commands/serve.rs`
- `crates/ai-memory-mcp/src/admin.rs`
- `docs/install.md`

### Testes

```powershell
$env:TAILWIND_SKIP = '1'
rtk cargo test -p ai-memory-consolidate plan_chunks -p ai-memory-mcp dry_run
```

### Criar branch e push

```powershell
rtk git checkout -B pr/feat-bootstrap-chunks upstream/main
rtk git cherry-pick 9772e9f
rtk git push -u origin pr/feat-bootstrap-chunks
rtk gh pr create --repo akitaonrails/ai-memory --head pedrofjr:pr/feat-bootstrap-chunks --base main --title "feat(bootstrap): chunk large bundles and prune before POST" --body-file docs/pr-bodies/feat-bootstrap-chunks.md
```

---

## PR ③ — `fix(embed,store): re-embed all projects and purge stale embeddings`

Branch: `pr/fix-embed-stale-purge` (1 commit, só Rust).

```powershell
rtk git fetch upstream
rtk git push -u origin pr/fix-embed-stale-purge
rtk gh pr create --repo akitaonrails/ai-memory `
  --head pedrofjr:pr/fix-embed-stale-purge --base main `
  --title "fix(embed,store): re-embed all projects and purge stale embeddings" `
  --body-file docs/pr-bodies/fix-embed-stale-purge.md
```

Testes: `rtk cargo test -p ai-memory-store delete_stale`

**Fork-only (não na branch):** `scripts/windows/inspect-db.py`, `repair-fork-v7-migration.py`

---

## PR ④ — `fix(llm): truncate embedding input and harden OpenAI-compatible calls`

Branch: `pr/fix-llm-embed-input` (1 commit; `config.rs` só embedding, sem Cursor).

```powershell
rtk git push -u origin pr/fix-llm-embed-input
rtk gh pr create --repo akitaonrails/ai-memory `
  --head pedrofjr:pr/fix-llm-embed-input --base main `
  --title "fix(llm): truncate embedding input and harden OpenAI-compatible calls" `
  --body-file docs/pr-bodies/fix-llm-embed-input.md
```

Testes:

```powershell
$env:TAILWIND_SKIP = '1'
rtk cargo test -p ai-memory-llm truncate_for_embedding
rtk cargo test -p ai-memory-cli openai_embedding_falls_back
```

**Depende de:** nada (complementa PR ③; pode mergear em paralelo).

---

## PR ⑤ — `feat(llm): Google Gemini embeddings via embedContent API`

### Escopo **deliberadamente estreito**

Upstream já tem **Gemini LLM** (`gemini.rs`). Este PR adiciona apenas o
**embedder** `GoogleEmbedder` para `AI_MEMORY_EMBEDDING_PROVIDER=google|gemini`.

**Fora do escopo:** Cursor bridge, scripts Windows, `.env.local.example-ptbr` (fork).

### Arquivos

- `crates/ai-memory-llm/src/google.rs` (novo)
- `crates/ai-memory-llm/src/factory.rs`, `lib.rs`, `embedding.rs`
- `crates/ai-memory-cli/src/config.rs` (só ramo de embedding Google)
- `crates/ai-memory-consolidate/tests/embeddings.rs`, `evals/src/main.rs` (se aplicável)

### Criar branch

A branch `pr/feat-google-embeddings` é montada manualmente (cherry-pick parcial
do commit `d5da983`), não use `cherry-pick` cego do commit inteiro.

```powershell
rtk git checkout -B pr/feat-google-embeddings upstream/main
# arquivos aplicados pelo maintainer deste fork — ver histórico da branch
rtk git push -u origin pr/feat-google-embeddings
```

---

## Checklist antes de cada PR

- [ ] `rtk cargo fmt --all`
- [ ] `$env:TAILWIND_SKIP='1'; rtk cargo test -p <crates afetados>`
- [ ] `$env:TAILWIND_SKIP='1'; rtk cargo clippy -p <crates> -- -D warnings`
- [ ] Atualizar `CHANGELOG.md` **Unreleased** (só no PR que o maintainer pedir)
- [ ] PR em inglês no GitHub; descrição com **problema → solução → test plan**

## Ordem de merge sugerida

1. PR ① (store/MCP/handoff) — sem dependência
2. PR ② (bootstrap) — independente de ①
3. PR ③ (embed migration) — após warn-on-mismatch já no upstream
4. PR ④ (embed input/LLM) — independente de ③; pode ir em paralelo
5. PR ⑤ (Google embed) — opcional se upstream ainda não tiver retry/truncamento
