//! Recoverable, bounded projections of authoritative resource links.
//!
//! Project metadata and agent instructions cannot share a transaction with the
//! catalog. Failures therefore become durable per-link debt, never rollback of
//! an ownership transition or a second source of resource authority.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_PROJECT_CONFIG_BYTES: usize = 256 * 1024;

/// Repair only recorded linked projects and the explicitly removed-link path.
/// The returned catalog includes synchronization-debt transactions. Callers
/// must publish this revision rather than their pre-maintenance snapshot.
pub(super) async fn synchronize_all(
    state: &ServerState,
    current: &mut Catalog,
    extra_project_path: Option<&Path>,
) -> Result<Option<String>, ResourceError> {
    let catalog = Arc::clone(current);
    let extra = extra_project_path.map(Path::to_path_buf);
    let paths = io(state, move || {
        let mut paths: Vec<PathBuf> = catalog
            .links
            .iter()
            .map(|link| PathBuf::from(&link.project_path))
            .collect();
        if let Some(path) = extra {
            paths.push(path.canonicalize().unwrap_or(path));
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    })
    .await?;
    let mut warning = None;
    for project in &paths.value {
        let outcome = synchronize(state, current, project).await;
        let reason = outcome
            .as_ref()
            .err()
            .map(|error| bounded_reason(&error.message));
        let catalog = Arc::clone(current);
        let target = project.clone();
        let expected = reason.clone();
        let changes = io(state, move || {
            Ok(catalog
                .links
                .iter()
                .filter(|link| {
                    Path::new(&link.project_path) == target && link.sync_debt != expected
                })
                .cloned()
                .collect::<Vec<_>>())
        })
        .await?;
        for link in &changes.value {
            let now = clock()?;
            let context = MutationContext {
                request_id: key(&now),
                clock: now,
                actor: ResourceActor::System,
                expected_revision: Some(current.revision),
                eligibility: BTreeMap::new(),
            };
            *current = committed_catalog(
                commit(
                    state,
                    ResourceCommand::SetSyncDebt {
                        project_id: link.project_id.clone(),
                        resource_id: link.resource_id.clone(),
                        reason: reason.clone(),
                    },
                    context,
                    None,
                )
                .await?,
            )
            .0;
        }
        if let Err(error) = outcome {
            if !current
                .links
                .iter()
                .any(|link| Path::new(&link.project_path) == project)
            {
                return Err(error);
            }
            if warning.is_none() {
                warning = Some(format!(
                    "Resource committed; instruction/settings synchronization needs repair: {}",
                    bounded_reason(&error.message)
                ));
            }
        }
    }
    Ok(warning)
}

fn bounded_reason(reason: &str) -> String {
    reason
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect()
}

pub(super) async fn synchronize(
    state: &ServerState,
    catalog: &Catalog,
    project: &Path,
) -> Result<(), ResourceError> {
    let catalog = Arc::clone(catalog);
    let project = project.to_path_buf();
    io(state,move||{let links:Vec<_>=catalog.links.iter().filter(|link|Path::new(&link.project_path)==project).cloned().collect();let bindings:Vec<_>=links.iter().filter_map(|link|catalog.resources.get(&link.resource_id).map(|record|serde_json::json!({"id":link.resource_id,"slug":link.slug,"title":record.definition.title,"description":record.definition.description,"rules":record.definition.rules,"kind":record.definition.kind,"physical_identity":record.definition.physical_identity,"definition_revision":record.definition.definition_revision}))).collect();synchronize_project(&project,&links,&bindings,catalog.revision)}).await.map(drop)
}

fn synchronize_project(
    project: &Path,
    links: &[ProjectResourceLink],
    bindings: &[serde_json::Value],
    authority_revision: u64,
) -> Result<(), ResourceError> {
    // Do not recreate deleted or moved projects while repairing a projection.
    let canonical = project
        .canonicalize()
        .map_err(|error| err("projection", error))?;
    if !canonical.is_dir() {
        return Err(err("projection", "Recorded project is not a directory"));
    }
    let json = serde_json::to_string(bindings).map_err(|error| err("projection", error))?;
    synchronize_instructions(&canonical, (!links.is_empty()).then_some(json.as_str()))?;
    // Installed revisions are written only after every selected target succeeds.
    // A partial instruction write is safe and is retried idempotently as debt.
    write_project_config(&canonical, links, authority_revision)
}

fn synchronize_instructions(
    project: &Path,
    catalog_json: Option<&str>,
) -> Result<(), ResourceError> {
    let mut targets = Vec::new();
    for name in ["CLAUDE.md", "GEMINI.md", "AGENTS.md"] {
        let target = project.join(name);
        let exists = match std::fs::symlink_metadata(&target) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(err("instructions", error)),
        };
        // Claude's project instructions are the existing chatroom default.
        // Other providers are maintained only when their target already exists.
        // Removing the final binding must not create an empty instruction file.
        if exists || (name == "CLAUDE.md" && catalog_json.is_some()) {
            targets.push(target);
        }
    }
    let global = ilium_instructions::global_claude_path().and_then(|path| path.canonicalize().ok());
    let mut resolved = BTreeSet::new();
    for target in targets {
        let canonical = match target.canonicalize() {
            Ok(path) => path,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && std::fs::symlink_metadata(&target).is_err() =>
            {
                target.clone()
            }
            Err(error) => return Err(err("instructions", error)),
        };
        if global.as_ref() == Some(&canonical) {
            return Err(err("instructions", "Project instruction target resolves to global Claude instructions; global files are not resource projections"));
        }
        if !resolved.insert(canonical) {
            continue;
        }
        ilium_instructions::synchronize_resource_instructions(&target, catalog_json)
            .map_err(|error| err("instructions", error))?;
    }
    Ok(())
}

fn read_config(path: &Path) -> Result<Option<String>, ResourceError> {
    let file = match ilium_platform::secure_fs::open_regular_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(err("projection", error)),
    };
    let mut contents = String::new();
    file.take((MAX_PROJECT_CONFIG_BYTES + 1) as u64)
        .read_to_string(&mut contents)
        .map_err(|error| err("projection", error))?;
    if contents.len() > MAX_PROJECT_CONFIG_BYTES {
        return Err(err("projection", "Project configuration exceeds 256 KiB"));
    }
    Ok(Some(contents))
}

fn config_target(path: &Path) -> Result<PathBuf, ResourceError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => path
            .canonicalize()
            .map_err(|error| err("projection", error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(err("projection", error)),
    }
}

fn write_project_config(
    project: &Path,
    links: &[ProjectResourceLink],
    revision: u64,
) -> Result<(), ResourceError> {
    let directory = project.join(".ilium");
    if links.is_empty() && !directory.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&directory).map_err(|error| err("projection", error))?;
    // This exact stable lock is shared with client::project_config::update_if.
    let _lock = ilium_platform::file_lock::ExclusiveFileLock::try_acquire(
        &directory.join(".config.yaml.lock"),
    )
    .map_err(|error| err("projection", error))?
    .ok_or_else(|| {
        err(
            "busy",
            "Project settings are being updated; resource projection will retry",
        )
    })?;
    let logical = directory.join("config.yaml");
    let target = config_target(&logical)?;
    let original = read_config(&target)?;
    if original.is_none() && links.is_empty() {
        return Ok(());
    }
    let mut value: serde_norway::Value = match original.as_deref() {
        Some(text) if !text.trim().is_empty() => {
            serde_norway::from_str(text).map_err(|error| err("projection", error))?
        }
        _ => serde_norway::Value::Mapping(Default::default()),
    };
    let mapping = value
        .as_mapping_mut()
        .ok_or_else(|| err("projection", "Project settings must be a mapping"))?;
    mapping.insert(
        serde_norway::Value::String("resource_project_id".into()),
        serde_norway::to_value(super::identity::project_identity(project)?)
            .map_err(|error| err("projection", error))?,
    );
    mapping.insert(
        serde_norway::Value::String("resources".into()),
        serde_norway::to_value(links).map_err(|error| err("projection", error))?,
    );
    let installed: BTreeMap<_, _> = links
        .iter()
        .map(|link| {
            (
                link.resource_id.0.clone(),
                link.desired_instruction_revision,
            )
        })
        .collect();
    let synchronization = serde_norway::to_value(serde_json::json!({"authority revision":revision,"installed instruction revisions":installed})).map_err(|error|err("projection",error))?;
    mapping.insert(
        serde_norway::Value::String("resource synchronization".into()),
        synchronization,
    );
    let encoded = serde_norway::to_string(&value).map_err(|error| err("projection", error))?;
    if encoded.len() > MAX_PROJECT_CONFIG_BYTES {
        return Err(err(
            "projection",
            "Projected project settings exceed 256 KiB",
        ));
    }
    if original.as_deref() == Some(encoded.as_str()) {
        return Ok(());
    }
    let parent = target
        .parent()
        .ok_or_else(|| err("projection", "Project settings have no parent directory"))?;
    let temporary = parent.join(format!(".resource-settings-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = ilium_platform::secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| err("projection", error))?;
        if let Ok(metadata) = std::fs::metadata(&target) {
            file.set_permissions(metadata.permissions())
                .map_err(|error| err("projection", error))?;
        }
        file.write_all(encoded.as_bytes())
            .map_err(|error| err("projection", error))?;
        file.sync_all().map_err(|error| err("projection", error))?;
        if config_target(&logical)? != target || read_config(&target)? != original {
            return Err(err(
                "busy",
                "Project configuration changed during resource projection; no replacement was made",
            ));
        }
        ilium_platform::secure_fs::replace_file_durably(&temporary, &target)
            .map_err(|error| err("projection", error))?;
        if read_config(&target)?.as_deref() != Some(encoded.as_str()) {
            return Err(err(
                "projection",
                "Project configuration readback differs from committed projection",
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
#[path = "projections_tests.rs"]
mod tests;
