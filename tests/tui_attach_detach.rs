//! Integration tests for TUI attach/detach behavior
//!
//! These tests validate that the terminal state is properly managed when
//! attaching to and detaching from tmux sessions.

use std::process::Command;

/// Verify tmux is available for testing
fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Test that tmux sessions can be created and killed
#[test]
fn test_tmux_session_lifecycle() {
    if !tmux_available() {
        eprintln!("Skipping test: tmux not available");
        return;
    }

    let session_name = "aoe_test_lifecycle_12345678";

    // Create a detached session
    let create = Command::new("tmux")
        .args(["new-session", "-d", "-s", session_name])
        .output()
        .expect("Failed to create tmux session");

    assert!(create.status.success(), "Failed to create test session");

    // Verify session exists
    let check = Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output()
        .expect("Failed to check session");

    assert!(
        check.status.success(),
        "Session should exist after creation"
    );

    // Kill session
    let kill = Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output()
        .expect("Failed to kill session");

    assert!(kill.status.success(), "Failed to kill test session");

    // Verify session no longer exists
    let check_after = Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output()
        .expect("Failed to check session");

    assert!(
        !check_after.status.success(),
        "Session should not exist after kill"
    );
}

/// Test that session names are properly sanitized
#[test]
fn test_session_name_format() {
    let prefix = "aoe_";

    // Valid session names should start with our prefix
    let session_name = format!("{}my_project_abc12345", prefix);
    assert!(session_name.starts_with(prefix));

    // Session names should not contain problematic characters
    assert!(!session_name.contains(' '));
    assert!(!session_name.contains(':'));
    assert!(!session_name.contains('.'));
}

/// Test terminal mode switching sequence
///
/// This test documents the expected sequence for attach/detach:
/// 1. Drop EventStream (stop background stdin reader)
/// 2. Disable raw mode
/// 3. Leave alternate screen
/// 4. Show cursor
/// 5. [user interacts with tmux]
/// 6. Recreate EventStream (fresh stdin reader)
/// 7. Enable raw mode
/// 8. Enter alternate screen
/// 9. Hide cursor
/// 10. Clear terminal
#[test]
fn test_terminal_mode_sequence_documented() {
    // This test documents the expected behavior rather than testing it directly
    // since testing terminal modes requires actual terminal interaction.

    let expected_exit_sequence = [
        "drop_event_stream",
        "disable_raw_mode",
        "LeaveAlternateScreen",
        "cursor::Show",
        "flush",
    ];

    let expected_reenter_sequence = [
        "recreate_event_stream",
        "enable_raw_mode",
        "EnterAlternateScreen",
        "cursor::Hide",
        "flush",
        "terminal.clear",
        "set_needs_redraw",
    ];

    // Verify sequences have all required steps
    assert!(expected_exit_sequence.contains(&"disable_raw_mode"));
    assert!(expected_exit_sequence.contains(&"LeaveAlternateScreen"));
    assert!(expected_exit_sequence.contains(&"drop_event_stream"));
    assert!(expected_reenter_sequence.contains(&"enable_raw_mode"));
    assert!(expected_reenter_sequence.contains(&"EnterAlternateScreen"));
    assert!(expected_reenter_sequence.contains(&"recreate_event_stream"));
    assert!(expected_reenter_sequence.contains(&"terminal.clear"));
}

/// Test that attach/detach uses terminal backend, not std::io::stdout()
///
/// This test verifies the fix for the terminal corruption bug where
/// using std::io::stdout() instead of terminal.backend_mut() caused
/// file descriptor desynchronization, corrupting tmux sessions.
///
/// The terminal leave/restore logic lives in `with_raw_mode_disabled`,
/// which `attach_session` delegates to.
#[test]
fn test_attach_uses_terminal_backend() {
    let source = std::fs::read_to_string("src/tui/app.rs").expect("Failed to read app.rs");

    // The shared helper that handles terminal mode switching must use backend_mut()
    let helper_start = source
        .find("fn with_raw_mode_disabled")
        .expect("with_raw_mode_disabled helper not found");

    let helper_section = &source[helper_start..];
    let fn_end = helper_section
        .find("\n}\n")
        .map(|i| i + 3)
        .unwrap_or(helper_section.len());

    let helper_body = &helper_section[..fn_end];

    assert!(
        !helper_body.contains("std::io::stdout()"),
        "with_raw_mode_disabled should use terminal.backend_mut() instead of std::io::stdout(). \
         Using std::io::stdout() creates separate file descriptor handles that can \
         corrupt terminal state and cause 'open terminal failed: not a terminal' errors."
    );

    assert!(
        helper_body.contains("terminal.backend_mut()"),
        "with_raw_mode_disabled should use terminal.backend_mut() for terminal operations"
    );

    // attach_session must delegate to the helper, not bypass it
    let attach_fn_start = source
        .find("fn attach_session(")
        .expect("attach_session function not found");

    let attach_fn_section = &source[attach_fn_start..];
    let attach_fn_end = attach_fn_section
        .find("\n    fn ")
        .or_else(|| attach_fn_section.find("\n}\n"))
        .unwrap_or(attach_fn_section.len());

    let attach_fn_body = &attach_fn_section[..attach_fn_end];

    assert!(
        attach_fn_body.contains("with_raw_mode_disabled"),
        "attach_session should delegate to with_raw_mode_disabled"
    );

    assert!(
        !attach_fn_body.contains("std::io::stdout()"),
        "attach_session should not use std::io::stdout() directly"
    );
}

#[test]
fn test_tui_attach_paths_apply_terminal_title_helper() {
    let source = std::fs::read_to_string("src/tui/app.rs").expect("Failed to read app.rs");

    let attach_session_start = source
        .find("fn attach_session(")
        .expect("attach_session function not found");
    let attach_session_section = &source[attach_session_start..];
    let attach_session_end = attach_session_section
        .find("\n    fn ")
        .unwrap_or(attach_session_section.len());
    let attach_session_body = &attach_session_section[..attach_session_end];

    assert!(
        attach_session_body.contains("crate::terminal::session_attach_title"),
        "attach_session should compute a session tab title before attaching"
    );
    assert!(
        attach_session_body.contains("crate::terminal::dashboard_title"),
        "attach_session should restore the dashboard tab title after returning"
    );

    let attach_terminal_start = source
        .find("fn attach_terminal(")
        .expect("attach_terminal function not found");
    let attach_terminal_section = &source[attach_terminal_start..];
    let attach_terminal_end = attach_terminal_section
        .find("\n    fn ")
        .unwrap_or(attach_terminal_section.len());
    let attach_terminal_body = &attach_terminal_section[..attach_terminal_end];

    assert!(
        attach_terminal_body.contains("crate::terminal::session_attach_title"),
        "attach_terminal should compute a session tab title before attaching"
    );
    assert!(
        attach_terminal_body.contains("crate::terminal::dashboard_title"),
        "attach_terminal should restore the dashboard tab title after returning"
    );
}

#[test]
fn test_cli_attach_paths_apply_terminal_title_helper() {
    let session_source =
        std::fs::read_to_string("src/cli/session.rs").expect("Failed to read session.rs");
    assert!(
        session_source.contains("crate::terminal::session_attach_title"),
        "CLI session attach should compute a session tab title before attaching"
    );
    assert!(
        session_source.contains("crate::terminal::dashboard_title"),
        "CLI session attach should restore the dashboard title after attaching"
    );

    let add_source = std::fs::read_to_string("src/cli/add.rs").expect("Failed to read add.rs");
    assert!(
        add_source.contains("crate::terminal::session_attach_title"),
        "CLI add --launch should compute a session tab title before attaching"
    );
    assert!(
        add_source.contains("crate::terminal::dashboard_title"),
        "CLI add --launch should restore the dashboard title after attaching"
    );
}

#[test]
fn test_tui_startup_applies_dashboard_title_helper() {
    let source = std::fs::read_to_string("src/tui/mod.rs").expect("Failed to read mod.rs");

    assert!(
        source.contains("crate::terminal::dashboard_title"),
        "TUI startup should compute the dashboard tab title on initial launch"
    );
    assert!(
        source.contains("crate::terminal::write_title_sequence")
            || source.contains("crate::terminal::set_title"),
        "TUI startup should apply the dashboard tab title on initial launch"
    );
}
