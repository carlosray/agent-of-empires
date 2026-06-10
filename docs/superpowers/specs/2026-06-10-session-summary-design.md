# Tool Session Summary in TUI Preview Panel

Date: 2026-06-10
Status: approved

## Goal

Show a very short, one line summary of what each agent session is about in the TUI preview panel, directly under the `Session ID:` line and before the Worktree/Git block. The summary must be visible for sessions in any status, including dead or errored panes (for example after the user terminates the tool with Ctrl+C), so that a red session in the list still carries enough context to recognize what was being worked on.

## Background and upstream research

- AoE already resolves a per session tool session id (`src/session/tool_session.rs`) and persists it on `Instance` as `ToolSession`. The preview panel renders it as `Session ID:` when tool session tracking is enabled.
- Upstream (`agent-of-empires/agent-of-empires`, formerly `njbrake/agent-of-empires`) has no LLM helper endpoint configuration, no session summary feature, and no open issues or PRs planning either (checked 2026-06-10). We build this in the fork as a fork feature.
- Upstream refactored settings into a `SettingsSection` derive macro (#1692) where a new annotated config field auto wires into the TUI, web dashboard, and validation. The fork is being synced with upstream before this feature is implemented, so the config section described below is wired through whatever settings machinery exists after the sync (the derive macro if the sync lands first, otherwise the manual FieldKey wiring described in AGENTS.md).
- Ready made summaries in tool artifacts are scarce: opencode keeps an auto generated `title` column in its sqlite db; claude, codex, and pi store only raw message logs. Therefore the design extracts the first user message as a baseline and optionally upgrades it with an LLM call.

## Scope

Summaries exist wherever tool session tracking works, currently the tools in `SUPPORTED_TOOLS`: claude, codex, opencode, pi. Each tool gets its own extractor next to its existing candidate discovery code, so a tool added to tracking later gets an extractor slot in the same place. Sessions running arbitrary command overrides or unsupported tools have no known artifact format and show no summary line, matching the existing behavior of the `Session ID:` line.

## Configuration

New generic `[llm]` config section (not summary specific; future LLM assisted features add their own `*_model` fields to the same section):

```toml
[llm]
api_base_url = ""    # OpenAI compatible endpoint, e.g. https://api.openai.com/v1
api_token = ""       # bearer token, stored in config.toml like other local secrets
summary_model = ""   # model used for session summaries; empty disables LLM summaries
```

Rules:

- Extraction based summaries (native title or truncated first user message) work with the section completely empty. The LLM call is attempted only when `api_base_url` and `summary_model` are both non empty.
- All three fields are editable in the settings TUI and support per profile overrides with the usual merge semantics, following the repository convention that every configurable field is editable in settings.

## Data model

New persisted struct on `Instance`:

```rust
pub struct ToolSessionSummary {
    /// The tool session display_id this summary was computed for.
    pub display_id: String,
    pub text: String,
    pub state: SummaryState, // Extracted | Final
}
```

- `display_id` keying means a session rebind (successor session id) invalidates the summary and triggers re evaluation.
- `Extracted` means the baseline text is set and an LLM upgrade may still replace it. `Final` means evaluation is complete: either the LLM call finished (success or failure) or no LLM is configured.
- Persisting on `Instance` gives dead and errored sessions their summary for free, and survives AoE restarts.

## Evaluation pipeline

Runs inside the existing background status poller, piggybacking on the tool session refresh cadence:

1. When an instance has a resolved `tool_session` and either no summary or a summary whose `display_id` does not match, attempt extraction:
   - opencode: read `title` from the session row. Placeholder titles (the `New session - <timestamp>` pattern) count as absent; opencode generates a real title after the first exchange, so the poller simply retries next cycle.
   - claude: first line in the session jsonl with `type == "user"` whose text is not tooling noise (skip `<command-message>` and `<command-name>` wrappers, `<local-command-stdout>` blocks, sidechain entries, and meta entries). Content may be a plain string or an array of text blocks.
   - codex: first user message item in the rollout jsonl, skipping injected `<user_instructions>` and `<environment_context>` payloads.
   - pi: first user message entry in the session jsonl.
2. If no usable message exists yet (user has not typed anything), leave the summary unset and retry on the next poll cycle. Once a summary is captured for a display_id it is never recomputed.
3. On successful extraction, immediately store the summary: native title (opencode) or the first user message collapsed to a single line and truncated to 120 characters. State is `Extracted` if an LLM call will follow, otherwise `Final`.
4. If LLM is configured, fire exactly one chat completions request in a detached thread so the poller is never blocked: `POST {api_base_url}/chat/completions` with a bearer token, a short system prompt ("Summarize in at most 10 words what this coding session is about"), and the first user message capped at 4 KB as input. Timeout around 15 seconds, using the existing `reqwest` dependency.
5. The result lands back through the existing poller update channel. Success replaces the text; failure keeps the extracted text and logs via `tracing`. Either way the state becomes `Final`. An in memory set of in flight display_ids in the poller prevents duplicate concurrent calls; no retries beyond the single attempt.

## Rendering

One new line in the preview panel info block (`src/tui/components/preview.rs`), directly under `Session ID:` and before the Worktree block:

```
Summary: <text>
```

- Dimmed label, normal text color, same pattern as the surrounding lines.
- Rendered whenever tool session tracking is enabled and a summary exists, regardless of instance status (Running, Idle, Error, Stopped, dead pane).
- Text is clipped to the panel width at render time; the stored text is already single line and capped at 120 characters.
- The info block height calculation gains one conditional line, mirroring how the `Session ID:` line is counted today.

## Error handling

- Extraction failures (unreadable file, unexpected format) are non fatal: log at debug level and retry on the next cycle.
- LLM failures (network, auth, bad response) are non fatal: keep the extracted text, log at info level, mark `Final` so the endpoint is not hammered.
- A malformed or partial `[llm]` config (base url without model, or vice versa) disables the LLM step without affecting extraction.

## Testing

- Unit tests for each per tool extractor with fixture jsonl/sqlite content, including the noise skipping rules (claude command wrappers, codex environment context, opencode placeholder titles).
- Unit test for the single line truncation helper.
- State machine tests: summary invalidation on display_id change, no recompute once `Final`, no duplicate in flight LLM calls.
- Preview render test asserting the `Summary:` line appears under `Session ID:` when set and is absent when tracking is disabled, following the existing tests in `preview.rs`.
- LLM client test against a local mock server (the update checker tests already use this pattern with a fake endpoint).

## Fork feature documentation

Per repository convention this fork feature ships with:

- A page in `docs/fork-features/` describing the summary line and the `[llm]` section.
- An entry in `docs/fork-features/index.md`.
- Sync entries in `website/scripts/sync-docs.mjs` and `website/src/data/docsNav.ts`.
- A README alignment check for the fork docs section.

## Out of scope

- Web dashboard display of the summary (TUI only for now).
- Summaries for tools outside `SUPPORTED_TOOLS` or sessions with command overrides.
- Re summarization as the session evolves; the summary is evaluated once per tool session id by design.
