Title: Páginas heurísticas de sessão crescem sem limite e impedem recuperação via consolidação LLM

**Versão**

Saída de `ai-memory --version`:

```
ai-memory 0.12.3
```

**Escopo: bug upstream vs setup do operador no fork**

Este relatório descreve **comportamento central do ai-memory** em `ai-memory-hooks` e `ai-memory-consolidate` que afeta **qualquer** usuário cuja consolidação LLM falha em uma sessão longa e que depois tenta `memory_consolidate` contra uma página heurística enorme no wiki. **Não** é pedido para corrigir uma integração privada do fork.

| Camada | O que é | Relevante para upstream? |
|--------|---------|--------------------------|
| `synth.rs` despeja todas as observações em `## Raw observations` sem limite | Upstream `ai-memory-hooks` | **Sim** |
| `build_request` limita obs do SQLite por janela mas anexa `current_body` do wiki inteiro | Upstream `ai-memory-consolidate` | **Sim** |
| Falha de consolidação no PreCompact é não fatal (`warn!`, sessão continua) | Router upstream `ai-memory-hooks` | **Sim** |
| `OBSERVATION_BUDGET_CHARS` dimensionado para modelos ~200K de contexto | Design upstream (comentários citam modelos classe Haiku) | **Sim** |
| Provider LLM Cursor via `cursor-bridge` Node + `scripts/windows/*` | **Somente fork** (não está no upstream) | Não — apenas transporte do operador |
| Mismatch serde `timeoutMs` no `cursor.rs` do fork | **Bug somente do fork** (corrigido localmente) | Não — mas **disparou** a armadilha no nosso ambiente |

Descobrimos isso via bridge Cursor específico do fork, mas a **falha de recuperação** é reproduzível no upstream puro com qualquer provider ~200K de contexto quando o PreCompact não tem sucesso.

**O que aconteceu**

Dois comportamentos acoplados no caminho SessionEnd → consolidate criam uma **armadilha de recuperação** para sessões longas e densas em hooks.

Páginas heurísticas de sessão existem como **entrada de checkpoint para o consolidador** — metadados, prompts, contagem de ferramentas e um índice completo de observações brutas que o LLM deve reescrever numa página consolidada. Não são a superfície durável que usuários leem no `/web` após a consolidação ter sucesso. O problema é que esse checkpoint pode crescer o suficiente para **quebrar a própria chamada de consolidação**, sendo ao mesmo tempo o artefato em que o operador precisa confiar quando a consolidação automática não rodou.

### 1. Páginas heurísticas crescem O(N) a cada observação

No **SessionEnd**, o ai-memory **sempre** grava uma página de sessão baseada em regras via `synthesize_session_page` (`ai-memory-hooks`). Além de metadados, prompts e contagem de chamadas de ferramenta, o corpo inclui **`## Raw observations`**: uma linha markdown por observação de hook da **sessão inteira**, sem limite:

```rust
// synth.rs
buf.push_str("## Raw observations\n\n");
for obs in observations {
    buf.push_str(&format!(
        "- `{}` @ {} — {}\n",
        kind,
        human_ts(&obs.created_at),
        obs.title.chars().take(80).collect::<String>(),
    ));
}
buf.push_str("\n_Synthesised by ai-memory (M3, no-LLM heuristic)._`\n");
```

Em trabalho de agente denso em hooks, o checkpoint cresce **linearmente com a contagem de observações**:

| Escala da sessão | Tamanho da página observado (nossos dados) |
|------------------|--------------------------------------------|
| ~11.204 observações | ~593 KB, ~11.470 linhas |
| Sessão longa interativa no pi (~40 min) | Mesma ordem de grandeza |

Não há guarda de tamanho antes de gravar a página nem antes de reenviá-la ao `memory_consolidate`.

### 2. `memory_consolidate` reenvia a página heurística inteira no prompt do LLM

Quando a consolidação LLM não roda durante a sessão (erro no PreCompact, timeout do provider, operador adia `memory_consolidate` até o fim da sessão), o SessionEnd materializa a página heurística enorme acima. Uma chamada posterior a **`memory_consolidate`** então não consegue ter sucesso dentro da janela de contexto do modelo, mesmo com o windowing de observações adicionado no v0.8.1+ para sessões longas.

Em `build_request` (`ai-memory-consolidate`), as **observações do SQLite** são limitadas a `OBSERVATION_BUDGET_CHARS` (400.000 chars — comentários upstream dimensionam isso para modelos ~200K de contexto, como Haiku 4.5), mas o `current_body` do wiki (a página heurística) é anexado **sem qualquer orçamento**:

```rust
// consolidator.rs — observações são limitadas por janela…
let (windowed, skipped) = window_observations_to_budget(observations, OBSERVATION_BUDGET_CHARS);

// …mas current_body entra inteiro:
if !current_body.trim().is_empty() {
    buf.push_str("\nCurrent (heuristic) page body:\n\n```\n");
    buf.push_str(current_body);  // sem limite
    buf.push_str("\n```\n");
}
```

**Evidência de duplicação no mesmo prompt:** o lado SQLite já renderiza observações (limitadas por janela) em `Observations (in order):`. O `current_body` heurístico tipicamente contém de novo as observações da mesma sessão em `## Raw observations` (kind, timestamp, título por linha). Os dois blocos estão presentes numa única requisição de consolidação quando existe página heurística no disco.

**Estimativa de tamanho do prompt (nossa sessão):**

- Observações SQLite (limitadas ao orçamento): até **400K chars** (~100K tokens a ~4 chars/token)
- `current_body` heurístico (sem limite): **~593 KB** (~148K tokens)
- System prompt + schema + reserva de `max_tokens: 32_000`

Esse total excede uma janela de **~200K tokens**. Nosso provider de consolidação é **`composer-2.5`** via bridge Cursor do fork (mesma classe de contexto que o orçamento dimensionado para Haiku no upstream). O windowing do v0.8.1 corrigiu o caso sabadell de 7.234 observações no **lado SQLite**; na nossa sessão o **corpo heurístico do wiki** também era enorme e foi reenviado inteiro.

`memory_consolidate` contra a heurística completa falhou com **erros do Cursor SDK** (`Cursor SDK request timed out after 120000ms`, depois `Cursor agent run cancelled`). Após dividir manualmente o corpo heurístico do wiki em duas partes (nenhuma observação descartada — mesmo conteúdo bruto, duas passadas de `write-page`), `memory_consolidate -Apply` em cada parte teve sucesso em ~87s + ~78s no mesmo caminho `composer-2.5`.

### 3. Falha no PreCompact é não fatal → armadilha se fecha

Falhas de consolidação no PreCompact são **não fatais** (`warn!` e continua em `router.rs`):

```
PreCompact consolidation failed; continuing
```

A sessão segue acumulando observações; o SessionEnd sempre grava o dump heurístico completo; depois o `memory_consolidate` não cabe no prompt. **Armadilha de recuperação:** consolidação falhou → heurística enorme → consolidar de novo → ainda grande demais.

Veja **Ambiente do operador** abaixo para como o bridge Cursor do fork disparou a falha inicial no PreCompact; as seções 2–3 valem independentemente do provider.

**O que eu esperava**

1. Depois que o SessionEnd grava um checkpoint heurístico completo (incluindo o dump integral de `## Raw observations`), o **`memory_consolidate` ainda deveria conseguir processar essa sessão** — inclusive quando a consolidação é rodada manualmente depois porque o PreCompact ou uma passada automática anterior falhou.
2. O windowing de observações (adicionado para o modo de falha sabadell de 7.234 observações) não deveria dar falsa impressão de que sessões longas estão cobertas quando o `current_body` heurístico também entra inteiro.
3. Uma sessão longa deveria permanecer recuperável sem o operador ter que editar markdown do wiki na mão fora das ferramentas suportadas.

**Passos para reproduzir (upstream — fork não necessário)**

1. Compilar o `ai-memory` stock com qualquer provider LLM configurado para modelo **~200K de contexto** (ex.: `AI_MEMORY_LLM_PROVIDER=anthropic` + Haiku 4.5).
2. Rodar sessão de agente que emita **5.000+ observações de hook** numa única sessão (uso denso de ferramentas basta).
3. Garantir que a **consolidação LLM no PreCompact não tenha sucesso** (desconfigurar provider temporariamente, forçar timeout ou bloquear a chamada LLM — falhas são não fatais).
4. Encerrar a sessão → inspecionar `wiki/.../sessions/<session-id>.md`:
   - Seção grande `## Raw observations` (~centenas de KB em sessões densas)
   - Rodapé `_Synthesised by ai-memory (M3, no-LLM heuristic)._`
   - Sem `consolidated: true`
5. Restaurar provider e chamar `memory_consolidate` para esse `session_id`.
6. Observar falha de consolidação: contexto do provider excedido, timeout opaco ou requisição travada. Tamanho do prompt excede ~200K tokens quando corpo heurístico + obs limitadas + reserva de 32K de saída são somados.

**Passos para reproduzir (caminho do nosso fork — opcional)**

Igual ao acima, mas com `AI_MEMORY_LLM_PROVIDER=cursor`, `CURSOR_MODEL=composer-2.5` do fork e bridge Node (veja Ambiente do operador). PreCompact falhou aos 120s por bug serde `timeoutMs` no fork até corrigido localmente; SessionEnd gravou heurística de ~593 KB; `memory_consolidate` falhou até dividirmos manualmente o corpo do wiki em duas passadas de consolidação.

**Ambiente**

- SO: Windows 11 (lógica é agnóstica de provider; repro em qualquer SO)
- Docker?: não (`ai-memory serve` local)
- Agente: [pi](https://github.com/earendil-works/pi) com `pi-cursor-sdk` e lifecycle hooks (alto volume de `pre-tool-use` / `post-tool-use`)
- Transporte: HTTP hooks + MCP HTTP (`http://127.0.0.1:49374`)

**Ambiente do operador** (fork — como descobrimos; não necessário para repro upstream)

| Item | Valor |
|------|-------|
| Base upstream | `akitaonrails/ai-memory` @ `0.12.3` |
| Delta do fork | Provider LLM Cursor (`crates/ai-memory-llm/src/cursor.rs`), `scripts/cursor-bridge/` (worker Node `worker.mjs`), `scripts/windows/*` |
| Binário | Build release do fork local (`target\release\ai-memory.exe`) |
| SO / shell | Windows 11, PowerShell 7 |
| Data dir | `C:\Users\pedro.ailton\.ai-memory` (`AI_MEMORY_DATA_DIR`) |
| Serve | `http://127.0.0.1:49374`, iniciado via `scripts\windows\serve.ps1` (carrega `.env` + `.env.local` opcional) |
| Agente | [pi](https://github.com/earendil-works/pi) + `pi-cursor-sdk` + lifecycle hooks; a superfície modular de hooks/MCP do pi integra-se limpo com o ai-memory |
| Consolidação LLM | `AI_MEMORY_LLM_PROVIDER=cursor`, `CURSOR_MODEL=composer-2.5` (~200K contexto), `CURSOR_TIMEOUT_MS=600000`, `CURSOR_AGENT_CWD` apontando para o repo ativo |
| Fluxo do bridge Cursor | Rust `CursorSdkProvider` dispara `node scripts/cursor-bridge/worker.mjs` com JSON no stdin; worker chama Cursor SDK via `pi-cursor-sdk`; **não** faz parte do upstream |
| Embeddings | OpenRouter via `AI_MEMORY_EMBED_*` (separado da consolidação) |
| Projeto | `default/consisanet` |
| Sessão | `019e8375-411a-7ea6-8cbc-02781b3fb04a` — 11.204 observações, página heurística ~593 KB |
| Caminho wiki | `%USERPROFILE%\.ai-memory\wiki\<workspace-uuid>\<project-uuid>\sessions\019e8375-….md` |

**Fator contribuinte somente do fork (corrigido localmente):** o worker do bridge lê `input.timeoutMs` (camelCase) mas o Rust `BridgeRequest` serializava `timeout_ms` (snake_case), então `CURSOR_TIMEOUT_MS=600000` configurado era ignorado e PreCompact/`memory_consolidate` batiam no **default de 120s** do worker. Correção local: `#[serde(rename = "timeoutMs")]` no `cursor.rs` do fork. Essa falha de timeout deixou só a página heurística e expôs a armadilha de recuperação upstream; não explica por que a consolidação ainda falha quando a heurística tem centenas de KB.

**Logs relevantes**

Na falha do PreCompact (não fatal, router upstream):

```
PreCompact consolidation failed; continuing
```

No SessionEnd:

```
session ended; summary page + open handoff created
```

No `memory_consolidate` com heurística completa (caminho Cursor do fork):

```
provider error 1: Cursor SDK request timed out after 120000ms
```

```
provider error 1: Cursor agent run cancelled
```

Após dividir `current_body` em duas partes (workaround do operador, dados preservados), o mesmo caminho `composer-2.5` teve sucesso nas duas passadas.

**Contexto adicional**

- `crates/ai-memory-hooks/src/synth.rs` — `render_body`, loop `## Raw observations` sem limite
- `crates/ai-memory-hooks/src/router.rs` — PreCompact `consolidate_or_synth` (fail-open) vs heurística no SessionEnd + opt-in `AI_MEMORY_CONSOLIDATE_ON_SESSION_END`
- `crates/ai-memory-consolidate/src/consolidator.rs` — `build_request`, `OBSERVATION_BUDGET_CHARS` (400K), `window_observations_to_budget`, `current_body` sem limite, `max_tokens: 32_000`
- Comentário de windowing do v0.8.1 referencia sessão sabadell de 7.234 observações; este relatório é o modo de falha complementar quando o **corpo heurístico do wiki** também é enorme
- Issue anterior `issue.md` (docs SessionEnd vs comportamento heurístico) — complementar; aquela é sobre **wording**; esta é sobre **checkpoint sem limite quebrando recuperação via consolidação**
- Provider Cursor do fork existe só no nosso fork; usuários upstream em Anthropic/OpenAI-compat batem na mesma lógica `build_request` / `synth.rs` com providers nativos

**Perguntas para os mantenedores**

1. A interação entre windowing de observações em `build_request` e o `current_body` sem limite é intencional?
2. `memory_consolidate` manual após um SessionEnd heurístico completo deveria ser caminho de recuperação suportado para sessões longas?
3. O projeto tem interesse em aceitar um PR dedicado ao provider Cursor bridge para que mais usuários — em especial assinantes do Cursor — possam utilizá-lo como backend LLM? O relator usa essa stack em produção desde o ai-memory ~0.2, o que garante no mínimo um mês inteiro de uso real. Ou a preferência é aguardar uma API oficial do Cursor?
