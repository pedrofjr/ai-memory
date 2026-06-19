#!/usr/bin/env python3
"""Backfill SQLite observations from Grok filesystem for hook-less sessions."""
from __future__ import annotations

import json
import sqlite3
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import (  # noqa: E402
    AIM_DB,
    GrokSession,
    discover_grok_sessions,
    ensure_out,
    parse_iso,
    resolve_project_name,
)
from mine import mine_session  # noqa: E402

EXTENSION = "grok-remediation"
SOURCE_EVENT = "filesystem-backfill"
MAX_TOOL_OBS = 96
MAX_PROMPT_OBS = 80


@dataclass
class ObsRow:
    kind: str
    title: str
    body: str
    importance: int = 5


def load_scope_ids(project_name: str) -> tuple[bytes, bytes]:
    conn = sqlite3.connect(AIM_DB)
    row = conn.execute(
        """
        SELECT w.id, p.id FROM projects p
        JOIN workspaces w ON w.id = p.workspace_id
        WHERE w.name = 'default' AND p.name = ?
        """,
        (project_name,),
    ).fetchone()
    conn.close()
    if not row:
        raise RuntimeError(f"project not found in ai-memory: {project_name}")
    return row[0], row[1]


def observation_count(session_id: str) -> int:
    conn = sqlite3.connect(AIM_DB)
    n = conn.execute(
        "SELECT COUNT(*) FROM observations WHERE session_id = ?",
        (uuid.UUID(session_id).bytes,),
    ).fetchone()[0]
    conn.close()
    return int(n)


def session_exists(session_id: str) -> bool:
    conn = sqlite3.connect(AIM_DB)
    n = conn.execute(
        "SELECT COUNT(*) FROM sessions WHERE id = ?",
        (uuid.UUID(session_id).bytes,),
    ).fetchone()[0]
    conn.close()
    return n > 0


def mine_tool_events(path: Path) -> list[ObsRow]:
    rows: list[ObsRow] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        update = (obj.get("params") or {}).get("update") or {}
        kind = update.get("sessionUpdate") or ""
        if kind == "tool_call":
            title = (update.get("title") or "tool").strip()
            raw = update.get("rawInput") or {}
            parts: list[str] = []
            if isinstance(raw, dict):
                for key in ("path", "command", "glob_pattern", "target_directory", "pattern"):
                    val = raw.get(key)
                    if isinstance(val, str) and val:
                        parts.append(f"{key}={val[:240]}")
            body = "; ".join(parts) if parts else json.dumps(raw, ensure_ascii=False)[:500]
            rows.append(ObsRow("pre-tool-use", title[:120], body))
        elif kind == "tool_call_update" and update.get("status") == "completed":
            title = (update.get("title") or "").strip()
            if not title:
                continue
            body = title[:500]
            rows.append(ObsRow("post-tool-use", title.split("`")[0].strip()[:120], body))
        if len(rows) >= MAX_TOOL_OBS * 2:
            break
    if len(rows) > MAX_TOOL_OBS:
        step = len(rows) / MAX_TOOL_OBS
        rows = [rows[int(i * step)] for i in range(MAX_TOOL_OBS)]
    return rows


def grok_to_observations(sess: GrokSession, mined: dict[str, Any] | None) -> list[ObsRow]:
    out: list[ObsRow] = []
    out.append(
        ObsRow(
            "session-start",
            "session-start",
            f"Grok Build session in `{sess.cwd}` (backfilled from ~/.grok/sessions).",
            6,
        )
    )

    prompts = (mined or {}).get("user_prompts") or []
    for p in prompts[:MAX_PROMPT_OBS]:
        text = (p.get("text") or "").strip()
        if not text:
            continue
        title = text.replace("\n", " ")[:80]
        out.append(ObsRow("user-prompt", title, text[:3000], 7))

    up = sess.updates()
    if up:
        out.extend(mine_tool_events(up))

    if not prompts and not out[1:]:
        summary_path = sess.dir / "summary.json"
        if summary_path.exists():
            data = json.loads(summary_path.read_text(encoding="utf-8"))
            summary = (data.get("session_summary") or data.get("generated_title") or "").strip()
            if summary:
                out.append(ObsRow("other", "session summary", summary[:2000], 5))

    out.append(
        ObsRow(
            "session-end",
            "session-end",
            f"Session ended ({sess.num_chat_messages} chat messages; Grok filesystem backfill).",
            5,
        )
    )
    return out


def spread_timestamps(base_us: int, end_us: int, count: int) -> list[int]:
    if count <= 1:
        return [base_us]
    if end_us <= base_us:
        end_us = base_us + count * 1_000_000
    span = end_us - base_us
    return [base_us + int(span * i / (count - 1)) for i in range(count)]


def ensure_session_row(
    conn: sqlite3.Connection,
    sess: GrokSession,
    ws_id: bytes,
    proj_id: bytes,
) -> None:
    summary = json.loads((sess.dir / "summary.json").read_text(encoding="utf-8"))
    started = parse_iso(summary.get("created_at", sess.created_at))
    ended_raw = summary.get("updated_at") or summary.get("last_active_at") or sess.created_at
    ended = parse_iso(ended_raw)
    started_us = int(started.timestamp() * 1_000_000)
    ended_us = int(ended.timestamp() * 1_000_000)
    conn.execute(
        """
        INSERT INTO sessions (id, workspace_id, project_id, agent_kind, cwd, started_at, ended_at)
        VALUES (?, ?, ?, 'grok', ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
          workspace_id = excluded.workspace_id,
          project_id = excluded.project_id,
          agent_kind = excluded.agent_kind,
          cwd = excluded.cwd,
          ended_at = COALESCE(sessions.ended_at, excluded.ended_at)
        """,
        (
            uuid.UUID(sess.session_id).bytes,
            ws_id,
            proj_id,
            sess.cwd,
            started_us,
            ended_us,
        ),
    )


def insert_observations(
    conn: sqlite3.Connection,
    session_id: str,
    ws_id: bytes,
    proj_id: bytes,
    observations: list[ObsRow],
    started_us: int,
    ended_us: int,
) -> int:
    stamps = spread_timestamps(started_us, ended_us, len(observations))
    sid = uuid.UUID(session_id).bytes
    inserted = 0
    for obs, created_at in zip(observations, stamps, strict=True):
        conn.execute(
            """
            INSERT INTO observations
              (id, session_id, workspace_id, project_id, kind, extension, source_event,
               title, body, importance, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                uuid.uuid4().bytes,
                sid,
                ws_id,
                proj_id,
                obs.kind,
                EXTENSION,
                SOURCE_EVENT,
                obs.title,
                obs.body,
                max(1, min(10, obs.importance)),
                created_at,
            ),
        )
        inserted += 1
    return inserted


def backfill_session(sess: GrokSession, *, force: bool = False) -> dict[str, Any]:
    project = resolve_project_name(sess.project, sess.cwd)
    entry: dict[str, Any] = {
        "session_id": sess.session_id,
        "project": project,
        "cwd": sess.cwd,
    }
    existing = observation_count(sess.session_id)
    if existing and not force:
        entry["skipped"] = True
        entry["reason"] = f"already has {existing} observations"
        return entry

    mined = mine_session(sess)
    observations = grok_to_observations(sess, mined)
    if len(observations) < 2:
        entry["skipped"] = True
        entry["reason"] = "no grok signal to backfill"
        return entry

    ws_id, proj_id = load_scope_ids(project)
    summary = json.loads((sess.dir / "summary.json").read_text(encoding="utf-8"))
    started_us = int(parse_iso(summary.get("created_at", sess.created_at)).timestamp() * 1_000_000)
    ended_raw = summary.get("updated_at") or summary.get("last_active_at") or sess.created_at
    ended_us = int(parse_iso(ended_raw).timestamp() * 1_000_000)

    conn = sqlite3.connect(AIM_DB)
    try:
        conn.execute("PRAGMA foreign_keys = ON")
        ensure_session_row(conn, sess, ws_id, proj_id)
        if force and existing:
            conn.execute(
                "DELETE FROM observations WHERE session_id = ? AND extension = ? AND source_event = ?",
                (uuid.UUID(sess.session_id).bytes, EXTENSION, SOURCE_EVENT),
            )
        inserted = insert_observations(
            conn, sess.session_id, ws_id, proj_id, observations, started_us, ended_us
        )
        conn.commit()
    finally:
        conn.close()

    entry["skipped"] = False
    entry["inserted"] = inserted
    entry["session_created"] = not session_exists(sess.session_id)  # best-effort before insert
    entry["prompts"] = len((mined or {}).get("user_prompts") or [])
    entry["tools_sampled"] = sum(1 for o in observations if o.kind.startswith("pre-tool"))
    return entry


def backfill_sessions(
    session_ids: list[str] | None = None,
    *,
    force: bool = False,
) -> list[dict[str, Any]]:
    by_id = {s.session_id: s for s in discover_grok_sessions()}
    targets = session_ids or list(by_id.keys())
    log: list[dict[str, Any]] = []
    for sid in targets:
        sess = by_id.get(sid)
        if not sess:
            log.append({"session_id": sid, "skipped": True, "reason": "not in grok filesystem"})
            continue
        log.append(backfill_session(sess, force=force))
    out = ensure_out()
    (out / "backfill-log.json").write_text(json.dumps(log, indent=2), encoding="utf-8")
    return log


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("session_ids", nargs="*", help="Session UUIDs (default: all with 0 obs)")
    parser.add_argument("--force", action="store_true", help="Replace prior grok-remediation backfill")
    args = parser.parse_args()

    if args.session_ids:
        ids = args.session_ids
    else:
        ids = [
            s.session_id
            for s in discover_grok_sessions()
            if s.ai_obs == 0 and s.wiki_action in ("create", "replace", "keep")
        ]

    log = backfill_sessions(ids, force=args.force)
    done = [x for x in log if not x.get("skipped")]
    print(f"Backfilled {len(done)}/{len(log)} sessions")
    for row in log:
        if row.get("skipped"):
            print(f"  SKIP {row['session_id'][:8]}… {row.get('reason', '')}")
        else:
            print(
                f"  OK {row['session_id'][:8]}… {row.get('project')} "
                f"obs={row.get('inserted')} prompts={row.get('prompts')}"
            )
    return 0 if done else 1


if __name__ == "__main__":
    raise SystemExit(main())