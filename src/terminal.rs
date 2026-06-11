//! Terminal utilities and helpers

use std::io::{self, Write};

/// Returns the current terminal size as (width, height), or None if unavailable.
pub fn get_size() -> Option<(u16, u16)> {
    crossterm::terminal::size().ok()
}

pub fn resolved_tmux_config(
    profile: &str,
    project_path: &std::path::Path,
) -> crate::session::TmuxConfig {
    crate::session::resolve_config_with_repo(profile, project_path)
        .or_else(|_| crate::session::resolve_config(profile))
        .map(|config| config.tmux)
        .unwrap_or_default()
}

pub fn session_attach_title(
    config: &crate::session::TmuxConfig,
    session_title: &str,
) -> Option<String> {
    config
        .rename_terminal_tab_on_attach
        .then(|| session_title.to_string())
}

pub fn dashboard_title(config: &crate::session::TmuxConfig) -> Option<String> {
    if !config.rename_terminal_tab_on_attach {
        return None;
    }

    let title = config.dashboard_tab_title.trim();
    if title.is_empty() {
        Some("AoE".to_string())
    } else {
        Some(title.to_string())
    }
}

pub fn dashboard_title_for_profile(profile: &str) -> Option<String> {
    let profile = if profile.is_empty() {
        "default"
    } else {
        profile
    };

    crate::session::resolve_config(profile)
        .ok()
        .and_then(|config| dashboard_title(&config.tmux))
}

pub fn write_title_sequence<W: Write>(writer: &mut W, title: &str) -> io::Result<()> {
    let sanitized: String = title
        .chars()
        .filter(|c| *c != '\x1b' && *c != '\x07')
        .collect();
    write!(writer, "\x1b]0;{}\x07", sanitized)?;
    writer.flush()
}

pub fn set_title(title: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    write_title_sequence(&mut handle, title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_attach_title_disabled_returns_none() {
        let config = crate::session::TmuxConfig::default();
        assert_eq!(session_attach_title(&config, "Empire"), None);
    }

    #[test]
    fn test_session_attach_title_enabled_returns_session_title() {
        let config = crate::session::TmuxConfig {
            rename_terminal_tab_on_attach: true,
            ..Default::default()
        };
        assert_eq!(
            session_attach_title(&config, "Empire"),
            Some("Empire".to_string())
        );
    }

    #[test]
    fn test_dashboard_title_uses_configured_value_when_enabled() {
        let config = crate::session::TmuxConfig {
            rename_terminal_tab_on_attach: true,
            dashboard_tab_title: "AoE Board".to_string(),
            ..Default::default()
        };
        assert_eq!(dashboard_title(&config), Some("AoE Board".to_string()));
    }

    #[test]
    fn test_dashboard_title_falls_back_to_default_when_blank() {
        let config = crate::session::TmuxConfig {
            rename_terminal_tab_on_attach: true,
            dashboard_tab_title: "   ".to_string(),
            ..Default::default()
        };
        assert_eq!(dashboard_title(&config), Some("AoE".to_string()));
    }

    #[test]
    fn test_write_title_sequence_uses_osc_title_escape() {
        let mut buf = Vec::new();
        write_title_sequence(&mut buf, "Empire").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "\x1b]0;Empire\x07");
    }
}
