## Summary

Adds a **Google Gemini embeddings** provider (`AI_MEMORY_EMBEDDING_PROVIDER=google` or `gemini`) using the Generative Language `embedContent` API. Complements the existing **Gemini LLM** provider without adding new runtime dependencies beyond `reqwest`.

## Scope

- New `GoogleEmbedder` in `crates/ai-memory-llm/src/google.rs`
- `EmbedderChoice::Google` in the factory
- Config wiring for `GEMINI_API_KEY` / `GOOGLE_API_KEY` on the embedding path

**Not included:** Cursor LLM bridge, Windows install scripts, or `.env.local` templates.

## Test plan

- [ ] `cargo test -p ai-memory-consolidate embeddings` (if present)
- [ ] `cargo test -p ai-memory-llm`
- [ ] Manual: `AI_MEMORY_EMBEDDING_PROVIDER=google AI_MEMORY_EMBEDDING_MODEL=gemini-embedding-2` + `memory_query` hybrid path
