# -*- coding: utf-8 -*-
"""Corrige mojibake (acentos/c) em páginas facts/gotchas/rules com migrated-clean."""
import re
from pathlib import Path


def needs_fix(t: str) -> bool:
    return bool(re.search(r"Ã.|â€|ï¿½|├|ÔÇ", t))


def fix_text(t: str) -> str:
    if not needs_fix(t):
        return t
    for enc in ("cp1252", "latin-1"):
        try:
            out = t.encode(enc).decode("utf-8")
            if not needs_fix(out):
                return out
        except (UnicodeEncodeError, UnicodeDecodeError):
            pass
    return t


def main() -> None:
    import os

    wiki = Path(os.environ.get("AI_MEMORY_DATA_DIR", Path.home() / ".ai-memory" / "wiki"))
    run = Path(
        os.environ.get(
            "AI_MEMORY_RUN_DIR",
            wiki.parent / "runs" / "consolidate-inventory-20260527-104637",
        )
    )
    run.mkdir(parents=True, exist_ok=True)
    log = run / "encoding-fix-v2.jsonl"

    fixed = failed = 0
    for p in sorted(wiki.rglob("*.md")):
        if not re.search(r"\\(facts|gotchas|rules)\\", str(p)):
            continue
        try:
            text = p.read_text(encoding="utf-8")
        except OSError:
            continue
        if "migrated-clean" not in text or not needs_fix(text):
            continue
        new = fix_text(text)
        if needs_fix(new):
            failed += 1
            continue
        p.write_text(new, encoding="utf-8")
        rel = str(p.relative_to(wiki)).replace("\\", "/")
        with log.open("a", encoding="utf-8") as lf:
            lf.write('{"path":"%s","ok":true}\n' % rel.replace('"', '\\"'))
        fixed += 1
        print("OK", rel)

    print(f"\nCorrigidas: {fixed} | Falhas: {failed}")


if __name__ == "__main__":
    main()
