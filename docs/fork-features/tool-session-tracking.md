# Tool Session Tracking and Restore

## What This Fork Adds

This fork can track the underlying tool session for supported host-run agents and reuse that mapping when AoE recreates a broken tmux session.

The feature is opt-in. It is disabled by default and only applies when you enable it in config or in the settings TUI.

Supported tools in v1:

- `claude`
- `codex`
- `opencode`
- `pi`

## Why It Exists

AoE already owns the tmux session lifecycle, but upstream behavior does not persist the matching tool session identity. That creates two gaps:

- AoE can lose the correct `tmux session -> tool session` mapping after the agent creates or switches sessions.
- Restoring a broken tmux session can start a fresh tool session, leaving you to run `/resume` manually and hunt for the right session yourself.

This fork closes that gap by keeping the latest safe mapping and reusing it during restore.

## How It Works

When tracking is enabled, AoE resolves the current tool session from the tool's own local state and stores a mapping on the AoE session:

- a display session ID for the Preview panel
- the exact resume target AoE should pass when it recreates tmux
- a source reference used to detect whether the mapping is still current

AoE refreshes the mapping in two places:

- right after launch or reattach, so the first tool session is captured quickly
- during normal status polling, so in-tool `/resume` changes are picked up automatically

When AoE must restore a broken tmux session, it uses the stored resume target instead of starting a brand-new tool session, as long as the mapping is still safe.

## Safety Rules

This feature prefers correctness over guessing.

- If tracking is disabled, AoE does nothing extra.
- If the mapping is ambiguous, AoE does not guess. It falls back to the normal fresh-start behavior.
- If you already launch the tool with an explicit resume or session argument, AoE does not inject its own auto-resume arguments.
- Sandboxed sessions and wrapped custom commands are skipped in v1.

## TUI Behavior

When tracking is enabled and AoE knows the current tool session, the Preview panel shows:

```text
Session ID: <tool session id>
```

The projects list is unchanged. The session ID is shown only in Preview.

## Configuration

```toml
[session]
tool_session_tracking = true
```

This setting is also available in the settings TUI under the `Session` category as `Tool Session Tracking`.

## Scope And Limits

- This is an opt-in fork feature, not an upstream default behavior.
- v1 supports host-run built-in `claude`, `codex`, `opencode`, and `pi`.
- The mapping is best-effort and local-only. If tool state cannot be read safely, AoE falls back cleanly to normal behavior.
- Existing stored mappings may remain on disk while the feature is disabled, but AoE ignores them until tracking is enabled again.
