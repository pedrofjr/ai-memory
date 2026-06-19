#!/usr/bin/env python3
"""End open Grok-era sessions in SQLite; preserve remediated wiki bodies."""
from __future__ import annotations

import json
import re
import sqlite3
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import (  # noqa: E402
    AIM_DB,
    CUTOFF_ISO,
    ensure_out,
    mcp_call,
    parse_iso,
    resolve_project_name,
    wiki_page_index,
)
from mine import mine_session, narrate  # noqa: E402
from lib import discover_grok_sessions  # noqa: E402

SKIP_PROJECTS = frozenset({".cursor"})
SKIP_SESSIONS = frozenset()  # finalize all grok open unless listed
MIN_BODY_RESTORE = 400
REMEDIATED_TAG = "grok-remediation"


def open_sessions() -> list[dict]:
    cutoff_us = int(parse_iso(CUTOFF_ISO).timestamp() * 1_000_000)
    conn = sqlite3.connect(AIM_DB)
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        """
        SELECT s.id, p.name as project, s.cwd, s.agent_kind, s.ended_at,
          (SELECT COUNT(*) FROM observations o WHERE o.session_id = s.id) as obs
        FROM sessions s
        JOIN projects p ON p.id = s.project_id
        WHERE s.ended_at IS NULL AND s.started_at >= ?
        ORDER BY obs DESC
        """,
        (cutoff_us,),
    ).fetchall()
    conn.close()
    out = []
    for r in rows:
        sid = str(uuid.UUID(bytes=r["id"]))
        if sid in SKIP_SESSIONS:
            continue
        if r["project"] in SKIP_PROJECTS:
            continue
        if r["agent_kind"] not in ("grok", "other") and r["obs"] < 20:
            continue
        out.append(
            {
                "session_id": sid,
                "project": r["project"],
                "cwd": r["cwd"] or "",
                "agent_kind": r["agent_kind"],
                "obs": int(r["obs"]),
            }
        )
    return out


def load_mined_body(sid: str) -> str | None:
    p = ensure_out() / "mined" / f"{sid}.json"
    if p.exists():
        data = json.loads(p.read_text(encoding="utf-8"))
        return data.get("body")
    return None


def backup_body(sid: str, project: str, cwd: str, wiki: dict) -> str | None:
    mined = load_mined_body(sid)
    if mined and len(mined) >= MIN_BODY_RESTORE:
        return mined
    info = wiki.get(sid)
    if info and info.get("full_path"):
        try:
            text = Path(info["full_path"]).read_text(encoding="utf-8")
        except OSError:
            text = ""
        if len(text) >= MIN_BODY_RESTORE and (
            REMEDIATED_TAG in text
            or "Remediated from Grok" in text
            or "User requests" in text
        ):
            return text
        if info.get("bytes", 0) >= 2000 and "Empty session" not in info.get("title", ""):
            return text
    # Re-mine from Grok filesystem
    grok = {s.session_id: s for s in discover_grok_sessions()}
    if sid in grok:
        m = mine_session(grok[sid])
        if m and m.get("signal_score", 0) >= 0.1:
            return narrate(m)
    return None


def main() -> int:
    wiki = wiki_page_index()
    sessions = open_sessions()
    log = []
    print(f"Finalizing {len(sessions)} open sessions...")

    for s in sessions:
        sid = s["session_id"]
        project = resolve_project_name(s["project"], s["cwd"])
        backup = backup_body(sid, project, s["cwd"], wiki)
        entry = {"session_id": sid, "project": project, "obs": s["obs"]}

        try:
            end_resp = mcp_call(
                "memory_session_end",
                {
                    "session_id": sid,
                    "project": project,
                    "workspace": "default",
                },
            )
            entry["end_status"] = json.loads(end_resp.split("\n")[0] if "\n" in end_resp else end_resp)
        except Exception as e:
            # Retry without scope validation mismatch
            try:
                end_resp = mcp_call("memory_session_end", {"session_id": sid})
                entry["end_status"] = json.loads(end_resp)
            except Exception as e2:
                entry["error"] = str(e2)
                log.append(entry)
                print(f"  FAIL {sid[:8]} {project}: {e2}")
                continue

        status = entry.get("end_status", {})
        if isinstance(status, dict):
            entry["status"] = status.get("status")
            entry["handoff_id"] = status.get("handoff_id")

        if backup and len(backup) >= MIN_BODY_RESTORE and entry.get("status") in (
            "ended",
            "already_ended",
        ):
            try:
                restore = mcp_call(
                    "memory_write_page",
                    {
                        "path": f"sessions/{sid}.md",
                        "body": backup,
                        "tier": "episodic",
                        "tags": [REMEDIATED_TAG, "phase-2"],
                        "project": project,
                        "workspace": "default",
                    },
                )
                entry["restored"] = True
                entry["restore_response"] = restore[:120]
            except Exception as e:
                entry["restored"] = False
                entry["restore_error"] = str(e)
        else:
            entry["restored"] = False
            entry["restore_skip"] = "no backup or short body"

        log.append(entry)
        print(
            f"  OK {sid[:8]} {project} status={entry.get('status')} "
            f"restored={entry.get('restored')} handoff={entry.get('handoff_id', '')[:8] if entry.get('handoff_id') else '-'}"
        )
        time.sleep(0.8)

    out = ensure_out()
    (out / "finalize-log.json").write_text(json.dumps(log, indent=2), encoding="utf-8")
    ended = sum(1 for x in log if x.get("status") == "ended")
    restored = sum(1 for x in log if x.get("restored"))
    print(f"Done: ended={ended}, restored={restored}, total={len(log)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())