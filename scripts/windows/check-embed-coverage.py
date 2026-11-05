#!/usr/bin/env python3
"""List workspaces, report embed coverage, exit 0 if clean."""
from __future__ import annotations

import sqlite3
import sys
from pathlib import Path

DB = Path.home() / ".ai-memory" / "db" / "memory.sqlite"
CFG_PROVIDER = "openai"
CFG_MODEL = "qwen/qwen3-embedding-8b"
CFG_DIM = 4096


def main() -> int:
    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row

    workspaces = [r[0] for r in conn.execute("SELECT name FROM workspaces ORDER BY name")]
    print("workspaces:", ", ".join(workspaces) or "(none)")

    print("\n=== coverage by workspace (latest pages) ===")
    for ws in workspaces:
        total = conn.execute(
            """
            SELECT COUNT(*) FROM pages p
            JOIN workspaces w ON w.id = p.workspace_id
            WHERE w.name = ? AND p.is_latest = 1
            """,
            (ws,),
        ).fetchone()[0]
        matched = conn.execute(
            """
            SELECT COUNT(*) FROM pages p
            JOIN workspaces w ON w.id = p.workspace_id
            JOIN page_embeddings pe ON pe.page_id = p.id
            WHERE w.name = ? AND p.is_latest = 1
              AND pe.provider = ? AND pe.model = ? AND pe.dim = ?
            """,
            (ws, CFG_PROVIDER, CFG_MODEL, CFG_DIM),
        ).fetchone()[0]
        missing = total - matched
        print(f"  {ws}: latest={total} embedded_ok={matched} missing_or_stale={missing}")

    print("\n=== embedding groups ===")
    for r in conn.execute(
        "SELECT provider, model, dim, COUNT(*) n FROM page_embeddings GROUP BY 1,2,3 ORDER BY n DESC"
    ):
        print(f"  {dict(r)}")

    print("\n=== mismatch latest (should be empty) ===")
    mism = list(
        conn.execute(
            """
            SELECT pe.provider, pe.model, pe.dim, COUNT(*) n
            FROM page_embeddings pe
            JOIN pages p ON p.id = pe.page_id AND p.is_latest = 1
            WHERE NOT (pe.provider = ? AND pe.model = ? AND pe.dim = ?)
            GROUP BY 1,2,3
            """,
            (CFG_PROVIDER, CFG_MODEL, CFG_DIM),
        )
    )
    if not mism:
        print("  (none)")
    else:
        for r in mism:
            print(f"  {dict(r)}")

    # missing: latest pages with no matching embedding row
    print("\n=== latest pages without current embedding (sample) ===")
    missing_rows = list(
        conn.execute(
            """
            SELECT w.name AS workspace, pr.name AS project, p.path
            FROM pages p
            JOIN workspaces w ON w.id = p.workspace_id
            JOIN projects pr ON pr.id = p.project_id
            WHERE p.is_latest = 1
              AND NOT EXISTS (
                SELECT 1 FROM page_embeddings pe
                WHERE pe.page_id = p.id
                  AND pe.provider = ?
                  AND pe.model = ?
                  AND pe.dim = ?
              )
            ORDER BY w.name, pr.name, p.path
            LIMIT 30
            """,
            (CFG_PROVIDER, CFG_MODEL, CFG_DIM),
        )
    )
    missing_count = conn.execute(
        """
        SELECT COUNT(*) FROM pages p
        WHERE p.is_latest = 1
          AND NOT EXISTS (
            SELECT 1 FROM page_embeddings pe
            WHERE pe.page_id = p.id
              AND pe.provider = ?
              AND pe.model = ?
              AND pe.dim = ?
          )
        """,
        (CFG_PROVIDER, CFG_MODEL, CFG_DIM),
    ).fetchone()[0]
    print(f"  total_missing={missing_count}")
    for r in missing_rows:
        print(f"  {r['workspace']}/{r['project']}: {r['path']}")

    # write workspace list for shell
    out = Path.home() / ".ai-memory" / "runs" / "embed-all-workspaces.txt"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(workspaces) + "\n", encoding="utf-8")
    print(f"\nwrote {out}")
    conn.close()
    return 0 if missing_count == 0 and not mism else 1


if __name__ == "__main__":
    raise SystemExit(main())
