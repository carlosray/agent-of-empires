//! Integration tests for the core session lifecycle: create, persist, load, remove.

use agent_of_empires::session::{
    ArchiveCleanupOptions, GroupTree, Instance, SandboxInfo, Status, Storage, TerminalInfo,
};
use anyhow::Result;
use serial_test::serial;
use std::fs;

mod common;
use common::setup_temp_home;

#[test]
#[serial]
fn test_create_session_persists() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let instance = Instance::new("My Project", "/home/user/project");
    let group_tree = GroupTree::new_with_groups(std::slice::from_ref(&instance), &[]);

    storage.save_with_groups(std::slice::from_ref(&instance), &group_tree)?;

    let (loaded, _groups) = storage.load_with_groups()?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].title, "My Project");
    assert_eq!(loaded[0].project_path, "/home/user/project");
    assert_eq!(loaded[0].id, instance.id);

    Ok(())
}

#[test]
#[serial]
fn test_create_multiple_sessions() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let instances = vec![
        Instance::new("Project A", "/path/a"),
        Instance::new("Project B", "/path/b"),
        Instance::new("Project C", "/path/c"),
    ];
    let group_tree = GroupTree::new_with_groups(&instances, &[]);

    storage.save_with_groups(&instances, &group_tree)?;

    let (loaded, _) = storage.load_with_groups()?;
    assert_eq!(loaded.len(), 3);
    assert_eq!(loaded[0].title, "Project A");
    assert_eq!(loaded[1].title, "Project B");
    assert_eq!(loaded[2].title, "Project C");

    Ok(())
}

#[test]
#[serial]
fn test_remove_session_by_id() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let inst_a = Instance::new("Keep Me", "/path/keep");
    let inst_b = Instance::new("Remove Me", "/path/remove");
    let remove_id = inst_b.id.clone();

    let instances = vec![inst_a, inst_b];
    let group_tree = GroupTree::new_with_groups(&instances, &[]);
    storage.save_with_groups(&instances, &group_tree)?;

    // Remove by filtering
    let (mut loaded, groups) = storage.load_with_groups()?;
    loaded.retain(|i| i.id != remove_id);
    let group_tree = GroupTree::new_with_groups(&loaded, &groups);
    storage.save_with_groups(&loaded, &group_tree)?;

    let (reloaded, _) = storage.load_with_groups()?;
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].title, "Keep Me");

    Ok(())
}

#[test]
#[serial]
fn test_create_session_with_group() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let mut instance = Instance::new("Grouped Session", "/path/grouped");
    instance.group_path = "work".to_string();

    let mut group_tree = GroupTree::new_with_groups(std::slice::from_ref(&instance), &[]);
    group_tree.create_group("work");

    storage.save_with_groups(std::slice::from_ref(&instance), &group_tree)?;

    let (loaded, loaded_groups) = storage.load_with_groups()?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].group_path, "work");

    let reloaded_tree = GroupTree::new_with_groups(&loaded, &loaded_groups);
    assert!(reloaded_tree.group_exists("work"));

    Ok(())
}

#[test]
#[serial]
fn test_session_backup_created() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;

    // First save
    let instances = vec![Instance::new("First", "/path/first")];
    storage.save(&instances)?;

    // Second save triggers backup of the first
    let instances2 = vec![Instance::new("Second", "/path/second")];
    storage.save(&instances2)?;

    // Verify backup exists by checking the profile directory
    let profile_dir = agent_of_empires::session::get_profile_dir("default")?;
    let backup_path = profile_dir.join("sessions.json.bak");
    assert!(backup_path.exists());

    let backup_content = fs::read_to_string(&backup_path)?;
    assert!(backup_content.contains("First"));

    Ok(())
}

#[test]
#[serial]
fn test_source_profile_not_serialized() {
    let _temp = setup_temp_home();

    let mut instance = Instance::new("Test", "/tmp/test");
    instance.source_profile = "work".to_string();

    let storage = Storage::new("default").unwrap();
    storage.save(std::slice::from_ref(&instance)).unwrap();

    // Read raw JSON -- source_profile should not appear
    let profile_dir = agent_of_empires::session::get_profile_dir("default").unwrap();
    let content = std::fs::read_to_string(profile_dir.join("sessions.json")).unwrap();
    assert!(
        !content.contains("source_profile"),
        "source_profile should not be serialized"
    );

    // Reload -- source_profile should default to empty
    let loaded = storage.load().unwrap();
    assert_eq!(loaded[0].source_profile, "");
}

#[test]
#[serial]
fn test_display_branch_persists_across_save_load() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let mut instance = Instance::new("Git Session", "/path/repo");
    instance.display_branch = Some("feature/persisted".to_string());

    storage.save(std::slice::from_ref(&instance))?;

    let loaded = storage.load()?;
    assert_eq!(
        loaded[0].display_branch.as_deref(),
        Some("feature/persisted")
    );

    Ok(())
}

#[test]
#[serial]
fn test_storage_defaults_to_default_profile() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("")?;
    assert_eq!(storage.profile(), "default");

    // Verify it can save and load
    let instances = vec![Instance::new("Test", "/path/test")];
    storage.save(&instances)?;
    let loaded = storage.load()?;
    assert_eq!(loaded.len(), 1);

    Ok(())
}

#[test]
#[serial]
fn test_archive_session_roundtrip() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let mut instance = Instance::new("Archived", "/tmp/archived");
    instance.status = Status::Running;

    storage.archive_instance(&instance, ArchiveCleanupOptions::default(), 100, None)?;

    let archived = storage.load_archive()?;
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, instance.id);
    assert_eq!(archived[0].instance.title, "Archived");
    assert_eq!(archived[0].source_profile, "default");
    assert_eq!(archived[0].last_status, Status::Running);

    Ok(())
}

#[test]
#[serial]
fn test_archive_prunes_to_max_entries() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let first = Instance::new("First", "/tmp/first");
    let second = Instance::new("Second", "/tmp/second");
    let third = Instance::new("Third", "/tmp/third");

    storage.archive_instance(&first, ArchiveCleanupOptions::default(), 2, None)?;
    storage.archive_instance(&second, ArchiveCleanupOptions::default(), 2, None)?;
    storage.archive_instance(&third, ArchiveCleanupOptions::default(), 2, None)?;

    let archived = storage.load_archive()?;
    assert_eq!(archived.len(), 2);
    assert!(archived.iter().any(|entry| entry.id == second.id));
    assert!(archived.iter().any(|entry| entry.id == third.id));
    assert!(!archived.iter().any(|entry| entry.id == first.id));

    Ok(())
}

#[test]
#[serial]
fn test_archived_restore_instance_is_safe_partial_restore() -> Result<()> {
    let temp = setup_temp_home();
    let project_path = temp.path().join("project");
    fs::create_dir_all(&project_path)?;

    let storage = Storage::new("default")?;
    let mut instance = Instance::new("Restore", project_path.to_string_lossy().as_ref());
    instance.status = Status::Running;
    instance.last_error = Some("old failure".to_string());
    instance.terminal_info = Some(TerminalInfo { created: true });
    instance.sandbox_info = Some(SandboxInfo {
        enabled: true,
        container_id: Some("stale-container".to_string()),
        image: "ubuntu:latest".to_string(),
        container_name: "aoe-test".to_string(),
        extra_env: None,
        custom_instruction: None,
    });

    storage.archive_instance(&instance, ArchiveCleanupOptions::default(), 100, None)?;
    let archived = storage.load_archive()?;
    let restored = archived[0].restore_instance()?;

    assert_eq!(restored.id, instance.id);
    assert_eq!(restored.status, Status::Stopped);
    assert!(restored.terminal_info.is_none());
    assert!(restored.last_error.is_none());
    let sandbox = restored.sandbox_info.as_ref().unwrap();
    assert!(sandbox.container_id.is_none());

    Ok(())
}

#[test]
#[serial]
fn test_archived_restore_fails_when_project_path_is_missing() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let instance = Instance::new("Missing", "/tmp/aoe-missing-restore-path");
    storage.archive_instance(&instance, ArchiveCleanupOptions::default(), 100, None)?;

    let archived = storage.load_archive()?;
    let err = archived[0].restore_instance().unwrap_err();
    assert!(err.to_string().contains("project path does not exist"));

    Ok(())
}
