//! On-disk copy of the attention queue, so it survives a client restart.
//!
//! One small JSON file per server session under the client's data directory.
//! Pane ids are only meaningful inside the session that issued them, so the
//! file name carries a digest of the session's socket path: two sessions can
//! never read each other's items. A restored item is still re-validated
//! against the live evidence fingerprints before it is shown (see
//! `AttentionQueue::from_persisted`), so a stale or foreign file cannot send
//! the user to the wrong agent.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::item::fingerprint_parts;
use super::queue::PersistedQueue;

/// Bumped when the stored shape changes; an unknown version is ignored.
const STORE_VERSION: u32 = 1;
/// A queue holds at most one item per pane, so this is generous.
const MAXIMUM_STORE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    queue: PersistedQueue,
}

/// Location of one session's queue file, or `None` when the platform has no
/// per-user data directory.
pub fn store_path_for_session(session_key: &Path) -> Option<PathBuf> {
    let digest = fingerprint_parts(&[session_key.as_os_str().as_encoded_bytes()]);
    directories::ProjectDirs::from("", "", "ilium").map(|dirs| {
        dirs.data_dir()
            .join("attention")
            .join(format!("queue-{digest:016x}.json"))
    })
}

/// Reads a stored queue. A missing, oversized, corrupt or foreign-version file
/// yields `None`: the instant layer rebuilds the queue anyway, so an unreadable
/// copy costs nothing worth surfacing.
pub fn load(path: &Path) -> Option<PersistedQueue> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAXIMUM_STORE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAXIMUM_STORE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAXIMUM_STORE_BYTES {
        return None;
    }
    let file: StoreFile = serde_json::from_slice(&bytes).ok()?;
    (file.version == STORE_VERSION).then_some(file.queue)
}

/// Writes the queue atomically: a temporary file in the same directory,
/// flushed, then renamed over the target, so a crash leaves either the old or
/// the new copy and never a torn one.
pub fn save(path: &Path, queue: &PersistedQueue) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("The attention queue file has no parent folder"))?;
    std::fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec(&StoreFile {
        version: STORE_VERSION,
        queue: queue.clone(),
    })
    .map_err(std::io::Error::other)?;
    if bytes.len() as u64 > MAXIMUM_STORE_BYTES {
        return Err(std::io::Error::other(
            "The attention queue file would exceed its size bound",
        ));
    }
    let temporary = parent.join(format!(".ilium-attention-{}.tmp", uuid::Uuid::new_v4()));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        ilium_platform::secure_fs::restrict_open_file_to_owner(&output)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        drop(output);
        ilium_platform::secure_fs::replace_file_durably(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{AttentionItem, AttentionKind, AttentionSource};
    use ilium_core::NodeId;

    fn sample_queue() -> PersistedQueue {
        PersistedQueue {
            items: vec![AttentionItem {
                pane_id: NodeId(7),
                project_id: Some(NodeId(1)),
                title: "build agent".into(),
                kind: AttentionKind::Question,
                reason: "Waiting for your answer or approval".into(),
                question: Some("Delete the folder?".into()),
                waiting_on: None,
                briefing: crate::attention::item::Briefing {
                    status_summary: Some("Claude waits to delete a folder.".into()),
                    why_here: Some("It needs a yes or no.".into()),
                    suggested_action: None,
                    from_llm: true,
                },
                priority: 90,
                source: AttentionSource::Instant,
                fingerprint: 0xfeed,
                generation: 3,
                created_ms: 1_000,
            }],
            last_looked_ms: vec![(NodeId(7), 500)],
            visited_as: vec![(
                NodeId(7),
                crate::attention::queue::VisitedAs {
                    fingerprint: 9,
                    kind: AttentionKind::Question,
                },
            )],
        }
    }

    #[test]
    fn a_saved_queue_loads_back_identically() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("attention").join("queue.json");
        save(&path, &sample_queue()).unwrap();
        assert_eq!(load(&path), Some(sample_queue()));
    }

    #[test]
    fn missing_corrupt_and_foreign_version_files_load_as_nothing() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(load(&folder.path().join("absent.json")), None);

        let corrupt = folder.path().join("corrupt.json");
        std::fs::write(&corrupt, b"{ not json").unwrap();
        assert_eq!(load(&corrupt), None);

        let foreign = folder.path().join("foreign.json");
        std::fs::write(
            &foreign,
            br#"{"version":99,"queue":{"items":[],"last_looked_ms":[]}}"#,
        )
        .unwrap();
        assert_eq!(load(&foreign), None);
    }

    #[test]
    fn an_oversized_file_is_ignored_without_reading_it() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("big.json");
        std::fs::write(&path, vec![b' '; (MAXIMUM_STORE_BYTES + 1) as usize]).unwrap();
        assert_eq!(load(&path), None);
    }

    #[test]
    fn different_sessions_get_different_files() {
        let first = store_path_for_session(Path::new("/run/user/1/ilium/a.sock"));
        let second = store_path_for_session(Path::new("/run/user/1/ilium/b.sock"));
        assert_ne!(first, second);
        assert_eq!(
            first,
            store_path_for_session(Path::new("/run/user/1/ilium/a.sock"))
        );
    }

    #[test]
    fn a_store_written_before_the_briefing_existed_still_loads() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("old.json");
        std::fs::write(
            &path,
            br#"{"version":1,"queue":{"items":[{"pane_id":7,"project_id":1,"title":"t",
            "kind":"question","reason":"r","question":null,"waiting_on":null,"priority":9,
            "source":"instant","fingerprint":5,"generation":1,"created_ms":2}],
            "last_looked_ms":[]}}"#,
        )
        .unwrap();
        let loaded = load(&path).expect("an old store loads");
        assert!(loaded.items[0].briefing.is_empty());
        assert!(!loaded.items[0].briefing.from_llm);
    }
}
