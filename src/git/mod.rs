//! Git worktree operations module.
//!
//! Layout:
//!   - `remote`   — repo cloning, origin-URL parsing
//!   - `worktree` — `GitWorktree` lifecycle, branch ops, template paths
//!   - `diff`     — diff rendering for the UI
//!   - `cleanup`  — stale-worktree cleanup
//!   - `template` — path-template expansion
//!   - this file  — module declarations, re-exports, and the shared
//!     `open_repo_at` helper used by sibling submodules.
//!
//! `remote` and `worktree` were extracted from a single 1,797-line `mod.rs`;
//! `diff`, `cleanup`, and `template` predate the split.

use anyhow::{anyhow, Context};
use std::ffi::OsStr;
use std::path::Path;

pub mod cleanup;
pub mod diff;
pub mod error;
mod remote;
pub mod template;
mod worktree;

pub use remote::{clone_repo, get_remote_owner};
pub use worktree::{GitWorktree, WorktreeEntry};

/// Open a git repository at the given path without searching parent directories.
/// Unlike `git2::Repository::discover`, this does not walk up the directory tree,
/// preventing unrelated ancestor repos (e.g., a dotfile-managed home directory)
/// from being found.
pub(crate) fn open_repo_at(path: &Path) -> std::result::Result<git2::Repository, git2::Error> {
    git2::Repository::open_ext(
        path,
        git2::RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&OsStr>(),
    )
}

pub fn resolve_display_branch(
    path: &Path,
    branch_command: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if !GitWorktree::is_git_repo(path) {
        return Ok(None);
    }

    if let Some(command) = branch_command {
        let shell = crate::session::user_shell();
        let output = std::process::Command::new(&shell)
            .args(["-lc", command])
            .current_dir(path)
            .output()
            .with_context(|| {
                format!("failed to run branch display command in {}", path.display())
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            };
            return Err(anyhow!("branch display command failed: {}", detail));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(stdout
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.to_string()));
    }

    Ok(Some(GitWorktree::get_current_branch(path)?))
}
