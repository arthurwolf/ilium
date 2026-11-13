//! Pure rules of the persistent project identity (port of the decision part of
//! `ilium-server/src/resources/identity.rs`).
//!
//! The sidecar `.ilium/resource-project-id` holds `{"schema_version":1,"project_id":"<uuid>"}`.
//! Reading and writing the file stays with the host (G10, `fs`); this module decides what a
//! sidecar content means. A broken sidecar is never replaced: the caller reports the error.

use serde::{Deserialize, Serialize};

/// The sidecar schema version this plugin reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// The longest sidecar the reader accepts, in bytes.
pub const MAXIMUM_SIDECAR_BYTES: usize = 1024;

#[derive(Serialize, Deserialize)]
struct Sidecar {
    schema_version: u32,
    project_id: String,
}

/// Whether `id` is a UUID in the 8-4-4-4-12 hexadecimal form.
pub fn is_valid_project_id(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    const LENGTHS: [usize; 5] = [8, 4, 4, 4, 12];
    groups.len() == LENGTHS.len()
        && groups.iter().zip(LENGTHS).all(|(group, length)| {
            group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

/// The identity a sidecar holds. An unknown schema version, an invalid id, malformed JSON or an
/// oversized content is an error, so the caller never replaces the file.
pub fn read_sidecar(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAXIMUM_SIDECAR_BYTES {
        return Err("Project identity sidecar is too large".to_string());
    }
    let sidecar: Sidecar =
        serde_json::from_slice(bytes).map_err(|error| format!("project_identity: {error}"))?;
    if sidecar.schema_version != SCHEMA_VERSION || !is_valid_project_id(&sidecar.project_id) {
        return Err(
            "Project identity sidecar is invalid; refusing identity replacement".to_string(),
        );
    }
    Ok(sidecar.project_id)
}

/// The sidecar content of an identity, or an error for an id that is not a UUID.
pub fn write_sidecar(project_id: &str) -> Result<Vec<u8>, String> {
    if !is_valid_project_id(project_id) {
        return Err("resource_project_id must be a UUID string".to_string());
    }
    serde_json::to_vec(&Sidecar {
        schema_version: SCHEMA_VERSION,
        project_id: project_id.to_string(),
    })
    .map_err(|error| format!("project_identity: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b9f7c1e-3d5a-4e8b-9c2f-1a6d4e7b8c90";

    /// REQ: PORT:ilium-server/src/resources/identity.rs::identity_survives_project_move_and_malformed_settings
    #[test]
    fn identity_round_trips_through_the_sidecar() {
        let bytes = write_sidecar(ID).unwrap();
        assert_eq!(read_sidecar(&bytes).unwrap(), ID);
    }

    /// REQ: PORT:ilium-server/src/resources/identity.rs::configured_uuid_is_adopted_and_durable
    #[test]
    fn only_a_uuid_is_accepted_as_identity() {
        assert!(is_valid_project_id(ID));
        for bad in [
            "",
            "broken",
            "0b9f7c1e3d5a4e8b9c2f1a6d4e7b8c90",
            "0b9f7c1e-3d5a-4e8b-9c2f-1a6d4e7b8c9g",
        ] {
            assert!(!is_valid_project_id(bad), "{bad}");
            assert!(write_sidecar(bad).is_err(), "{bad}");
        }
    }

    /// REQ: PORT:ilium-server/src/resources/identity.rs::malformed_sidecar_is_never_replaced
    #[test]
    fn a_malformed_or_foreign_sidecar_is_an_error_not_a_new_identity() {
        assert!(read_sidecar(b"broken").is_err());
        assert!(read_sidecar(
            br#"{"schema_version":2,"project_id":"0b9f7c1e-3d5a-4e8b-9c2f-1a6d4e7b8c90"}"#
        )
        .is_err());
        assert!(read_sidecar(br#"{"schema_version":1,"project_id":"nope"}"#).is_err());
        assert!(read_sidecar(&vec![b' '; MAXIMUM_SIDECAR_BYTES + 1]).is_err());
    }
}
