/**
 * stdin: JSON BridgeRequest
 * stdout: JSON BridgeResponse
 * env: CURSOR_API_KEY (required), AI_MEMORY_CURSOR_CHILD=1 (set by Rust parent)
 */
import { Agent, CursorAgentError } from "@cursor/sdk";

const CANCEL_WAIT_MS = 5_000;

async function readStdin() {
  const chunks = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function cancelRunBounded(run, cancelWaitMs) {
  await Promise.race([
    run.cancel(),
    new Promise((_, reject) => {
      setTimeout(() => reject(new Error("cancel_wait_exceeded")), cancelWaitMs);
    }),
  ]);
}

async function waitRunWithCancel(run, timeoutMs) {
  let timer;
  const message = `Cursor SDK request timed out after ${timeoutMs}ms`;
  const waitPromise = run.wait().finally(() => {
    if (timer !== undefined) clearTimeout(timer);
  });
  const timeoutPromise = new Promise((_, reject) => {
    timer = setTimeout(() => {
      void (async () => {
        try {
          await cancelRunBounded(run, CANCEL_WAIT_MS);
        } catch {
          /* best-effort */
        }
        reject(new Error(message));
      })();
    }, timeoutMs);
  });
  try {
    return await Promise.race([waitPromise, timeoutPromise]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

function buildModelSelection(modelId, modelFast, modelEffort) {
  const params = [{ id: "fast", value: modelFast ? "true" : "false" }];
  const effort = typeof modelEffort === "string" ? modelEffort.trim() : "";
  if (effort) {
    params.unshift({ id: "effort", value: effort });
  }
  return { id: modelId, params };
}

function buildPrompt(input) {
  const parts = [
    "You are a background memory worker for ai-memory.",
    "Reply with ONLY the output requested. No preamble unless instructed.",
  ];
  if (input.system) {
    parts.push("", "## System", input.system);
  }
  for (const m of input.messages ?? []) {
    const label = m.role === "assistant" ? "Assistant" : "User";
    parts.push("", `## ${label}`, m.content);
  }
  if (input.schema) {
    parts.push(
      "",
      "## Output format",
      "Return ONLY valid JSON (no markdown fences) matching this JSON Schema:",
      JSON.stringify(input.schema),
    );
  }
  return parts.join("\n");
}

function extractJsonObject(text) {
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start === -1 || end <= start) {
    throw new Error("structured response did not contain a JSON object");
  }
  return JSON.parse(text.slice(start, end + 1));
}

function fail(error) {
  process.stdout.write(JSON.stringify({ ok: false, error: String(error) }));
  process.exit(1);
}

function ok(body) {
  process.stdout.write(JSON.stringify({ ok: true, ...body }));
  process.exit(0);
}

async function main() {
  const apiKey = process.env.CURSOR_API_KEY?.trim();
  if (!apiKey) {
    fail("CURSOR_API_KEY is not set");
  }

  let input;
  try {
    input = JSON.parse(await readStdin());
  } catch (e) {
    fail(`invalid stdin JSON: ${e}`);
  }

  const model = buildModelSelection(
    input.model ?? "composer-2.5",
    Boolean(input.modelFast),
    input.modelEffort,
  );
  const cwd = input.cwd ?? process.cwd();
  const timeoutMs = Number(input.timeoutMs) > 0 ? Number(input.timeoutMs) : 120_000;
  const prompt = buildPrompt(input);

  let agent;
  try {
    agent = await Agent.create({
      apiKey,
      model,
      local: { cwd },
    });
  } catch (err) {
    if (err instanceof CursorAgentError) {
      fail(`Cursor SDK startup failed: ${err.message}`);
    }
    fail(err);
  }

  try {
    const run = await agent.send(prompt, { model });
    const result = await waitRunWithCancel(run, timeoutMs);

    if (result.status === "error") {
      fail(`Cursor agent run failed (run ${result.id ?? "unknown"})`);
    }
    if (result.status === "cancelled") {
      fail("Cursor agent run cancelled");
    }

    const text = (result.result ?? "").trim();
    if (input.schema) {
      const json = extractJsonObject(text);
      ok({ json, model: input.model ?? "composer-2.5" });
    } else {
      ok({ text, model: input.model ?? "composer-2.5" });
    }
  } catch (err) {
    if (err instanceof CursorAgentError) {
      fail(`Cursor SDK error: ${err.message}`);
    }
    fail(err);
  } finally {
    try {
      agent.close();
    } catch {
      /* ignore */
    }
  }
}

main().catch((e) => fail(e));
