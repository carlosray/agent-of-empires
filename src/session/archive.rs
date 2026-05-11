use std::path::Path;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Instance, Status};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveCleanupOptions {
    #[serde(default)]
    pub delete_worktree: bool,
    #[serde(default)]
    pub delete_branch: bool,
    #[serde(default)]
    pub delete_sandbox: bool,
    #[serde(default)]
    pub force_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedSession {
    pub id: String,
    pub archived_at: DateTime<Utc>,
    pub source_profile: String,
    pub last_status: Status,
    #[serde(default)]
    pub cleanup: ArchiveCleanupOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub instance: Instance,
}

impl ArchivedSession {
    pub fn new(
        mut instance: Instance,
        source_profile: String,
        cleanup: ArchiveCleanupOptions,
        reason: Option<String>,
    ) -> Self {
        let last_status = instance.status;
        instance.source_profile = source_profile.clone();

        Self {
            id: instance.id.clone(),
            archived_at: Utc::now(),
            source_profile,
            last_status,
            cleanup,
            reason,
            instance,
        }
    }

    pub fn restore_instance(&self) -> Result<Instance> {
        self.validate_paths()?;

        let mut instance = self.instance.clone();
        instance.source_profile = self.source_profile.clone();
        instance.status = Status::Stopped;
        instance.terminal_info = None;
        instance.last_error = None;
        instance.last_error_check = None;
        instance.last_start_time = None;
        instance.tool_session_probe = None;

        if let Some(ref mut sandbox) = instance.sandbox_info {
            sandbox.container_id = None;
        }

        Ok(instance)
    }

    pub fn validate_restore(&self, active: &[Instance]) -> Result<()> {
        if active.iter().any(|instance| instance.id == self.id) {
            return Err(anyhow!(
                "Cannot restore '{}': an active session with the same id already exists",
                self.instance.title
            ));
        }

        self.validate_paths()
    }

    fn validate_paths(&self) -> Result<()> {
        if !self.instance.project_path.is_empty()
            && !Path::new(&self.instance.project_path).exists()
        {
            return Err(anyhow!(
                "Cannot restore '{}': project path does not exist: {}",
                self.instance.title,
                self.instance.project_path
            ));
        }

        if let Some(workspace) = &self.instance.workspace_info {
            if !workspace.workspace_dir.is_empty() && !Path::new(&workspace.workspace_dir).exists()
            {
                return Err(anyhow!(
                    "Cannot restore '{}': workspace path does not exist: {}",
                    self.instance.title,
                    workspace.workspace_dir
                ));
            }

            for repo in &workspace.repos {
                if !repo.worktree_path.is_empty() && !Path::new(&repo.worktree_path).exists() {
                    return Err(anyhow!(
                        "Cannot restore '{}': workspace repo path does not exist: {}",
                        self.instance.title,
                        repo.worktree_path
                    ));
                }
            }
        }

        Ok(())
    }
}
