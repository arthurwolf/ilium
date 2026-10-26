//! Persistent project identity, independent of a directory's spelling/location.
//! The small sidecar remains readable when an unrelated YAML setting is broken.
use super::{err, ResourceError};
use ilium_core::resource_queue::ResourceProjectId;
use ilium_platform::resource_fs::ResourceLedgerFile;
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};
#[derive(Serialize, Deserialize)]
struct ProjectIdentity {
    schema_version: u32,
    project_id: ResourceProjectId,
}
fn valid(id: &ResourceProjectId) -> bool {
    uuid::Uuid::parse_str(&id.0).is_ok()
}
/// Blocking filesystem operation: callers must use the admitted I/O boundary.
pub(super) fn project_identity(project: &Path) -> Result<ResourceProjectId, ResourceError> {
    let canonical = project
        .canonicalize()
        .map_err(|error| err("project_identity", error))?;
    if !canonical.is_dir() {
        return Err(err("project_identity", "Project path is not a directory"));
    }
    let path = canonical.join(".ilium/resource-project-id");
    let ledger = ResourceLedgerFile::try_lock(&path)
        .map_err(|error| err("project_identity", error))?
        .ok_or_else(|| err("busy", "Project identity is busy; retry the same request"))?;
    if let Some(bytes) = ledger
        .read_reconciled(1024)
        .map_err(|error| err("project_identity", error))?
    {
        let identity: ProjectIdentity =
            serde_json::from_slice(&bytes).map_err(|error| err("project_identity", error))?;
        if identity.schema_version != 1 || !valid(&identity.project_id) {
            return Err(err(
                "project_identity",
                "Project identity sidecar is invalid; refusing identity replacement",
            ));
        }
        return Ok(identity.project_id);
    }
    let configured = read_config_identity(&canonical.join(".ilium/config.yaml"))?;
    let project_id =
        configured.unwrap_or_else(|| ResourceProjectId(uuid::Uuid::new_v4().to_string()));
    let bytes = serde_json::to_vec(&ProjectIdentity {
        schema_version: 1,
        project_id: project_id.clone(),
    })
    .map_err(|error| err("project_identity", error))?;
    ledger.replace(&bytes).map_err(|error| {
        err(
            "outcome_unknown",
            format!("Project identity persistence needs reconciliation: {error}"),
        )
    })?;
    Ok(project_id)
}
fn read_config_identity(path: &Path) -> Result<Option<ResourceProjectId>, ResourceError> {
    let target = match std::fs::symlink_metadata(path) {
        Ok(_) => path
            .canonicalize()
            .map_err(|error| err("project_identity", error))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(err("project_identity", error)),
    };
    let file = match ilium_platform::secure_fs::open_regular_file(&target) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(err("project_identity", error)),
    };
    let mut bytes = Vec::new();
    file.take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| err("project_identity", error))?;
    // Broken/oversized metadata is reported by projection synchronization. It
    // must not cause a second identity or roll back committed resource state.
    if bytes.len() > 256 * 1024 {
        return Ok(None);
    }
    let Ok(value) = serde_norway::from_slice::<serde_norway::Value>(&bytes) else {
        return Ok(None);
    };
    let Some(value) = value.get("resource_project_id") else {
        return Ok(None);
    };
    let Some(value) = value.as_str() else {
        return Err(err(
            "project_identity",
            "resource_project_id must be a UUID string",
        ));
    };
    let id = ResourceProjectId(value.into());
    if !valid(&id) {
        return Err(err(
            "project_identity",
            "resource_project_id must be a UUID string",
        ));
    }
    Ok(Some(id))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_survives_project_move_and_malformed_settings() {
        let directory = super::super::store::durable_test_directory();
        let original = directory.path().join("original");
        std::fs::create_dir(&original).unwrap();
        let first = project_identity(&original).unwrap();
        std::fs::write(original.join(".ilium/config.yaml"), "[broken").unwrap();
        assert_eq!(project_identity(&original).unwrap(), first);
        let moved = directory.path().join("moved");
        std::fs::rename(&original, &moved).unwrap();
        assert_eq!(project_identity(&moved).unwrap(), first);
    }
    #[test]
    fn configured_uuid_is_adopted_and_durable() {
        let directory = super::super::store::durable_test_directory();
        std::fs::create_dir(directory.path().join(".ilium")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        std::fs::write(
            directory.path().join(".ilium/config.yaml"),
            format!("resource_project_id: {id}\nunrelated: retained\n"),
        )
        .unwrap();
        assert_eq!(project_identity(directory.path()).unwrap().0, id);
        std::fs::remove_file(directory.path().join(".ilium/config.yaml")).unwrap();
        assert_eq!(project_identity(directory.path()).unwrap().0, id);
    }
    #[test]
    fn malformed_sidecar_is_never_replaced() {
        let directory = super::super::store::durable_test_directory();
        std::fs::create_dir(directory.path().join(".ilium")).unwrap();
        let path = directory.path().join(".ilium/resource-project-id");
        std::fs::write(&path, b"broken").unwrap();
        assert!(project_identity(directory.path()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"broken");
    }
}
