use crate::GitError;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

const MAXIMUM_STDOUT_BYTES: usize = 4 * 1024 * 1024;
const MAXIMUM_STDERR_BYTES: usize = 64 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(120);

#[cfg(not(target_os = "linux"))]
pub(crate) async fn run(
    directory: &Path,
    arguments: &[OsString],
    read_only: bool,
) -> Result<Vec<u8>, GitError> {
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // `-C` must select the repository. These inherited variables can silently
    // redirect Git to a different checkout, including on removal commands.
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
    ] {
        command.env_remove(variable);
    }
    if read_only {
        command.env("GIT_OPTIONAL_LOCKS", "0");
    } else {
        command.env_remove("GIT_OPTIONAL_LOCKS");
    }
    ilium_platform::process_control::prepare_process_tree(command.as_std_mut());
    ilium_platform::process_control::lower_background_child_priority(command.as_std_mut());

    let mut child = command.spawn().map_err(GitError::Spawn)?;
    let process_id = child.id().ok_or(GitError::MissingProcessId)?;
    let mut process_tree =
        match ilium_platform::process_control::ProcessTreeGuard::attach(process_id) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = child.kill().await;
                return Err(GitError::ProcessGuard(error));
            }
        };
    let stdout = child.stdout.take().ok_or(GitError::MissingPipe("stdout"))?;
    let stderr = child.stderr.take().ok_or(GitError::MissingPipe("stderr"))?;
    let timeout = if read_only {
        READ_TIMEOUT
    } else {
        MUTATION_TIMEOUT
    };
    let completed = tokio::time::timeout(timeout, async {
        tokio::join!(
            child.wait(),
            read_bounded(stdout, MAXIMUM_STDOUT_BYTES),
            read_bounded(stderr, MAXIMUM_STDERR_BYTES)
        )
    })
    .await;
    let (exit, stdout, stderr) = match completed {
        Ok(result) => result,
        Err(_) => {
            let _ = process_tree.terminate();
            let _ = child.kill().await;
            return Err(GitError::Timeout(timeout));
        }
    };
    // Hooks or filters can leave descendants behind after Git exits. They
    // remain inside this process group and must not escape the operation.
    process_tree.terminate().map_err(GitError::ProcessGuard)?;
    let exit = exit.map_err(GitError::Wait)?;
    let stdout = stdout.map_err(GitError::Output)?;
    let stderr = stderr.map_err(GitError::Output)?;
    if stdout.exceeded || stderr.exceeded {
        return Err(GitError::OutputTooLarge);
    }
    if !exit.success() {
        return Err(GitError::Command {
            exit: exit.code(),
            stderr: String::from_utf8_lossy(&stderr.bytes).trim().to_owned(),
        });
    }
    Ok(stdout.bytes)
}

/// The direct child remains waitable until the last process-group signal is sent.
#[cfg(target_os = "linux")]
pub(crate) async fn run(
    directory: &Path,
    arguments: &[OsString],
    read_only: bool,
) -> Result<Vec<u8>, GitError> {
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // `-C` must select the repository. These inherited variables can silently
    // redirect Git to a different checkout, including on removal commands.
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
    ] {
        command.env_remove(variable);
    }
    if read_only {
        command.env("GIT_OPTIONAL_LOCKS", "0");
    } else {
        command.env_remove("GIT_OPTIONAL_LOCKS");
    }
    ilium_platform::process_control::prepare_process_tree(command.as_std_mut());
    ilium_platform::process_control::lower_background_child_priority(command.as_std_mut());
    let mut child = command.spawn().map_err(GitError::Spawn)?;
    let process_id = child.id().ok_or(GitError::MissingProcessId)?;
    let process_tree = match ilium_platform::process_control::ProcessTreeGuard::attach(process_id) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            return Err(GitError::ProcessGuard(error));
        }
    };
    let stdout = child.stdout.take().ok_or(GitError::MissingPipe("stdout"))?;
    let stderr = child.stderr.take().ok_or(GitError::MissingPipe("stderr"))?;
    let deadline = if read_only {
        READ_TIMEOUT
    } else {
        MUTATION_TIMEOUT
    };
    let completed = tokio::time::timeout(deadline, async {
        tokio::join!(
            observe_direct_child(process_id),
            read_bounded(stdout, MAXIMUM_STDOUT_BYTES),
            read_bounded(stderr, MAXIMUM_STDERR_BYTES)
        )
    })
    .await;
    // Cleanup is not a dropped timeout future; it has its own bounded exit observations.
    let exit = finish_linux_command(process_tree, &mut child, process_id).await?;
    let (observed, stdout, stderr) = completed.map_err(|_| GitError::Timeout(deadline))?;
    observed.map_err(GitError::Wait)?;
    let stdout = stdout.map_err(GitError::Output)?;
    let stderr = stderr.map_err(GitError::Output)?;
    if stdout.exceeded || stderr.exceeded {
        return Err(GitError::OutputTooLarge);
    }
    if !exit.success() {
        return Err(GitError::Command {
            exit: exit.code(),
            stderr: String::from_utf8_lossy(&stderr.bytes).trim().to_owned(),
        });
    }
    Ok(stdout.bytes)
}
#[cfg(target_os = "linux")]
async fn observe_direct_child(process_id: u32) -> std::io::Result<()> {
    loop {
        if ilium_platform::process_control::child_exited_without_reaping(process_id)? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
#[cfg(target_os = "linux")]
async fn finish_linux_command(
    mut process_tree: ilium_platform::process_control::ProcessTreeGuard,
    child: &mut tokio::process::Child,
    process_id: u32,
) -> Result<std::process::ExitStatus, GitError> {
    let termination = process_tree.terminate();
    // Drop may retry a failed signal, but only while the direct child's identity is held.
    drop(process_tree);
    if termination.is_err() {
        let _ = child.start_kill();
    }
    let exit = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .map_err(|_| GitError::Timeout(Duration::from_secs(5)))?
        .map_err(GitError::Wait)?;
    termination.map_err(GitError::ProcessGuard)?;
    wait_for_group_exit(process_id, Duration::from_secs(5))
        .await
        .map_err(GitError::ProcessGuard)?;
    Ok(exit)
}

#[cfg(target_os = "linux")]
async fn wait_for_group_exit(process_id: u32, timeout: Duration) -> std::io::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if !ilium_platform::process_control::process_group_exists(process_id)? {
            return Ok(());
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "command process group did not disappear",
            ));
        }
        tokio::time::sleep((deadline - now).min(Duration::from_millis(10))).await;
    }
}

pub(crate) fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsStr::new).map(OsStr::to_owned).collect()
}

struct BoundedOutput {
    bytes: Vec<u8>,
    exceeded: bool,
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    maximum_bytes: usize,
) -> std::io::Result<BoundedOutput> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut exceeded = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(BoundedOutput { bytes, exceeded });
        }
        if bytes.len().saturating_add(read) > maximum_bytes {
            // Keep draining so Git cannot block on a full pipe. Retained
            // memory remains capped and the whole operation still times out.
            exceeded = true;
        }
        if !exceeded {
            bytes.extend_from_slice(&buffer[..read]);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::finish_linux_command;
    use std::time::Duration;

    #[test]
    fn linux_command_cleanup_does_not_wait_for_blocking_pool_capacity() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
            let (release_sender, release_receiver) = std::sync::mpsc::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                let _ = started_sender.send(());
                release_receiver
                    .recv()
                    .expect("release blocking-pool worker");
            });
            started_receiver
                .await
                .expect("blocking-pool worker started");

            let mut command = tokio::process::Command::new("sh");
            command.args(["-c", "sleep 0.05"]);
            ilium_platform::process_control::prepare_process_tree(command.as_std_mut());
            let mut child = command.spawn().expect("spawn guarded child");
            let process_id = child.id().expect("child process id");
            let process_tree =
                ilium_platform::process_control::ProcessTreeGuard::attach(process_id)
                    .expect("attach process-group guard");

            let result = tokio::time::timeout(
                Duration::from_millis(500),
                finish_linux_command(process_tree, &mut child, process_id),
            )
            .await;
            release_sender.send(()).expect("release blocker");
            blocker.await.expect("blocking-pool worker exited");
            assert!(
                result.is_ok(),
                "Git teardown waited for the saturated Tokio blocking pool"
            );
            assert!(result.unwrap().is_ok(), "guarded command cleanup succeeded");
        });
    }
}
