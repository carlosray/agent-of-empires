---
layout: ../../../layouts/Docs.astro
title: Terminal Tab Titles On Attach
description: Rename the outer terminal tab or window title on AoE attach and restore a dashboard title on return.
---

## What This Fork Adds

This fork can rename the outer terminal tab or window title when AoE attaches to a session, then restore a dashboard title when control returns to AoE.

The session title becomes the terminal title while you are attached. When you come back to the dashboard, AoE restores a configurable dashboard title such as `AoE`.

## Why It Exists

When several AoE dashboards or attached sessions are open at once, the terminal tab title is a useful second layer of navigation. This is especially helpful in terminals such as Ghostty, iTerm2, or Terminal.app where tab titles stay visible even when tmux status bars are not.

## How It Works

When the feature is enabled, AoE writes a standard terminal title escape sequence before attach and after return.

- On attach, AoE uses the current session title.
- On return to the dashboard, AoE uses the configured dashboard title.
- Blank dashboard titles fall back to `AoE`.
- The dashboard title is also applied when the TUI starts, so a freshly opened `aoe` dashboard does not stay on the shell's default tab title.

The behavior is used in both TUI and CLI attach flows, including `aoe add -l`.

## Configuration

```toml
[tmux]
rename_terminal_tab_on_attach = true
dashboard_tab_title = "AoE"
```

These settings are also editable in the TUI settings screen.

## Notes And Limits

- This is separate from tmux session names, tmux window names, and the tmux status bar.
- It is best-effort. The terminal emulator must honor title escape sequences for the rename to be visible.
- AoE restores a configured dashboard title, not the exact title that was present before AoE started.
