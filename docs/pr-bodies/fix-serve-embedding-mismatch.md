## Summary

Fixes a startup deadlock when the configured embedding `(provider, model, dim)` differs from rows already stored in `page_embeddings` (e.g. after switching from one Gemini embedding model to another).

Previously `serve` refused to start with a fatal error pointing users to `ai-memory embed --reembed`, but the `embed` subcommand is a thin HTTP client to the running server — so migration was impossible without manually editing SQLite or temporarily reverting config.

## Changes

- On mismatch at startup, log a structured **warning** and continue booting with the configured embedder.
- Hybrid search already loads only rows matching the configured triple; stale rows are ignored until re-embedded.
- Update `docs/deploy.md` troubleshooting to describe warn-and-migrate flow.

## Test plan

- [x] Manual: change `AI_MEMORY_EMBEDDING_MODEL` (or provider) with existing `page_embeddings` rows → server starts with WARN instead of exit
- [x] `ai-memory embed --force` (or `--reembed` when alias is present) migrates rows after server is up
- [ ] `cargo test -p ai-memory-consolidate embeddings` (refuse-on-mismatch query still passes)
