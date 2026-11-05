## Summary

Completes the embedding-model migration story started when `serve` began warning (instead of aborting) on `(provider, model, dim)` mismatch in `page_embeddings`.

Users could start the server and run `ai-memory embed --force`, but re-embedding only targeted the CLI's default project and stale rows for the old triple remained until manually cleaned.

## Problem

- `embed --force` without `--project` still scoped to a single project → hybrid search kept seeing mismatched rows in other projects.
- Re-embedding did not delete rows for the previous `(provider, model, dim)` → `embedding_meta_for_mismatch` stayed noisy.
- Orphan embeddings for superseded page versions (`is_latest = 0`) were never removed.

## Changes

- **CLI** (`embed.rs`): `--force` with no `--project` sets `all_projects` on the admin request.
- **Store**: `delete_stale_page_embeddings` removes rows not matching the configured triple and rows tied to non-latest pages.
- **Admin**: purge stale embeddings before a forced re-embed; fan out embed across all workspace projects when requested.
- **Reader**: `embedding_meta_for_mismatch` counts only `is_latest` pages.
- **Admin**: short sleep between provider calls to reduce rate-limit failures on shared gateways.

## Out of scope

Fork-only helpers (`scripts/windows/inspect-db.py`, `repair-fork-v7-migration.py`) stay in the fork.

## Test plan

- [x] `cargo test -p ai-memory-store delete_stale`
- [ ] Manual: configure new embedding model with existing `page_embeddings` rows → `ai-memory embed --force` (no `--project`) → mismatch WARN clears for latest pages
- [ ] Manual: two projects with pages → re-embed updates both namespaces

## Depends on

Upstream already ships warn-on-mismatch at `serve` startup (`019b523` / `789970c`). This PR is the migration half.
