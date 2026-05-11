//! `agent-of-empires archive` command implementation

use anyhow::{bail, Result};
use clap::Subcommand;
use serde::Serialize;

use crate::session::{ArchivedSession, GroupTree, Status, Storage};

const TABLE_COL_TITLE: usize = 20;
const TABLE_COL_GROUP: usize = 15;
const TABLE_COL_PATH: usize = 40;
const TABLE_COL_ID_DISPLAY: usize = 12;

#[derive(Subcommand)]
pub enum ArchiveCommands {
    /// List archived sessions
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// List archived sessions from all profiles
        #[arg(long)]
        all: bool,
    },

    /// Show archived session details
    Show {
        /// Archived session ID, ID prefix, title, or path
        identifier: String,
    },

    /// Restore an archived session
    Restore {
        /// Archived session ID, ID prefix, title, or path
        identifier: String,
    },

    /// Permanently delete an archived session
    Delete {
        /// Archived session ID, ID prefix, title, or path
        identifier: String,
    },
}

#[derive(Serialize)]
struct ArchivedSessionJson {
    id: String,
    title: String,
    path: String,
    group: String,
    tool: String,
    profile: String,
    created_at: chrono::DateTime<chrono::Utc>,
    archived_at: chrono::DateTime<chrono::Utc>,
    last_status: Status,
}

pub async fn run(profile: &str, command: ArchiveCommands) -> Result<()> {
    match command {
        ArchiveCommands::List { json, all } => list_archived(profile, json, all).await,
        ArchiveCommands::Show { identifier } => show_archived(profile, &identifier).await,
        ArchiveCommands::Restore { identifier } => restore_archived(profile, &identifier).await,
        ArchiveCommands::Delete { identifier } => delete_archived(profile, &identifier).await,
    }
}

async fn list_archived(profile: &str, json: bool, all: bool) -> Result<()> {
    let entries = if all {
        load_all_archives()?
    } else {
        let storage = Storage::new(profile)?;
        storage.load_archive()?
    };

    if entries.is_empty() {
        println!("No archived sessions found.");
        return Ok(());
    }

    if json {
        let rows: Vec<_> = entries.iter().map(to_json).collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    print_table_header();
    for entry in &entries {
        print_table_row(entry);
    }
    println!("\nTotal: {} archived sessions", entries.len());
    Ok(())
}

async fn show_archived(profile: &str, identifier: &str) -> Result<()> {
    let storage = Storage::new(profile)?;
    let entries = storage.load_archive()?;
    let entry = resolve_archive(identifier, &entries)?;

    println!("Title:       {}", entry.instance.title);
    println!("ID:          {}", entry.id);
    println!("Profile:     {}", entry.source_profile);
    println!("Path:        {}", entry.instance.project_path);
    println!("Group:       {}", entry.instance.group_path);
    println!("Tool:        {}", entry.instance.tool);
    println!("Created:     {}", entry.instance.created_at);
    println!("Archived:    {}", entry.archived_at);
    println!("Last status: {:?}", entry.last_status);
    println!(
        "Restore:     {}",
        if std::path::Path::new(&entry.instance.project_path).exists() {
            "available"
        } else {
            "project path missing"
        }
    );
    Ok(())
}

async fn restore_archived(profile: &str, identifier: &str) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;
    let entries = storage.load_archive()?;
    let entry = resolve_archive(identifier, &entries)?.clone();
    entry.validate_restore(&instances)?;
    let restored = entry.restore_instance()?;

    instances.push(restored);
    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;
    storage.delete_archived_session(&entry.id)?;

    println!(
        "Restored archived session: {} (profile '{}')",
        entry.instance.title,
        storage.profile()
    );
    Ok(())
}

async fn delete_archived(profile: &str, identifier: &str) -> Result<()> {
    let storage = Storage::new(profile)?;
    let entries = storage.load_archive()?;
    let entry = resolve_archive(identifier, &entries)?;
    let title = entry.instance.title.clone();
    let id = entry.id.clone();
    storage.delete_archived_session(&id)?;
    println!(
        "Permanently deleted archived session: {} (profile '{}')",
        title,
        storage.profile()
    );
    Ok(())
}

fn load_all_archives() -> Result<Vec<ArchivedSession>> {
    let mut all = Vec::new();
    for profile in crate::session::list_profiles()? {
        let storage = Storage::new(&profile)?;
        all.extend(storage.load_archive()?);
    }
    all.sort_by(|left, right| right.archived_at.cmp(&left.archived_at));
    Ok(all)
}

fn resolve_archive<'a>(
    identifier: &str,
    entries: &'a [ArchivedSession],
) -> Result<&'a ArchivedSession> {
    if let Some(entry) = entries.iter().find(|entry| entry.id == identifier) {
        return Ok(entry);
    }
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.id.starts_with(identifier))
    {
        return Ok(entry);
    }
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.instance.title == identifier)
    {
        return Ok(entry);
    }
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.instance.project_path == identifier)
    {
        return Ok(entry);
    }

    bail!("Archived session not found: {}", identifier)
}

fn to_json(entry: &ArchivedSession) -> ArchivedSessionJson {
    ArchivedSessionJson {
        id: entry.id.clone(),
        title: entry.instance.title.clone(),
        path: entry.instance.project_path.clone(),
        group: entry.instance.group_path.clone(),
        tool: entry.instance.tool.clone(),
        profile: entry.source_profile.clone(),
        created_at: entry.instance.created_at,
        archived_at: entry.archived_at,
        last_status: entry.last_status,
    }
}

fn print_table_header() {
    println!(
        "{:<width_title$} {:<width_group$} {:<width_path$} ID",
        "TITLE",
        "GROUP",
        "PATH",
        width_title = TABLE_COL_TITLE,
        width_group = TABLE_COL_GROUP,
        width_path = TABLE_COL_PATH
    );
    println!(
        "{}",
        "-".repeat(TABLE_COL_TITLE + TABLE_COL_GROUP + TABLE_COL_PATH + TABLE_COL_ID_DISPLAY + 5)
    );
}

fn print_table_row(entry: &ArchivedSession) {
    let title = super::truncate(&entry.instance.title, TABLE_COL_TITLE);
    let group = super::truncate(&entry.instance.group_path, TABLE_COL_GROUP);
    let path = super::truncate(&entry.instance.project_path, TABLE_COL_PATH);
    let id_display = super::truncate_id(&entry.id, TABLE_COL_ID_DISPLAY);
    println!(
        "{:<width_title$} {:<width_group$} {:<width_path$} {}",
        title,
        group,
        path,
        id_display,
        width_title = TABLE_COL_TITLE,
        width_group = TABLE_COL_GROUP,
        width_path = TABLE_COL_PATH
    );
}
