//! Manual "regenerate session summary" dialog.
//!
//! A small state machine over a single modal:
//!   1. **Confirm** mirrors the delete dialog (a `[Yes] [No]` prompt) and shows
//!      when the current summary was last updated.
//!   2. **Loading** shows a spinner while a detached thread waits on the LLM.
//!   3. **Error** shows the failure message until the user dismisses it.
//!
//! The dialog is UI-only: it owns the completion channel but never makes the
//! network call itself. `HomeView` resolves the `[llm]` config plus the session
//! input, spawns the request thread, and hands the receiver to
//! [`RegenerateSummaryDialog::begin_loading`]. Each tick `HomeView` calls
//! [`RegenerateSummaryDialog::poll`]; on success it applies the new text and
//! closes the modal, on failure the dialog transitions to its Error state.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::components::buttons::render_yes_no;
use crate::tui::components::hover::HoverState;
use crate::tui::styles::Theme;

use rattles::presets::prelude as spinners;

/// What a key/click did to the dialog, from `HomeView`'s perspective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegenInput {
    /// Nothing actionable; the dialog stays open.
    Continue,
    /// The user confirmed: `HomeView` must kick off the LLM request.
    Confirm,
    /// The user dismissed the dialog (No / Esc / dismissing an error).
    Cancel,
}

/// Outcome of draining the completion channel on a tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegenPoll {
    /// Still waiting (or not loading yet); keep the modal open.
    Pending,
    /// The LLM returned this text; `HomeView` applies it and closes the modal.
    Done(String),
}

enum State {
    Confirm,
    Loading,
    Error(String),
}

pub struct RegenerateSummaryDialog {
    session_id: String,
    title: String,
    last_updated: Option<DateTime<Utc>>,
    state: State,
    /// Confirm state: true = Yes focused.
    selected: bool,
    rx: Option<Receiver<Result<String, String>>>,
    /// Hard wall-clock deadline; a guard in case the worker thread never
    /// reports (the reqwest client has its own timeout, so this is belt and
    /// suspenders).
    deadline: Option<Instant>,
    yes_button_area: Rect,
    no_button_area: Rect,
    hover: HoverState,
}

impl RegenerateSummaryDialog {
    pub fn new(session_id: String, title: String, last_updated: Option<DateTime<Utc>>) -> Self {
        Self {
            session_id,
            title,
            last_updated,
            state: State::Confirm,
            selected: false,
            rx: None,
            deadline: None,
            yes_button_area: Rect::default(),
            no_button_area: Rect::default(),
            hover: HoverState::default(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.state, State::Loading)
    }

    /// Move into the Loading state, taking ownership of the worker's result
    /// channel and a hard deadline after which a silent worker is treated as a
    /// timeout.
    pub fn begin_loading(&mut self, rx: Receiver<Result<String, String>>, deadline: Instant) {
        self.state = State::Loading;
        self.rx = Some(rx);
        self.deadline = Some(deadline);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> RegenInput {
        match &self.state {
            State::Confirm => match key.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => RegenInput::Cancel,
                KeyCode::Char('y') | KeyCode::Char('Y') => RegenInput::Confirm,
                KeyCode::Enter => {
                    if self.selected {
                        RegenInput::Confirm
                    } else {
                        RegenInput::Cancel
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    self.selected = true;
                    RegenInput::Continue
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    self.selected = false;
                    RegenInput::Continue
                }
                KeyCode::Tab => {
                    self.selected = !self.selected;
                    RegenInput::Continue
                }
                _ => RegenInput::Continue,
            },
            // While loading, Esc abandons the wait (the worker keeps running but
            // its result is dropped when the dialog closes). Everything else is
            // inert so stray keys don't leak to the list underneath.
            State::Loading => match key.code {
                KeyCode::Esc => RegenInput::Cancel,
                _ => RegenInput::Continue,
            },
            // Any dismissing key closes the error.
            State::Error(_) => match key.code {
                KeyCode::Esc
                | KeyCode::Enter
                | KeyCode::Char('y')
                | KeyCode::Char('Y')
                | KeyCode::Char('n')
                | KeyCode::Char('N')
                | KeyCode::Char(' ') => RegenInput::Cancel,
                _ => RegenInput::Continue,
            },
        }
    }

    /// Route a left-click. In Confirm, the `[Yes]`/`[No]` buttons map to
    /// Confirm/Cancel; clicking the error dismisses it. Clicks elsewhere keep
    /// the dialog open.
    pub fn handle_click(&self, col: u16, row: u16) -> RegenInput {
        match &self.state {
            State::Confirm => {
                let pos = Position::from((col, row));
                if self.yes_button_area.contains(pos) {
                    RegenInput::Confirm
                } else if self.no_button_area.contains(pos) {
                    RegenInput::Cancel
                } else {
                    RegenInput::Continue
                }
            }
            State::Error(_) => RegenInput::Cancel,
            State::Loading => RegenInput::Continue,
        }
    }

    pub fn handle_hover(&mut self, col: u16, row: u16) -> bool {
        if !matches!(self.state, State::Confirm) {
            return false;
        }
        self.hover
            .update(col, row, &[self.yes_button_area, self.no_button_area])
    }

    /// Drain the worker channel. Returns `Done(text)` exactly once on success;
    /// on failure or timeout it transitions to the Error state and returns
    /// `Pending` (the modal then renders the message until dismissed).
    pub fn poll(&mut self) -> RegenPoll {
        if !matches!(self.state, State::Loading) {
            return RegenPoll::Pending;
        }
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Ok(text)) => {
                    self.rx = None;
                    return RegenPoll::Done(text);
                }
                Ok(Err(message)) => {
                    self.rx = None;
                    self.state = State::Error(message);
                    return RegenPoll::Pending;
                }
                Err(TryRecvError::Empty) => {
                    if self.deadline.is_some_and(|d| Instant::now() >= d) {
                        self.rx = None;
                        self.state = State::Error("Timed out waiting for the LLM.".to_string());
                    }
                    return RegenPoll::Pending;
                }
                Err(TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.state =
                        State::Error("The summary worker stopped unexpectedly.".to_string());
                    return RegenPoll::Pending;
                }
            }
        }
        RegenPoll::Pending
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let (width, height) = (58, 9);
        let dialog_area = super::centered_rect(area, width, height);
        frame.render_widget(Clear, dialog_area);

        let emphasis = match &self.state {
            State::Error(_) => theme.error,
            _ => theme.waiting,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(emphasis))
            .title(" Regenerate Summary ")
            .title_style(Style::default().fg(emphasis).bold());
        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        match &self.state {
            State::Confirm => self.render_confirm(frame, inner, theme),
            State::Loading => self.render_loading(frame, inner, theme),
            State::Error(message) => Self::render_error(frame, inner, theme, message),
        }
    }

    fn render_confirm(&mut self, frame: &mut Frame, inner: Rect, theme: &Theme) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1), // prompt
                Constraint::Length(1), // spacer
                Constraint::Length(1), // last-updated
                Constraint::Length(1), // spacer
                Constraint::Length(2), // buttons
            ])
            .split(inner);

        let prompt = Paragraph::new(Line::from(vec![
            Span::styled(
                "Regenerate the summary for ",
                Style::default().fg(theme.text),
            ),
            Span::styled(
                truncate(&self.title, 24),
                Style::default().fg(theme.text).bold(),
            ),
            Span::styled("?", Style::default().fg(theme.text)),
        ]))
        .wrap(Wrap { trim: true });
        frame.render_widget(prompt, chunks[0]);

        let updated = match self.last_updated {
            Some(ts) => ts.format("%Y-%m-%d %H:%M UTC").to_string(),
            None => "never".to_string(),
        };
        let updated_line = Paragraph::new(Line::from(vec![
            Span::styled("Last updated: ", Style::default().fg(theme.dimmed)),
            Span::styled(updated, Style::default().fg(theme.text)),
        ]));
        frame.render_widget(updated_line, chunks[2]);

        let (yes, no) = render_yes_no(frame, chunks[4], theme, self.selected, self.hover.current());
        self.yes_button_area = yes;
        self.no_button_area = no;
    }

    fn render_loading(&self, frame: &mut Frame, inner: Rect, theme: &Theme) {
        let spinner = spinners::dots()
            .set_interval(std::time::Duration::from_millis(120))
            .current_frame();
        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("{spinner} "), Style::default().fg(theme.waiting)),
                Span::styled(
                    "Asking the model for a fresh summary…",
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Esc to stop waiting",
                Style::default().fg(theme.dimmed),
            )),
        ];
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    }

    fn render_error(frame: &mut Frame, inner: Rect, theme: &Theme, message: &str) {
        let lines = vec![
            Line::from(Span::styled(
                "Could not regenerate the summary:",
                Style::default().fg(theme.error).bold(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                message.to_string(),
                Style::default().fg(theme.text),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to dismiss",
                Style::default().fg(theme.dimmed),
            )),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            inner,
        );
    }
}

/// Truncate to `max` characters with an ellipsis, for the inline title.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::sync::mpsc;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn dialog() -> RegenerateSummaryDialog {
        RegenerateSummaryDialog::new("sess-1".to_string(), "My Session".to_string(), None)
    }

    #[test]
    fn confirm_yes_via_y_and_enter() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('y'))), RegenInput::Confirm);

        let mut d = dialog();
        // Default focus is No, so Enter cancels.
        assert_eq!(d.handle_key(key(KeyCode::Enter)), RegenInput::Cancel);
        // Move focus to Yes, then Enter confirms.
        d.handle_key(key(KeyCode::Left));
        assert_eq!(d.handle_key(key(KeyCode::Enter)), RegenInput::Confirm);
    }

    #[test]
    fn confirm_cancel_via_n_and_esc() {
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Char('n'))), RegenInput::Cancel);
        let mut d = dialog();
        assert_eq!(d.handle_key(key(KeyCode::Esc)), RegenInput::Cancel);
    }

    #[test]
    fn poll_is_pending_before_loading() {
        let mut d = dialog();
        assert_eq!(d.poll(), RegenPoll::Pending);
    }

    #[test]
    fn poll_returns_done_on_success() {
        let mut d = dialog();
        let (tx, rx) = mpsc::channel();
        d.begin_loading(rx, Instant::now() + std::time::Duration::from_secs(60));
        assert!(d.is_loading());
        tx.send(Ok("a fresh summary".to_string())).unwrap();
        assert_eq!(d.poll(), RegenPoll::Done("a fresh summary".to_string()));
    }

    #[test]
    fn poll_transitions_to_error_on_failure() {
        let mut d = dialog();
        let (tx, rx) = mpsc::channel();
        d.begin_loading(rx, Instant::now() + std::time::Duration::from_secs(60));
        tx.send(Err("endpoint returned 401".to_string())).unwrap();
        assert_eq!(d.poll(), RegenPoll::Pending);
        assert!(matches!(d.state, State::Error(_)));
        // A key dismisses the error.
        assert_eq!(d.handle_key(key(KeyCode::Enter)), RegenInput::Cancel);
    }

    #[test]
    fn poll_times_out_when_deadline_passes() {
        let mut d = dialog();
        let (_tx, rx) = mpsc::channel();
        d.begin_loading(rx, Instant::now() - std::time::Duration::from_secs(1));
        assert_eq!(d.poll(), RegenPoll::Pending);
        assert!(matches!(d.state, State::Error(_)));
    }

    #[test]
    fn loading_swallows_non_esc_keys() {
        let mut d = dialog();
        let (_tx, rx) = mpsc::channel();
        d.begin_loading(rx, Instant::now() + std::time::Duration::from_secs(60));
        assert_eq!(d.handle_key(key(KeyCode::Char('y'))), RegenInput::Continue);
        assert_eq!(d.handle_key(key(KeyCode::Esc)), RegenInput::Cancel);
    }
}
