Title: SessionEnd: docs/MCP imply LLM consolidation but hook only runs rule-based synthesis

**Version**

Output of `ai-memory --version`:

```
ai-memory 0.3.2
```

**What happened**

Documentation and agent-facing instructions (README, `MEMORY_INSTRUCTIONS`, MCP tool descriptions, `CLAUDE.md` / `AGENTS.md` routing snippets, parts of `docs/design-decisions.md`) state that **SessionEnd auto-consolidates** sessions via LLM (`memory_consolidate`-style).

In practice, **SessionEnd** only runs **rule-based synthesis** (`synthesize_session_page` in `ai-memory-hooks`): prompts, tool-call counts, and **full raw observations** (one line per hook). That produces large `sessions/<id>.md` files (e.g. ~100 KB for long sessions). The web UI (`/web`) renders the page body as-is, so the “readable” part is only the top sections.

**LLM consolidation** today runs on:

- **PreCompact** hook (when an LLM provider is configured) — checkpoint before context compaction
- **`memory_consolidate`** MCP tool — manual
- **Not** on SessionEnd (upstream has no env flag for “run LLM consolidation when the session ends”)

**What I expected**

Either:

1. SessionEnd runs LLM consolidation when a provider is configured (as several docs imply), **or**
2. Docs and MCP instructions state clearly that SessionEnd writes a **heuristic** session page by default, and LLM consolidation is **manual**, **PreCompact**, or behind an explicit opt-in.

Large session pages in `/web` are a separate UX concern; the main bug here is **documentation and product text vs hook behavior**.

**Suggested fix (for maintainer decision)**

Keep current capture behavior by default, make LLM at session end explicit, and align all user-facing text.

| Concern | Proposal |
|--------|----------|
| **Default SessionEnd** | Continue writing the full rule-based session page (including raw observations) — no silent trimming of hook data |
| **LLM at SessionEnd** | Opt-in env, e.g. `AI_MEMORY_CONSOLIDATE_ON_SESSION_END=true`, calling `consolidate_session` when a provider is configured; on LLM failure, log a warning and fall back to the heuristic page |
| **Multi-page fan-out** | Optional second flag, e.g. `AI_MEMORY_CONSOLIDATE_MULTI_PAGE_ON_SESSION_END=true`, for M7b-style fan-out at SessionEnd |
| **Documentation** | Update `ARCHITECTURE.md`, `design-decisions.md`, `usage.md`, routing snippets, and MCP tool descriptions so they match the chosen default |

Alternative paths you might prefer instead:

- **Docs-only:** change all “auto-consolidate at session end” wording to describe heuristic SessionEnd + PreCompact/manual MCP (no new env flags).
- **Behavior change:** run LLM consolidation on every SessionEnd when `AI_MEMORY_LLM_PROVIDER` is set (matches old docs, higher cost/latency on session end).

**Steps to reproduce**

1. Configure `AI_MEMORY_LLM_PROVIDER` and run `ai-memory serve` with hooks installed.
2. Run an agent session that emits many hook observations, then end the session.
3. Open `wiki/.../sessions/<session-id>.md` or `/web` → page shows **Raw observations** with thousands of lines.
4. Compare with docs / `memory_consolidate` description (“usually automatic at session end”).
5. Observe: LLM rewrite occurs on **PreCompact** or via **`memory_consolidate`**, not on SessionEnd in current upstream.

**Environment**

- OS: Windows 11 (also reproducible on Linux/macOS — hook path is cross-platform)
- Docker?: no (local `ai-memory serve`)
- Agent: Cursor / Claude Code / other with lifecycle hooks
- Transport: HTTP hooks + MCP HTTP (`http://127.0.0.1:49374`)

**Relevant logs**

With LLM configured and default SessionEnd behavior, serve may log something like:

```
memory_consolidate + PreCompact LLM checkpointing enabled
```

On SessionEnd:

```
session ended; summary page + open handoff created
```

No LLM consolidation runs unless PreCompact fired or `memory_consolidate` was called manually.

**Additional context**

- `crates/ai-memory-hooks/src/synth.rs` — heuristic session page
- `crates/ai-memory-hooks/src/router.rs` — `SessionEnd` vs `PreCompact` branches
- `docs/design-decisions.md` — references consolidation at session end in places that do not match the hook router
- `/web` does not paginate or collapse long **Raw observations** sections (could be a follow-up issue without changing capture)

**Questions for maintainers**

1. Which default do you want: heuristic SessionEnd only, or LLM on every SessionEnd when a provider exists?
2. If heuristic-by-default, is an opt-in env flag acceptable, or should SessionEnd never call the LLM automatically?
3. Should `design-decisions.md` explicitly document PreCompact + manual MCP as the primary LLM consolidation paths?
4. Interest in a separate issue for `/web` presentation of large session pages?
