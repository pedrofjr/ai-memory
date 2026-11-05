# -*- coding: utf-8 -*-
"""Reescreve páginas migrated-clean com mojibake via llm-test + write-page (UTF-8 seguro)."""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

CORRUPT = re.compile(r"├|ÔÇ|Ã§|Ã£|Ã©|Ã³|Ã¡|crit├")
PROMPT_TEMPLATE = """Limpe esta página migrada do agentmemory para wiki ai-memory.
Responda só JSON: {{"title":"...","body_markdown":"...","tags":["migrated-clean"],"suggested_path":"facts/slug.md","tier":"semantic","kind":"fact"}}
kind: fact|decision|gotcha|rule. suggested_path sem imported/. body_markdown sem YAML.
Origem ({import_kind}): {source_path}
--- CONTEÚDO ---
{source_body}
"""


def load_dotenv(repo: Path) -> None:
    for name in (".env", ".env.local"):
        path = repo / name
        if not path.is_file():
            continue
        for line in path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, val = line.partition("=")
            key = key.strip()
            val = val.strip().strip('"').strip("'")
            if key and key not in os.environ:
                os.environ[key] = val


def extract_json(text: str) -> dict:
    m = re.search(r"(?s)```(?:json)?\s*(\{.*?\})\s*```", text)
    if m:
        return json.loads(m.group(1))
    start = text.find("{")
    end = text.rfind("}")
    if start >= 0 and end > start:
        return json.loads(text[start : end + 1])
    raise ValueError("no JSON in LLM output")


def find_exe(repo: Path) -> Path:
    for sub in ("release", "debug"):
        exe = repo / "target" / sub / "ai-memory.exe"
        if exe.is_file():
            return exe
    raise FileNotFoundError("ai-memory.exe not found; run cargo build")


def llm_rewrite(
    exe: Path, repo: Path, provider: str, model: str, source_body: str, source_path: str, kind: str
) -> dict:
    prompt = PROMPT_TEMPLATE.format(
        import_kind=kind,
        source_path=source_path,
        source_body=source_body,
    )
    cmd = [
        str(exe),
        "llm-test",
        "--provider",
        provider,
        "--model",
        model,
        "--prompt",
        prompt,
    ]
    proc = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env={**os.environ, "PYTHONIOENCODING": "utf-8"},
        timeout=600,
        cwd=str(repo),
    )
    out = (proc.stdout or "") + (proc.stderr or "")
    if proc.returncode != 0:
        raise RuntimeError(f"llm-test exit {proc.returncode}: {out[-500:]}")
    return extract_json(out)


def write_page(base_url: str, token: str | None, project: str, path: str, payload: dict) -> None:
    body = {
        "workspace": "default",
        "project": project,
        "path": path,
        "body": payload["body_markdown"],
        "title": payload["title"],
        "tier": payload.get("tier") or "semantic",
        "tags": list(dict.fromkeys(payload.get("tags", []) + ["migrated-clean", "from-imported", "encoding-repair"])),
        "pinned": False,
    }
    data = json.dumps(body, ensure_ascii=False).encode("utf-8")
    headers = {"Content-Type": "application/json; charset=utf-8", "Accept": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(
        f"{base_url.rstrip('/')}/admin/write-page",
        data=data,
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=300) as resp:
        if resp.status != 200:
            raise RuntimeError(f"write-page HTTP {resp.status}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, default=None)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--throttle", type=int, default=45)
    parser.add_argument("--only-target", action="append", default=[])
    args = parser.parse_args()

    repo = Path(os.environ.get("AI_MEMORY_REPO", r"C:\GIT\ai-memory"))
    load_dotenv(repo)
    data_dir = Path(os.environ.get("AI_MEMORY_DATA_DIR", Path.home() / ".ai-memory"))
    wiki = data_dir / "wiki"
    run_dir = args.run_dir or (data_dir / "runs" / "consolidate-inventory-20260527-104637")
    backup_root = run_dir / "backup-wiki-imported"
    batch_log = run_dir / "batch-imported-20260527-112800.jsonl"
    manifest_path = run_dir / "manifest-imported.json"

    provider = (os.environ.get("AI_MEMORY_LLM_PROVIDER") or "").replace("_", "-").lower()
    model = os.environ.get("AI_MEMORY_LLM_MODEL") or os.environ.get("CURSOR_MODEL")
    if not provider or not model:
        print("AI_MEMORY_LLM_PROVIDER e model ausentes", file=sys.stderr)
        return 1

    base_url = os.environ.get("AI_MEMORY_SERVER_URL", "http://127.0.0.1:49374")
    token = os.environ.get("AI_MEMORY_AUTH_TOKEN")

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    path_to_project = {row["path"]: row["project"] for row in manifest["pending"]}
    imported_to_project = dict(path_to_project)

    target_to_imported: dict[str, str] = {}

    def ingest_batch_log(log_path: Path) -> None:
        if not log_path.is_file():
            return
        for line in log_path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            row = json.loads(line)
            if not row.get("ok"):
                continue
            tgt = row.get("target") or row.get("target_path")
            src = row.get("path") or row.get("source_path")
            if tgt and src:
                target_to_imported[tgt] = src
            if src and row.get("project"):
                imported_to_project[src] = row["project"]

    ingest_batch_log(batch_log)
    ingest_batch_log(run_dir / "pilot-imported-20260527-111334.jsonl")

    exe = find_exe(repo)
    repair_log = run_dir / f"encoding-repair-{time.strftime('%Y%m%d-%H%M%S')}.jsonl"

    todo: list[tuple[str, Path]] = []
    for p in sorted(wiki.rglob("*.md")):
        rel = p.relative_to(wiki).as_posix()
        parts = rel.split("/")
        if len(parts) < 4:
            continue
        subpath = "/".join(parts[2:])
        if not re.match(r"^(facts|gotchas|rules)/", subpath):
            continue
        text = p.read_text(encoding="utf-8")
        if not CORRUPT.search(text):
            continue
        if args.only_target and subpath not in args.only_target:
            continue
        todo.append((subpath, p))

    print(f"Páginas corrompidas: {len(todo)} | apply={args.apply}")
    ok = fail = 0
    for i, (target, wiki_path) in enumerate(todo, 1):
        imported_rel = target_to_imported.get(target)
        if not imported_rel:
            print(f"[{i}/{len(todo)}] SKIP {target} (sem mapping no batch log)")
            fail += 1
            continue
        project = imported_to_project.get(imported_rel) or path_to_project.get(imported_rel)
        if not project:
            print(f"[{i}/{len(todo)}] SKIP {target} (sem project no manifest)")
            fail += 1
            continue
        backup_hits = list(backup_root.rglob(imported_rel.replace("/", os.sep)))
        if not backup_hits:
            print(f"[{i}/{len(todo)}] SKIP {target} (backup ausente)")
            fail += 1
            continue
        source = backup_hits[0].read_text(encoding="utf-8")
        kind = "lesson" if "/lessons/" in imported_rel else "memory"
        print(f"[{i}/{len(todo)}] {target} <- {imported_rel} ({project})")
        try:
            if not args.apply:
                ok += 1
                continue
            rewritten = llm_rewrite(exe, repo, provider, model, source, imported_rel, kind)
            path = rewritten.get("suggested_path") or target
            write_page(base_url, token, project, path, rewritten)
            entry = {"target": target, "path": imported_rel, "ok": True, "written": path}
            with repair_log.open("a", encoding="utf-8") as lf:
                lf.write(json.dumps(entry, ensure_ascii=False) + "\n")
            fixed_text = wiki_path.read_text(encoding="utf-8")
            if CORRUPT.search(fixed_text):
                print("  AVISO: ainda com mojibake após write")
            else:
                print("  OK encoding")
            ok += 1
        except (urllib.error.URLError, RuntimeError, ValueError, json.JSONDecodeError) as exc:
            print(f"  ERRO: {exc}")
            with repair_log.open("a", encoding="utf-8") as lf:
                lf.write(
                    json.dumps(
                        {"target": target, "path": imported_rel, "ok": False, "error": str(exc)},
                        ensure_ascii=False,
                    )
                    + "\n"
                )
            fail += 1
        if args.apply and args.throttle > 0 and i < len(todo):
            time.sleep(args.throttle)

    print(f"\nFim: ok={ok} fail={fail} log={repair_log}")
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
