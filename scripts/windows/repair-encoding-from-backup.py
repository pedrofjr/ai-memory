# -*- coding: utf-8 -*-
"""Regrava páginas com mojibake usando texto do backup imported (sem LLM)."""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

CORRUPT = re.compile(r"├|ÔÇ|Ã§|Ã£|Ã©|Ã³|Ã¡|crit├")


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


def split_frontmatter(text: str) -> tuple[dict[str, str], str]:
    if not text.startswith("---"):
        return {}, text
    parts = text.split("---", 2)
    if len(parts) < 3:
        return {}, text
    meta: dict[str, str] = {}
    for line in parts[1].splitlines():
        if ":" in line:
            k, _, v = line.partition(":")
            meta[k.strip()] = v.strip().strip("'\"")
    return meta, parts[2].lstrip("\n")


def body_from_backup(source: str) -> tuple[str, str]:
    meta, body = split_frontmatter(source)
    title = meta.get("title", "").strip()
    body = body.strip()
    if not body:
        body = title
    if title and not body.startswith("#"):
        md = f"# {title}\n\n{body}"
    else:
        md = body
    return title or "Página migrada", md


def write_page(
    base_url: str,
    token: str | None,
    project: str,
    path: str,
    title: str,
    body: str,
    tags: list[str],
    tier: str = "semantic",
) -> None:
    payload = {
        "workspace": "default",
        "project": project,
        "path": path,
        "body": body,
        "title": title,
        "tier": tier,
        "tags": tags,
        "pinned": False,
    }
    data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    headers = {"Content-Type": "application/json; charset=utf-8", "Accept": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(
        f"{base_url.rstrip('/')}/admin/write-page",
        data=data,
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        if resp.status != 200:
            raise RuntimeError(f"write-page HTTP {resp.status}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, default=None)
    parser.add_argument("--apply", action="store_true")
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

    base_url = os.environ.get("AI_MEMORY_SERVER_URL", "http://127.0.0.1:49374")
    token = os.environ.get("AI_MEMORY_AUTH_TOKEN")

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    path_to_project = {row["path"]: row["project"] for row in manifest["pending"]}
    imported_to_project = dict(path_to_project)
    target_to_imported: dict[str, str] = {}

    def ingest_log(log_path: Path) -> None:
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

    ingest_log(batch_log)
    ingest_log(run_dir / "pilot-imported-20260527-111334.jsonl")

    todo: list[tuple[str, Path]] = []
    for p in sorted(wiki.rglob("*.md")):
        rel = p.relative_to(wiki).as_posix()
        parts = rel.split("/")
        if len(parts) < 4:
            continue
        subpath = "/".join(parts[2:])
        if not re.match(r"^(facts|gotchas|rules)/", subpath):
            continue
        if not CORRUPT.search(p.read_text(encoding="utf-8")):
            continue
        if args.only_target and subpath not in args.only_target:
            continue
        todo.append((subpath, p))

    print(f"Corrompidas: {len(todo)} | apply={args.apply}")
    ok = fail = 0
    for i, (target, wiki_path) in enumerate(todo, 1):
        imported_rel = target_to_imported.get(target)
        if not imported_rel:
            print(f"[{i}] SKIP {target} (sem mapping)")
            fail += 1
            continue
        project = imported_to_project.get(imported_rel)
        if not project:
            print(f"[{i}] SKIP {target} (sem project)")
            fail += 1
            continue
        hits = list(backup_root.rglob(imported_rel.replace("/", os.sep)))
        if not hits:
            print(f"[{i}] SKIP {target} (sem backup)")
            fail += 1
            continue
        source = hits[0].read_text(encoding="utf-8")
        title, body = body_from_backup(source)
        tags = ["migrated-clean", "from-imported", "encoding-repair-backup"]
        print(f"[{i}/{len(todo)}] {target} ({project})")
        if not args.apply:
            ok += 1
            continue
        try:
            write_page(base_url, token, project, target, title, body, tags)
            check = wiki_path.read_text(encoding="utf-8")
            if CORRUPT.search(check):
                print("  AVISO: mojibake restante")
                fail += 1
            else:
                print("  OK")
                ok += 1
        except (urllib.error.URLError, OSError, RuntimeError) as exc:
            print(f"  ERRO: {exc}")
            fail += 1

    print(f"\nFim: ok={ok} fail={fail}")
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
