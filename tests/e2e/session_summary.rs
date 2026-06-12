//! E2E coverage for the tool session summary line in the preview panel
//! (fork feature, docs/fork-features/session-summary.md).
//!
//! Drives the full pipeline through the real binary: the background status
//! poller picks up the persisted tool_session, the claude extractor reads the
//! first user message from the session jsonl under the isolated `$HOME`, the
//! summary is persisted onto the instance, and the preview renders a
//! `Summary:` line under `Session ID:`. The agent stub has already exited by
//! the time the TUI starts, so this also proves the summary shows for dead
//! sessions, the headline use case.

use serial_test::serial;
use std::fs;

use crate::harness::{app_dir_in, require_tmux, TuiTestHarness};

/// Mirror of `claude_project_dir_name` in `src/session/tool_session.rs`:
/// the per-project directory name claude uses under `~/.claude/projects/`.
fn claude_project_dir_name(project_path: &std::path::Path) -> String {
    format!(
        "-{}",
        project_path
            .to_string_lossy()
            .trim_start_matches('/')
            .replace(['/', '\\', ':'], "-")
    )
}

#[test]
#[serial]
fn test_preview_shows_summary_for_dead_session() {
    require_tmux!();

    const DISPLAY_ID: &str = "e2e-summary-session";
    const FIRST_MESSAGE: &str = "Fix the login redirect loop in the dashboard";

    let mut h = TuiTestHarness::new("session_summary");

    // Enable tool session tracking (default off) in the pre-seeded config.
    let config_path = app_dir_in(h.home_path()).join("config.toml");
    let existing = fs::read_to_string(&config_path).expect("read pre-seeded config.toml");
    fs::write(
        &config_path,
        format!("{existing}\n[session]\ntool_session_tracking = true\n"),
    )
    .expect("write tracking config");

    // Create the agent session. The harness's claude stub exits immediately,
    // leaving a dead pane, exactly the scenario the summary must survive.
    let project = h.project_path();
    let add = h.run_cli(&["add", project.to_str().unwrap(), "-t", "SummarySession"]);
    assert!(
        add.status.success(),
        "aoe add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    // Plant a real-format claude session jsonl in the isolated $HOME, at the
    // path the extractor derives from the project path and display_id.
    let claude_dir = h
        .home_path()
        .join(".claude")
        .join("projects")
        .join(claude_project_dir_name(&project));
    fs::create_dir_all(&claude_dir).expect("create claude project dir");
    let now = chrono::Utc::now().to_rfc3339();
    fs::write(
        claude_dir.join(format!("{DISPLAY_ID}.jsonl")),
        format!(
            concat!(
                "{{\"sessionId\":\"{id}\",\"timestamp\":\"{ts}\",\"type\":\"summary\"}}\n",
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{msg}\"}},\"timestamp\":\"{ts}\"}}\n",
            ),
            id = DISPLAY_ID,
            ts = now,
            msg = FIRST_MESSAGE,
        ),
    )
    .expect("write claude session jsonl");

    // Persist a resolved tool_session onto the instance, standing in for the
    // initial bind that normally happens while the agent is alive. The summary
    // pipeline keys off this persisted value, so a TUI started long after the
    // agent died must still evaluate and render the summary.
    let sessions_path = app_dir_in(h.home_path()).join("profiles/default/sessions.json");
    let sessions_str = fs::read_to_string(&sessions_path).expect("read sessions.json");
    let mut sessions: serde_json::Value =
        serde_json::from_str(&sessions_str).expect("parse sessions.json");
    sessions[0]["tool_session"] = serde_json::json!({
        "display_id": DISPLAY_ID,
        "resume_target": DISPLAY_ID,
        "source_ref": DISPLAY_ID,
        "updated_at": now,
    });
    fs::write(&sessions_path, sessions.to_string()).expect("write sessions.json");

    // Widen the window so the preview pane clears the stacked breakpoint;
    // compact layouts hide the info header the summary line lives in.
    h.set_spawn_size(160, 40);
    h.spawn_tui();
    h.wait_for("SummarySession");

    // The first poll cycle covers every tier, so the extraction runs promptly
    // even though the dead session sits in a slow polling tier. Depending on
    // how fast tmux reaps the stub's session, the row lands in the Error
    // (dead pane, info header) or Stopped (placeholder) presentation; the
    // summary line must render in either.
    h.wait_for("Summary:");
    h.assert_screen_contains("Fix the login redirect loop");

    // The summary must persist: it lands in sessions.json so it survives
    // restarts without re-extraction.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let persisted = fs::read_to_string(&sessions_path)
            .ok()
            .is_some_and(|s| s.contains("Fix the login redirect loop"));
        if persisted {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "summary was rendered but never persisted to sessions.json"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
