//! The concurrency shim: `std` types normally, `loom` types under the cargo feature `loom`, so the
//! concurrent core can be model-checked (test plan section 2.7). A cargo feature, not
//! `--cfg loom`, because ni-build rejects config and RUSTFLAGS overrides.

#[cfg(feature = "loom")]
pub use loom::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "loom")]
pub use loom::sync::{Arc, Mutex};

#[cfg(not(feature = "loom"))]
pub use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(feature = "loom"))]
pub use std::sync::{Arc, Mutex};

use crate::error::HostError;

/// Constant-time byte comparison for secrets (reference-threat-model.md SEC-376).
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    // Token lengths are public protocol metadata. For equal lengths, subtle
    // prevents content-dependent early returns and optimizer removal.
    bool::from(left.ct_eq(right))
}

/// One installed plugin as recorded in a lockfile (reference-requirements-user-admin.md USR-241).
#[derive(Debug, Clone, PartialEq)]
pub struct LockEntry {
    pub plugin_id: String,
    pub version: String,
    pub content_hash: String,
    pub signing_key_id: String,
    pub granted_permissions: Vec<String>,
}

/// Serialised form of one lockfile entry.
#[derive(serde::Serialize, serde::Deserialize)]
struct LockRecord {
    id: String,
    version: String,
    hash: String,
    key: String,
    grants: Vec<String>,
}

/// Serialised form of a whole lockfile.
#[derive(serde::Serialize, serde::Deserialize)]
struct LockDocument {
    #[serde(default)]
    plugin: Vec<LockRecord>,
}

const LOCKFILE_HEADER: &str = "# ilium plugin lockfile, format 1\n";

/// Writes the installed plugin set as deterministic lockfile text: sorted entries, no timestamps,
/// no machine paths, so two machines with the same set produce byte-identical files (USR-241, USR-243).
pub fn export_lockfile(entries: &[LockEntry]) -> String {
    let mut sorted: Vec<&LockEntry> = entries.iter().collect();
    sorted.sort_by(|left, right| {
        (&left.plugin_id, &left.version).cmp(&(&right.plugin_id, &right.version))
    });
    let document = LockDocument {
        plugin: sorted
            .into_iter()
            .map(|entry| LockRecord {
                id: entry.plugin_id.clone(),
                version: entry.version.clone(),
                hash: entry.content_hash.clone(),
                key: entry.signing_key_id.clone(),
                grants: entry.granted_permissions.clone(),
            })
            .collect(),
    };
    // Serialising plain strings and string arrays to TOML cannot fail; an empty text keeps the
    // function total if that ever changes.
    let body = toml::to_string(&document).unwrap_or_default();
    format!("{LOCKFILE_HEADER}{body}")
}

/// Reads lockfile text back into entries for `ilium plugin sync --locked` (USR-242).
pub fn replay_lockfile(lockfile_text: &str) -> Result<Vec<LockEntry>, HostError> {
    let document: LockDocument = toml::from_str(lockfile_text)
        .map_err(|error| HostError::EngineError(format!("lockfile is not valid: {error}")))?;
    Ok(document
        .plugin
        .into_iter()
        .map(|record| LockEntry {
            plugin_id: record.id,
            version: record.version,
            content_hash: record.hash,
            signing_key_id: record.key,
            granted_permissions: record.grants,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, export_lockfile, replay_lockfile, LockEntry};

    fn sample_entry(plugin_id: &str) -> LockEntry {
        LockEntry {
            plugin_id: plugin_id.to_string(),
            version: "1.2.3".to_string(),
            content_hash: "sha256:abc123".to_string(),
            signing_key_id: "key-main".to_string(),
            granted_permissions: vec!["fs.read".to_string()],
        }
    }

    /// REQ: USR-241
    #[test]
    fn lockfile_records_id_version_hash_key_and_grants() {
        let text = export_lockfile(&[sample_entry("ilium.example.notes")]);
        assert!(text.contains("ilium.example.notes"));
        assert!(text.contains("1.2.3"));
        assert!(text.contains("sha256:abc123"));
        assert!(text.contains("key-main"));
        assert!(text.contains("fs.read"));
    }

    /// REQ: USR-243
    #[test]
    fn lockfile_is_byte_identical_regardless_of_input_order() {
        let first = sample_entry("ilium.example.alpha");
        let second = sample_entry("ilium.example.beta");
        assert_eq!(
            export_lockfile(&[first.clone(), second.clone()]),
            export_lockfile(&[second, first])
        );
    }

    /// REQ: USR-241
    #[test]
    fn lockfile_round_trip_returns_every_recorded_field() {
        let entry = sample_entry("ilium.example.notes");
        let text = export_lockfile(std::slice::from_ref(&entry));
        let replayed = replay_lockfile(&text).expect("a lockfile this host wrote must replay");
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].plugin_id, entry.plugin_id);
        assert_eq!(replayed[0].version, entry.version);
        assert_eq!(replayed[0].content_hash, entry.content_hash);
        assert_eq!(replayed[0].signing_key_id, entry.signing_key_id);
        assert_eq!(replayed[0].granted_permissions, entry.granted_permissions);
    }

    /// REQ: USR-243
    #[test]
    fn lockfile_contains_no_machine_paths() {
        let text = export_lockfile(&[sample_entry("ilium.example.notes")]);
        assert!(
            !text.contains("/home/") && !text.contains("C:\\") && !text.contains("/tmp/"),
            "a lockfile must not record machine paths, or two machines would differ"
        );
    }

    /// REQ: SEC-376
    #[test]
    fn identical_non_empty_slices_compare_equal() {
        let token = b"lock-token-0123456789abcdef";
        assert!(constant_time_eq(token, token));
    }

    /// REQ: SEC-376
    #[test]
    fn two_empty_slices_compare_equal() {
        assert!(constant_time_eq(&[], &[]));
    }

    /// REQ: SEC-376
    #[test]
    fn empty_and_non_empty_slices_compare_unequal() {
        assert!(!constant_time_eq(&[], b"x"));
        assert!(!constant_time_eq(b"x", &[]));
    }

    /// REQ: SEC-376
    #[test]
    fn single_byte_slices_compare_by_value() {
        assert!(constant_time_eq(&[0x00], &[0x00]));
        assert!(!constant_time_eq(&[0x00], &[0x01]));
        assert!(!constant_time_eq(&[0xFF], &[0x7F]));
    }

    /// REQ: SEC-376
    #[test]
    fn difference_in_first_byte_is_detected() {
        assert!(!constant_time_eq(b"Xbcdef", b"abcdef"));
    }

    /// REQ: SEC-376
    #[test]
    fn difference_in_last_byte_is_detected() {
        assert!(!constant_time_eq(b"abcdeX", b"abcdef"));
    }

    /// REQ: SEC-376
    #[test]
    fn difference_in_middle_byte_is_detected() {
        assert!(!constant_time_eq(b"abXdef", b"abcdef"));
    }

    /// REQ: SEC-376
    #[test]
    fn single_bit_difference_is_detected() {
        let base = [0u8; 64];
        let mut flipped = [0u8; 64];
        flipped[63] = 0b0000_0001;
        assert!(!constant_time_eq(&base, &flipped));
        assert!(!constant_time_eq(&flipped, &base));
    }

    /// REQ: SEC-376
    #[test]
    fn all_zero_and_all_ones_slices_compare_unequal() {
        assert!(constant_time_eq(&[0u8; 32], &[0u8; 32]));
        assert!(!constant_time_eq(&[0u8; 32], &[0xFFu8; 32]));
    }

    /// REQ: SEC-376
    #[test]
    fn prefix_of_longer_token_compares_unequal() {
        assert!(!constant_time_eq(b"secret", b"secret-and-more"));
        assert!(!constant_time_eq(b"secret-and-more", b"secret"));
    }

    /// REQ: SEC-376
    #[test]
    fn different_length_slices_with_equal_leading_bytes_compare_unequal() {
        assert!(!constant_time_eq(b"abc\0", b"abc"));
    }

    /// REQ: SEC-376
    #[test]
    fn comparison_is_symmetric() {
        let pairs: [(&[u8], &[u8]); 4] = [
            (b"token-a", b"token-b"),
            (b"same", b"same"),
            (b"", b"x"),
            (b"longer-value", b"longer-valu"),
        ];
        for (left, right) in pairs {
            assert_eq!(constant_time_eq(left, right), constant_time_eq(right, left));
        }
    }

    /// REQ: SEC-376
    #[test]
    fn high_bit_and_non_ascii_bytes_compare_by_value() {
        let left = [0x80u8, 0xC3, 0xA9, 0xFF];
        let right = [0x80u8, 0xC3, 0xA9, 0xFE];
        assert!(constant_time_eq(&left, &left));
        assert!(!constant_time_eq(&left, &right));
    }
}
