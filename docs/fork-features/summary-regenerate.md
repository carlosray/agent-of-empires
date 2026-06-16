# Regenerate Session Summary

## What This Fork Adds

[Tool session summaries](session-summary.md) are computed once per resolved tool
session and never recomputed automatically. This fork adds a way to **manually
regenerate** the summary on demand, so you can retry after the first message was
unrepresentative, after a slow or failed LLM call, or simply to get a fresh
phrasing.

The action is available from:

- the **command palette** (Ctrl+K, then "Regenerate session summary"), and
- the session **right-click context menu** ("Regen summary", mnemonic `s`).

It has no dedicated single-key hotkey by design; it lives in the palette and
context menu only.

## The Flow

1. **Confirm modal.** Picking the action opens a confirmation modal (the same
   style as the delete confirmation) showing the session title and **when the
   summary was last updated** (`Last updated: <timestamp>`, or `never`).
2. **Loading.** Choosing Yes fires a single LLM request on a detached thread and
   the modal shows a spinner ("Asking the model for a fresh summary…"). The UI
   never blocks; you can press Esc to stop waiting.
3. **Result.** On success the new one-line summary replaces the old one (and the
   `Last updated` timestamp is refreshed). On failure the modal switches to an
   error state showing the message; any key dismisses it. The existing summary
   is left untouched on failure.

The request timeout is **60 seconds** (more generous than the 15s background
timeout, since you are actively waiting and may be pointing at a slow
self-hosted model).

### Gating

Regenerate is only offered when it can actually run. If a precondition is not
met, an info dialog explains why instead of opening the modal:

- tool session tracking must be enabled for the session,
- the `[llm]` section must be configured (`api_base_url` + `summary_model`),
- the session must have an extractable first message to summarize.

## Where The System Prompt Lives

The system prompt is defined in `src/session/summary.rs` as the `SYSTEM_PROMPT`
constant:

```
Summarize in at most 10 words what this coding session is about.
Reply with only the summary, no preamble or quotes.
```

The same prompt is used for the background upgrade and the manual regenerate, so
there is a single source of truth.

## What Is Sent As Input

The model receives **only the first genuine user message of the session**, not
the entire session JSON/context and not the last message. The extraction is
per tool (`extract_first_message` in `src/session/summary.rs`):

- **claude**: the first real user message from
  `~/.claude/projects/<project-dir>/<display-id>.jsonl`, skipping slash-command
  wrappers (`<command-name>` / `<command-message>` / `<local-command-stdout>`),
  sidechain and meta entries, and tool-result turns.
- **codex**: the first user message in the rollout jsonl, skipping injected
  `<user_instructions>`, `<environment_context>`, and AGENTS.md context blocks.
- **opencode**: the native session title from opencode's sqlite database.
- **pi**: the first user message in the session jsonl.

That message is collapsed to a single line and capped at **4096 bytes** before
being sent as the `user` turn. The request uses `max_tokens: 32` and
`temperature: 0.2`. The returned text is collapsed to one line and truncated to
120 characters for storage; the preview clips further to the panel width.

## Scope And Limits

- TUI only; the web dashboard does not expose a manual regenerate control.
- Supports the same tools as tool session tracking: `claude`, `codex`,
  `opencode`, and `pi`. Sessions with command overrides or other tools cannot
  regenerate (the action is gated off).
- A manual regenerate always calls the LLM (it does not fall back to a plain
  re-extraction), so it requires a configured `[llm]` endpoint even for
  opencode, whose background summary normally uses the native title as-is.
