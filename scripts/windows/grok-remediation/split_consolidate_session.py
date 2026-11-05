#!/usr/bin/env python3
"""Split a huge heuristic session page into thematic chunks and consolidate each."""
from __future__ import annotations

import argparse
import json
import re
import sqlite3
import sys
import time
import uuid
from collections import Counter
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import AIM_DB, AIM_WIKI, blob_uuid, ensure_out, mcp_call  # noqa: E402

PARENT_SESSION = "019e8375-411a-7ea6-8cbc-02781b3fb04a"
PROJECT = "consisanet"
WORKSPACE = "default"
EXTENSION = "chunk-split"
SOURCE_EVENT = "019e8375-parent"
THROTTLE_SEC = 45

CHUNK_NS = uuid.UUID("6ba7b810-9dad-11d1-80b4-00c04fd430c8")


@dataclass(frozen=True)
class ChunkSpec:
    key: str
    prompt_start: int
    prompt_end: int
    title: str
    target_path: str
    summary: str


CHUNKS: list[ChunkSpec] = [
    ChunkSpec(
        "A",
        1,
        34,
        "DDS-386661 Rel_TermoHorario regressão e diagnóstico",
        "decisions/DDS-386661-termo-horario-regressao.md",
        "Regressão pós-correção no relatório Termo de Horário, logs de montagem de frase, revert.",
    ),
    ChunkSpec(
        "B",
        35,
        92,
        "DDS-355705 MonitorUsuario e empresa comercial",
        "decisions/DDS-355705-monitor-comercial.md",
        "Monitor de usuário, empresa com comercial, commits/reverts do TICKET-355705.",
    ),
    ChunkSpec(
        "C",
        93,
        148,
        "TICKET-375458 DARF/CSLL (urgência antes do 390068)",
        "decisions/TICKET-375458-darf-csll-urgencia.md",
        "Urgência que adiou 375458: restauração de logs, caso DARF/CSLL, validação com planilha.",
    ),
    ChunkSpec(
        "D1",
        149,
        200,
        "TICKET-390068 IN RFB 2.305/2025 — implementação",
        "decisions/TICKET-390068-in-rfb-2305-implementacao.md",
        "Implementação do acréscimo de presunção DARF (commit 3990e40), branches e alinhamento planilha.",
    ),
    ChunkSpec(
        "D2",
        201,
        256,
        "TICKET-390068 validação, massa de teste e merge",
        "decisions/TICKET-390068-validacao-merge.md",
        "Testes Per=P, correção de massa/alíquotas, remoção de logs, merge TICKET-390068.",
    ),
]


def child_session_id(parent_id: str, chunk_key: str) -> str:
    return str(uuid.uuid5(CHUNK_NS, f"{parent_id}#{chunk_key}"))


def find_parent_wiki_path(parent_id: str) -> Path:
    matches = list(AIM_WIKI.rglob(f"sessions/{parent_id}.md"))
    if not matches:
        raise FileNotFoundError(f"wiki page not found for session {parent_id}")
    return matches[0]


def load_prompts(wiki_path: Path) -> list[tuple[int, str]]:
    lines = wiki_path.read_text(encoding="utf-8", errors="replace").splitlines()
    prompts: list[tuple[int, str]] = []
    in_prompts = False
    for line in lines:
        if line == "## Prompts":
            in_prompts = True
            continue
        if in_prompts and line.startswith("## "):
            break
        m = re.match(r"^(\d+)\.\s+(.*)", line)
        if m:
            prompts.append((int(m.group(1)), m.group(2)))
    return prompts


def load_parent_row(conn: sqlite3.Connection, parent_id: str) -> dict[str, Any]:
    row = conn.execute(
        """
        SELECT s.workspace_id, s.project_id, s.agent_kind, s.cwd, s.started_at, s.ended_at
        FROM sessions s WHERE s.id = ?
        """,
        (uuid.UUID(parent_id).bytes,),
    ).fetchone()
    if not row:
        raise RuntimeError(f"parent session not in SQLite: {parent_id}")
    return {
        "workspace_id": row[0],
        "project_id": row[1],
        "agent_kind": row[2],
        "cwd": row[3],
        "started_at": row[4],
        "ended_at": row[5],
    }


def prompt_bounds(
    conn: sqlite3.Connection, parent_id: str, p_start: int, p_end: int
) -> tuple[int, int]:
    rows = conn.execute(
        """
        SELECT created_at FROM observations
        WHERE session_id = ? AND kind = 'user-prompt'
        ORDER BY created_at ASC
        """,
        (uuid.UUID(parent_id).bytes,),
    ).fetchall()
    if len(rows) < p_end:
        raise RuntimeError(f"expected >={p_end} user-prompt obs, got {len(rows)}")
    return rows[p_start - 1][0], rows[p_end - 1][0]


def tool_summary(conn: sqlite3.Connection, parent_id: str, t0: int, t1: int) -> list[tuple[str, int]]:
    rows = conn.execute(
        """
        SELECT title FROM observations
        WHERE session_id = ? AND created_at >= ? AND created_at <= ?
          AND kind IN ('pre-tool-use', 'post-tool-use')
        """,
        (uuid.UUID(parent_id).bytes, t0, t1),
    ).fetchall()
    ctr: Counter[str] = Counter()
    for (title,) in rows:
        name = (title or "tool").split("`")[0].strip()[:80]
        ctr[name] += 1
    return ctr.most_common(20)


def fmt_us(us: int) -> str:
    return datetime.fromtimestamp(us / 1_000_000, tz=timezone.utc).strftime("%Y-%m-%d")


def build_chunk_body(
    spec: ChunkSpec,
    parent_id: str,
    prompts: list[tuple[int, str]],
    t0: int,
    t1: int,
    tools: list[tuple[str, int]],
    obs_count: int,
) -> str:
    subset = prompts[spec.prompt_start - 1 : spec.prompt_end]
    lines = [
        f"# {spec.title}",
        "",
        "## Parent session",
        "",
        f"- parent_session_id: `{parent_id}`",
        f"- chunk: `{spec.key}` (prompts {spec.prompt_start}–{spec.prompt_end})",
        f"- period: {fmt_us(t0)} → {fmt_us(t1)}",
        f"- project: {PROJECT}",
        f"- observations in window: {obs_count}",
        "",
        "## Summary",
        "",
        spec.summary,
        "",
        "## User requests",
        "",
    ]
    for i, (_, text) in enumerate(subset, 1):
        one = text.replace("\n", " ").strip()
        if len(one) > 600:
            one = one[:600] + "…"
        lines.append(f"{i}. {one}")
    lines.append("")
    if tools:
        lines.append("## Tool activity (sample)")
        lines.append("")
        for name, count in tools:
            lines.append(f"- `{name}`: {count}")
        lines.append("")
    lines.append("## Note")
    lines.append("")
    lines.append(
        "Chunk extracted from heuristic session page for split consolidation. "
        "Raw observations omitted; SQLite subset supplied separately."
    )
    return "\n".join(lines)


def delete_child_artifacts(conn: sqlite3.Connection, child_id: str) -> None:
    cb = uuid.UUID(child_id).bytes
    conn.execute("DELETE FROM observations WHERE session_id = ?", (cb,))
    conn.execute("DELETE FROM sessions WHERE id = ?", (cb,))


def ensure_child_session(
    conn: sqlite3.Connection,
    parent: dict[str, Any],
    child_id: str,
    t0: int,
    t1: int,
) -> None:
    delete_child_artifacts(conn, child_id)
    conn.execute(
        """
        INSERT INTO sessions (id, workspace_id, project_id, agent_kind, cwd, started_at, ended_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        """,
        (
            uuid.UUID(child_id).bytes,
            parent["workspace_id"],
            parent["project_id"],
            parent["agent_kind"],
            parent["cwd"],
            t0,
            t1,
        ),
    )


def copy_observations(
    conn: sqlite3.Connection,
    parent_id: str,
    child_id: str,
    parent: dict[str, Any],
    t0: int,
    t1: int,
) -> int:
    rows = conn.execute(
        """
        SELECT kind, title, body, importance, created_at, extension, source_event
        FROM observations
        WHERE session_id = ? AND created_at >= ? AND created_at <= ?
        ORDER BY created_at ASC
        """,
        (uuid.UUID(parent_id).bytes, t0, t1),
    ).fetchall()
    child_b = uuid.UUID(child_id).bytes
    for kind, title, body, importance, created_at, ext, src in rows:
        conn.execute(
            """
            INSERT INTO observations
              (id, session_id, workspace_id, project_id, kind, extension, source_event,
               title, body, importance, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                uuid.uuid4().bytes,
                child_b,
                parent["workspace_id"],
                parent["project_id"],
                kind,
                EXTENSION,
                SOURCE_EVENT,
                title,
                body,
                importance,
                created_at,
            ),
        )
    return len(rows)


def build_index_body(parent_id: str, results: list[dict[str, Any]]) -> str:
    lines = [
        f"# Sessão pi consisanet — índice ({parent_id[:8]}…)",
        "",
        "Sessão longa dividida em páginas consolidadas por ticket/tema.",
        "",
        "## Consolidated pages",
        "",
    ]
    for r in results:
        if not r.get("ok"):
            lines.append(f"- **{r['chunk']}** — FAILED: {r.get('error', '?')}")
            continue
        title = r.get("title") or r.get("target_path", "")
        path = r.get("target_path", "")
        lines.append(f"- [{title}]({path})")
    lines.extend(
        [
            "",
            "## Parent metadata",
            "",
            f"- session_id: `{parent_id}`",
            f"- project: {PROJECT}",
            "- agent: omp (pi + hooks)",
            "",
            "_Index written by split_consolidate_session.py after chunk consolidation._",
        ]
    )
    return "\n".join(lines)


def run_chunk(
    spec: ChunkSpec,
    parent_id: str,
    prompts: list[tuple[int, str]],
    parent: dict[str, Any],
    *,
    dry_run: bool,
    req_id: int,
) -> dict[str, Any]:
    child_id = child_session_id(parent_id, spec.key)
    conn = sqlite3.connect(AIM_DB)
    try:
        t0, t1 = prompt_bounds(conn, parent_id, spec.prompt_start, spec.prompt_end)
        obs_count = conn.execute(
            """
            SELECT COUNT(*) FROM observations
            WHERE session_id = ? AND created_at >= ? AND created_at <= ?
            """,
            (uuid.UUID(parent_id).bytes, t0, t1),
        ).fetchone()[0]
        tools = tool_summary(conn, parent_id, t0, t1)
    finally:
        conn.close()

    body = build_chunk_body(spec, parent_id, prompts, t0, t1, tools, obs_count)
    out_dir = ensure_out() / "chunks"
    out_dir.mkdir(exist_ok=True)
    chunk_file = out_dir / f"{parent_id[:8]}-{spec.key}.md"
    chunk_file.write_text(body, encoding="utf-8")

    entry: dict[str, Any] = {
        "chunk": spec.key,
        "child_session_id": child_id,
        "prompts": f"{spec.prompt_start}-{spec.prompt_end}",
        "obs_count": obs_count,
        "target_path": spec.target_path,
        "chunk_file": str(chunk_file),
        "period": [fmt_us(t0), fmt_us(t1)],
    }

    if dry_run:
        entry["dry_run"] = True
        return entry

    conn = sqlite3.connect(AIM_DB)
    try:
        conn.execute("PRAGMA foreign_keys = ON")
        ensure_child_session(conn, parent, child_id, t0, t1)
        copied = copy_observations(conn, parent_id, child_id, parent, t0, t1)
        conn.commit()
        entry["obs_copied"] = copied
    finally:
        conn.close()

    mcp_call(
        "memory_write_page",
        {
            "path": f"sessions/{child_id}.md",
            "body": body,
            "tier": "episodic",
            "tags": ["chunk-split", spec.key, "consisanet"],
            "project": PROJECT,
            "workspace": WORKSPACE,
        },
        req_id=req_id,
    )

    text = mcp_call(
        "memory_consolidate",
        {
            "session_id": child_id,
            "dry_run": False,
            "multi_page": False,
        },
        req_id=req_id + 100,
    )
    outcome = json.loads(text)
    entry["ok"] = True
    entry["title"] = outcome.get("new_title")
    entry["consolidated_path"] = outcome.get("path")

    promo_body = outcome.get("new_body_markdown") or body
    if not promo_body.lstrip().startswith("#"):
        promo_body = f"# {outcome.get('new_title', spec.title)}\n\n{promo_body}"

    mcp_call(
        "memory_write_page",
        {
            "path": spec.target_path,
            "body": promo_body,
            "tier": "semantic",
            "tags": outcome.get("tags") or ["chunk-split", spec.key],
            "project": PROJECT,
            "workspace": WORKSPACE,
        },
        req_id=req_id + 200,
    )
    entry["promoted_to"] = spec.target_path
    return entry


def load_child_ids_from_log() -> list[dict[str, str]]:
    log_path = ensure_out() / "split-consolidate-log.json"
    if not log_path.exists():
        raise FileNotFoundError(f"missing {log_path}; run --apply first")
    data = json.loads(log_path.read_text(encoding="utf-8"))
    out: list[dict[str, str]] = []
    for row in data:
        if not row.get("ok") or not row.get("child_session_id"):
            continue
        out.append(
            {
                "chunk": row["chunk"],
                "child_session_id": row["child_session_id"],
                "wiki_path": f"sessions/{row['child_session_id']}.md",
                "promoted_to": row.get("promoted_to", ""),
            }
        )
    return out


def cleanup_children(*, project: str = PROJECT, workspace: str = WORKSPACE) -> list[dict[str, Any]]:
    children = load_child_ids_from_log()
    log: list[dict[str, Any]] = []

    # MCP deletes first — do not hold a SQLite write connection during HTTP calls.
    for i, child in enumerate(children, 1):
        sid = child["child_session_id"]
        entry: dict[str, Any] = {"chunk": child["chunk"], "child_session_id": sid}
        try:
            resp = mcp_call(
                "memory_delete_page",
                {
                    "path": child["wiki_path"],
                    "project": project,
                    "workspace": workspace,
                },
                req_id=500 + i,
            )
            entry["wiki_deleted"] = True
            entry["delete_response"] = resp[:200]
        except Exception as e:
            entry["wiki_deleted"] = False
            entry["wiki_error"] = str(e)
        entry["promoted_to"] = child["promoted_to"]
        log.append(entry)
        if i < len(children):
            time.sleep(0.5)

    conn = sqlite3.connect(AIM_DB)
    try:
        conn.execute("PRAGMA foreign_keys = ON")
        for entry in log:
            cb = uuid.UUID(entry["child_session_id"]).bytes
            obs_before = conn.execute(
                "SELECT COUNT(*) FROM observations WHERE session_id = ?", (cb,)
            ).fetchone()[0]
            conn.execute("DELETE FROM observations WHERE session_id = ?", (cb,))
            conn.execute("DELETE FROM sessions WHERE id = ?", (cb,))
            entry["obs_deleted"] = obs_before
            entry["session_deleted"] = True
        conn.commit()
    finally:
        conn.close()
    out = ensure_out()
    (out / "split-consolidate-cleanup-log.json").write_text(
        json.dumps(log, indent=2), encoding="utf-8"
    )
    return log


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--session", default=PARENT_SESSION)
    parser.add_argument("--project", default=PROJECT)
    parser.add_argument("--dry-run", action="store_true", help="Only extract chunk markdown files")
    parser.add_argument("--apply", action="store_true", help="Run full split + consolidate pipeline")
    parser.add_argument(
        "--cleanup",
        action="store_true",
        help="Delete child sessions/*.md duplicates and SQLite child rows",
    )
    args = parser.parse_args()

    if args.cleanup:
        print(f"Cleaning {len(load_child_ids_from_log())} child sessions...")
        log = cleanup_children(project=args.project)
        for row in log:
            print(
                f"  {row['chunk']} wiki={row.get('wiki_deleted')} "
                f"obs={row.get('obs_deleted')} -> kept {row.get('promoted_to')}"
            )
        ok = sum(1 for x in log if x.get("wiki_deleted") and x.get("session_deleted"))
        print(f"Done: {ok}/{len(log)} cleaned")
        return 0 if ok == len(log) else 1

    if not args.dry_run and not args.apply:
        parser.error("pass --dry-run, --apply, or --cleanup")

    parent_id = args.session
    wiki_path = find_parent_wiki_path(parent_id)
    prompts = load_prompts(wiki_path)
    print(f"Parent {parent_id[:8]}… wiki={wiki_path.name} prompts={len(prompts)}")

    conn = sqlite3.connect(AIM_DB)
    try:
        parent = load_parent_row(conn, parent_id)
    finally:
        conn.close()

    log: list[dict[str, Any]] = []
    for i, spec in enumerate(CHUNKS, 1):
        print(f"[{i}/{len(CHUNKS)}] chunk {spec.key}: prompts {spec.prompt_start}-{spec.prompt_end}")
        try:
            entry = run_chunk(
                spec,
                parent_id,
                prompts,
                parent,
                dry_run=args.dry_run,
                req_id=i * 10,
            )
            entry["ok"] = entry.get("ok", args.dry_run)
            print(
                f"  obs={entry.get('obs_count')} "
                f"file={Path(entry.get('chunk_file','')).name} "
                f"{'(dry-run)' if args.dry_run else 'OK -> ' + str(entry.get('promoted_to',''))}"
            )
        except Exception as e:
            entry = {
                "chunk": spec.key,
                "ok": False,
                "error": str(e),
                "target_path": spec.target_path,
            }
            print(f"  FAIL: {e}")
        log.append(entry)
        if args.apply and i < len(CHUNKS):
            time.sleep(THROTTLE_SEC)

    if args.apply and any(x.get("ok") for x in log):
        index_body = build_index_body(parent_id, log)
        try:
            mcp_call(
                "memory_write_page",
                {
                    "path": f"sessions/{parent_id}.md",
                    "body": index_body,
                    "tier": "episodic",
                    "tags": ["chunk-split-index", "consisanet"],
                    "project": PROJECT,
                    "workspace": WORKSPACE,
                },
                req_id=999,
            )
            print(f"Index written to sessions/{parent_id}.md")
        except Exception as e:
            print(f"Index write FAIL: {e}")
            log.append({"index": True, "ok": False, "error": str(e)})

    out = ensure_out()
    (out / "split-consolidate-log.json").write_text(json.dumps(log, indent=2), encoding="utf-8")
    ok = sum(1 for x in log if x.get("ok") is True)
    print(f"Done: {ok}/{len(CHUNKS)} chunks")
    return 0 if ok == len(CHUNKS) else 1


if __name__ == "__main__":
    raise SystemExit(main())