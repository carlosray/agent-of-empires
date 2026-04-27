use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::Value;

use super::{
    resolve_config_with_repo, Config, Instance, ToolSession, ToolSessionProbe,
    ToolSessionProbeState,
};
use crate::tmux;

const INITIAL_BIND_GRACE: i64 = 5;
const SUPPORTED_TOOLS: &[&str] = &["claude", "codex", "opencode", "pi"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSessionCandidate {
    pub display_id: String,
    pub resume_target: String,
    pub source_ref: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshDecision {
    Keep,
    Update(ToolSessionCandidate),
    Clear,
}

#[derive(Debug, Clone)]
pub struct ToolSessionStateChange {
    pub tool_session: Option<ToolSession>,
    pub tool_session_probe: Option<ToolSessionProbe>,
}

pub fn tracking_enabled(instance: &Instance) -> bool {
    effective_config(instance)
        .map(|config| config.session.tool_session_tracking)
        .unwrap_or(false)
}

pub fn is_supported_tool(tool: &str) -> bool {
    SUPPORTED_TOOLS.contains(&tool)
}

pub fn is_eligible(instance: &Instance) -> bool {
    tracking_enabled(instance)
        && is_supported_tool(&instance.tool)
        && !instance.is_sandboxed()
        && !instance.has_command_override()
}

pub fn has_explicit_resume_target(tool: &str, command: &str, extra_args: &str) -> bool {
    let joined = if extra_args.is_empty() {
        command.to_string()
    } else if command.is_empty() {
        extra_args.to_string()
    } else {
        format!("{command} {extra_args}")
    };

    match tool {
        "claude" => {
            joined.contains("--resume")
                || joined.contains("--continue")
                || joined.contains("--session-id")
        }
        "codex" => joined
            .split_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|window| matches!(window, [binary, "resume"] if *binary == "codex")),
        "opencode" => joined.contains("--session") || joined.contains("--continue"),
        "pi" => {
            joined.contains("--resume")
                || joined.contains("--continue")
                || joined.contains("--session")
                || joined.contains("--fork")
        }
        _ => false,
    }
}

pub fn inject_resume_args(
    tool: &str,
    command: &str,
    extra_args: &str,
    resume_target: &str,
) -> Option<String> {
    let suffix = if extra_args.is_empty() {
        String::new()
    } else {
        format!(" {extra_args}")
    };

    match tool {
        "claude" => Some(format!("{command} --resume {resume_target}{suffix}")),
        "codex" => Some(format!("{command} resume {resume_target}{suffix}")),
        "opencode" => Some(format!("{command} --session {resume_target}{suffix}")),
        "pi" => Some(format!(
            "{command} --resume --session {resume_target}{suffix}"
        )),
        _ => None,
    }
}

pub fn build_start_command(instance: &Instance, command: &str, extra_args: &str) -> Option<String> {
    if !is_eligible(instance) || has_explicit_resume_target(&instance.tool, command, extra_args) {
        return None;
    }
    let tool_session = instance.tool_session.as_ref()?;
    inject_resume_args(
        &instance.tool,
        command,
        extra_args,
        &tool_session.resume_target,
    )
}

pub fn build_probe(instance: &Instance) -> Option<ToolSessionProbe> {
    if !is_eligible(instance)
        || has_explicit_resume_target(
            &instance.tool,
            instance.get_tool_command(),
            &instance.extra_args,
        )
        || instance.tool_session.is_some()
    {
        return None;
    }

    let baseline_source_refs = discover_candidates(instance)
        .unwrap_or_default()
        .into_iter()
        .map(|candidate| candidate.source_ref)
        .collect();

    Some(ToolSessionProbe {
        launch_started_at: Utc::now(),
        baseline_source_refs,
        state: ToolSessionProbeState::Pending,
    })
}

pub fn refresh(instance: &Instance) -> Result<Option<ToolSessionStateChange>> {
    if !is_eligible(instance) {
        return Ok(None);
    }

    if instance.tool == "claude" {
        if let Some(candidate) = discover_claude_from_pid(instance) {
            let next = candidate_to_tool_session(candidate.clone());
            let already_current = instance.tool_session.as_ref().is_some_and(|current| {
                current.source_ref == next.source_ref && current.display_id == next.display_id
            });
            if !already_current {
                return Ok(Some(ToolSessionStateChange {
                    tool_session: Some(next),
                    tool_session_probe: Some(ToolSessionProbe {
                        launch_started_at: Utc::now(),
                        baseline_source_refs: vec![candidate.source_ref],
                        state: ToolSessionProbeState::Resolved,
                    }),
                }));
            }
        }
    }

    if instance.tool == "codex" {
        if let Some(candidate) = discover_codex_from_pid(instance) {
            let next = candidate_to_tool_session(candidate.clone());
            let already_current = instance.tool_session.as_ref().is_some_and(|current| {
                current.source_ref == next.source_ref && current.display_id == next.display_id
            });
            if !already_current {
                return Ok(Some(ToolSessionStateChange {
                    tool_session: Some(next),
                    tool_session_probe: Some(ToolSessionProbe {
                        launch_started_at: Utc::now(),
                        baseline_source_refs: vec![candidate.source_ref],
                        state: ToolSessionProbeState::Resolved,
                    }),
                }));
            }
        }
    }

    let candidates = discover_candidates(instance)?;

    if let Some(current) = &instance.tool_session {
        let current_candidate = ToolSessionCandidate {
            display_id: current.display_id.clone(),
            resume_target: current.resume_target.clone(),
            source_ref: current.source_ref.clone(),
            created_at: current.updated_at,
            updated_at: current.updated_at,
        };

        match select_refreshed_tool_session(&current_candidate, &candidates) {
            RefreshDecision::Keep => Ok(None),
            RefreshDecision::Update(candidate) => Ok(Some(ToolSessionStateChange {
                tool_session: Some(candidate_to_tool_session(candidate)),
                tool_session_probe: Some(ToolSessionProbe {
                    launch_started_at: Utc::now(),
                    baseline_source_refs: vec![],
                    state: ToolSessionProbeState::Resolved,
                }),
            })),
            RefreshDecision::Clear => Ok(Some(ToolSessionStateChange {
                tool_session: None,
                tool_session_probe: Some(ToolSessionProbe {
                    launch_started_at: instance
                        .tool_session_probe
                        .as_ref()
                        .map(|probe| probe.launch_started_at)
                        .unwrap_or_else(Utc::now),
                    baseline_source_refs: instance
                        .tool_session_probe
                        .as_ref()
                        .map(|probe| probe.baseline_source_refs.clone())
                        .unwrap_or_default(),
                    state: ToolSessionProbeState::Ambiguous,
                }),
            })),
        }
    } else if let Some(probe) = &instance.tool_session_probe {
        match probe.state {
            ToolSessionProbeState::Pending | ToolSessionProbeState::Ambiguous => {
                if let Some(candidate) = select_initial_tool_session(
                    &probe.baseline_source_refs,
                    probe.launch_started_at,
                    &candidates,
                    Duration::seconds(INITIAL_BIND_GRACE),
                ) {
                    Ok(Some(ToolSessionStateChange {
                        tool_session: Some(candidate_to_tool_session(candidate)),
                        tool_session_probe: Some(ToolSessionProbe {
                            launch_started_at: probe.launch_started_at,
                            baseline_source_refs: probe.baseline_source_refs.clone(),
                            state: ToolSessionProbeState::Resolved,
                        }),
                    }))
                } else {
                    let baseline: HashSet<_> = probe.baseline_source_refs.iter().collect();
                    let eligible = candidates
                        .iter()
                        .filter(|candidate| {
                            !baseline.contains(&candidate.source_ref)
                                && candidate.created_at
                                    >= probe.launch_started_at
                                        - Duration::seconds(INITIAL_BIND_GRACE)
                        })
                        .count();

                    if eligible > 1 && probe.state != ToolSessionProbeState::Ambiguous {
                        Ok(Some(ToolSessionStateChange {
                            tool_session: None,
                            tool_session_probe: Some(ToolSessionProbe {
                                launch_started_at: probe.launch_started_at,
                                baseline_source_refs: probe.baseline_source_refs.clone(),
                                state: ToolSessionProbeState::Ambiguous,
                            }),
                        }))
                    } else {
                        Ok(None)
                    }
                }
            }
            ToolSessionProbeState::Resolved => Ok(None),
        }
    } else {
        Ok(None)
    }
}

pub fn select_initial_tool_session(
    baseline_source_refs: &[String],
    launch_started_at: DateTime<Utc>,
    candidates: &[ToolSessionCandidate],
    grace: Duration,
) -> Option<ToolSessionCandidate> {
    let baseline: HashSet<_> = baseline_source_refs.iter().collect();
    let mut fresh: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            !baseline.contains(&candidate.source_ref)
                && candidate.created_at >= launch_started_at - grace
        })
        .cloned()
        .collect();

    fresh.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    if fresh.len() == 1 {
        return fresh.into_iter().next();
    }

    if fresh.is_empty() {
        let mut rebound: Vec<_> = candidates
            .iter()
            .filter(|candidate| {
                baseline.contains(&candidate.source_ref)
                    && candidate.updated_at >= launch_started_at
            })
            .cloned()
            .collect();
        rebound.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        if rebound.len() == 1 {
            return rebound.into_iter().next();
        }
        return None;
    }

    None
}

pub fn select_refreshed_tool_session(
    current: &ToolSessionCandidate,
    candidates: &[ToolSessionCandidate],
) -> RefreshDecision {
    if candidates
        .iter()
        .any(|candidate| candidate.source_ref == current.source_ref)
    {
        return RefreshDecision::Keep;
    }

    let mut successors: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.updated_at > current.updated_at)
        .cloned()
        .collect();
    successors.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

    match successors.len() {
        1 => RefreshDecision::Update(successors.remove(0)),
        _ => RefreshDecision::Clear,
    }
}

fn effective_config(instance: &Instance) -> Result<Config> {
    let profile = if instance.source_profile.is_empty() {
        super::config::resolve_default_profile()
    } else {
        instance.source_profile.clone()
    };
    resolve_config_with_repo(&profile, Path::new(&instance.project_path))
}

fn candidate_to_tool_session(candidate: ToolSessionCandidate) -> ToolSession {
    ToolSession {
        display_id: candidate.display_id,
        resume_target: candidate.resume_target,
        source_ref: candidate.source_ref,
        updated_at: candidate.updated_at,
    }
}

fn discover_candidates(instance: &Instance) -> Result<Vec<ToolSessionCandidate>> {
    match instance.tool.as_str() {
        "claude" => discover_claude_candidates(Path::new(&instance.project_path)),
        "codex" => discover_codex_candidates(Path::new(&instance.project_path)),
        "opencode" => discover_opencode_candidates(Path::new(&instance.project_path)),
        "pi" => discover_pi_candidates(Path::new(&instance.project_path)),
        _ => Ok(Vec::new()),
    }
}

fn discover_codex_from_pid(instance: &Instance) -> Option<ToolSessionCandidate> {
    let session = tmux::Session::new(&instance.id, &instance.title).ok()?;
    let foreground_pid = session.get_foreground_pid()?;
    let path = find_open_rollout_for_pid(foreground_pid)?;
    codex_candidate_from_path(&path, Path::new(&instance.project_path))
}

fn discover_claude_from_pid(instance: &Instance) -> Option<ToolSessionCandidate> {
    let session = tmux::Session::new(&instance.id, &instance.title).ok()?;
    let pane_pid = session.get_pane_pid()?;
    let path = dirs::home_dir()?
        .join(".claude")
        .join("sessions")
        .join(format!("{pane_pid}.json"));
    let (display_id, cwd, created_at, updated_at) = read_claude_pid_session(&path)?;
    if cwd != Path::new(&instance.project_path) {
        return None;
    }
    Some(ToolSessionCandidate {
        display_id: display_id.clone(),
        resume_target: display_id.clone(),
        source_ref: display_id,
        created_at,
        updated_at,
    })
}

fn discover_claude_candidates(project_path: &Path) -> Result<Vec<ToolSessionCandidate>> {
    let Some(home) = dirs::home_dir() else {
        return Ok(Vec::new());
    };
    let project_dir = home
        .join(".claude")
        .join("projects")
        .join(claude_project_dir_name(project_path));
    if !project_dir.exists() {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(project_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some((display_id, created_at)) = read_claude_session_header(&path) else {
            continue;
        };
        let updated_at = modified_to_utc(&path).unwrap_or_else(Utc::now);
        candidates.push(ToolSessionCandidate {
            display_id: display_id.clone(),
            resume_target: display_id.clone(),
            source_ref: display_id.clone(),
            created_at,
            updated_at,
        });
    }
    candidates.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(candidates)
}

fn discover_codex_candidates(project_path: &Path) -> Result<Vec<ToolSessionCandidate>> {
    let Some(home) = dirs::home_dir() else {
        return Ok(Vec::new());
    };
    let root = home.join(".codex").join("sessions");
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for path in collect_matching_files(&root, &|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
    })? {
        if let Some(candidate) = codex_candidate_from_path(&path, project_path) {
            candidates.push(candidate);
        }
    }

    candidates.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(candidates)
}

fn discover_opencode_candidates(project_path: &Path) -> Result<Vec<ToolSessionCandidate>> {
    let Some(home) = dirs::home_dir() else {
        return Ok(Vec::new());
    };
    let db_path = home
        .join(".local")
        .join("share")
        .join("opencode")
        .join("opencode.db");
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let project = project_path.to_string_lossy().replace('\'', "''");
    let query = format!(
        "select id, time_created, time_updated from session where directory = '{project}' order by time_updated desc;"
    );
    let output = Command::new("sqlite3").arg(&db_path).arg(query).output();
    let Ok(output) = output else {
        return Ok(Vec::new());
    };
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split('|');
        let Some(id) = parts.next() else {
            continue;
        };
        let Some(created_at_raw) = parts.next() else {
            continue;
        };
        let Some(updated_at_raw) = parts.next() else {
            continue;
        };
        let Some(created_at_ms) = created_at_raw.parse::<i64>().ok() else {
            continue;
        };
        let Some(updated_at_ms) = updated_at_raw.parse::<i64>().ok() else {
            continue;
        };
        let Some(created_at) = Utc.timestamp_millis_opt(created_at_ms).single() else {
            continue;
        };
        let Some(updated_at) = Utc.timestamp_millis_opt(updated_at_ms).single() else {
            continue;
        };
        candidates.push(ToolSessionCandidate {
            display_id: id.to_string(),
            resume_target: id.to_string(),
            source_ref: id.to_string(),
            created_at,
            updated_at,
        });
    }

    Ok(candidates)
}

fn discover_pi_candidates(project_path: &Path) -> Result<Vec<ToolSessionCandidate>> {
    let base_dir = std::env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".pi").join("agent")))
        .unwrap_or_else(|| PathBuf::from(".pi/agent"));
    let session_dir = base_dir
        .join("sessions")
        .join(pi_project_dir_name(project_path));
    if !session_dir.exists() {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(session_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some((display_id, created_at)) = read_pi_session_header(&path) else {
            continue;
        };
        let updated_at = modified_to_utc(&path).unwrap_or_else(Utc::now);
        candidates.push(ToolSessionCandidate {
            display_id,
            resume_target: path.to_string_lossy().to_string(),
            source_ref: path.to_string_lossy().to_string(),
            created_at,
            updated_at,
        });
    }

    candidates.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(candidates)
}

fn claude_project_dir_name(project_path: &Path) -> String {
    format!(
        "-{}",
        project_path
            .to_string_lossy()
            .trim_start_matches('/')
            .replace(['/', '\\', ':'], "-")
    )
}

fn pi_project_dir_name(project_path: &Path) -> String {
    format!(
        "--{}--",
        project_path
            .to_string_lossy()
            .trim_start_matches('/')
            .replace(['/', '\\', ':'], "-")
    )
}

fn collect_matching_files(root: &Path, predicate: &dyn Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_matching_files(&path, predicate)?);
        } else if predicate(&path) {
            files.push(path);
        }
    }

    Ok(files)
}

fn codex_candidate_from_path(path: &Path, project_path: &Path) -> Option<ToolSessionCandidate> {
    let (cwd, session_id, created_at) = read_codex_rollout_header(path)?;
    if cwd != project_path {
        return None;
    }
    Some(ToolSessionCandidate {
        display_id: session_id.clone(),
        resume_target: session_id,
        source_ref: path.to_string_lossy().to_string(),
        created_at,
        updated_at: modified_to_utc(path).unwrap_or_else(Utc::now),
    })
}

fn read_codex_rollout_header(path: &Path) -> Option<(PathBuf, String, DateTime<Utc>)> {
    let content = fs::read_to_string(path).ok()?;
    let first_line = content.lines().next()?;
    let value: Value = serde_json::from_str(first_line).ok()?;
    let payload = value.get("payload").or_else(|| {
        value
            .get("session_meta")
            .and_then(|meta| meta.get("payload"))
    })?;
    let cwd = payload.get("cwd")?.as_str()?;
    let id = payload.get("id")?.as_str()?;
    let created_at = parse_timestamp(payload.get("timestamp")?.as_str()?)?;
    Some((PathBuf::from(cwd), id.to_string(), created_at))
}

fn read_claude_session_header(path: &Path) -> Option<(String, DateTime<Utc>)> {
    let content = fs::read_to_string(path).ok()?;
    let first_line = content.lines().next()?;
    let value: Value = serde_json::from_str(first_line).ok()?;
    let display_id = value
        .get("sessionId")
        .and_then(|id| id.as_str())
        .or_else(|| path.file_stem().and_then(|stem| stem.to_str()))?
        .to_string();
    let created_at = parse_timestamp(value.get("timestamp")?.as_str()?)?;
    Some((display_id, created_at))
}

fn read_claude_pid_session(path: &Path) -> Option<(String, PathBuf, DateTime<Utc>, DateTime<Utc>)> {
    let content = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    let display_id = value.get("sessionId")?.as_str()?.to_string();
    let cwd = PathBuf::from(value.get("cwd")?.as_str()?);
    let created_at_ms = value.get("startedAt")?.as_i64()?;
    let updated_at_ms = value.get("updatedAt")?.as_i64()?;
    let created_at = Utc.timestamp_millis_opt(created_at_ms).single()?;
    let updated_at = Utc.timestamp_millis_opt(updated_at_ms).single()?;
    Some((display_id, cwd, created_at, updated_at))
}

fn read_pi_session_header(path: &Path) -> Option<(String, DateTime<Utc>)> {
    let content = fs::read_to_string(path).ok()?;
    let first_line = content.lines().next()?;
    let value: Value = serde_json::from_str(first_line).ok()?;
    let display_id = value
        .get("id")
        .and_then(|id| id.as_str())
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.trim_end_matches(".jsonl").rsplit_once('_'))
                .map(|(_, id)| id)
        })?
        .to_string();
    let created_at = parse_timestamp(value.get("timestamp")?.as_str()?)?;
    Some((display_id, created_at))
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn modified_to_utc(path: &Path) -> Option<DateTime<Utc>> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Utc>::from(modified))
}

fn find_open_rollout_for_pid(pid: u32) -> Option<PathBuf> {
    let output = Command::new("lsof")
        .args(["-Fn", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let path = line.strip_prefix('n')?;
        if path.contains("/.codex/sessions/")
            && path.ends_with(".jsonl")
            && path.contains("rollout-")
        {
            return Some(PathBuf::from(path));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration as StdDuration;

    use anyhow::{anyhow, Result};
    use chrono::{DateTime, Duration, Utc};
    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        build_probe, build_start_command, has_explicit_resume_target, inject_resume_args,
        read_codex_rollout_header, select_initial_tool_session, select_refreshed_tool_session,
        RefreshDecision, ToolSessionCandidate,
    };
    use crate::session::{
        save_repo_config, Instance, RepoConfig, SandboxInfo, SessionConfigOverride, ToolSession,
    };

    fn write_tracking_repo_config(project_path: &Path, enabled: bool) {
        save_repo_config(
            project_path,
            &RepoConfig {
                session: Some(SessionConfigOverride {
                    tool_session_tracking: Some(enabled),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
    }

    fn tracked_instance(project_path: &Path, tool: &str) -> Instance {
        let mut instance = Instance::new("Tracked", project_path.to_str().unwrap());
        instance.tool = tool.to_string();
        instance.source_profile = "default".to_string();
        instance.tool_session = Some(ToolSession {
            display_id: "session-123".to_string(),
            resume_target: "resume-123".to_string(),
            source_ref: "source-123".to_string(),
            updated_at: Utc::now(),
        });
        instance
    }

    #[test]
    fn test_select_initial_tool_session_ignores_baseline_and_binds_single_new_candidate() {
        let launch_started_at = Utc::now();
        let current = ToolSessionCandidate {
            display_id: "new-session".to_string(),
            resume_target: "new-session".to_string(),
            source_ref: "new-ref".to_string(),
            created_at: launch_started_at + Duration::seconds(1),
            updated_at: launch_started_at + Duration::seconds(1),
        };
        let baseline = vec!["old-ref".to_string()];
        let candidates = vec![
            ToolSessionCandidate {
                display_id: "old-session".to_string(),
                resume_target: "old-session".to_string(),
                source_ref: "old-ref".to_string(),
                created_at: launch_started_at - Duration::seconds(30),
                updated_at: launch_started_at - Duration::seconds(30),
            },
            current.clone(),
        ];

        let selected = select_initial_tool_session(
            &baseline,
            launch_started_at,
            &candidates,
            Duration::seconds(5),
        )
        .expect("should bind exactly one new candidate");

        assert_eq!(selected.display_id, current.display_id);
        assert_eq!(selected.source_ref, current.source_ref);
    }

    #[test]
    fn test_select_initial_tool_session_returns_none_for_ambiguous_new_candidates() {
        let launch_started_at = Utc::now();
        let candidates = vec![
            ToolSessionCandidate {
                display_id: "one".to_string(),
                resume_target: "one".to_string(),
                source_ref: "ref-one".to_string(),
                created_at: launch_started_at + Duration::seconds(1),
                updated_at: launch_started_at + Duration::seconds(1),
            },
            ToolSessionCandidate {
                display_id: "two".to_string(),
                resume_target: "two".to_string(),
                source_ref: "ref-two".to_string(),
                created_at: launch_started_at + Duration::seconds(2),
                updated_at: launch_started_at + Duration::seconds(2),
            },
        ];

        assert!(select_initial_tool_session(
            &[],
            launch_started_at,
            &candidates,
            Duration::seconds(5)
        )
        .is_none());
    }

    #[test]
    fn test_select_initial_tool_session_rebinds_single_updated_baseline_candidate() {
        let launch_started_at = Utc::now();
        let baseline = vec!["existing-ref".to_string()];
        let rebound = ToolSessionCandidate {
            display_id: "existing-session".to_string(),
            resume_target: "existing-session".to_string(),
            source_ref: "existing-ref".to_string(),
            created_at: launch_started_at - Duration::days(1),
            updated_at: launch_started_at + Duration::seconds(1),
        };

        let selected = select_initial_tool_session(
            &baseline,
            launch_started_at,
            std::slice::from_ref(&rebound),
            Duration::seconds(5),
        )
        .expect("should rebind the only baseline candidate updated after launch");

        assert_eq!(selected.display_id, rebound.display_id);
        assert_eq!(selected.source_ref, rebound.source_ref);
    }

    #[test]
    fn test_select_refreshed_tool_session_keeps_current_when_source_ref_still_exists() {
        let current = ToolSessionCandidate {
            display_id: "current".to_string(),
            resume_target: "current".to_string(),
            source_ref: "current-ref".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let candidates = vec![current.clone()];

        assert_eq!(
            select_refreshed_tool_session(&current, &candidates),
            RefreshDecision::Keep
        );
    }

    #[test]
    fn test_select_refreshed_tool_session_updates_when_exactly_one_successor_exists() {
        let now = Utc::now();
        let current = ToolSessionCandidate {
            display_id: "current".to_string(),
            resume_target: "current".to_string(),
            source_ref: "current-ref".to_string(),
            created_at: now,
            updated_at: now,
        };
        let successor = ToolSessionCandidate {
            display_id: "next".to_string(),
            resume_target: "next".to_string(),
            source_ref: "next-ref".to_string(),
            created_at: now + Duration::seconds(10),
            updated_at: now + Duration::seconds(10),
        };

        assert_eq!(
            select_refreshed_tool_session(&current, std::slice::from_ref(&successor)),
            RefreshDecision::Update(successor)
        );
    }

    #[test]
    fn test_select_refreshed_tool_session_clears_when_multiple_successors_exist() {
        let now = Utc::now();
        let current = ToolSessionCandidate {
            display_id: "current".to_string(),
            resume_target: "current".to_string(),
            source_ref: "current-ref".to_string(),
            created_at: now,
            updated_at: now,
        };
        let candidates = vec![
            ToolSessionCandidate {
                display_id: "next-one".to_string(),
                resume_target: "next-one".to_string(),
                source_ref: "next-one".to_string(),
                created_at: now + Duration::seconds(10),
                updated_at: now + Duration::seconds(10),
            },
            ToolSessionCandidate {
                display_id: "next-two".to_string(),
                resume_target: "next-two".to_string(),
                source_ref: "next-two".to_string(),
                created_at: now + Duration::seconds(11),
                updated_at: now + Duration::seconds(11),
            },
        ];

        assert_eq!(
            select_refreshed_tool_session(&current, &candidates),
            RefreshDecision::Clear
        );
    }

    #[test]
    fn test_has_explicit_resume_target_is_tool_specific() {
        assert!(has_explicit_resume_target(
            "claude",
            "claude",
            "--resume abc"
        ));
        assert!(has_explicit_resume_target("codex", "codex resume 123", ""));
        assert!(has_explicit_resume_target(
            "opencode",
            "opencode",
            "--session ses_1"
        ));
        assert!(has_explicit_resume_target(
            "pi",
            "pi",
            "--fork session.jsonl"
        ));
        assert!(!has_explicit_resume_target(
            "claude",
            "claude",
            "--model opus"
        ));
    }

    #[test]
    fn test_inject_resume_args_builds_expected_command_prefixes() {
        assert_eq!(
            inject_resume_args("claude", "claude", "--model opus", "abc"),
            Some("claude --resume abc --model opus".to_string())
        );
        assert_eq!(
            inject_resume_args("codex", "codex", "--model gpt-5", "thread-1"),
            Some("codex resume thread-1 --model gpt-5".to_string())
        );
        assert_eq!(
            inject_resume_args("opencode", "opencode", "--print", "ses_123"),
            Some("opencode --session ses_123 --print".to_string())
        );
        assert_eq!(
            inject_resume_args("pi", "pi", "--provider openai", "/tmp/session.jsonl"),
            Some("pi --resume --session /tmp/session.jsonl --provider openai".to_string())
        );
    }

    #[test]
    fn test_build_start_command_injects_resume_target_when_tracking_enabled() {
        let temp = tempdir().unwrap();
        write_tracking_repo_config(temp.path(), true);

        let instance = tracked_instance(temp.path(), "codex");

        assert_eq!(
            build_start_command(&instance, "codex", "--model gpt-5"),
            Some("codex resume resume-123 --model gpt-5".to_string())
        );
    }

    #[test]
    fn test_build_start_command_skips_when_explicit_resume_is_already_present() {
        let temp = tempdir().unwrap();
        write_tracking_repo_config(temp.path(), true);

        let instance = tracked_instance(temp.path(), "claude");

        assert_eq!(
            build_start_command(&instance, "claude", "--resume existing-session"),
            None
        );
    }

    #[test]
    fn test_build_start_command_skips_for_command_override() {
        let temp = tempdir().unwrap();
        write_tracking_repo_config(temp.path(), true);

        let mut instance = tracked_instance(temp.path(), "claude");
        instance.command = "my-claude-wrapper".to_string();

        assert_eq!(build_start_command(&instance, &instance.command, ""), None);
    }

    #[test]
    fn test_build_probe_skips_when_tracking_disabled() {
        let temp = tempdir().unwrap();
        write_tracking_repo_config(temp.path(), false);
        let mut instance = Instance::new("Tracked", temp.path().to_str().unwrap());
        instance.tool = "codex".to_string();
        instance.source_profile = "default".to_string();

        assert!(build_probe(&instance).is_none());
    }

    #[test]
    fn test_build_probe_skips_for_sandboxed_session() {
        let temp = tempdir().unwrap();
        write_tracking_repo_config(temp.path(), true);

        let mut instance = Instance::new("Tracked", temp.path().to_str().unwrap());
        instance.tool = "codex".to_string();
        instance.source_profile = "default".to_string();
        instance.sandbox_info = Some(SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test-image".to_string(),
            container_name: "sandbox".to_string(),
            created_at: None,
            extra_env: None,
            custom_instruction: None,
        });

        assert!(build_probe(&instance).is_none());
    }

    #[test]
    fn test_read_codex_rollout_header_reads_current_rollout_shape() {
        let temp = tempdir().unwrap();
        let rollout = temp.path().join("rollout.jsonl");
        let line = serde_json::json!({
            "timestamp": "2026-04-24T14:07:24.415Z",
            "type": "session_meta",
            "payload": {
                "id": "019dbfd1-135a-7690-ac84-2c59d3bc53cb",
                "timestamp": "2026-04-24T14:07:23.503Z",
                "cwd": "/tmp/example"
            }
        })
        .to_string();
        std::fs::write(&rollout, format!("{line}\n")).unwrap();

        let (cwd, id, created_at) = read_codex_rollout_header(&rollout).unwrap();
        assert_eq!(cwd, Path::new("/tmp/example"));
        assert_eq!(id, "019dbfd1-135a-7690-ac84-2c59d3bc53cb");
        assert_eq!(
            created_at,
            chrono::DateTime::parse_from_rfc3339("2026-04-24T14:07:23.503Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    fn live_resolution_result(tool: &str) -> Result<String> {
        let project_path = std::env::current_dir()?;
        let mut instance = Instance::new(
            &format!("live-tool-session-{tool}-{}", std::process::id()),
            project_path.to_str().unwrap(),
        );
        instance.tool = tool.to_string();
        instance.source_profile = "default".to_string();

        instance.start_with_size_opts(Some((120, 40)), false)?;

        let result = (|| -> Result<String> {
            for _ in 0..30 {
                if let Ok(session) = instance.tmux_session() {
                    if let Ok(pane) = session.capture_pane(20) {
                        if pane.contains("Do you trust the contents of this directory?") {
                            let _ = session.send_keys("Enter");
                        }
                    }
                }
                if let Some(change) = super::refresh(&instance)? {
                    if let Some(tool_session) = change.tool_session {
                        return Ok(tool_session.display_id);
                    }
                }
                std::thread::sleep(StdDuration::from_millis(500));
            }

            let pane = instance
                .tmux_session()
                .ok()
                .and_then(|session| session.capture_pane(40).ok())
                .unwrap_or_else(|| "<unable to capture tmux pane>".to_string());
            let (pane_pid, foreground_pid) = instance
                .tmux_session()
                .ok()
                .map(|session| (session.get_pane_pid(), session.get_foreground_pid()))
                .unwrap_or((None, None));
            let process_summary = foreground_pid
                .or(pane_pid)
                .map(debug_process_details)
                .unwrap_or_else(|| "<no pid>".to_string());
            let recent_files = instance
                .tool_session_probe
                .as_ref()
                .map(|probe| debug_recent_tool_files(tool, probe.launch_started_at))
                .unwrap_or_else(|| "<no probe>".to_string());
            let extra_debug = debug_tool_runtime_state(
                tool,
                pane_pid,
                instance
                    .tool_session_probe
                    .as_ref()
                    .map(|probe| probe.launch_started_at),
            );
            let pid_candidate = super::discover_codex_from_pid(&instance);
            let candidates = super::discover_candidates(&instance).unwrap_or_default();
            let initial_selection = instance.tool_session_probe.as_ref().and_then(|probe| {
                super::select_initial_tool_session(
                    &probe.baseline_source_refs,
                    probe.launch_started_at,
                    &candidates,
                    Duration::seconds(super::INITIAL_BIND_GRACE),
                )
            });
            let candidate_summary = candidates
                .iter()
                .take(5)
                .map(|candidate| {
                    format!(
                        "{} | created={} | updated={} | {}",
                        candidate.display_id,
                        candidate.created_at,
                        candidate.updated_at,
                        candidate.source_ref
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            Err(anyhow!(
                "AoE did not resolve a tool session for {tool}.\nprobe={:?}\npane_pid={:?}\nforeground_pid={:?}\nprocess_summary=\n{}\nrecent_files=\n{}\nextra_debug=\n{}\npid_candidate={:?}\ninitial_selection={:?}\ncandidates_seen={}\n{}\nPane snapshot:\n{}",
                instance.tool_session_probe,
                pane_pid,
                foreground_pid,
                process_summary,
                recent_files,
                extra_debug,
                pid_candidate,
                initial_selection,
                candidates.len(),
                candidate_summary,
                pane
            ))
        })();

        let _ = instance.stop();
        result
    }

    fn debug_process_details(pid: u32) -> String {
        let ps = std::process::Command::new("ps")
            .args(["-o", "pid=,ppid=,pgid=,command=", "-p", &pid.to_string()])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_else(|| "<ps unavailable>".to_string());
        let lsof = std::process::Command::new("lsof")
            .args(["-p", &pid.to_string()])
            .output()
            .ok()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|line| {
                        line.contains(".claude/projects/")
                            || line.contains(".codex/sessions/")
                            || line.contains(".pi/agent/sessions/")
                            || line.contains("opencode.db")
                    })
                    .take(10)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        format!("ps: {ps}\nlsof:\n{lsof}")
    }

    fn debug_recent_tool_files(tool: &str, launch_started_at: DateTime<Utc>) -> String {
        let Some(root) = tool_debug_root(tool) else {
            return "<no root>".to_string();
        };
        if !root.exists() {
            return format!("{} (missing)", root.display());
        }

        let threshold = launch_started_at - Duration::seconds(2);
        let mut recent = Vec::new();
        if let Ok(paths) = super::collect_matching_files(&root, &|_| true) {
            for path in paths {
                if let Some(modified) = super::modified_to_utc(&path) {
                    if modified >= threshold {
                        recent.push((modified, path));
                    }
                }
            }
        }
        recent.sort_by(|left, right| right.0.cmp(&left.0));
        if recent.is_empty() {
            return format!(
                "{}: <no files modified since {}>",
                root.display(),
                threshold
            );
        }

        recent
            .into_iter()
            .take(10)
            .map(|(modified, path)| format!("{modified} {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tool_debug_root(tool: &str) -> Option<std::path::PathBuf> {
        let home = dirs::home_dir()?;
        match tool {
            "claude" => Some(home.join(".claude")),
            "codex" => Some(home.join(".codex")),
            "opencode" => Some(home.join(".local").join("share").join("opencode")),
            "pi" => Some(home.join(".pi")),
            _ => None,
        }
    }

    fn debug_tool_runtime_state(
        tool: &str,
        pane_pid: Option<u32>,
        launch_started_at: Option<DateTime<Utc>>,
    ) -> String {
        match tool {
            "claude" => pane_pid
                .and_then(debug_claude_pid_session_file)
                .unwrap_or_else(|| "<no claude pid session file>".to_string()),
            "opencode" => launch_started_at
                .map(debug_opencode_recent_rows)
                .unwrap_or_else(|| "<no launch time>".to_string()),
            _ => "<none>".to_string(),
        }
    }

    fn debug_claude_pid_session_file(pid: u32) -> Option<String> {
        let path = dirs::home_dir()?
            .join(".claude")
            .join("sessions")
            .join(format!("{pid}.json"));
        let text = std::fs::read_to_string(&path).ok()?;
        Some(format!("{}:\n{}", path.display(), text))
    }

    fn debug_opencode_recent_rows(launch_started_at: DateTime<Utc>) -> String {
        let Some(home) = dirs::home_dir() else {
            return "<no home>".to_string();
        };
        let db_path = home
            .join(".local")
            .join("share")
            .join("opencode")
            .join("opencode.db");
        let threshold_ms = launch_started_at.timestamp_millis();
        let query = format!(
            "select id, directory, title, time_created, time_updated from session where time_updated >= {threshold_ms} order by time_updated desc limit 10;"
        );
        let output = std::process::Command::new("sqlite3")
            .arg(&db_path)
            .arg(query)
            .output();
        let Ok(output) = output else {
            return "<sqlite3 failed>".to_string();
        };
        if !output.status.success() {
            return String::from_utf8_lossy(&output.stderr).trim().to_string();
        }
        let rows = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if rows.is_empty() {
            "<no recent session rows>".to_string()
        } else {
            rows
        }
    }

    #[test]
    #[ignore = "live test that starts real codex/claude/opencode/pi sessions"]
    #[serial]
    fn live_resolves_supported_tool_sessions() -> Result<()> {
        let mut failures = Vec::new();
        for tool in ["codex", "claude", "opencode", "pi"] {
            match live_resolution_result(tool) {
                Ok(display_id) => {
                    assert!(
                        !display_id.is_empty(),
                        "resolved display_id for {tool} should not be empty"
                    );
                    eprintln!("{tool}: resolved {display_id}");
                }
                Err(error) => failures.push(format!("{tool}: {error:#}")),
            }
        }

        if !failures.is_empty() {
            return Err(anyhow!(
                "live tool session resolution failures:\n{}",
                failures.join("\n\n")
            ));
        }

        Ok(())
    }
}
