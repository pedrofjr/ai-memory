#!/usr/bin/env python3
"""LLM consolidate remediated Grok session pages (explicit allowlist)."""
from __future__ import annotations

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from backfill_observations import backfill_session, observation_count  # noqa: E402
from lib import discover_grok_sessions, ensure_out, mcp_call  # noqa: E402

# Sessions rewritten in phase-2 remediation (ended in SQLite).
ALLOWLIST: list[tuple[str, str]] = [
    ("019ed614-5238-7a63-be8c-745be9bf1401", "utilitarios"),
    ("019ed70d-22ae-70b0-8f97-1f71ebce8a42", "consisanet"),
    ("019ed6e3-5938-70a1-9ab6-34c73f3dd8ba", "Grok_cp1252_mcp"),
    ("019ed565-965e-7771-9583-4671311c3270", "cpb-js"),
    ("019ed162-61db-7a93-a2a0-475e91ec5448", "pedro.ailton"),
    ("019ed15d-3c05-7c13-9f66-586801a47242", "pedro.ailton"),
    ("019ee11d-d0d6-7d00-9c29-59f510c21802", "ai-memory"),
]

# Never consolidate via this batch (handled separately later).
BLOCKLIST = frozenset(
    {
        "019e8375-411a-7ea6-8cbc-02781b3fb04a",
    }
)

THROTTLE_SEC = 45


def main() -> int:
    out = ensure_out()
    log: list[dict] = []
    queue = [(sid, proj) for sid, proj in ALLOWLIST if sid not in BLOCKLIST]

    grok_by_id = {s.session_id: s for s in discover_grok_sessions()}
    print(f"Consolidating {len(queue)} sessions (blocklist: {len(BLOCKLIST)})")
    for i, (sid, project) in enumerate(queue, 1):
        print(f"[{i}/{len(queue)}] {sid[:8]}… {project}")
        entry = {"session_id": sid, "project": project}
        if observation_count(sid) == 0 and sid in grok_by_id:
            try:
                bf = backfill_session(grok_by_id[sid])
                entry["backfill"] = bf
                print(
                    f"  backfill: inserted={bf.get('inserted')} "
                    f"prompts={bf.get('prompts')} tools={bf.get('tools_sampled')}"
                )
            except Exception as e:
                entry["backfill_error"] = str(e)
                print(f"  backfill FAIL: {e}")
        try:
            text = mcp_call(
                "memory_consolidate",
                {
                    "session_id": sid,
                    "dry_run": False,
                    "multi_page": False,
                },
                req_id=i + 100,
            )
            entry["ok"] = True
            try:
                entry["outcome"] = json.loads(text)
            except json.JSONDecodeError:
                entry["outcome_raw"] = text[:500]
            print(f"  OK: {text[:200]}")
        except Exception as e:
            entry["ok"] = False
            entry["error"] = str(e)
            print(f"  FAIL: {e}")
        log.append(entry)
        if i < len(queue):
            time.sleep(THROTTLE_SEC)

    (out / "consolidate-log.json").write_text(json.dumps(log, indent=2), encoding="utf-8")
    ok = sum(1 for x in log if x.get("ok"))
    print(f"Done: {ok}/{len(log)} succeeded")
    return 0 if ok == len(log) else 1


if __name__ == "__main__":
    raise SystemExit(main())