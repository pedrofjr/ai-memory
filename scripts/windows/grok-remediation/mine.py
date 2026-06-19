#!/usr/bin/env python3
"""Mine Grok session JSONL into structured JSON."""
from __future__ import annotations

import json
import re
from collections import Counter
from pathlib import Path
from typing import Any

from lib import GrokSession, SKIP_MINE_WRITE, ensure_out

USER_QUERY_RE = re.compile(r"<user_query>\s*(.*?)\s*</user_query>", re.DOTALL | re.I)
SYNTHETIC_RE = re.compile(
    r"conversation was summarized|summary_content|Your conversation was summarized",
    re.I,
)
MAX_PROMPTS = 80
MAX_PROMPT_CHARS = 2000


def extract_user_text(raw: str) -> str | None:
    if SYNTHETIC_RE.search(raw):
        return None
    m = USER_QUERY_RE.search(raw)
    if m:
        text = m.group(1).strip()
    else:
        text = raw.strip()
    if not text or len(text) < 3:
        return None
    if text.startswith("<") and "user_info" in text[:500]:
        return None
    if len(text) > MAX_PROMPT_CHARS:
        text = text[:MAX_PROMPT_CHARS] + "…"
    return text


def mine_chat_history(path: Path) -> list[dict[str, Any]]:
    prompts: list[dict[str, Any]] = []
    seen: set[str] = set()
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if obj.get("type") != "user":
            continue
        content = obj.get("content")
        chunks: list[str] = []
        if isinstance(content, str):
            chunks = [content]
        elif isinstance(content, list):
            for part in content:
                if isinstance(part, dict) and part.get("type") == "text":
                    chunks.append(part.get("text", ""))
        for raw in chunks:
            text = extract_user_text(raw)
            if not text:
                continue
            key = text[:200]
            if key in seen:
                continue
            seen.add(key)
            prompts.append({"text": text, "order": len(prompts) + 1})
            if len(prompts) >= MAX_PROMPTS:
                return prompts
    return prompts


def mine_updates_user_chunks(path: Path) -> list[str]:
    texts: list[str] = []
    seen: set[str] = set()
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        update = (obj.get("params") or {}).get("update") or {}
        if update.get("sessionUpdate") != "user_message_chunk":
            continue
        content = update.get("content") or {}
        raw = content.get("text", "") if isinstance(content, dict) else ""
        text = extract_user_text(raw)
        if not text:
            continue
        key = text[:200]
        if key in seen:
            continue
        seen.add(key)
        texts.append(text)
        if len(texts) >= MAX_PROMPTS:
            break
    return texts


def mine_updates(path: Path) -> tuple[Counter[str], list[str]]:
    tools: Counter[str] = Counter()
    paths: list[str] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        params = obj.get("params") or {}
        update = params.get("update") or {}
        kind = update.get("sessionUpdate") or ""
        if kind == "tool_call":
            title = update.get("title") or update.get("toolCallId", "tool")
            tools[title] += 1
            raw = update.get("rawInput") or {}
            for key in ("path", "target_directory", "command", "glob_pattern"):
                val = raw.get(key)
                if isinstance(val, str) and val and len(val) < 260:
                    paths.append(val)
        elif kind == "tool_call_update":
            title = update.get("title")
            if title:
                tools[title.split("`")[0].strip()[:60]] += 1
            for loc in update.get("locations") or []:
                p = loc.get("path")
                if p:
                    paths.append(p)
    uniq_paths = list(dict.fromkeys(paths))[:40]
    return tools, uniq_paths


def signal_score(prompts: list, tools: Counter[str], nmsg: int) -> float:
    score = 0.0
    score += min(len(prompts) * 0.05, 0.5)
    score += min(sum(tools.values()) * 0.002, 0.3)
    score += min(nmsg * 0.001, 0.2)
    return round(min(score, 1.0), 3)


def mine_session(sess: GrokSession) -> dict[str, Any] | None:
    if sess.session_id in SKIP_MINE_WRITE:
        return None
    if sess.priority == "skip" and sess.wiki_action not in ("create", "replace"):
        return None

    prompts: list[dict[str, Any]] = []
    tools: Counter[str] = Counter()
    paths: list[str] = []

    ch = sess.chat_history()
    if ch:
        prompts = mine_chat_history(ch)
    up = sess.updates()
    if up:
        t, p = mine_updates(up)
        tools.update(t)
        paths = p
        extra = mine_updates_user_chunks(up)
        seen = {x["text"][:200] for x in prompts}
        for text in extra:
            key = text[:200]
            if key in seen:
                continue
            seen.add(key)
            prompts.append({"text": text, "order": len(prompts) + 1})
            if len(prompts) >= MAX_PROMPTS:
                break

    if not prompts and not tools and sess.num_chat_messages < 2:
        return None

    return {
        "session_id": sess.session_id,
        "project": sess.project,
        "cwd": sess.cwd,
        "title": sess.title,
        "created_at": sess.created_at,
        "num_chat_messages": sess.num_chat_messages,
        "user_prompts": prompts,
        "tools": [{"name": k, "count": v} for k, v in tools.most_common(30)],
        "paths": paths,
        "signal_score": signal_score(prompts, tools, sess.num_chat_messages),
        "wiki_action": sess.wiki_action,
        "priority": sess.priority,
    }


def narrate(mined: dict[str, Any]) -> str:
    title = mined["title"] or mined["session_id"][:8]
    lines = [f"# {title}", ""]
    prompts = mined.get("user_prompts") or []
    if prompts:
        lines.append("## User requests")
        lines.append("")
        for i, p in enumerate(prompts, 1):
            text = p["text"].replace("\n", " ").strip()
            if len(text) > 500:
                text = text[:500] + "…"
            lines.append(f"{i}. {text}")
        lines.append("")

    tools = mined.get("tools") or []
    paths = mined.get("paths") or []
    if tools or paths:
        lines.append("## What happened")
        lines.append("")
        if tools:
            lines.append("Tool activity (from Grok `updates.jsonl`):")
            for t in tools[:20]:
                lines.append(f"- `{t['name']}`: {t['count']}")
            lines.append("")
        if paths:
            lines.append("Paths touched (sample):")
            for p in paths[:15]:
                lines.append(f"- `{p}`")
            lines.append("")

    lines.append("## Durable context")
    lines.append("")
    lines.append(
        f"Grok Build session `{mined['session_id']}` in `{mined['cwd']}` "
        f"({mined.get('num_chat_messages', 0)} chat messages). "
        "Page rebuilt from `~/.grok/sessions` during remediation phase 2 "
        "(ai-memory hook capture was incomplete in this period)."
    )
    lines.append("")

    if prompts:
        last = prompts[-1]["text"].replace("\n", " ").strip()
        if len(last) > 400:
            last = last[:400] + "…"
        lines.append("## Open questions")
        lines.append("")
        lines.append(f"- Continue from: {last}")
        lines.append("")

    lines.append("---")
    lines.append("_Remediated from Grok Build session files (phase 2)._")
    return "\n".join(lines)


def run_mine(sessions: list[GrokSession]) -> list[dict[str, Any]]:
    ensure_out()
    results: list[dict[str, Any]] = []
    for sess in sessions:
        if sess.wiki_action == "keep" and sess.session_id not in SKIP_MINE_WRITE:
            continue
        if sess.wiki_action == "delete" and sess.priority != "high":
            continue
        if sess.wiki_action == "delete" and sess.num_chat_messages < 10:
            continue
        mined = mine_session(sess)
        if not mined:
            continue
        if mined["signal_score"] < 0.05 and sess.priority != "high":
            continue
        out_path = ensure_out() / "mined" / f"{sess.session_id}.json"
        mined["body"] = narrate(mined)
        out_path.write_text(json.dumps(mined, ensure_ascii=False, indent=2), encoding="utf-8")
        results.append(mined)
    return results