//! Migration v014: import fork archive.json entries into upstream archived_at on Instance.
//!
//! The fork stored archived sessions in a separate archive.json file per profile.
//! Upstream archives sessions in-place in sessions.json using an `archived_at` timestamp.
//! This migration:
//!   1. Reads archive.json from each profile directory (if present).
//!   2. Adds each archived session's `instance` to sessions.json with `archived_at` set.
//!   3. Removes archive.json (and its .bak) so the old format is gone.
//!
//! Idempotent: re-running on already-migrated data is a no-op (archive.json absent).

use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::info;

pub fn run() -> Result<()> {
    let app_dir = crate::session::get_app_dir()?;
    let profiles_dir = app_dir.join("profiles");
    if profiles_dir.exists() {
        for entry in fs::read_dir(&profiles_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                migrate_profile(&entry.path())?;
            }
        }
    }
    // Legacy top-level archive.json (pre-profiles layout).
    migrate_profile(&app_dir)?;
    Ok(())
}

fn migrate_profile(profile_dir: &Path) -> Result<()> {
    let archive_path = profile_dir.join("archive.json");
    if !archive_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&archive_path)?;
    let entries: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(
                "v014: failed to parse {}: {e}, skipping",
                archive_path.display()
            );
            return Ok(());
        }
    };

    let Some(entries) = entries.as_array() else {
        return Ok(());
    };
    if entries.is_empty() {
        fs::remove_file(&archive_path).ok();
        let bak = profile_dir.join("archive.json.bak");
        if bak.exists() {
            fs::remove_file(&bak).ok();
        }
        return Ok(());
    }

    let sessions_path = profile_dir.join("sessions.json");
    let sessions_content = if sessions_path.exists() {
        fs::read_to_string(&sessions_path)?
    } else {
        "[]".to_string()
    };
    let mut sessions: serde_json::Value = match serde_json::from_str(&sessions_content) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(
                "v014: failed to parse {}: {e}, skipping import",
                sessions_path.display()
            );
            return Ok(());
        }
    };

    let Some(sessions_arr) = sessions.as_array_mut() else {
        return Ok(());
    };

    let existing_ids: std::collections::HashSet<String> = sessions_arr
        .iter()
        .filter_map(|s| s.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    let mut imported = 0usize;
    for entry in entries {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let Some(instance) = obj.get("instance").and_then(|v| v.as_object()) else {
            continue;
        };
        let id = match instance.get("id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        if existing_ids.contains(id) {
            continue;
        }
        let archived_at = match obj.get("archived_at").and_then(|v| v.as_str()) {
            Some(ts) => ts,
            None => continue,
        };
        let mut inst = serde_json::Value::Object(instance.clone());
        inst["archived_at"] = serde_json::Value::String(archived_at.to_string());
        sessions_arr.push(inst);
        imported += 1;
    }

    if imported > 0 {
        fs::write(&sessions_path, serde_json::to_string_pretty(&sessions)?)?;
        info!(
            "v014: imported {} archived sessions into {}",
            imported,
            sessions_path.display()
        );
    }

    fs::remove_file(&archive_path)?;
    let bak = profile_dir.join("archive.json.bak");
    if bak.exists() {
        fs::remove_file(&bak).ok();
    }
    info!("v014: removed {}", archive_path.display());
    Ok(())
}
