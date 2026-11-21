//! Finds the rollout files of Codex sub-agent threads spawned by a session.
//!
//! Codex stores every spawned thread as its own `rollout-<time>-<id>.jsonl`
//! next to (or after) its parent, and names the parent in the first record:
//! `source.subagent.thread_spawn.parent_thread_id`. A session's real spend is
//! its own file plus every transitive child, so statistics must follow them.
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Upper bound on rollout files inspected for one session.
const MAX_CANDIDATES: usize = 50_000;
/// The first record carries the parent link; never read more than this.
const FIRST_RECORD_BYTES: usize = 128 * 1024;
/// Remembered first-record results; a rollout's first record never changes.
const MEMO_LIMIT: usize = 100_000;

/// `None` = inspected, not a spawned thread. Keyed by path.
fn memo() -> &'static Mutex<HashMap<PathBuf, Option<String>>> {
    static MEMO: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Session id embedded in `rollout-<time>-<uuid>.jsonl`.
fn rollout_session_id(path: &Path) -> Option<&str> {
    let stem = path.file_name()?.to_str()?.strip_suffix(".jsonl")?;
    stem.strip_prefix("rollout-")?;
    let id = stem.get(stem.len().checked_sub(36)?..)?;
    (id.len() == 36
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-'))
    .then_some(id)
}

fn parent_of(path: &Path) -> Option<String> {
    if let Some(known) = memo().lock().ok().and_then(|map| map.get(path).cloned()) {
        return known;
    }
    let mut file = ilium_platform::secure_fs::open_regular_file(path).ok()?;
    let mut buffer = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    while buffer.len() < FIRST_RECORD_BYTES && !buffer.contains(&b'\n') {
        let read = file.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    if !buffer.contains(&b'\n') && buffer.len() < FIRST_RECORD_BYTES {
        // The first record is still being written: decide on a later pass.
        return None;
    }
    let first = buffer.split(|byte| *byte == b'\n').next().unwrap_or(&[]);
    let parent = parent_thread_id(first);
    if let Ok(mut map) = memo().lock() {
        if map.len() >= MEMO_LIMIT {
            map.clear();
        }
        map.insert(path.to_path_buf(), parent.clone());
    }
    parent
}

/// True when `path` is a spawned sub-agent thread, which belongs to its
/// parent's statistics and is never a top-level session of its own.
pub(crate) fn is_sub_agent_thread(path: &Path) -> bool {
    parent_of(path).is_some()
}

fn parent_thread_id(first_record: &[u8]) -> Option<String> {
    static PATTERN: OnceLock<regex::bytes::Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::bytes::Regex::new(
            r#""thread_spawn"\s*:\s*\{[^}]*?"parent_thread_id"\s*:\s*"([0-9A-Fa-f-]{36})""#,
        )
        .expect("static pattern is valid")
    });
    let captures = pattern.captures(first_record)?;
    String::from_utf8(captures.get(1)?.as_bytes().to_vec()).ok()
}

/// Rollout files of every sub-agent thread descending from `main`, found by
/// scanning the same sessions tree from the parent's day onwards.
pub(crate) fn children_of(main: &Path) -> Vec<PathBuf> {
    let Some(main_id) = rollout_session_id(main) else {
        return Vec::new();
    };
    // sessions/YYYY/MM/DD/rollout-*.jsonl
    let Some(day_dir) = main.parent() else {
        return Vec::new();
    };
    let Some(root) = day_dir.ancestors().nth(3) else {
        return Vec::new();
    };
    let main_day = day_key(day_dir);
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
    let mut inspected = 0usize;
    for year in sorted_dirs(root) {
        for month in sorted_dirs(&year) {
            for day in sorted_dirs(&month) {
                if main_day.is_some() && day_key(&day) < main_day {
                    continue;
                }
                let Ok(entries) = std::fs::read_dir(&day) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path == main || rollout_session_id(&path).is_none() {
                        continue;
                    }
                    inspected += 1;
                    if inspected > MAX_CANDIDATES {
                        return resolve(main_id, candidates);
                    }
                    if let Some(parent) = parent_of(&path) {
                        candidates.push((path, parent));
                    }
                }
            }
        }
    }
    resolve(main_id, candidates)
}

fn day_key(day_dir: &Path) -> Option<(String, String, String)> {
    let day = day_dir.file_name()?.to_str()?.to_owned();
    let month_dir = day_dir.parent()?;
    let month = month_dir.file_name()?.to_str()?.to_owned();
    let year = month_dir.parent()?.file_name()?.to_str()?.to_owned();
    Some((year, month, day))
}

fn sorted_dirs(directory: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| {
            // Date folders only (YYYY, MM, DD); skips unrelated directories.
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs
}

/// Transitive closure of "spawned by": a child of a child still belongs.
fn resolve(main_id: &str, candidates: Vec<(PathBuf, String)>) -> Vec<PathBuf> {
    let mut family: HashSet<String> = HashSet::from([main_id.to_ascii_lowercase()]);
    let mut taken = vec![false; candidates.len()];
    let mut found = Vec::new();
    loop {
        let before = found.len();
        for (index, (path, parent)) in candidates.iter().enumerate() {
            if taken[index] || !family.contains(&parent.to_ascii_lowercase()) {
                continue;
            }
            taken[index] = true;
            if let Some(id) = rollout_session_id(path) {
                family.insert(id.to_ascii_lowercase());
            }
            found.push(path.clone());
        }
        if found.len() == before {
            break;
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: &str = "01a1c2cd-0199-7f22-8a9c-c67c741d0ad0";
    const CHILD: &str = "01a1c2f8-90da-7e43-b09d-9815212356ff";
    const GRANDCHILD: &str = "01a1c2f9-0000-7e43-b09d-9815212356ff";
    const OTHER: &str = "01a1c2fa-0000-7e43-b09d-9815212356ff";

    fn meta(parent: Option<&str>) -> String {
        match parent {
            Some(parent) => format!(
                r#"{{"type":"session_meta","payload":{{"source":{{"subagent":{{"thread_spawn":{{"parent_thread_id":"{parent}","depth":1}}}}}}}}}}"#
            ),
            None => r#"{"type":"session_meta","payload":{"source":"cli"}}"#.to_owned(),
        }
    }

    fn write(root: &Path, day: &str, id: &str, parent: Option<&str>) -> PathBuf {
        let directory = root.join("2026/11").join(day);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("rollout-2026-11-{day}T10-00-00-{id}.jsonl"));
        std::fs::write(&path, format!("{}\n{{\"x\":1}}\n", meta(parent))).unwrap();
        path
    }

    #[test]
    fn follows_children_and_grandchildren_but_not_unrelated_threads() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("sessions");
        let main = write(&root, "10", MAIN, None);
        let child = write(&root, "10", CHILD, Some(MAIN));
        let grandchild = write(&root, "11", GRANDCHILD, Some(CHILD));
        let _unrelated = write(
            &root,
            "11",
            OTHER,
            Some("ffffffff-0000-0000-0000-000000000000"),
        );
        let found = children_of(&main);
        assert_eq!(found, vec![child, grandchild]);
    }

    #[test]
    fn ignores_files_from_days_before_the_parent_and_non_rollouts() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("sessions");
        let main = write(&root, "10", MAIN, None);
        let _earlier = write(&root, "09", CHILD, Some(MAIN));
        std::fs::write(root.join("2026/11/10/notes.jsonl"), "{}\n").unwrap();
        assert!(children_of(&main).is_empty());
        assert!(children_of(Path::new("/nonexistent/x.jsonl")).is_empty());
    }

    #[test]
    fn parent_link_is_read_only_from_the_spawn_record() {
        assert_eq!(
            parent_thread_id(meta(Some(MAIN)).as_bytes()).as_deref(),
            Some(MAIN)
        );
        assert_eq!(parent_thread_id(meta(None).as_bytes()), None);
    }
}
