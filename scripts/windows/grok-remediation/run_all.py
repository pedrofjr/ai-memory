#!/usr/bin/env python3
"""Execute full Grok Build remediation pipeline."""
from __future__ import annotations

import json
import re
import sys
import time
from difflib import SequenceMatcher
from pathlib import Path

# Allow running as script
sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib import (  # noqa: E402
    ARCHIVE,
    DELETE_SESSION_IDS,
    KEEP_SESSIONS,
    discover_grok_sessions,
    ensure_out,
    mcp_call,
    wiki_page_index,
)
from mine import run_mine  # noqa: E402

EMPTY_HANDOFF_RE = re.compile(
    r"^Session ended; \d+ observations recorded\.?$", re.I
)


def index_phase() -> list:
    sessions = discover_grok_sessions()
    wiki = wiki_page_index()
    pages_manifest = []

    for sid, info in wiki.items():
        if sid in KEEP_SESSIONS:
            action = "keep"
        elif sid in DELETE_SESSION_IDS:
            action = "delete"
        elif "teste_grok" in info.get("project", "").lower():
            action = "delete"
        elif EMPTY_HANDOFF_RE.search(info.get("title", "")) or GARBAGE(info):
            action = "delete"
        else:
            action = "keep"
        pages_manifest.append(
            {
                "session_id": sid,
                "path": info["path"],
                "project": info["project"],
                "title": info.get("title", ""),
                "action": action,
            }
        )

    for s in sessions:
        if s.wiki_action == "delete" and not any(p["session_id"] == s.session_id for p in pages_manifest):
            pages_manifest.append(
                {
                    "session_id": s.session_id,
                    "path": f"sessions/{s.session_id}.md",
                    "project": s.project,
                    "title": s.title,
                    "action": "delete",
                }
            )

    out = ensure_out()
    (out / "manifest-sessions.json").write_text(
        json.dumps([s.__dict__ for s in sessions], ensure_ascii=False, indent=2, default=str),
        encoding="utf-8",
    )
    (out / "manifest-pages.json").write_text(
        json.dumps(pages_manifest, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    return sessions


def GARBAGE(info: dict) -> bool:
    blob = (info.get("title", "") + info.get("snippet", "")).lower()
    return any(
        x in blob
        for x in (
            "empty session",
            "lifecycle-only",
            "no durable context",
            "single `stop`",
            "tool telemetry",
        )
    )


def delete_phase(pages_manifest: list) -> list[dict]:
    log = []
    for p in pages_manifest:
        if p["action"] != "delete":
            continue
        sid = p["session_id"]
        if sid in KEEP_SESSIONS:
            continue
        try:
            text = mcp_call(
                "memory_delete_page",
                {
                    "path": p["path"],
                    "project": p["project"],
                    "workspace": "default",
                },
            )
            log.append({"session_id": sid, "project": p["project"], "ok": True, "response": text[:200]})
        except Exception as e:
            log.append({"session_id": sid, "project": p["project"], "ok": False, "error": str(e)})
        time.sleep(0.3)
    out = ensure_out()
    with (out / "delete-log.jsonl").open("w", encoding="utf-8") as f:
        for row in log:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
    return log


def write_phase(mined_list: list[dict]) -> list[dict]:
    from lib import resolve_project_name

    log = []
    for m in mined_list:
        if m.get("wiki_action") == "keep":
            continue
        if m.get("wiki_action") == "delete" and m.get("priority") == "high":
            m["wiki_action"] = "replace"
        if m.get("signal_score", 0) < 0.08 and m.get("priority") != "high":
            continue
        proj = resolve_project_name(m["project"], m.get("cwd") or "")
        try:
            text = mcp_call(
                "memory_write_page",
                {
                    "path": f"sessions/{m['session_id']}.md",
                    "body": m["body"],
                    "tier": "episodic",
                    "tags": ["grok-remediation", "phase-2"],
                    "project": proj,
                    "workspace": "default",
                },
            )
            log.append({"session_id": m["session_id"], "project": m["project"], "ok": True, "response": text[:200]})
        except Exception as e:
            log.append({"session_id": m["session_id"], "project": m["project"], "ok": False, "error": str(e)})
        time.sleep(0.5)
    out = ensure_out()
    with (out / "write-log.jsonl").open("w", encoding="utf-8") as f:
        for row in log:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
    return log


def wiki_corpus() -> list[tuple[str, str, str]]:
    """(project, path, text) for all latest wiki pages."""
    from lib import AIM_WIKI

    corpus = []
    for md in AIM_WIKI.rglob("*.md"):
        if "handoffs-archive" in str(md):
            continue
        try:
            text = md.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        corpus.append(("unknown", str(md.relative_to(AIM_WIKI)), text))
    return corpus


def similarity(a: str, b: str) -> float:
    a = a.lower().strip()[:800]
    b = b.lower().strip()[:800]
    if not a or not b:
        return 0.0
    return SequenceMatcher(None, a, b).ratio()


def handoffs_phase() -> list[dict]:
    export_path = ARCHIVE / "_export.json"
    if not export_path.exists():
        return []
    handoffs = json.loads(export_path.read_text(encoding="utf-8"))
    corpus = wiki_corpus()
    log = []

    for h in handoffs:
        hid = h.get("handoff_id", "")
        summary = (h.get("summary") or "").strip()
        project = h.get("project") or "unknown"
        from_agent = h.get("from_agent") or ""
        sid_hex = h.get("from_session_id_hex") or ""
        sid = ""
        if sid_hex and len(sid_hex) == 32:
            sid = f"{sid_hex[:8]}-{sid_hex[8:12]}-{sid_hex[12:16]}-{sid_hex[16:20]}-{sid_hex[20:]}"

        action = "skip"
        reason = ""
        target_path = ""

        if EMPTY_HANDOFF_RE.match(summary):
            action = "skip"
            reason = "empty grok handoff"
        elif len(summary) < 40:
            action = "skip"
            reason = "summary too short"
        else:
            best = 0.0
            for _, _, text in corpus:
                best = max(best, similarity(summary, text))
            if best >= 0.55:
                action = "skip"
                reason = f"dedup wiki similarity {best:.2f}"
            else:
                # Promote to decisions or sessions
                slug = re.sub(r"[^\w\-]+", "-", project.lower())[:40]
                if sid and from_agent == "grok":
                    target_path = f"sessions/{sid}.md"
                else:
                    target_path = f"decisions/handoff-archive-{hid[:8]}.md"

                title_line = summary.split("\n")[0][:120]
                oq = h.get("open_questions") or []
                ns = h.get("next_steps") or []
                body_parts = [
                    f"# Handoff archive: {title_line}",
                    "",
                    "## Summary",
                    "",
                    summary,
                    "",
                    "## Metadata",
                    "",
                    f"- handoff_id: `{hid}`",
                    f"- from_agent: {from_agent}",
                    f"- project: {project}",
                    f"- cwd: `{h.get('cwd', '')}`",
                    f"- created: {h.get('created_at', '')}",
                ]
                if oq:
                    body_parts += ["", "## Open questions", ""] + [f"- {x}" for x in oq]
                if ns:
                    body_parts += ["", "## Next steps", ""] + [f"- {x}" for x in ns]
                body_parts += ["", "---", "_Promoted from docs/handoffs-archive during Grok remediation phase 2._"]
                body = "\n".join(body_parts)

                try:
                    from lib import resolve_project_name

                    proj = resolve_project_name(project, h.get("cwd") or "")
                    resp = mcp_call(
                        "memory_write_page",
                        {
                            "path": target_path,
                            "body": body,
                            "tier": "episodic" if target_path.startswith("sessions/") else "semantic",
                            "tags": ["handoff-archive", "grok-remediation"],
                            "project": proj,
                            "workspace": "default",
                        },
                    )
                    action = "write"
                    reason = resp[:120]
                    corpus.append((project, target_path, body))
                except Exception as e:
                    action = "error"
                    reason = str(e)
                time.sleep(0.5)

        log.append(
            {
                "handoff_id": hid,
                "project": project,
                "from_agent": from_agent,
                "action": action,
                "reason": reason,
                "target_path": target_path,
            }
        )

    out = ensure_out()
    with (out / "handoff-promote.jsonl").open("w", encoding="utf-8") as f:
        for row in log:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
    return log


def main() -> int:
    print("=== Phase 1: Index ===")
    sessions = index_phase()
    pages = json.loads((ensure_out() / "manifest-pages.json").read_text(encoding="utf-8"))
    print(f"  sessions: {len(sessions)}, pages in manifest: {len(pages)}")

    print("=== Phase 2: Delete ===")
    del_log = delete_phase(pages)
    print(f"  deleted attempts: {len(del_log)}, ok: {sum(1 for x in del_log if x.get('ok'))}")

    print("=== Phase 3-4: Mine + Write ===")
    mined = run_mine(sessions)
    print(f"  mined: {len(mined)}")
    write_log = write_phase(mined)
    print(f"  written: {sum(1 for x in write_log if x.get('ok'))}/{len(write_log)}")

    print("=== Phase 5: Handoffs archive ===")
    ho_log = handoffs_phase()
    print(f"  promoted: {sum(1 for x in ho_log if x.get('action')=='write')}, skipped: {sum(1 for x in ho_log if x.get('action')=='skip')}")

    summary = {
        "sessions_indexed": len(sessions),
        "delete_ok": sum(1 for x in del_log if x.get("ok")),
        "mined": len(mined),
        "write_ok": sum(1 for x in write_log if x.get("ok")),
        "handoffs_write": sum(1 for x in ho_log if x.get("action") == "write"),
        "handoffs_skip": sum(1 for x in ho_log if x.get("action") == "skip"),
    }
    (ensure_out() / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print("=== Done ===", json.dumps(summary))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())