# Git Branch Labels for All Git Sessions

## What This Fork Adds

Upstream AoE focuses branch display around managed worktrees and multi-repo workspaces. This fork broadens that behavior so the TUI can show a git branch label for any session whose project path is a git repository, even when the session does not use worktrees.

The branch label is stored with the session and shown in the dashboard list and preview panel.

## Why It Exists

Many sessions in this fork are started directly in an existing repository checkout instead of a dedicated worktree. Those sessions still benefit from a visible branch label in the TUI, especially when several sessions point at related repos or long-lived feature branches.

## How It Works

When a session starts, AoE checks whether the session project path is a git repository.

- If it is not a git repository, no branch label is stored.
- If it is a git repository, AoE resolves a display branch and persists it with the session.
- By default, AoE uses the repository's current branch name.
- If a custom branch command is configured, AoE runs that command in the session repository and uses the first non-empty line from stdout.

That stored branch label stays fixed for the running session. AoE does not silently replace it while the session is open.

## Manual Refresh In The TUI

Press `B` on a selected session in the dashboard to refresh the stored branch label.

- If the session is not backed by a git repository, AoE keeps the existing value and shows an informational message.
- If the newly resolved branch matches the stored value, AoE shows an "already up to date" message.
- If the branch changed, AoE shows a confirmation dialog with the old and new labels.
- Choosing `No` keeps the stored branch that is already shown in the list and preview.
- Choosing `Yes` saves the new branch label and updates the visible TUI state.

This gives you a deliberate refresh point instead of branch labels changing behind your back.

## Configuration

```toml
[worktree]
show_branch_in_tui = true
branch_command = "git rev-parse --abbrev-ref HEAD"
```

`show_branch_in_tui` controls whether the persisted branch label is shown in the session list and preview.

`branch_command` is optional. It runs on the host in the session repository and should print the branch label you want to display. This makes it possible to format or trim branch names without changing AoE itself. For example:

```toml
[worktree]
branch_command = "git rev-parse --abbrev-ref HEAD | sed 's#.*/##'"
```

These settings are also editable in the TUI settings screen.

## Scope And Limits

- This feature applies to any git-backed session, not only worktrees.
- The label is captured on session start and refreshed only when you explicitly ask for it.
- The tmux status bar still uses worktree and workspace metadata only. This fork does not broaden tmux status bar branch display to every git session.
- Non-git sessions remain unchanged.
- Archived sessions (those toggled with `z`) also display their branch label in the list row alongside their session summary.
