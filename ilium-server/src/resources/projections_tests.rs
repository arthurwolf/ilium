use super::*;

fn project_directory() -> tempfile::TempDir {
    // Project identity is durable authority too; /tmp may be volatile or layered.
    crate::resources::store::durable_test_directory()
}

fn link(project: &Path, slug: &str) -> ProjectResourceLink {
    ProjectResourceLink {
        project_id: ResourceProjectId(project.to_string_lossy().into_owned()),
        resource_id: ResourceId("synthetic-resource".into()),
        slug: slug.into(),
        project_path: project.to_string_lossy().into_owned(),
        created_at_unix_millis: 1234,
        desired_instruction_revision: 7,
        sync_debt: None,
    }
}
fn bindings(slug: &str) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"id":"synthetic-resource","slug":slug,"title":"Synthetic GPU","description":"Fixture only","rules":"Release after cleanup"}),
    ]
}
fn yaml(project: &Path) -> serde_norway::Value {
    serde_norway::from_str(&std::fs::read_to_string(project.join(".ilium/config.yaml")).unwrap())
        .unwrap()
}

#[test]
fn structured_binding_readback_preserves_unknown_yaml_and_installed_revision() {
    let project = project_directory();
    std::fs::create_dir(project.path().join(".ilium")).unwrap();
    std::fs::write(
        project.path().join(".ilium/config.yaml"),
        "project name: Authored\ncustom: !Keep [one, two]\nnonfinite: .inf\n",
    )
    .unwrap();
    write_project_config(project.path(), &[link(project.path(), "gpu")], 19).unwrap();
    let readback = yaml(project.path());
    assert_eq!(readback["project name"].as_str(), Some("Authored"));
    assert!(readback["nonfinite"].as_f64().unwrap().is_infinite());
    assert_eq!(readback["resources"][0]["slug"].as_str(), Some("gpu"));
    assert_eq!(
        readback["resources"][0]["created_at_unix_millis"].as_i64(),
        Some(1234)
    );
    assert_eq!(
        readback["resource synchronization"]["authority revision"].as_u64(),
        Some(19)
    );
    assert_eq!(
        readback["resource synchronization"]["installed instruction revisions"]
            ["synthetic-resource"]
            .as_u64(),
        Some(7)
    );
    assert!(
        std::fs::read_to_string(project.path().join(".ilium/config.yaml"))
            .unwrap()
            .contains("!Keep")
    );
}

#[test]
fn metadata_writer_uses_the_existing_client_lock() {
    let project = project_directory();
    let directory = project.path().join(".ilium");
    std::fs::create_dir(&directory).unwrap();
    let lock =
        ilium_platform::file_lock::ExclusiveFileLock::acquire(&directory.join(".config.yaml.lock"))
            .unwrap();
    let failure =
        write_project_config(project.path(), &[link(project.path(), "gpu")], 1).unwrap_err();
    assert_eq!(failure.code, "busy");
    assert!(!directory.join("config.yaml").exists());
    drop(lock);
    // A settings writer's intervening field must survive resource maintenance.
    std::fs::write(directory.join("config.yaml"), "new preference: true\n").unwrap();
    write_project_config(project.path(), &[link(project.path(), "gpu")], 2).unwrap();
    assert_eq!(yaml(project.path())["new preference"].as_bool(), Some(true));
}

#[test]
fn deleting_final_link_keeps_unrelated_metadata() {
    let project = project_directory();
    std::fs::create_dir(project.path().join(".ilium")).unwrap();
    std::fs::write(
        project.path().join(".ilium/config.yaml"),
        "project icon: custom\n",
    )
    .unwrap();
    write_project_config(project.path(), &[link(project.path(), "gpu")], 2).unwrap();
    write_project_config(project.path(), &[], 3).unwrap();
    let readback = yaml(project.path());
    assert_eq!(readback["project icon"].as_str(), Some("custom"));
    assert!(readback["resources"].as_sequence().unwrap().is_empty());
}

#[test]
fn missing_projects_are_not_recreated_by_maintenance() {
    let parent = project_directory();
    let missing = parent.path().join("moved-project");
    let error =
        synchronize_project(&missing, &[link(&missing, "gpu")], &bindings("gpu"), 1).unwrap_err();
    assert_eq!(error.code, "projection");
    assert!(!missing.exists());
}

#[test]
fn provider_target_policy_creates_only_default_project_instructions() {
    let project = project_directory();
    synchronize_project(
        project.path(),
        &[link(project.path(), "gpu")],
        &bindings("gpu"),
        1,
    )
    .unwrap();
    assert!(project.path().join("CLAUDE.md").is_file());
    assert!(!project.path().join("GEMINI.md").exists());
    assert!(!project.path().join("AGENTS.md").exists());
    let text = std::fs::read_to_string(project.path().join("CLAUDE.md")).unwrap();
    assert!(text.contains("Synthetic GPU"));
    assert!(text.contains("gpu"));
}

#[test]
fn existing_provider_targets_and_local_slugs_are_updated() {
    let project = project_directory();
    std::fs::write(
        project.path().join("GEMINI.md"),
        "Authored provider instructions\n",
    )
    .unwrap();
    synchronize_project(
        project.path(),
        &[link(project.path(), "board-local")],
        &bindings("board-local"),
        8,
    )
    .unwrap();
    let provider = std::fs::read_to_string(project.path().join("GEMINI.md")).unwrap();
    assert!(provider.starts_with("Authored provider instructions\n"));
    assert!(provider.contains("board-local"));
    assert_eq!(
        yaml(project.path())["resources"][0]["resource_id"].as_str(),
        Some("synthetic-resource")
    );
}

#[test]
fn no_binding_maintenance_does_not_create_instruction_or_settings_files() {
    let project = project_directory();
    synchronize_project(project.path(), &[], &[], 1).unwrap();
    assert!(!project.path().join("CLAUDE.md").exists());
    assert!(!project.path().join(".ilium").exists());
}

#[test]
fn malformed_settings_are_preserved_and_not_replaced() {
    let project = project_directory();
    std::fs::create_dir(project.path().join(".ilium")).unwrap();
    let path = project.path().join(".ilium/config.yaml");
    let authored = "broken: [not closed\n";
    std::fs::write(&path, authored).unwrap();
    assert!(write_project_config(project.path(), &[link(project.path(), "gpu")], 1).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), authored);
}

#[test]
fn oversized_config_is_rejected_without_truncation() {
    let project = project_directory();
    std::fs::create_dir(project.path().join(".ilium")).unwrap();
    let path = project.path().join(".ilium/config.yaml");
    let bytes = "x".repeat(MAX_PROJECT_CONFIG_BYTES + 1);
    std::fs::write(&path, &bytes).unwrap();
    assert!(write_project_config(project.path(), &[link(project.path(), "gpu")], 1).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
}

#[test]
fn malformed_instruction_target_prevents_installed_revision_receipt() {
    let project = project_directory();
    std::fs::write(
        project.path().join("CLAUDE.md"),
        "<!-- ilium-agent-feature: resources version=1 -->\nunterminated\n",
    )
    .unwrap();
    assert!(synchronize_project(
        project.path(),
        &[link(project.path(), "gpu")],
        &bindings("gpu"),
        1
    )
    .is_err());
    assert!(!project.path().join(".ilium/config.yaml").exists());
}

#[test]
fn identical_metadata_maintenance_does_not_replace_file() {
    let project = project_directory();
    let links = [link(project.path(), "gpu")];
    write_project_config(project.path(), &links, 9).unwrap();
    let path = project.path().join(".ilium/config.yaml");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    write_project_config(project.path(), &links, 9).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
}
