# Tool Session Summary in TUI Preview Panel

## What This Fork Adds

When tool session tracking is enabled, the TUI preview panel shows a one-line `Summary:` field directly below the `Session ID:` line and above the Worktree/Git block. The summary is visible for sessions in any status, including dead and errored panes, so a red session in the list still carries enough context to identify what it was working on.

Summaries are available for the same tools that tool session tracking supports: `claude`, `codex`, `opencode`, and `pi`. Sessions using command overrides or unsupported tools show no summary line.

This feature is not in upstream.

## TUI Behavior

```text
Session ID: abc123
Summary:    fix the login redirect loop
```

The label is dimmed; the text uses normal foreground color, matching the surrounding info lines. The text is already capped at 120 characters and clipped to the panel width at render time.

## How Summaries Are Computed

A summary is computed once per resolved tool session display ID and is never recomputed for the same ID. Once evaluated it persists on the instance record, so the summary survives AoE restarts and remains visible after the pane has died.

Computation runs inside the existing background status poller, piggybacking on the tool session refresh cadence.

### Extraction per tool

Each supported tool has its own extractor:

- **opencode**: reads the native `title` column from the opencode sqlite database. Placeholder titles matching the `New session - <timestamp>` pattern are treated as absent; the poller retries on the next cycle until a real title appears.
- **claude**: finds the first user message in the session JSONL file whose content is not tooling noise. Skipped entries include `<command-message>` and `<command-name>` wrappers, `<local-command-stdout>` blocks, sidechain entries, and meta entries. Content may be a plain string or an array of text blocks.
- **codex**: reads the first user message item in the rollout JSONL, skipping injected `<user_instructions>` and `<environment_context>` payloads.
- **pi**: reads the first user message entry in the session JSONL.

If no usable message exists yet (the user has not typed anything), the summary is left unset and the poller retries on the next cycle.

The extracted text is collapsed to a single line and truncated to 120 characters. For opencode the native title is used as-is.

### LLM upgrade (optional)

When an OpenAI-compatible LLM endpoint is configured, AoE fires a single chat completions request to replace the extracted text with a tighter summary:

- System prompt: "Summarize in at most 10 words what this coding session is about."
- Input: the first user message capped at 4 KB.
- Timeout: approximately 15 seconds, using the existing `reqwest` dependency.
- One attempt per session; failures keep the extracted text and log at info level.

The request fires in a detached thread so the poller is never blocked. An in-memory set of in-flight display IDs prevents duplicate concurrent calls.

Summary state is tracked as `Extracted` (baseline set, LLM call may follow) or `Final` (evaluation complete, either LLM finished or no LLM is configured). Once `Final`, the summary is not re-evaluated.

## Configuration

A new `[llm]` config section holds the endpoint credentials. This section is generic and not specific to session summaries; future LLM-assisted features add their own `*_model` fields to the same section.

```toml
[llm]
api_base_url = ""    # OpenAI-compatible endpoint, e.g. https://api.openai.com/v1
api_token = ""       # bearer token
summary_model = ""   # model for session summaries; empty disables LLM summaries
```

Rules:

- Extraction-based summaries work with the section empty or absent; no LLM call is needed.
- The LLM call fires only when both `api_base_url` and `summary_model` are non-empty. A partial config (base URL without model, or vice versa) disables the LLM step without affecting extraction.
- All three fields are editable in the settings TUI and support per-profile overrides with the usual merge semantics.

## Error Handling

- Extraction failures (unreadable file, unexpected format) are non-fatal: logged at debug level, retried on the next cycle.
- LLM failures (network errors, auth failures, bad responses) are non-fatal: the extracted text is kept, the error is logged at info level, and the summary is marked `Final` so the endpoint is not called again for that session.

## Scope and Limits

- TUI only; the web dashboard does not display summaries.
- Supports `claude`, `codex`, `opencode`, and `pi`. Sessions with command overrides or tools outside this set show no summary line.
- Summaries are evaluated once per tool session display ID. Re-summarization as the session evolves is out of scope by design.
