I'm using the writing-plans skill to create the implementation plan.

# Mac Focus Automation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a toggleable macOS focus/terminal automation feature and per-session metadata plumbing so AoE can track window/pane focus history and optionally raise the dashboard when attaching.

**Architecture:** The new toggle becomes part of the existing `TmuxConfig`/`SessionConfig` hierarchy (`src/session/config.rs`, `src/session/profile_config.rs`), is editable from the settings TUI, and gates runtime helpers in `src/terminal.rs`/`src/session/instance.rs` that record foreground process IDs before/after attach. Personal metadata lives on `Instance` and flows through `Storage` so the dashboard can remember which pid/window a session last owned.

**Tech Stack:** Rust (config + tui + tmux helpers), toml/serde for config serialization, tui-rs for UI fields, and Markdown docs inside `docs/`.

---

### Task 1: Extend the config schema

**Files:** Modify `src/session/config.rs`, `src/session/profile_config.rs`

**Step 1:** Add a `auto_focus_on_attach: bool` (or similar) field next to `rename_terminal_tab_on_attach` inside `TmuxConfig` (`src/session/config.rs:407-432`) with default `false` and documentation describing macOS focus automation; extend `Default` accordingly.
**Step 2:** Mirror that field inside `TmuxConfigOverride` in `src/session/profile_config.rs:150-220`, ensuring overrides can opt out.
**Step 3:** Update any config-loading tests in `src/session/config.rs:537-700` (or add new ones) to verify the new field round-trips and defaults stay intact.

### Task 2: Surface the toggle in the settings UI

**Files:** Update `src/tui/settings/fields.rs`, `src/tui/settings/mod.rs`, `src/tui/settings/input.rs`

**Step 1:** Introduce a new `FieldKey::AutoFocusTerminal` (or analogous) near the Tmux keys in `fields.rs:42-94`.
**Step 2:** Extend `build_tmux_fields` (`fields.rs:820-1013`) to emit a boolean field with a clear label/description, including its inherited display metadata.
**Step 3:** Update `apply_field_to_global`/`apply_field_to_profile` in `fields.rs:1230-1520` so the field writes through to the new config flag for both scope types (Global/Profile/Repo).
**Step 4:** Confirm `SettingsView::apply_field_to_config` (`src/tui/settings/mod.rs:248-277`) already routes every field through `fields::apply_field_to_config`; no change is needed beyond tests verifying the new field toggles correctly.

### Task 3: Persist per-session metadata

**Files:** Modify `src/session/instance.rs`, `src/session/storage.rs`

**Step 1:** Add optional metadata fields to `Instance` (e.g., `last_foreground_pid: Option<u32>` or `last_window_name: Option<String>`) alongside the existing metadata block (`src/session/instance.rs:88-140`), and ensure `Instance::new` initializes them to `None`.
**Step 2:** Because `serde`/`Storage` already round-trips every `Instance`, no serialization change is required, but add a small unit test (e.g., in `src/session/instance.rs` or `tests/session_storage.rs`) verifying the `Instance` struct serializes the new field.
**Step 3:** Update any session creation paths (e.g., where `Instance` is written to `Storage`) to store initial metadata if the new feature requires it.

### Task 4: Wire runtime behavior to the toggle and metadata

**Files:** Update `src/terminal.rs`, `src/session/instance.rs`, and possibly `src/tmux/session.rs`

**Step 1:** Add helpers in `src/terminal.rs:1-50` that, when `auto_focus_on_attach` is enabled, query `process::get_foreground_pid` (mac-specific) before an attach/detach and store the result on the active `Instance` metadata.
**Step 2:** Where AoE transitions between the dashboard and tmux sessions (e.g., attach helpers inside `src/tmux/session.rs` or `src/session/instance.rs::attach`), call the helper so metadata is refreshed and the PID/window information survives the round-trip.
**Step 3:** On macOS, guard the foreground-pid logic with `cfg(target_os = "macos")` and fall back to `None` elsewhere; rely on `src/process/macos.rs:1-110` for the actual syscall implementation.
**Step 4:** Optionally expose the metadata (last PID/window) on the dashboard so the toggle can skip repeated attachments when nothing changed.

### Task 5: Update docs and messaging

**Files:** Update `docs/guides/tmux-status-bar.md`, `docs/fork-features/terminal-tab-title.md`, `docs/guides/workflow.md`, `docs/quick-start.md`, `docs/guides/apple-containers.md`, `docs/guides/sandbox.md`

**Step 1:** In the tmux status guide, add a subsection describing the toggle, its user-visible effect, and how it differs from `rename_terminal_tab_on_attach` (`docs/guides/tmux-status-bar.md`).
**Step 2:** Mirror the write-up in the terminal-tab-title fork doc so reviewers see the feature in the fork matrix (`docs/fork-features/terminal-tab-title.md`).
**Step 3:** Highlight the behavior in workflows/quick-start so users know the dashboard/terminal-view focus remains consistent when the toggle is on (`docs/guides/workflow.md`, `docs/quick-start.md`).
**Step 4:** Extend the Apple Container/Sandbox guides with notes about macOS-specific tooling and Keychain behavior that the automation relies on (`docs/guides/apple-containers.md`, `docs/guides/sandbox.md`).

## Acceptance Criteria

- The global/profile config files include `auto_focus_on_attach` with default `false`, and the settings view exposes it beside `rename_terminal_tab_on_attach`.
- `Instance` now carries optional focus metadata, and session storage still loads/saves without errors.
- Runtime helpers consult the toggle and update metadata using `process::get_foreground_pid` on macOS, with safe fallbacks on other platforms.
- User-facing docs explain the toggle, its relationship with tab-title renaming, and the macOS focus behavior.

## Verification Steps

1. `cargo fmt` and `cargo clippy` for the touched files.
2. `cargo test --lib session::config session::instance` (or the equivalent) to exercise serialization and defaults.
3. Manual `aoe` run on macOS to ensure the new toggle appears and metadata/logs change when toggled.
4. Confirm docs updates appear in the generated nav (rerun `cargo xtask gen-docs` if required).

## Risks & Mitigations

- **macOS-only logic:** Guard foreground-pid helpers with `cfg(target_os = "macos")` and keep metadata optional so Linux builds remain unaffected.
- **Config compatibility:** Keep the new boolean default `false` so existing configs behave the same; cover it with serde tests.
- **Session storage schema drift:** Add the new metadata as optional fields so older JSON still parses and the data only appears when present.

Plan complete and saved to `docs/plans/2025-09-04-mac-focus-automation.md`. Two execution options:

1. **Subagent-Driven (this session)** - continue here with fresh subagents per task, review between steps.
2. **Parallel Session (separate)** - open a new session running `superpowers:executing-plans` that batches the tasks.

Which approach do you want?