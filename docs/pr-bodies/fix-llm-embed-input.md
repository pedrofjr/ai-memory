## Summary

Hardens OpenAI-compatible and Google embedding calls for real wiki pages: long session logs, aggregator gateways (OpenRouter), and transient provider errors.

## Problem

- Large `sessions/*.md` pages hit provider input limits (HTTP 400 / 8192-token ceiling) because bodies were sent untruncated.
- OpenRouter and similar gateways return errors inside HTTP 200 JSON bodies; strict struct parsing surfaced them as "no data".
- No retry on HTTP 429; shared free tiers failed long `embed --force` runs.
- Embeddings used `OPENAI_API_KEY` only, while chat already accepted `LLM_API_KEY` for OpenAI-compat providers.

## Changes

- **`truncate_for_embedding`** (`text.rs`): token budget plus hard 8k-byte cap for dense markdown.
- **`OpenAiEmbedder`** (`embedding.rs`): truncate before POST; parse `error` and `data[]` flexibly; retry on 429; no retry on 400.
- **`GoogleEmbedder`** (`google.rs`): retry loop on 429 (exponential backoff).
- **`Config`** (`config.rs`): `openai_embedding_api_key()` — `OPENAI_API_KEY` then `LLM_API_KEY`.
- **Docs**: `.env.local.example` embedding section; one line in `docs/install.md`.

## Out of scope

- Cursor LLM provider / `config.rs` Cursor env fields (fork-only).
- Admin embed throttle and multi-project fan-out (sibling PR `fix-embed-stale-purge`).

## Test plan

- [x] `cargo test -p ai-memory-llm truncate_for_embedding`
- [x] `cargo test -p ai-memory-llm parse_openai_embedding` (if present)
- [x] `cargo test -p ai-memory-cli openai_embedding_falls_back`
- [ ] Manual: `AI_MEMORY_EMBEDDING_BASE_URL=https://openrouter.ai/api/v1` + `LLM_API_KEY` → `ai-memory embed --force` succeeds on large session pages
