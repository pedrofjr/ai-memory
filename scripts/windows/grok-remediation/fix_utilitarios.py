#!/usr/bin/env python3
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from lib import discover_grok_sessions, mcp_call, resolve_project_name
from mine import mine_session, narrate

SID = "019ed614-5238-7a63-be8c-745be9bf1401"
sessions = [s for s in discover_grok_sessions() if s.session_id == SID]
if not sessions:
    raise SystemExit("session not found")
s = sessions[0]
s.wiki_action = "replace"
m = mine_session(s)
if not m:
    raise SystemExit("mine failed")
m["body"] = narrate(m)
m["wiki_action"] = "replace"
proj = resolve_project_name(m["project"], m["cwd"])
print("prompts", len(m["user_prompts"]), "score", m["signal_score"])
r = mcp_call(
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
print(r[:400])