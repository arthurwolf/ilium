//! One explicitly supplied, user-owned post-create shell command.
//!
//! This is deliberately not a Git runner and not a provider PTY. The caller
//! must retain the checkout and copied files after calling `run`, including
//! when spawning or finalizing the command fails. Output is drained without
//! being retained, logged, or included in IPC errors: setup may print secrets.

use std::path::Path;
use std::time::Duration;

const MAX_COMMAND_BYTES: usize = 8 * 1024;

pub(super) fn validate(command: Option<&str>) -> Result<Option<String>, String> {
    let Some(command) = command else {
        return Ok(None);
    };
    if command.len() > MAX_COMMAND_BYTES || command.contains('\0') {
        return Err("setup command must be at most 8192 bytes and contain no NUL".into());
    }
    if command.trim().is_empty() {
        return Ok(None);
    }
    if !cfg!(target_os = "linux") {
        return Err("configured worktree setup currently requires Linux non-reaping child observation and process-group exit verification".into());
    }
    Ok(Some(command.to_owned()))
}

#[derive(Clone, Copy)]
struct Limits {
    execution: Duration,
    cleanup: Duration,
    stdout_bytes: usize,
    stderr_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            execution: Duration::from_secs(120),
            cleanup: Duration::from_secs(5),
            stdout_bytes: 1024 * 1024,
            stderr_bytes: 256 * 1024,
        }
    }
}

pub(super) async fn run<F>(
    command: &str,
    cwd: &Path,
    worktree_root: &Path,
    cancelled: F,
) -> Result<(), String>
where
    F: Fn() -> bool + Send + Sync,
{
    run_with_limits(command, cwd, worktree_root, cancelled, Limits::default()).await
}

#[cfg(not(target_os = "linux"))]
async fn run_with_limits<F>(
    _command: &str,
    _cwd: &Path,
    _worktree_root: &Path,
    _cancelled: F,
    limits: Limits,
) -> Result<(), String>
where
    F: Fn() -> bool + Send + Sync,
{
    // Keep all limit fields exercised on this build without pretending that
    // the Linux non-reaping child contract exists on another platform.
    let _ = (
        limits.execution,
        limits.cleanup,
        limits.stdout_bytes,
        limits.stderr_bytes,
    );
    Err("configured worktree setup is unavailable on this platform".into())
}

#[cfg(target_os = "linux")]
async fn run_with_limits<F>(
    command: &str,
    cwd: &Path,
    worktree_root: &Path,
    cancelled: F,
    limits: Limits,
) -> Result<(), String>
where
    F: Fn() -> bool + Send + Sync,
{
    use ilium_platform::process_control::{
        lower_background_child_priority, prepare_process_tree, ProcessTreeGuard,
    };
    use std::process::Stdio;

    validate(Some(command))?;
    if cancelled() {
        return Err("setup cancelled before spawn".into());
    }
    let root = ilium_platform::paths::canonicalize(worktree_root)
        .map_err(|error| format!("setup worktree is unavailable: {error}"))?;
    let directory = ilium_platform::paths::canonicalize(cwd)
        .map_err(|error| format!("setup launch directory is unavailable: {error}"))?;
    if root != worktree_root
        || directory != cwd
        || !directory.starts_with(&root)
        || !directory.is_dir()
    {
        return Err(
            "setup requires an exact canonical launch directory inside its worktree".into(),
        );
    }

    let mut child_command = tokio::process::Command::new("/bin/sh");
    child_command
        .arg("-c")
        .arg(command)
        .current_dir(&directory)
        .env("PWD", &directory)
        .env("ILIUM_WORKTREE", &root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
        "GIT_OPTIONAL_LOCKS",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "ILIUM_BRANCH",
        "ILIUM_PANE_ID",
        "OLDPWD",
        "ENV",
        "BASH_ENV",
    ] {
        child_command.env_remove(name);
    }
    prepare_process_tree(child_command.as_std_mut());
    lower_background_child_priority(child_command.as_std_mut());
    let mut child = child_command
        .spawn()
        .map_err(|error| format!("setup shell could not start: {error}"))?;
    let Some(process_id) = child.id() else {
        let _ = tokio::time::timeout(limits.cleanup, child.kill()).await;
        return Err("setup shell has no process ID; process custody is unverified".into());
    };
    let mut guard = match ProcessTreeGuard::attach(process_id) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = tokio::time::timeout(limits.cleanup, child.kill()).await;
            return Err(format!(
                "setup process custody could not be established: {error}"
            ));
        }
    };
    let pipes = (child.stdout.take(), child.stderr.take());
    let (Some(stdout), Some(stderr)) = pipes else {
        let cleanup = finalize(&mut child, &mut guard, process_id, limits.cleanup).await;
        return Err(with_cleanup(
            "setup output pipes are unavailable".into(),
            cleanup,
        ));
    };

    // Cancellation never drops the supervisor itself. It drops only the
    // wait/read futures, then explicitly stops and reaps the guarded command.
    let outcome = tokio::select! {
        biased;
        _ = cancellation(&cancelled) => Err("setup cancelled".to_string()),
        result = tokio::time::timeout(limits.execution, async {
            tokio::join!(
                observe_child_exit(process_id),
                drain(stdout, limits.stdout_bytes),
                drain(stderr, limits.stderr_bytes),
            )
        }) => {
            match result {
                Err(_) => Err(format!("setup exceeded its {} second execution limit", limits.execution.as_secs_f64())),
                Ok((status, stdout, stderr)) => {
                    match (status, stdout, stderr) {
                        (Err(error), _, _) => Err(format!("setup wait failed: {error}")),
                        (_, Err(error), _) | (_, _, Err(error)) => Err(format!("setup output read failed: {error}")),
                        (Ok(()), Ok(stdout_exceeded), Ok(stderr_exceeded)) => {
                            if stdout_exceeded || stderr_exceeded {
                                Err("setup exceeded its output limit; output was not retained".into())
                            } else {
                                Ok(())
                            }
                        }
                    }
                }
            }
        }
    };
    let cleanup = finalize(&mut child, &mut guard, process_id, limits.cleanup).await;
    match outcome {
        Err(reason) => Err(with_cleanup(reason, cleanup)),
        Ok(()) => {
            let status = cleanup?;
            if !status.success() {
                return Err(format!(
                    "setup exited with {status}; output was not retained"
                ));
            }
            if cancelled() {
                Err("setup completed but creation was cancelled before pane commit".into())
            } else {
                Ok(())
            }
        }
    }
}

// Do not use Child::wait/try_wait in this phase: reaping the group leader
// before the final group signal opens a numeric-PGID reuse window. Linux's
// WNOWAIT observation keeps the direct child's identity reserved until the
// supervisor has signalled the group and explicitly reaps it in finalize.
#[cfg(target_os = "linux")]
async fn observe_child_exit(process_id: u32) -> std::io::Result<()> {
    loop {
        if ilium_platform::process_control::workspace_setup_child_exited_without_reaping(
            process_id,
        )? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(target_os = "linux")]
async fn cancellation<F: Fn() -> bool>(cancelled: &F) {
    loop {
        if cancelled() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(target_os = "linux")]
async fn drain<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    maximum: usize,
) -> std::io::Result<bool> {
    use tokio::io::AsyncReadExt;
    let mut buffer = [0_u8; 8192];
    let mut count = 0_usize;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(count > maximum);
        }
        count = count.saturating_add(read);
    }
}

#[cfg(target_os = "linux")]
async fn finalize(
    child: &mut tokio::process::Child,
    guard: &mut ilium_platform::process_control::ProcessTreeGuard,
    process_id: u32,
    timeout: Duration,
) -> Result<std::process::ExitStatus, String> {
    let mut problems = Vec::new();
    // Signal the still-owned group synchronously, before reaping the leader.
    // The subsequent worker only observes exit; it never signals a reused ID.
    if let Err(error) = guard.terminate() {
        // Keep the direct child unreaped while the still-armed guard takes
        // its Drop fallback. Reaping first would invalidate numeric custody.
        return Err(format!(
            "cannot terminate setup process group; custody is unverified: {error}"
        ));
    }
    // The leader has deliberately not been reaped. On a successful command
    // its original exit status is retained even though the group is stopped.
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => Some(status),
        Ok(Err(error)) => {
            problems.push(format!("cannot reap setup shell: {error}"));
            None
        }
        Err(_) => {
            problems.push("setup shell reap timed out".into());
            let _ = child.start_kill();
            None
        }
    };
    let observed = tokio::task::spawn_blocking(move || {
        ilium_platform::process_control::wait_for_process_group_exit(process_id, timeout)
    });
    match tokio::time::timeout(timeout + Duration::from_millis(100), observed).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(error))) => problems.push(format!("setup group exit is unverified: {error}")),
        Ok(Err(error)) => problems.push(format!("setup exit probe failed: {error}")),
        Err(_) => problems.push("setup exit probe timed out".into()),
    }
    if problems.is_empty() {
        status.ok_or_else(|| "setup exit status is unavailable".into())
    } else {
        Err(problems.join("; "))
    }
}

#[cfg(target_os = "linux")]
fn with_cleanup(reason: String, cleanup: Result<std::process::ExitStatus, String>) -> String {
    match cleanup {
        Ok(_) => reason,
        Err(error) => format!("{reason}; {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_is_bounded_and_preserves_the_exact_command() {
        assert_eq!(validate(None).unwrap(), None);
        assert_eq!(validate(Some(" \n\t")).unwrap(), None);
        assert!(validate(Some("a\0b")).is_err());
        assert!(validate(Some(&"x".repeat(MAX_COMMAND_BYTES + 1))).is_err());
        #[cfg(target_os = "linux")]
        assert_eq!(
            validate(Some(" printf ok ")).unwrap(),
            Some(" printf ok ".into())
        );
        #[cfg(not(target_os = "linux"))]
        assert!(validate(Some("echo ok")).is_err());
    }

    #[cfg(target_os = "linux")]
    fn limits() -> Limits {
        Limits {
            execution: Duration::from_secs(5),
            cleanup: Duration::from_secs(2),
            stdout_bytes: 1024,
            stderr_bytes: 1024,
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn setup_uses_the_project_subdirectory_and_worktree_environment() {
        let temp = tempfile::tempdir().unwrap();
        let root = ilium_platform::paths::canonicalize(temp.path()).unwrap();
        let cwd = root.join("sub");
        std::fs::create_dir(&cwd).unwrap();
        std::fs::write(cwd.join("copied"), "input").unwrap();
        run_with_limits(
            "test \"$PWD\" = \"$ILIUM_WORKTREE/sub\" && test -r copied && printf ready > result",
            &cwd,
            &root,
            || false,
            limits(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(cwd.join("result")).unwrap(), b"ready");
        assert!(!root.join("result").exists());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn nonzero_setup_preserves_output_files_without_returning_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let root = ilium_platform::paths::canonicalize(temp.path()).unwrap();
        std::fs::write(root.join("copied"), "before").unwrap();
        let error = run_with_limits(
            "printf after > copied; printf token-value >&2; printf user-data > ignored; exit 7",
            &root,
            &root,
            || false,
            limits(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("7"), "{error}");
        assert!(!error.contains("token-value"), "{error}");
        assert_eq!(std::fs::read(root.join("copied")).unwrap(), b"after");
        assert_eq!(std::fs::read(root.join("ignored")).unwrap(), b"user-data");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn timeout_is_bounded_and_retains_the_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = ilium_platform::paths::canonicalize(temp.path()).unwrap();
        let mut bounds = limits();
        bounds.execution = Duration::from_millis(100);
        let error = run_with_limits(
            "printf started > result; exec sleep 30",
            &root,
            &root,
            || false,
            bounds,
        )
        .await
        .unwrap_err();
        assert!(error.contains("execution limit"), "{error}");
        assert!(root.is_dir());
        assert_eq!(std::fs::read(root.join("result")).unwrap(), b"started");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn cancellation_stops_the_supervised_command_and_retains_its_files() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let temp = tempfile::tempdir().unwrap();
        let root = ilium_platform::paths::canonicalize(temp.path()).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let writer = Arc::clone(&flag);
        let ready = root.join("ready");
        let ready_for_task = ready.clone();
        let cancellation = tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
            while !ready_for_task.exists() && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            writer.store(true, Ordering::Release);
        });
        let error = run_with_limits(
            "printf yes > ready; exec sleep 30",
            &root,
            &root,
            || flag.load(Ordering::Acquire),
            limits(),
        )
        .await
        .unwrap_err();
        cancellation.await.unwrap();
        assert!(error.contains("cancelled"), "{error}");
        assert_eq!(std::fs::read(ready).unwrap(), b"yes");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn excess_output_is_drained_without_becoming_an_ipc_payload() {
        let temp = tempfile::tempdir().unwrap();
        let root = ilium_platform::paths::canonicalize(temp.path()).unwrap();
        let mut bounds = limits();
        bounds.stdout_bytes = 16;
        let error = run_with_limits("printf '%080d' 0", &root, &root, || false, bounds)
            .await
            .unwrap_err();
        assert!(error.contains("output limit"), "{error}");
        assert!(!error.contains("0000000000"), "{error}");
    }
}
