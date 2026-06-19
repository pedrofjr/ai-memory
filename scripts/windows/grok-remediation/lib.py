#!/usr/bin/env python3
"""Shared helpers for Grok Build remediation pipeline."""
from __future__ import annotations

import json
import re
import sqlite3
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone, timedelta
from pathlib import Path
from typing import Any

import urllib.request

GROK_SESSIONS = Path.home() / ".grok" / "sessions"
AIM_DB = Path.home() / ".ai-memory" / "db" / "memory.sqlite"
AIM_WIKI = Path.home() / ".ai-memory" / "wiki"
REPO = Path(__file__).resolve().parents[3]
OUT = REPO / "target" / "grok-remediation"
ARCHIVE = REPO / "docs" / "handoffs-archive"
CUTOFF_ISO = "2026-06-16T16:50:00+00:00"  # 13:50 BRT
MCP_URL = "http://127.0.0.1:49374/mcp"

KEEP_SESSIONS = frozenset(
    {
        "019edaf8-6f32-7482-add2-8a455430d5e0",
    }
)

DELETE_SESSION_IDS = frozenset(
    {
        "019ee055-cb6b-7e31-b9e7-da5fa2c6d9b3",
        "019ee059-cbac-7c70-b469-20104c526653",
        "019ee05d-1d51-7d90-88a3-15c7910dc285",
        "019edb09-d10b-7953-b0d8-b99101b7af2e",
        "019edb09-d10d-7732-ace1-cb25aeff2ef1",
        "019edbc5-7b4e-70a1-9890-1b300fcadae6",
        "019edbc5-7b50-7441-81df-1d0959302bb1",
        "dadff3b6-8531-5442-90ec-b1951ae247ca",
        "1f3005e2-fddb-56fa-bb40-3140230a6e81",
        "caa0fab2-99bf-563f-88a6-e9abeae37e6c",
    }
)

SKIP_MINE_WRITE = frozenset(
    {
        "019ee055-cb6b-7e31-b9e7-da5fa2c6d9b3",
        "019ee059-cbac-7c70-b469-20104c526653",
        "019ee05d-1d51-7d90-88a3-15c7910dc285",
        "019ee0ea-d023-76f2-b1cd-b901a2e51f0c",
        "019ee0f8-92db-7bd3-8c3a-71b253f0df03",
    }
)

GARBAGE_TITLE_PATTERNS = re.compile(
    r"empty session|lifecycle-only|session ended with a single .stop|"
    r"session ended; \d+ observations|no durable context|tool telemetry",
    re.I,
)


def ensure_out() -> Path:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "mined").mkdir(exist_ok=True)
    return OUT


def blob_uuid(b: bytes) -> str:
    if len(b) == 16:
        return str(uuid.UUID(bytes=b))
    return b.hex()


def parse_iso(s: str) -> datetime:
    if s.endswith("Z"):
        s = s[:-1] + "+00:00"
    return datetime.fromisoformat(s)


def project_from_cwd(cwd: str) -> str:
    p = Path(cwd)
    name = p.name or p.drive.replace(":", "") or "unknown"
    if name.lower() in ("pedro.ailton", "users"):
        return "home"
    return name


def resolve_project_name(name: str, cwd: str = "") -> str:
    """Map filesystem/cwd basename to existing ai-memory project name."""
    conn = sqlite3.connect(AIM_DB)
    rows = [r[0] for r in conn.execute("SELECT DISTINCT name FROM projects").fetchall()]
    conn.close()
    if name in rows:
        return name
    lower_map = {r.lower(): r for r in rows}
    if name.lower() in lower_map:
        return lower_map[name.lower()]
    if cwd:
        norm = cwd.replace("\\", "/").lower()
        for r in rows:
            if r.lower() in norm or norm.endswith("/" + r.lower()):
                return r
    # basename fallbacks
    aliases = {
        "grok_cp1252_mcp": "Grok_cp1252_mcp",
        "opencode_cp1252_plugin": "Opencode_cp1252_plugin",
        "cpb-js": "cpb-js",
        "utilitarios": "utilitarios",
        "consisanet": "consisanet",
        "ai-memory": "ai-memory",
    }
    key = name.lower()
    if key in aliases and aliases[key] in rows:
        return aliases[key]
    for r in rows:
        if r.lower() == key:
            return r
    return name


def mcp_call(tool: str, arguments: dict[str, Any], req_id: int = 1) -> str:
    payload = {
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    }
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        MCP_URL,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=600) as resp:
        raw = resp.read().decode("utf-8", errors="replace")
    m = re.search(r"\{.*\}", raw, re.DOTALL)
    if not m:
        raise RuntimeError(f"MCP empty response for {tool}")
    rpc = json.loads(m.group())
    if rpc.get("error"):
        raise RuntimeError(f"MCP {tool}: {rpc['error']}")
    content = rpc.get("result", {}).get("content", [])
    texts = []
    for c in content:
        if isinstance(c, dict) and c.get("type") == "text":
            texts.append(c.get("text", ""))
    return "\n".join(texts)


def load_db_projects() -> dict[str, str]:
    """Map project name -> hex id string."""
    conn = sqlite3.connect(AIM_DB)
    rows = conn.execute(
        "SELECT p.id, p.name FROM projects p JOIN workspaces w ON w.id = p.workspace_id WHERE w.name = 'default'"
    ).fetchall()
    conn.close()
    return {name: blob_uuid(pid) for pid, name in rows}


def wiki_page_index() -> dict[str, dict[str, Any]]:
    """session_id or path stem -> page info."""
    index: dict[str, dict[str, Any]] = {}
    if not AIM_WIKI.exists():
        return index
    conn = sqlite3.connect(AIM_DB)
    proj_rows = conn.execute(
        """
        SELECT p.id, p.name FROM projects p
        JOIN workspaces w ON w.id = p.workspace_id
        """
    ).fetchall()
    conn.close()
    proj_by_hex = {blob_uuid(r[0]): r[1] for r in proj_rows}

    for md in AIM_WIKI.rglob("sessions/*.md"):
        stem = md.stem
        parts = md.relative_to(AIM_WIKI).parts
        proj_hex = parts[1] if len(parts) >= 3 else ""
        project = proj_by_hex.get(proj_hex, "unknown")
        text = md.read_text(encoding="utf-8", errors="replace")
        title = ""
        for line in text.splitlines()[:20]:
            if line.startswith("# "):
                title = line[2:].strip()
                break
            if line.startswith("title:"):
                title = line.split(":", 1)[1].strip()
        index[stem] = {
            "path": f"sessions/{stem}.md",
            "project": project,
            "title": title,
            "bytes": md.stat().st_size,
            "snippet": text[:1500],
            "full_path": str(md),
        }
    return index


def ai_memory_sessions() -> dict[str, dict[str, Any]]:
    conn = sqlite3.connect(AIM_DB)
    conn.row_factory = sqlite3.Row
    cutoff_us = int(parse_iso(CUTOFF_ISO).timestamp() * 1_000_000)
    rows = conn.execute(
        """
        SELECT s.id, p.name as project, s.cwd, s.agent_kind, s.started_at, s.ended_at,
          (SELECT COUNT(*) FROM observations o WHERE o.session_id = s.id) as obs
        FROM sessions s
        JOIN projects p ON p.id = s.project_id
        WHERE s.started_at >= ?
        """,
        (cutoff_us,),
    ).fetchall()
    conn.close()
    out = {}
    for r in rows:
        sid = blob_uuid(r["id"])
        out[sid] = dict(r)
        out[sid]["id"] = sid
    return out


@dataclass
class GrokSession:
    session_id: str
    cwd: str
    project: str
    title: str
    created_at: str
    num_chat_messages: int
    dir: Path
    priority: str = "medium"
    wiki_action: str = "create"
    ai_obs: int = 0
    agent_kind: str = ""

    def chat_history(self) -> Path | None:
        p = self.dir / "chat_history.jsonl"
        return p if p.exists() else None

    def updates(self) -> Path | None:
        p = self.dir / "updates.jsonl"
        return p if p.exists() else None


def discover_grok_sessions() -> list[GrokSession]:
    cutoff = parse_iso(CUTOFF_ISO)
    aim = ai_memory_sessions()
    wiki = wiki_page_index()
    sessions: list[GrokSession] = []

    for summary_path in GROK_SESSIONS.rglob("summary.json"):
        data = json.loads(summary_path.read_text(encoding="utf-8"))
        info = data.get("info", {})
        sid = info.get("id") or summary_path.parent.name
        cwd = info.get("cwd") or ""
        created = data.get("created_at", "")
        if not created:
            continue
        try:
            created_dt = parse_iso(created)
        except ValueError:
            continue
        if created_dt < cutoff:
            continue

        project = resolve_project_name(project_from_cwd(cwd), cwd)
        title = data.get("generated_title") or data.get("session_summary") or sid[:8]
        nmsg = int(data.get("num_chat_messages") or 0)

        if sid in KEEP_SESSIONS:
            action = "keep"
        elif sid in DELETE_SESSION_IDS or sid in SKIP_MINE_WRITE:
            action = "delete" if sid in DELETE_SESSION_IDS else "skip"
        elif sid in wiki:
            page = wiki[sid]
            garbage = GARBAGE_TITLE_PATTERNS.search(page.get("title", "") + page.get("snippet", ""))
            if garbage and nmsg >= 10:
                action = "replace"
            elif garbage:
                action = "delete"
            elif page.get("bytes", 0) < 900 and nmsg > 10:
                action = "replace"
            else:
                action = "keep" if page.get("bytes", 0) > 2000 else "replace"
        else:
            action = "create" if nmsg > 0 else "skip"

        if "teste_grok" in cwd.lower() or project.lower() == "teste_grok":
            action = "delete" if action != "keep" else action
            priority = "skip"
        elif nmsg >= 20 or project in ("utilitarios", "consisanet", "ai-memory", "Grok_cp1252_mcp", "cpb-js"):
            priority = "high"
        elif nmsg < 3:
            priority = "skip"
        else:
            priority = "medium"

        aim_row = aim.get(sid, {})
        sessions.append(
            GrokSession(
                session_id=sid,
                cwd=cwd,
                project=project if project != "home" else aim_row.get("project", "home"),
                title=title,
                created_at=created,
                num_chat_messages=nmsg,
                dir=summary_path.parent,
                priority=priority,
                wiki_action=action,
                ai_obs=int(aim_row.get("obs") or 0),
                agent_kind=aim_row.get("agent_kind") or "",
            )
        )

    sessions.sort(key=lambda s: s.created_at)
    return sessions