## Summary

Fixes three production issues discovered while running ai-memory with Oh My Pi (OMP) hooks and MCP read tools on Windows:

1. **SQLite CHECK on `sessions.agent_kind`** — Pi hooks persist `omp`, but V01 only allowed `claude-code|codex|open-code|other`, causing every hook to fail with `CHECK constraint failed`.
2. **Handoff namespace mismatch** — `memory_handoff_begin` / `memory_handoff_accept` used the server's baked-in `--project` default while read tools (`memory_briefing`, etc.) used `effective_ids()` (ActiveProject from hooks). Handoffs were written to `scratch` but read from the active cwd project.
3. **FTS5 column syntax in agent queries** — Natural-language queries containing `word:` (e.g. `pick: handoff`) were interpreted as FTS5 column qualifiers, producing `no such column: pick` instead of search results.

## Changes

- Migration `V07__agent_kind_omp.sql` expands the `sessions.agent_kind` CHECK to include `'omp'`.
- Handoff MCP tools resolve project via `effective_ids()` (optional `project` arg), matching read tools.
- New `prepare_fts5_query()` neutralises colons and quotes tokens; wired through `normalize_fts_query()` in the reader.

## Test plan

- [x] `cargo test -p ai-memory-store search_colon`
- [x] `cargo test -p ai-memory-mcp handoff_begin_pending_count_matches_briefing_active_project`
- [ ] Manual: pi hooks no longer WARN on `agent_kind` after migration V07
- [ ] Manual: `memory_handoff_begin` + `memory_briefing` show consistent `pending_handoff_count`
- [ ] Manual: `memory_query` with `pick: handoff` returns hits (no SQLite error)
