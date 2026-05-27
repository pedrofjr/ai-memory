Title: SessionEnd: docs/MCP implicam consolidação LLM mas o hook só executa síntese baseada em regras

**Versão**

Saída de `ai-memory --version`:

```
ai-memory 0.3.2
```

**O que aconteceu**

Documentação e instruções voltadas ao agente (README, `MEMORY_INSTRUCTIONS`, descrições de ferramentas MCP, snippets de roteamento em `CLAUDE.md` / `AGENTS.md`, partes de `docs/design-decisions.md`) afirmam que o **SessionEnd auto-consolida** sessões via LLM (estilo `memory_consolidate`).

Na prática, o **SessionEnd** só executa **síntese baseada em regras** (`synthesize_session_page` em `ai-memory-hooks`): prompts, contagem de chamadas de ferramenta e **observações brutas completas** (uma linha por hook). Isso produz arquivos `sessions/<id>.md` grandes (ex.: ~100 KB em sessões longas). A web UI (`/web`) renderiza o corpo da página como está, então a parte “legível” fica só nas seções do topo.

A **consolidação LLM** hoje roda em:

- Hook **PreCompact** (quando um provider LLM está configurado) — checkpoint antes da compactação de contexto
- Ferramenta MCP **`memory_consolidate`** — manual
- **Não** no SessionEnd (upstream não tem flag de env para “rodar consolidação LLM quando a sessão termina”)

**O que eu esperava**

Ou:

1. SessionEnd roda consolidação LLM quando um provider está configurado (como várias docs implicam), **ou**
2. Docs e instruções MCP deixam claro que SessionEnd grava uma página de sessão **heurística** por padrão, e consolidação LLM é **manual**, **PreCompact**, ou atrás de um opt-in explícito.

Páginas de sessão grandes no `/web` são preocupação de UX separada; o bug principal aqui é **texto de documentação/produto vs comportamento do hook**.

**Correção sugerida (para decisão do mantenedor)**

Manter comportamento de captura atual por padrão, tornar LLM no fim da sessão explícito e alinhar todo texto voltado ao usuário.

| Preocupação | Proposta |
|--------|----------|
| **SessionEnd padrão** | Continuar gravando a página de sessão completa baseada em regras (incluindo observações brutas) — sem truncar silenciosamente dados de hook |
| **LLM no SessionEnd** | Env opt-in, ex. `AI_MEMORY_CONSOLIDATE_ON_SESSION_END=true`, chamando `consolidate_session` quando provider configurado; em falha LLM, logar warning e cair para página heurística |
| **Fan-out multi-página** | Segunda flag opcional, ex. `AI_MEMORY_CONSOLIDATE_MULTI_PAGE_ON_SESSION_END=true`, para fan-out estilo M7b no SessionEnd |
| **Documentação** | Atualizar `ARCHITECTURE.md`, `design-decisions.md`, `usage.md`, snippets de roteamento e descrições de ferramentas MCP para refletir o padrão escolhido |

Caminhos alternativos que vocês podem preferir:

- **Só docs:** mudar toda redação de “auto-consolidar no fim da sessão” para descrever SessionEnd heurístico + PreCompact/MCP manual (sem novas flags de env).
- **Mudança de comportamento:** rodar consolidação LLM em todo SessionEnd quando `AI_MEMORY_LLM_PROVIDER` estiver setado (bate com docs antigas, maior custo/latência no fim da sessão).

**Passos para reproduzir**

1. Configurar `AI_MEMORY_LLM_PROVIDER` e rodar `ai-memory serve` com hooks instalados.
2. Rodar sessão de agente que emita muitas observações de hook, depois encerrar a sessão.
3. Abrir `wiki/.../sessions/<session-id>.md` ou `/web` → página mostra **Raw observations** com milhares de linhas.
4. Comparar com docs / descrição de `memory_consolidate` (“geralmente automático no fim da sessão”).
5. Observar: reescrita LLM ocorre no **PreCompact** ou via **`memory_consolidate`**, não no SessionEnd no upstream atual.

**Ambiente**

- SO: Windows 11 (também reproduzível em Linux/macOS — caminho de hooks é cross-platform)
- Docker?: não (`ai-memory serve` local)
- Agente: Cursor / Claude Code / outro com lifecycle hooks
- Transporte: HTTP hooks + MCP HTTP (`http://127.0.0.1:49374`)

**Logs relevantes**

Com LLM configurado e comportamento padrão de SessionEnd, o serve pode logar algo como:

```
memory_consolidate + PreCompact LLM checkpointing enabled
```

No SessionEnd:

```
session ended; summary page + open handoff created
```

Nenhuma consolidação LLM roda a menos que PreCompact tenha disparado ou `memory_consolidate` tenha sido chamado manualmente.

**Contexto adicional**

- `crates/ai-memory-hooks/src/synth.rs` — página de sessão heurística
- `crates/ai-memory-hooks/src/router.rs` — ramos `SessionEnd` vs `PreCompact`
- `docs/design-decisions.md` — referencia consolidação no fim da sessão em trechos que não batem com o router de hooks
- `/web` não pagina nem colapsa seções longas de **Raw observations** (pode ser issue de follow-up sem mudar captura)

**Perguntas para os mantenedores**

1. Qual padrão vocês querem: só SessionEnd heurístico, ou LLM em todo SessionEnd quando provider existe?
2. Se heurístico por padrão, flag de env opt-in é aceitável, ou SessionEnd nunca deveria chamar LLM automaticamente?
3. `design-decisions.md` deveria documentar explicitamente PreCompact + MCP manual como caminhos primários de consolidação LLM?
4. Interesse em issue separada para apresentação no `/web` de páginas de sessão grandes?
