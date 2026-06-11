//! Tool session summaries: a one line description of what an agent session is
//! about, shown in the TUI preview panel under `Session ID:`.
//!
//! Two stages. First a cheap **extraction** from the tool's own session
//! artifacts (opencode's native sqlite title, or the first user message for
//! claude/codex/pi), collapsed to a single line. Then, when an `[llm]` endpoint
//! is configured, an optional **LLM upgrade** that replaces the extracted text
//! with a short generated summary. The upgrade runs in a detached thread via
//! [`SummaryService`] so neither the poller nor the UI ever blocks on the
//! network. Each summary is evaluated once per tool session `display_id`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

use super::config::LlmConfig;
use super::instance::{Instance, SummaryState, ToolSession, ToolSessionSummary};

/// Max stored summary length, in characters. The preview clips further to the
/// panel width at render time.
const MAX_SUMMARY_LEN: usize = 120;
/// Upper bound on the first-message text sent to the LLM, in bytes.
const LLM_INPUT_CAP_BYTES: usize = 4096;
/// Per-request timeout for the LLM call.
const LLM_TIMEOUT_SECS: u64 = 15;

const SYSTEM_PROMPT: &str = "Summarize in at most 10 words what this coding session is about. \
     Reply with only the summary, no preamble or quotes.";

/// Collapse all whitespace runs to single spaces and truncate to `max`
/// characters, appending an ellipsis when the text is cut.
pub fn collapse_single_line(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", truncated.trim_end())
}

/// Whether `instance` needs a fresh summary for `tool_session`: it has none, or
/// the stored one was computed for a different `display_id` (a session rebind).
pub fn needs_eval(instance: &Instance, tool_session: &ToolSession) -> bool {
    match &instance.tool_session_summary {
        Some(existing) => existing.display_id != tool_session.display_id,
        None => true,
    }
}

/// Compute a fresh baseline summary for `instance`/`tool_session`, or `None`
/// when no summary is needed yet (already evaluated for this `display_id`, or no
/// usable message has been written to the session artifact yet).
///
/// `llm_enabled` decides the resulting state: a first-message extraction that an
/// LLM will upgrade is `Extracted`; an opencode native title (already a good
/// summary) or any extraction with no LLM configured is `Final`.
pub fn compute_summary_update(
    instance: &Instance,
    tool_session: &ToolSession,
    llm_enabled: bool,
) -> Option<ToolSessionSummary> {
    if !needs_eval(instance, tool_session) {
        return None;
    }
    let raw = extract_first_message(instance, tool_session)?;
    let text = collapse_single_line(&raw, MAX_SUMMARY_LEN);
    if text.is_empty() {
        return None;
    }
    // opencode hands us its own generated title, which is already a summary; do
    // not spend an LLM call rewriting it.
    let native_title = instance.tool == "opencode";
    let state = if llm_enabled && !native_title {
        SummaryState::Extracted
    } else {
        SummaryState::Final
    };
    Some(ToolSessionSummary {
        display_id: tool_session.display_id.clone(),
        text,
        state,
    })
}

/// Extract the raw baseline text for a tool session: opencode's native title or
/// the first genuine user message for the other tools. Returns `None` when the
/// artifact is missing, unreadable, or has no usable message yet.
pub fn extract_first_message(instance: &Instance, tool_session: &ToolSession) -> Option<String> {
    match instance.tool.as_str() {
        "claude" => extract_claude(Path::new(&instance.project_path), &tool_session.display_id),
        "codex" => first_user_text(Path::new(&tool_session.source_ref), codex_line_text),
        "opencode" => extract_opencode(&tool_session.source_ref),
        "pi" => first_user_text(Path::new(&tool_session.source_ref), pi_line_text),
        _ => None,
    }
}

/// Walk a jsonl session file line by line, returning the first line for which
/// `extract` yields a usable user message.
fn first_user_text(path: &Path, extract: fn(&Value) -> Option<String>) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(text) = extract(&value) {
            return Some(text);
        }
    }
    None
}

fn extract_claude(project_path: &Path, display_id: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let path = home
        .join(".claude")
        .join("projects")
        .join(super::tool_session::claude_project_dir_name(project_path))
        .join(format!("{display_id}.jsonl"));
    first_user_text(&path, claude_line_text)
}

/// A claude jsonl `user` entry. Skips sidechain and meta entries, tool-result
/// turns (which carry no typed text), and slash-command wrappers.
fn claude_line_text(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("user") {
        return None;
    }
    if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let content = value.get("message")?.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => join_text_blocks(blocks, "text")?,
        _ => return None,
    };
    clean_user_text(&text).filter(|t| !is_claude_command_noise(t))
}

/// A codex rollout `response_item` user message. Skips the injected
/// `<user_instructions>` / `<environment_context>` / AGENTS.md context blocks
/// that precede the first real prompt.
fn codex_line_text(value: &Value) -> Option<String> {
    let payload = value.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    if payload.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    for block in payload.get("content")?.as_array()? {
        if block.get("type").and_then(Value::as_str) != Some("input_text") {
            continue;
        }
        let text = block.get("text").and_then(Value::as_str).unwrap_or("");
        if is_codex_injected(text) {
            continue;
        }
        if let Some(cleaned) = clean_user_text(text) {
            return Some(cleaned);
        }
    }
    None
}

/// A pi jsonl `message` entry with `role == "user"`.
fn pi_line_text(value: &Value) -> Option<String> {
    let message = value.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let content = message.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => join_text_blocks(blocks, "text")?,
        _ => return None,
    };
    clean_user_text(&text)
}

/// Join the `text` of every block of the given `kind` in a content array,
/// returning `None` if there are no such blocks (e.g. a tool-result turn).
fn join_text_blocks(blocks: &[Value], kind: &str) -> Option<String> {
    let mut parts = Vec::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some(kind) {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                parts.push(text);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn clean_user_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_claude_command_noise(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<command-name>")
        || t.starts_with("<command-message>")
        || t.starts_with("<local-command-stdout>")
        || t.contains("<local-command-stdout>")
}

fn is_codex_injected(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<user_instructions>")
        || t.starts_with("<environment_context>")
        || t.starts_with("<INSTRUCTIONS>")
        || t.starts_with("# AGENTS.md instructions for")
}

fn extract_opencode(session_id: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let db = home
        .join(".local")
        .join("share")
        .join("opencode")
        .join("opencode.db");
    if !db.exists() {
        return None;
    }
    let query = format!(
        "select title from session where id = '{}';",
        session_id.replace('\'', "''")
    );
    let output = std::process::Command::new("sqlite3")
        .arg(&db)
        .arg(query)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if title.is_empty() || is_opencode_placeholder(&title) {
        return None;
    }
    Some(title)
}

/// opencode seeds a row with a `New session - <timestamp>` title and replaces it
/// with a generated one after the first exchange. Treat the placeholder as
/// absent so the poller retries until the real title lands.
fn is_opencode_placeholder(title: &str) -> bool {
    title.starts_with("New session - ")
}

/// Call the configured OpenAI-compatible endpoint to summarize `input`. Runs a
/// single blocking request on a private current-thread runtime, so it must be
/// invoked from a detached thread (see [`SummaryService::spawn`]).
pub fn summarize_via_llm(config: &LlmConfig, input: &str) -> anyhow::Result<String> {
    let base = config.api_base_url.trim().trim_end_matches('/');
    let url = format!("{base}/chat/completions");
    let capped = cap_bytes(input, LLM_INPUT_CAP_BYTES);
    let body = serde_json::json!({
        "model": config.summary_model,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": capped },
        ],
        "max_tokens": 32,
        "temperature": 0.2,
    });
    let token = config.api_token.trim().to_string();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(LLM_TIMEOUT_SECS))
            .build()?;
        let mut request = client.post(&url).json(&body);
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            anyhow::bail!("llm endpoint returned {}", response.status());
        }
        let json: Value = response.json().await?;
        let text = json
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if text.is_empty() {
            anyhow::bail!("llm returned an empty summary");
        }
        Ok(text)
    })
}

fn cap_bytes(input: &str, max: usize) -> &str {
    if input.len() <= max {
        return input;
    }
    let mut end = max;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    &input[..end]
}

/// A request to upgrade an extracted summary with the LLM, carried from the
/// poller (which resolved the config) to the app (which dispatches it).
#[derive(Debug, Clone)]
pub struct SummaryLlmRequest {
    pub display_id: String,
    pub input: String,
    pub config: LlmConfig,
}

/// Outcome of a completed LLM upgrade. An empty `text` means the call failed;
/// the caller keeps the extracted text and marks the summary `Final`.
#[derive(Debug, Clone)]
pub struct SummaryLlmResult {
    pub display_id: String,
    pub text: String,
}

/// Drives detached LLM summary upgrades. Owns the completion channel and an
/// in-flight set so at most one request runs per `display_id` and completed
/// upgrades can be drained without blocking.
pub struct SummaryService {
    tx: mpsc::Sender<SummaryLlmResult>,
    rx: mpsc::Receiver<SummaryLlmResult>,
    in_flight: HashSet<String>,
}

impl SummaryService {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            in_flight: HashSet::new(),
        }
    }

    /// Spawn the detached request unless one is already in flight for this
    /// `display_id`. On completion the result is delivered via [`Self::drain`].
    pub fn spawn(&mut self, request: SummaryLlmRequest) {
        if !self.in_flight.insert(request.display_id.clone()) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let text = match summarize_via_llm(&request.config, &request.input) {
                Ok(summary) => collapse_single_line(&summary, MAX_SUMMARY_LEN),
                Err(err) => {
                    tracing::info!("llm summary failed for {}: {err}", request.display_id);
                    String::new()
                }
            };
            let _ = tx.send(SummaryLlmResult {
                display_id: request.display_id,
                text,
            });
        });
    }

    /// Drain completed upgrades, clearing each from the in-flight set.
    pub fn drain(&mut self) -> Vec<SummaryLlmResult> {
        let mut out = Vec::new();
        while let Ok(result) = self.rx.try_recv() {
            self.in_flight.remove(&result.display_id);
            out.push(result);
        }
        out
    }
}

impl Default for SummaryService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn instance_with_tool(tool: &str) -> Instance {
        let mut inst = Instance::new("t", "/tmp/project");
        inst.tool = tool.to_string();
        inst
    }

    fn tool_session(display_id: &str, source_ref: &str) -> ToolSession {
        ToolSession {
            display_id: display_id.to_string(),
            resume_target: display_id.to_string(),
            source_ref: source_ref.to_string(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn collapse_single_line_collapses_and_truncates() {
        assert_eq!(collapse_single_line("a\n  b\tc", 80), "a b c");
        let long = "word ".repeat(40);
        let out = collapse_single_line(&long, 20);
        assert!(out.chars().count() <= 20);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn claude_first_message_skips_command_wrappers_and_tool_results() {
        let lines = [
            // slash-command wrapper: skipped
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#,
            // tool-result turn (no text block): skipped
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
            // sidechain: skipped
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"subagent ask"}}"#,
            // real first prompt
            r#"{"type":"user","message":{"role":"user","content":"Fix the   login bug"}}"#,
        ]
        .join("\n");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("s.jsonl");
        std::fs::write(&path, lines).unwrap();
        let got = first_user_text(&path, claude_line_text);
        assert_eq!(got.as_deref(), Some("Fix the   login bug"));
    }

    #[test]
    fn claude_array_text_block_is_extracted() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Add tests"}]}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        assert_eq!(claude_line_text(&value).as_deref(), Some("Add tests"));
    }

    #[test]
    fn codex_skips_injected_context_blocks() {
        let lines = [
            r##"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for /x\n<INSTRUCTIONS>foo</INSTRUCTIONS>"},{"type":"input_text","text":"<environment_context>\n<cwd>/x</cwd>\n</environment_context>"}]}}"##,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Explore uncommitted changes"}]}}"#,
        ]
        .join("\n");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rollout.jsonl");
        std::fs::write(&path, lines).unwrap();
        assert_eq!(
            first_user_text(&path, codex_line_text).as_deref(),
            Some("Explore uncommitted changes")
        );
    }

    #[test]
    fn pi_first_user_message_skips_other_roles() {
        let lines = [
            r#"{"type":"message","message":{"role":"bashExecution","command":"pi update","output":"..."}}"#,
            r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"Explore project"}]}}"#,
        ]
        .join("\n");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("pi.jsonl");
        std::fs::write(&path, lines).unwrap();
        assert_eq!(
            first_user_text(&path, pi_line_text).as_deref(),
            Some("Explore project")
        );
    }

    #[test]
    fn opencode_placeholder_titles_are_absent() {
        assert!(is_opencode_placeholder(
            "New session - 2026-04-29T12:44:35.549Z"
        ));
        assert!(!is_opencode_placeholder("Refactor the auth module"));
    }

    #[test]
    fn compute_update_skips_when_display_id_matches() {
        let mut inst = instance_with_tool("claude");
        let ts = tool_session("sid-1", "sid-1");
        inst.tool_session_summary = Some(ToolSessionSummary {
            display_id: "sid-1".to_string(),
            text: "existing".to_string(),
            state: SummaryState::Final,
        });
        assert!(!needs_eval(&inst, &ts));
        assert!(compute_summary_update(&inst, &ts, false).is_none());
    }

    #[test]
    fn compute_update_invalidates_on_display_id_change() {
        let mut inst = instance_with_tool("claude");
        inst.tool_session_summary = Some(ToolSessionSummary {
            display_id: "old".to_string(),
            text: "stale".to_string(),
            state: SummaryState::Final,
        });
        let ts = tool_session("new", "new");
        assert!(needs_eval(&inst, &ts));
    }

    #[test]
    fn cap_bytes_respects_char_boundaries() {
        let s = "héllo wörld";
        let capped = cap_bytes(s, 3);
        assert!(s.starts_with(capped));
        assert!(capped.len() <= 3);
    }
}
