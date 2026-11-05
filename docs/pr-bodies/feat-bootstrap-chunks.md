## Summary

Large-repo `ai-memory bootstrap` runs (tens of thousands of git commits) hit HTTP **413** (unpruned JSON body) and provider **502** (single LLM call exceeding context limits). This PR prunes sources client-side before POST and splits the pruned bundle into sequential LLM chunks.

## Changes

- CLI calls `prune_sources_to_budget` before `POST /admin/bootstrap` (avoids 413 on the default 10 MiB hook/MCP limit).
- Server runs `plan_bootstrap_chunks` with configurable `--chunk-input-tokens` (default 24k); merges pages by path across chunks; continues ADR numbering in later chunks.
- Admin routes accept bodies up to 32 MiB; hooks/MCP stay at 10 MiB.
- Dry-run and live outcomes report `llm_chunks`; `sources_collected` honours client-side pre-prune counts.

## Test plan

- [x] `cargo test -p ai-memory-consolidate plan_chunks`
- [x] `cargo test -p ai-memory-mcp dry_run_honours_sources_collected_hint`
- [ ] Manual: `bootstrap --dry-run` on a large repo logs `llm_chunks > 1`
- [ ] Manual: live bootstrap on Consisanet-scale repo completes without 413/502 (with LLM configured)
