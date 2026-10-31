//! Bounded, process-bound reading of Claude's version-sensitive session record.

use crate::process_control::PtyProcessIdentity;
use std::path::Path;

/// Return a session only while the record and its listening socket belong to
/// the captured process. The caller must supply that process's provider home.
#[cfg(target_os = "linux")]
pub fn session_for_process(
    process_id: u32,
    process: &PtyProcessIdentity,
    config_directory: &Path,
    project_directory: &Path,
) -> Option<String> {
    use std::io::Read;
    if !process.is_for_process(process_id)
        || process.pidfs_inode().is_none()
        || !crate::process_control::captured_process_is_current(process).ok()?
    {
        return None;
    }
    let path = config_directory
        .join("sessions")
        .join(format!("{process_id}.json"));
    let mut file = crate::secure_fs::open_regular_file_without_atime(&path).ok()?;
    let generation = crate::file_identity::identity(&file).ok()?;
    let mut bytes = Vec::new();
    (&mut file).take(65537).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 65536 {
        return None;
    }
    let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if record.get("pid")?.as_u64()? != u64::from(process_id)
        || record.get("procStart")?.as_str()?.parse::<u64>().ok()? != process.birth_start_ticks()?
        || record.get("kind")?.as_str()? != "interactive"
        || record.get("entrypoint")?.as_str()? != "cli"
    {
        return None;
    }
    let cwd = Path::new(record.get("cwd")?.as_str()?);
    if std::fs::canonicalize(cwd).ok()? != std::fs::canonicalize(project_directory).ok()? {
        return None;
    }
    let session = record.get("sessionId")?.as_str()?;
    if session.len() != 36
        || !session.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
    {
        return None;
    }
    let socket = Path::new(record.get("messagingSocketPath")?.as_str()?);
    if !socket.is_absolute() || !process_owns_socket(process_id, socket) {
        return None;
    }
    let mut reopened = crate::secure_fs::open_regular_file_without_atime(&path).ok()?;
    if crate::file_identity::identity(&reopened).ok()? != generation {
        return None;
    }
    let mut after = Vec::new();
    (&mut reopened).take(65537).read_to_end(&mut after).ok()?;
    if bytes != after
        || !process_owns_socket(process_id, socket)
        || !crate::process_control::captured_process_is_current(process).ok()?
    {
        return None;
    }
    Some(session.to_string())
}

#[cfg(target_os = "linux")]
fn process_owns_socket(process_id: u32, socket: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(format!("/proc/{process_id}/net/unix")) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 16 * 1024 * 1024
    {
        return false;
    }
    let Ok(table) = std::str::from_utf8(&bytes) else {
        return false;
    };
    let Some(socket_path) = socket.to_str() else {
        return false;
    };
    let mut inodes = Vec::new();
    for line in table.lines().skip(1) {
        let mut fields = line.split_whitespace();
        let (Some(_), Some(_), Some(_), Some(_), Some(_), Some(_), Some(inode), Some(path)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        if path == socket_path && fields.next().is_none() {
            inodes.push(format!("socket:[{inode}]"));
        }
    }
    if inodes.len() != 1 {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(format!("/proc/{process_id}/fd")) else {
        return false;
    };
    let mut found = false;
    for (index, entry) in entries.enumerate() {
        if index >= 4096 {
            return false;
        }
        let Ok(entry) = entry else {
            return false;
        };
        if std::fs::read_link(entry.path())
            .ok()
            .is_some_and(|target| target.as_os_str() == inodes[0].as_str())
        {
            found = true;
        }
    }
    found
}

#[cfg(not(target_os = "linux"))]
pub fn session_for_process(
    _process_id: u32,
    _process: &PtyProcessIdentity,
    _config_directory: &Path,
    _project_directory: &Path,
) -> Option<String> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn session_record_is_bound_to_the_live_process_socket_and_changes_with_the_session() {
        let config = tempfile::tempdir().unwrap();
        let socket = config.path().join("session.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        let process_id = std::process::id();
        let process = crate::process_control::capture_pty_process(process_id).unwrap();
        let project = std::env::current_dir().unwrap();
        let sessions = config.path().join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        let record = sessions.join(format!("{process_id}.json"));
        let write_record = |session: &str, ticks: u64| {
            std::fs::write(
                &record,
                serde_json::json!({
                    "pid": process_id,
                    "procStart": ticks.to_string(),
                    "sessionId": session,
                    "cwd": project,
                    "version": "2.1.312",
                    "kind": "interactive",
                    "entrypoint": "cli",
                    "messagingSocketPath": socket,
                })
                .to_string(),
            )
            .unwrap();
        };
        let start = process.birth_start_ticks().unwrap();
        let first = "83915b3c-d052-4897-a1fa-97371407727f";
        let second = "e520e9d1-47ca-4ce2-9764-c97dcadda308";
        write_record(first, start);
        assert_eq!(
            session_for_process(process_id, &process, config.path(), &project).as_deref(),
            Some(first)
        );
        write_record(second, start);
        assert_eq!(
            session_for_process(process_id, &process, config.path(), &project).as_deref(),
            Some(second)
        );
        write_record(first, start.saturating_add(1));
        assert_eq!(
            session_for_process(process_id, &process, config.path(), &project),
            None
        );
        write_record(first, start);
        let mut altered: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
        altered["messagingSocketPath"] = serde_json::json!(config.path().join("not-owned.sock"));
        std::fs::write(&record, serde_json::to_vec(&altered).unwrap()).unwrap();
        assert_eq!(
            session_for_process(process_id, &process, config.path(), &project),
            None
        );
        altered["messagingSocketPath"] = serde_json::json!(socket);
        // Claude Code updates must not break the binding: the process, start
        // ticks, project and socket ownership already identify the record.
        altered["version"] = serde_json::json!("9.9.999");
        std::fs::write(&record, serde_json::to_vec(&altered).unwrap()).unwrap();
        assert_eq!(
            session_for_process(process_id, &process, config.path(), &project).as_deref(),
            Some(first)
        );
        altered["sessionId"] = serde_json::json!("not-a-session-id");
        std::fs::write(&record, serde_json::to_vec(&altered).unwrap()).unwrap();
        assert_eq!(
            session_for_process(process_id, &process, config.path(), &project),
            None
        );
    }
}
