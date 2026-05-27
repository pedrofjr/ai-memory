Title: Heuristic session pages grow unbounded and break LLM consolidation recovery

**Version**

Output of `ai-memory --version`:

```
ai-memory 0.12.3
```

**Scope: upstream bug vs fork operator setup**

This report describes **core ai-memory behaviour** in `ai-memory-hooks` and `ai-memory-consolidate` that affects **any** user whose LLM consolidation fails on a long session and who then tries `memory_consolidate` against a huge heuristic wiki page. It is **not** a request to fix a private fork integration.

| Layer | What it is | Relevant to upstream? |
|-------|------------|----------------------|
| `synth.rs` dumps all observations into `## Raw observations` with no cap | Upstream `ai-memory-hooks` | **Yes** |
| `build_request` window-budgets SQLite obs but appends full wiki `current_body` | Upstream `ai-memory-consolidate` | **Yes** |
| PreCompact consolidation failure is non-fatal (`warn!`, session continues) | Upstream `ai-memory-hooks` router | **Yes** |
| `OBSERVATION_BUDGET_CHARS` sized for ~200K-context models | Upstream design (comments cite Haiku-class models) | **Yes** |
| Cursor LLM provider via Node `cursor-bridge` + `scripts/windows/*` | **Fork-only** (not in upstream) | No — operator transport only |
| `timeoutMs` serde mismatch in fork `cursor.rs` | **Fork-only bug** (fixed locally) | No — but it **triggered** the trap in our environment |

We discovered this through a fork-specific Cursor bridge, but the **recovery failure** is reproducible on stock upstream with any ~200K-context provider once PreCompact does not succeed.

**What happened**

Two coupled behaviours in the SessionEnd → consolidate path create a **recovery trap** for long, hook-dense agent sessions.

Heuristic session pages exist as **checkpoint input for the consolidator** — metadata, prompts, tool counts, and a full raw observation index the LLM is meant to rewrite into a consolidated page. They are not the durable surface users read in `/web` after consolidation succeeds. The problem is that this checkpoint can grow large enough to **break the consolidation call itself**, while still being the artefact an operator must rely on when automatic consolidation did not run.

### 1. Heuristic pages grow O(N) with every observation

On **SessionEnd**, ai-memory **always** writes a rule-based session page via `synthesize_session_page` (`ai-memory-hooks`). Besides metadata, prompts, and tool-call counts, the body includes **`## Raw observations`**: one markdown line per hook observation for the **entire session**, with no cap:

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

For hook-dense agent work the checkpoint grows **linearly with observation count**:

| Session scale | Observed page size (our data) |
|---------------|-------------------------------|
| ~11,204 observations | ~593 KB, ~11,470 lines |
| Long interactive pi session (~40 min) | Same order of magnitude |

There is no size guard before writing the page or before feeding it back into `memory_consolidate`.

### 2. `memory_consolidate` re-feeds the entire heuristic page into the LLM prompt

When LLM consolidation does not run during the session (PreCompact error, provider timeout, operator defers `memory_consolidate` until session end), SessionEnd materialises the huge heuristic page above. A later **`memory_consolidate`** call then cannot succeed within the model's context window, even though v0.8.1+ added observation windowing for long sessions.

In `build_request` (`ai-memory-consolidate`), **SQLite observations** are budgeted to `OBSERVATION_BUDGET_CHARS` (400,000 chars — upstream comments size this for ~200K-context models such as Haiku 4.5), but the wiki `current_body` (the heuristic page) is appended **without any budget**:

```rust
// consolidator.rs — observations are windowed…
let (windowed, skipped) = window_observations_to_budget(observations, OBSERVATION_BUDGET_CHARS);

// …but current_body is included in full:
if !current_body.trim().is_empty() {
    buf.push_str("\nCurrent (heuristic) page body:\n\n```\n");
    buf.push_str(current_body);  // unbounded
    buf.push_str("\n```\n");
}
```

**Evidence of duplication in the same prompt:** the SQLite side already renders observations (windowed) under `Observations (in order):`. The heuristic `current_body` typically contains the same session's observations again under `## Raw observations` (kind, timestamp, title per line). Both blocks are present in one consolidation request when a heuristic page exists on disk.

**Prompt size estimate (our session):**

- SQLite observations (windowed to budget): up to **400K chars** (~100K tokens at ~4 chars/token)
- Heuristic `current_body` (unbounded): **~593 KB** (~148K tokens)
- System prompt + schema + `max_tokens: 32_000` reservation

That total exceeds a **~200K-token** window. Our consolidation provider is **`composer-2.5`** via a fork Cursor bridge (same context class as the Haiku-sized budget upstream targets). v0.8.1 windowing fixed the sabadell 7,234-observation case on the **SQLite side**; in our session the **wiki heuristic body** was also huge and fed back wholesale.

`memory_consolidate` against the full heuristic failed with **Cursor SDK errors** (`Cursor SDK request timed out after 120000ms`, later `Cursor agent run cancelled`). After manually splitting the heuristic wiki body into two parts (no observations dropped — same raw content, two `write-page` passes), `memory_consolidate -Apply` on each part succeeded in ~87s + ~78s on the same `composer-2.5` path.

### 3. PreCompact failure is non-fatal → trap closes

PreCompact consolidation failures are **non-fatal** (`warn!` and continue in `router.rs`):

```
PreCompact consolidation failed; continuing
```

The session keeps accumulating observations; SessionEnd always writes the full heuristic dump; later `memory_consolidate` cannot fit the prompt. **Recovery trap:** consolidation failed → huge heuristic → consolidate again → still too large.

See **Operator environment** below for how our fork Cursor bridge triggered the initial PreCompact failure; sections 2–3 remain regardless of provider.

**What I expected**

1. After SessionEnd writes a full heuristic checkpoint (including the complete `## Raw observations` dump), **`memory_consolidate` should still be able to process that session** — including when consolidation is run manually later because PreCompact or an earlier automatic pass failed.
2. Observation windowing (added for the sabadell 7,234-observation failure mode) should not give a false sense that long sessions are covered when the heuristic `current_body` is also included in full.
3. A single long session should remain recoverable without the operator having to hand-edit wiki markdown outside supported tooling.

**Steps to reproduce (upstream — no fork required)**

1. Build stock `ai-memory` with any LLM provider configured for a **~200K-context** model (e.g. `AI_MEMORY_LLM_PROVIDER=anthropic` + Haiku 4.5).
2. Run an agent session that emits **5,000+ hook observations** in one session (dense tool use is enough).
3. Ensure **PreCompact LLM consolidation does not succeed** (temporarily misconfigure provider, force timeout, or block the LLM call — failures are non-fatal).
4. End the session → inspect `wiki/.../sessions/<session-id>.md`:
   - Large `## Raw observations` section (~hundreds of KB for dense sessions)
   - `_Synthesised by ai-memory (M3, no-LLM heuristic)._` footer
   - No `consolidated: true`
5. Restore provider and call `memory_consolidate` for that `session_id`.
6. Observe consolidation failure: provider context exceeded, opaque timeout, or hung request. Prompt size exceeds ~200K tokens when heuristic body + windowed obs + 32K output reservation are combined.

**Steps to reproduce (our fork path — optional)**

Same as above, but with fork `AI_MEMORY_LLM_PROVIDER=cursor`, `CURSOR_MODEL=composer-2.5`, and the Node bridge (see Operator environment). PreCompact failed at 120s due to a fork `timeoutMs` serde bug until fixed locally; SessionEnd wrote the ~593 KB heuristic; `memory_consolidate` then failed until we manually split the wiki body into two consolidation passes.

**Environment**

- OS: Windows 11 (logic is provider-agnostic; repro on any OS)
- Docker?: no (local `ai-memory serve`)
- Agent: [pi](https://github.com/earendil-works/pi) with `pi-cursor-sdk` and lifecycle hooks (high `pre-tool-use` / `post-tool-use` volume)
- Transport: HTTP hooks + MCP HTTP (`http://127.0.0.1:49374`)

**Operator environment** (fork — how we discovered this; not required for upstream repro)

| Item | Value |
|------|--------|
| Upstream base | `akitaonrails/ai-memory` @ `0.12.3` |
| Fork delta | Cursor LLM provider (`crates/ai-memory-llm/src/cursor.rs`), `scripts/cursor-bridge/` (Node worker `worker.mjs`), `scripts/windows/*` |
| Binary | Release build from local fork (`target\release\ai-memory.exe`) |
| OS / shell | Windows 11, PowerShell 7 |
| Data dir | `C:\Users\pedro.ailton\.ai-memory` (`AI_MEMORY_DATA_DIR`) |
| Serve | `http://127.0.0.1:49374`, started via `scripts\windows\serve.ps1` (loads `.env` + optional `.env.local`) |
| Agent | [pi](https://github.com/earendil-works/pi) + `pi-cursor-sdk` + lifecycle hooks; pi's modular hook/MCP surface integrates cleanly with ai-memory |
| LLM consolidation | `AI_MEMORY_LLM_PROVIDER=cursor`, `CURSOR_MODEL=composer-2.5` (~200K context), `CURSOR_TIMEOUT_MS=600000`, `CURSOR_AGENT_CWD` pointed at the active repo |
| Cursor bridge flow | Rust `CursorSdkProvider` spawns `node scripts/cursor-bridge/worker.mjs` with JSON on stdin; worker calls Cursor SDK via `pi-cursor-sdk`; **not** part of upstream |
| Embeddings | OpenRouter via `AI_MEMORY_EMBED_*` (separate from consolidation) |
| Project | `default/consisanet` |
| Session | `019e8375-411a-7ea6-8cbc-02781b3fb04a` — 11,204 observations, ~593 KB heuristic page |
| Wiki path | `%USERPROFILE%\.ai-memory\wiki\<workspace-uuid>\<project-uuid>\sessions\019e8375-….md` |

**Fork-only contributing factor (fixed locally):** the bridge worker reads `input.timeoutMs` (camelCase) but Rust `BridgeRequest` was serializing `timeout_ms` (snake_case), so configured `CURSOR_TIMEOUT_MS=600000` was ignored and PreCompact/`memory_consolidate` hit the worker's **120s default**. Local fix: `#[serde(rename = "timeoutMs")]` in fork `cursor.rs`. That timeout failure left only the heuristic page and surfaced the upstream recovery trap; it does not explain why consolidation still fails once the heuristic is hundreds of KB.

**Relevant logs**

On PreCompact failure (non-fatal, upstream router):

```
PreCompact consolidation failed; continuing
```

On SessionEnd:

```
session ended; summary page + open handoff created
```

On `memory_consolidate` with full heuristic (fork Cursor path):

```
provider error 1: Cursor SDK request timed out after 120000ms
```

```
provider error 1: Cursor agent run cancelled
```

After splitting `current_body` into two parts (operator workaround, data preserved), same `composer-2.5` path succeeded on both passes.

**Additional context**

- `crates/ai-memory-hooks/src/synth.rs` — `render_body`, unbounded `## Raw observations` loop
- `crates/ai-memory-hooks/src/router.rs` — PreCompact `consolidate_or_synth` (fail-open) vs SessionEnd heuristic + opt-in `AI_MEMORY_CONSOLIDATE_ON_SESSION_END`
- `crates/ai-memory-consolidate/src/consolidator.rs` — `build_request`, `OBSERVATION_BUDGET_CHARS` (400K), `window_observations_to_budget`, unbounded `current_body`, `max_tokens: 32_000`
- v0.8.1 observation windowing comment references sabadell's 7,234-observation session; this report is the complementary failure mode when the **wiki heuristic body** is also huge
- Prior issue `issue.md` (SessionEnd docs vs heuristic behaviour) — complementary; that issue is about **wording**; this one is about **unbounded checkpoint size breaking consolidation recovery**
- Fork Cursor provider lives only in our fork; upstream users on Anthropic/OpenAI-compat hit the same `build_request` / `synth.rs` logic with their native providers

**Questions for maintainers**

1. Is the interaction between observation windowing in `build_request` and the unbounded `current_body` intentional?
2. Should manual `memory_consolidate` after a full heuristic SessionEnd be a supported recovery path for long sessions?
3. Is the project interested in accepting a dedicated PR for the Cursor bridge provider so more users — especially Cursor subscribers — can use it as their LLM backend? The reporter has run this stack in production since ai-memory ~0.2, which implies at least a full month of real-world use. Or is the preference to wait for an official Cursor API instead?
