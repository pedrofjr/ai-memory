# Arquivo de handoffs (2026-06-19)

Handoffs que estavam `open` no SQLite global foram exportados, salvos aqui e **cancelados** (`memory_handoff_cancel` → `state=expired`).

## Conteúdo

| Arquivo | Descrição |
|---------|-----------|
| `INDEX.md` | Tabela com links para os 24 handoffs |
| `*.md` | Um arquivo por handoff (metadados + summary + open_questions + next_steps) |
| `_export.json` | Export JSON completo (backup machine-readable) |
| `_generate_md.py` / `_cancel_all.py` | Scripts usados na exportação/cancelamento |

## Uso

1. Abra `INDEX.md` para localizar o handoff por projeto/data.
2. Leia o `.md` correspondente para retomar o contexto manualmente.
3. Para reativar no ai-memory, use `memory_handoff_begin` ou `memory_write_page` com o conteúdo relevante — estes arquivos **não** reinjetam automaticamente.

## Verificação pós-cancelamento

- `pending_handoff_count`: **0** (confirmado via `memory_briefing` em 2026-06-19)
- Handoffs no SQLite: `state=expired` (não serão consumidos no `SessionStart`)