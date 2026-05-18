# Codex Status Detection Fix

This fork keeps Codex sessions from staying in `Waiting` after a turn has actually finished.

## What This Fork Fixes

Codex can print approval review results into the terminal after a command or file edit completes, for example `Request approved` or `allow decision`. Those lines remain in tmux scrollback after Codex returns to the normal input prompt.

AoE status detection used to treat any recent `approve` or `allow` text as a live approval prompt. That made completed Codex sessions appear amber as `Waiting` even though the session was idle and ready for a new message.

The fork now distinguishes live approval prompts from completed approval logs. It also treats Codex's normal bottom `›` prompt after a completed turn as `Idle`, even if repeated warning lines push the earlier approval log out of the recent pane window.

## Waiting Still Works

Codex is still shown as `Waiting` for real input gates:

- explicit yes/no prompts, such as `(y/n)` or `[y/n]`
- prompt text such as `continue?`, `proceed?`, `run command?`, `enter to select`, or `esc to cancel`
- approval questions that include `approve?` or `allow?`
- numbered approval menus with yes/no style options, even when Codex renders the options without a selector glyph
- interrupted-turn prompts that ask the user to tell Codex what to do differently

Completed output that merely mentions approved actions remains `Idle`.

## Why It Exists

AoE reads the recent tmux pane text to classify Codex as running, waiting, or idle. Codex's own completed approval review logs can contain the same words that appear in live approval prompts. The detector has to look for prompt shape, not just approval words.

## Regression Coverage

Keep this focused test set green when changing Codex pane parsing:

```bash
cargo test tmux::status_detection::tests::test_detect_codex_status
```

For broader detector changes, run:

```bash
cargo test status_detection
```

The tests cover stale approval review output, completed turns followed by ordinary `›` prompts, repeated warning tails, ordinary numbered final-answer lists, explicit yes/no prompts, interrupted-turn prompts, and numbered approval menus without a selector glyph.
