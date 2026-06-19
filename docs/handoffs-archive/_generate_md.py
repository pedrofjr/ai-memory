import json
import pathlib
import re

EXPORT = pathlib.Path(__file__).with_name("_export.json")
BASE = pathlib.Path(__file__).parent


def slug(s: str, n: int = 48) -> str:
    s = re.sub(r"[^\w\-]+", "-", s.lower()).strip("-")
    return (s[:n] or "handoff").rstrip("-")


def main() -> None:
    export = json.loads(EXPORT.read_text(encoding="utf-8"))
    BASE.mkdir(parents=True, exist_ok=True)

    index_lines = [
        "# Handoffs arquivados (open → cancelados)",
        "",
        "Exportados em **2026-06-19** antes de `memory_handoff_cancel`.",
        f"Total: **{len(export)}** handoffs.",
        "",
        "| Arquivo | handoff_id | Projeto | Agente | Criado |",
        "|---------|------------|---------|--------|--------|",
    ]

    for h in export:
        hid = h["handoff_id"]
        proj = h["project"]
        created = h["created_at"][:10]
        fname = f"{created}_{slug(proj)}_{hid}.md"
        to_agent = h["to_agent"] or "—"
        cwd = h["cwd"] or ""
        from_session = h["from_session_id_hex"] or "—"

        body = [
            f"# Handoff {hid}",
            "",
            "## Metadados",
            "",
            "| Campo | Valor |",
            "|-------|-------|",
            f"| handoff_id | `{hid}` |",
            f"| state (no export) | `{h['state']}` |",
            f"| created_at | {h['created_at']} |",
            f"| workspace | {h['workspace']} |",
            f"| project | {proj} |",
            f"| from_agent | {h['from_agent']} |",
            f"| to_agent | {to_agent} |",
            f"| cwd | `{cwd}` |",
            f"| from_session_id | `{from_session}` |",
            "",
            "## Summary",
            "",
            h["summary"] or "(vazio)",
            "",
        ]
        if h["open_questions"]:
            body += ["## Open questions", ""]
            for q in h["open_questions"]:
                body.append(f"- {q}")
            body.append("")
        if h["next_steps"]:
            body += ["## Next steps", ""]
            for n in h["next_steps"]:
                body.append(f"- {n}")
            body.append("")
        if h["files_touched"]:
            body += ["## Files touched", ""]
            for f in h["files_touched"]:
                body.append(f"- {f}")
            body.append("")

        (BASE / fname).write_text("\n".join(body), encoding="utf-8")
        index_lines.append(
            f"| [{fname}]({fname}) | `{hid[:8]}…` | {proj} | {h['from_agent']} | {created} |"
        )

    (BASE / "INDEX.md").write_text("\n".join(index_lines) + "\n", encoding="utf-8")
    print(f"wrote {len(export)} md files + INDEX.md")


if __name__ == "__main__":
    main()