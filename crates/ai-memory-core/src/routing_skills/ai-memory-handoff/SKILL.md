---
name: ai-memory-handoff
description: "Use this skill for any request whose goal is session continuity across agents or time: finding a pending handoff, resuming previous work, saving next-session context, wrapping up, or discarding a mistaken handoff. Trigger by semantic intent rather than exact wording."
---
<!-- ai-memory-managed: routing-skill -->

# ai-memory handoff

Use this skill for single-use cross-session handoffs. Handoffs are for the next agent, not durable project documentation.

## Tools in this cluster

- `memory_handoff_accept` consumes the pending handoff when the user asks where we left off and no already-fetched handoff block is visible.
- `memory_handoff_begin` creates a terse next-session handoff only when the user is wrapping up, ending the session, or explicitly asks to save context for the next session.
- `memory_handoff_cancel` expires a mistaken pending handoff by exact handoff id.
- `memory_session_end` ends the session itself: it writes the `sessions/<id>.md` summary page and opens the handoff, running the same path a real session-end hook takes.

## Single-use handoff behavior

The SessionStart hook usually fetches and consumes any pending handoff before the agent sees its first prompt. If the current context already contains a pending handoff block, answer from that block directly. Do not call the accept tool again to find it in another project, because handoffs are single-use and the tool will normally return null after SessionStart consumed it.

If no pending handoff block is visible and the user asks where we left off, then use the accept tool. Keep the default current-project scope unless the user explicitly names a sibling workspace and project.

## Creating a handoff

Create a handoff only at session end or when the user explicitly asks to save context for the next session. Do not use handoffs for status checks, briefings, project notes, or permanent memory. Keep the summary to two or three concise sentences, and put details in open questions and next steps bullets.

Lifecycle hooks already capture routine prompts and tool calls, so do not manually write a handoff just to record normal progress.

On a shared server, a handoff belongs to the operator who created it. Set `shared: true` only when the user explicitly wants any operator in the project to receive the baton; do not infer sharing from ordinary collaboration prose.

## Ending the session

`memory_handoff_begin` leaves a baton but does not end the session. When the user is actually wrapping up, prefer `memory_session_end`: it closes the session, writes its summary page, and opens the handoff in one step.

It matters most on agents whose CLI never fires a real session-end hook. Grok Build skips it on `/exit` and `/home`, and Codex has no session-end hook at all, so without this call their sessions stay open indefinitely — no summary page, and nothing for the next agent to pick up.

Omit `session_id` to close the newest open session you own in the current project, which is the normal case. On Grok Build also pass `project`, because its MCP calls carry no cwd and the current project cannot be inferred. A session that already ended stays ended rather than being summarised a second time.

## Canceling a handoff

Cancel only when the user asks to discard a handoff or you created one by mistake. Use the exact handoff id returned by the begin tool. Cancellation is idempotent from the user's point of view, but it should still target only the known handoff.

Accept and cancel normally act only on the caller's own plus shared handoffs. `any_owner: true` is a root-only recovery action over another operator's context; use it only on an explicit user request.

## Scope default

Default to the current project. Pass workspace and project together only when the user names a different project. Never pass scope arguments just because the user says this project, here, we, or our work.
