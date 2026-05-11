# Session Archive

Deleted sessions are archived by default in this fork. The archive keeps the session metadata needed to inspect or safely restore a session later.

## What Is Archived

AoE stores the full session record plus archive metadata: session id, title, project path, group, tool, worktree or workspace references, sandbox configuration, tool session id when present, original status, source profile, archive timestamp, cleanup options, and an optional reason field. It does not copy tmux scrollback or agent logs.

## Storage

Each profile has its own archive file:

```text
profiles/<profile>/archive.json
```

The active session list remains in `sessions.json`. Archive writes create `archive.json.bak` in the same profile directory.

## TUI Behavior

- `d` archives an active session when `session.archive_on_delete = true`.
- `Ctrl+d` permanently deletes an active session and bypasses the archive.
- In strict key mode, `Ctrl+Shift+D` is the permanent delete shortcut because `Ctrl+D` opens Diff View.
- `a` toggles Archive View.
- Archive View uses the same grouping and sorting controls as Agent View and Terminal View.
- In Archive View, `r` restores the selected archived session when safe.
- In Archive View, `d` permanently deletes the selected archived session.

## Restore Behavior

Restore is a safe partial restore. AoE recreates the session record with `Stopped` status and clears stale tmux terminal state and stale sandbox container ids. It keeps the project path, group, command, tool settings, worktree or workspace references, and tracked tool session id when present.

Restore is blocked if an active session with the same id already exists or if the project/worktree path no longer exists. When restore is blocked, the archive entry stays in place.

## CLI and Web

The CLI supports:

```bash
aoe archive list
aoe archive show <id-or-title>
aoe archive restore <id-or-title>
aoe archive delete <id-or-title>
aoe remove <id-or-title> --permanent
```

The web dashboard exposes an Archive view and session context actions for Archive and Delete permanently.

## Deleting From tmux

AoE stores the active AoE session id in tmux as a hidden `AOE_INSTANCE_ID` environment variable. A custom tmux binding can use that id to archive the current session from inside the attached agent session:

```text
bind-key X run-shell -b 'id=$(tmux show-environment -h -t "#{session_name}" AOE_INSTANCE_ID | sed -n "s/^AOE_INSTANCE_ID=//p"); test -n "$id" && aoe remove "$id"'
```

With the default settings, this archives the session and closes the tmux session. To bypass the archive, use `aoe remove --permanent "$id"` in the binding.

## Settings

```toml
[session]
archive_on_delete = true
archive_max_entries = 100
```

`archive_max_entries` is enforced per profile. There is no time-based retention setting; count-based pruning is predictable and avoids deleting older metadata solely because it aged out.
