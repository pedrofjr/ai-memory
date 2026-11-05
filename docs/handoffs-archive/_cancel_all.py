"""Cancel all handoffs listed in _export.json via MCP HTTP."""
import json
import pathlib
import urllib.request

EXPORT = pathlib.Path(__file__).with_name("_export.json")
MCP_URL = "http://127.0.0.1:49374/mcp"


def mcp_call(tool: str, arguments: dict) -> dict:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    }
    req = urllib.request.Request(
        MCP_URL,
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "Host": "127.0.0.1:49374",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        raw = json.loads(resp.read().decode("utf-8"))
    if "error" in raw:
        raise RuntimeError(raw["error"])
    content = raw.get("result", {}).get("content", [])
    if content and content[0].get("type") == "text":
        return json.loads(content[0]["text"])
    return raw.get("result", {})


def main() -> None:
    export = json.loads(EXPORT.read_text(encoding="utf-8"))
    ok, fail = 0, []
    for h in export:
        args = {"handoff_id": h["handoff_id"]}
        if h.get("project"):
            args["project"] = h["project"]
            args["workspace"] = h.get("workspace") or "default"
        try:
            mcp_call("memory_handoff_cancel", args)
            ok += 1
            print(f"OK  {h['handoff_id'][:8]}… {h['project']}")
        except Exception as e:
            fail.append((h["handoff_id"], str(e)))
            print(f"FAIL {h['handoff_id'][:8]}… {e}")
    print(f"\nCancelled: {ok}/{len(export)}")
    if fail:
        print("Failures:")
        for hid, err in fail:
            print(f"  {hid}: {err}")
        raise SystemExit(1)


if __name__ == "__main__":
    main()