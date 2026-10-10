//! End-to-end test of `ilium panes` and `ilium broadcast` across two
//! projects, each with its own detached `ilium-server`.
//!
//! Real processes throughout: the servers, their PTYs, agent detection and
//! the one-shot CLI commands. The agents are stand-ins: a shell script named
//! exactly `codex`, spawned by absolute path (never through `PATH`, so a real
//! Codex install can never be reached), which agent detection identifies by
//! process name. Its standard input is captured to a file, so the assertions
//! are on the bytes that reached each agent's terminal.
//!
//! Isolation: every process gets throwaway XDG, runtime and socket
//! directories, so the scan sees only this test's two sessions and never the
//! developer's real agents.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

const WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_NAME: &str = "default";

fn ilium_binary() -> String {
    std::env::var_os("ILIUM_PTY_SMOKE_BINARY")
        .map(|binary_path| binary_path.to_string_lossy().into_owned())
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_ilium").to_string())
}

/// The throwaway directories every spawned process shares.
struct IsolatedDirs {
    pairs: Vec<(&'static str, PathBuf)>,
    /// Short directory under `/tmp` so the session sockets fit `sockaddr_un`.
    _runtime_root: tempfile::TempDir,
}

impl IsolatedDirs {
    fn under(root: &Path) -> Self {
        let runtime_root = tempfile::Builder::new()
            .prefix("ib")
            .tempdir_in("/tmp")
            .expect("short runtime directory");
        let runtime_dir = runtime_root.path().to_path_buf();
        let config_home = root.join("config");
        let pairs = vec![
            ("XDG_DATA_HOME", root.join("data")),
            ("XDG_CONFIG_HOME", config_home.clone()),
            ("XDG_RUNTIME_DIR", runtime_dir.clone()),
            (
                ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                runtime_dir.join("ilium"),
            ),
            (
                ilium_platform::paths::CONFIG_DIR_ENV,
                config_home.join("ilium"),
            ),
            (
                ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV,
                root.join("debug-logs"),
            ),
            (ilium_client::AGENT_SETUP_HOME_ENV, root.join("agent-home")),
        ];
        // The runtime and socket directories are left to the processes: the
        // socket directory must be created owner-only, as the session code
        // does itself.
        for (key, directory) in &pairs {
            let is_runtime =
                *key == "XDG_RUNTIME_DIR" || *key == ilium_platform::runtime_dir::SOCKET_DIR_ENV;
            if !is_runtime {
                std::fs::create_dir_all(directory).expect("create isolated directory");
            }
        }
        Self {
            pairs,
            _runtime_root: runtime_root,
        }
    }
}

struct KillSessionOnDrop<'a> {
    dirs: &'a IsolatedDirs,
    projects: Vec<PathBuf>,
}

impl Drop for KillSessionOnDrop<'_> {
    fn drop(&mut self) {
        for project in &self.projects {
            let mut command = std::process::Command::new(ilium_binary());
            command
                .args(["kill-session", SESSION_NAME])
                .current_dir(project)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            for (key, value) in &self.dirs.pairs {
                command.env(key, value);
            }
            if let Ok(mut child) = command.spawn() {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                while std::time::Instant::now() < deadline {
                    if matches!(child.try_wait(), Ok(Some(_))) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

impl CommandOutput {
    /// The JSONL records, each parsed and required to carry `type`.
    fn records(&self) -> Vec<Value> {
        self.stdout
            .lines()
            .map(|line| {
                let record: Value =
                    serde_json::from_str(line).unwrap_or_else(|error| panic!("{line:?}: {error}"));
                assert!(record["type"].is_string(), "record without a type: {line}");
                record
            })
            .collect()
    }

    fn of_type(&self, kind: &str) -> Vec<Value> {
        self.records()
            .into_iter()
            .filter(|record| record["type"] == kind)
            .collect()
    }

    fn summary(&self) -> Value {
        let summary = self.records().pop().expect("a summary record");
        assert_eq!(summary["type"], "summary", "{}", self.stdout);
        summary
    }
}

async fn run_ilium(
    dirs: &IsolatedDirs,
    cwd: &Path,
    args: &[&str],
    stdin: Option<&str>,
) -> CommandOutput {
    use tokio::io::AsyncWriteExt;

    let mut command = tokio::process::Command::new(ilium_binary());
    command
        .args(args)
        .current_dir(cwd)
        .stdin(if stdin.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in &dirs.pairs {
        command.env(key, value);
    }
    // A pane environment must never leak in from the developer's own Ilium
    // session: it would mark one of its panes as the caller.
    for variable in [
        ilium_ipc::pane_env::PANE_ID,
        ilium_ipc::pane_env::SESSION_NAME,
        ilium_ipc::pane_env::SESSION_SOCKET,
    ] {
        command.env_remove(variable);
    }
    let mut child = command.spawn().expect("spawn ilium");
    if let Some(input) = stdin {
        let mut pipe = child.stdin.take().expect("piped stdin");
        pipe.write_all(input.as_bytes()).await.expect("write stdin");
    }
    let output = tokio::time::timeout(Duration::from_secs(90), child.wait_with_output())
        .await
        .unwrap_or_else(|_| panic!("`ilium {args:?}` did not finish in time"))
        .expect("wait for ilium");
    CommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A stand-in agent named exactly `codex` that appends its terminal input to
/// the file named by its first argument. It deliberately does not `exec`
/// `cat`: the script's own process, named `codex`, must stay alive for agent
/// detection to find.
fn write_fake_codex(bin_dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(bin_dir).expect("bin dir");
    let path = bin_dir.join("codex");
    std::fs::write(&path, "#!/bin/sh\ncat >> \"$1\"\n").expect("write fake codex");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("make fake codex executable");
    path
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

async fn wait_for_file(path: &Path, needle: &str) -> bool {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if read(path).contains(needle) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn broadcast_reaches_selected_agents_across_projects_and_never_shells() {
    let temp_root = tempfile::tempdir().expect("tempdir");
    let dirs = IsolatedDirs::under(temp_root.path());
    let alpha = temp_root.path().join("alpha");
    let beta = temp_root.path().join("beta");
    // Each project is its own Git repository, so the server's launch
    // admission never walks up into whatever (possibly broken) `.git`
    // happens to sit above the temporary directory on the host.
    for project in [&alpha, &beta] {
        std::fs::create_dir_all(project).expect("project dir");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(project)
            .status()
            .expect("run git init");
        assert!(initialized.success(), "git init failed in {project:?}");
    }
    let _cleanup = KillSessionOnDrop {
        dirs: &dirs,
        projects: vec![alpha.clone(), beta.clone()],
    };
    let fake_codex = write_fake_codex(&temp_root.path().join("bin"));
    let alpha_received = temp_root.path().join("alpha-agent.txt");
    let beta_received = temp_root.path().join("beta-agent.txt");
    let shell_received = temp_root.path().join("alpha-shell.txt");

    // No session runs yet: a broadcast reaches nobody and says so.
    let nobody = run_ilium(&dirs, &alpha, &["broadcast", "hello"], None).await;
    assert!(!nobody.success, "{}", nobody.stdout);
    assert_eq!(nobody.summary()["recipients"], 0);

    for (project, received) in [(&alpha, &alpha_received), (&beta, &beta_received)] {
        let created = run_ilium(
            &dirs,
            project,
            &[
                "new-pane",
                "--",
                &fake_codex.to_string_lossy(),
                &received.to_string_lossy(),
            ],
            None,
        )
        .await;
        assert!(
            created.success,
            "new-pane failed: {} {}",
            created.stdout, created.stderr
        );
    }
    let shell_script = format!("cat >> {}", shell_received.display());
    let shell = run_ilium(
        &dirs,
        &alpha,
        &["new-pane", "--", "sh", "-c", &shell_script],
        None,
    )
    .await;
    assert!(
        shell.success,
        "new-pane failed: {} {}",
        shell.stdout, shell.stderr
    );

    // Agent detection runs on the servers' own schedule; wait until both
    // stand-ins are reported as Codex agents.
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let listed = run_ilium(&dirs, &alpha, &["panes", "--agent", "codex"], None).await;
        if listed.of_type("pane").len() == 2 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "agents never detected: {}",
            run_ilium(&dirs, &alpha, &["panes"], None).await.stdout
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    // The listing covers both sessions and every pane kind.
    let all = run_ilium(&dirs, &alpha, &["panes"], None).await;
    assert!(all.success, "{}", all.stdout);
    assert_eq!(all.summary()["sessions"], 2, "{}", all.stdout);
    assert_eq!(all.of_type("pane").len(), 3, "{}", all.stdout);
    let shells = run_ilium(&dirs, &alpha, &["panes", "--kind", "shell", "--here"], None).await;
    let shell_records = shells.of_type("pane");
    assert_eq!(shell_records.len(), 1, "{}", shells.stdout);
    assert!(shell_records[0]["project"]
        .as_str()
        .unwrap()
        .ends_with("alpha"));

    // A dry run plans both agents and sends nothing.
    let planned = run_ilium(
        &dirs,
        &alpha,
        &["broadcast", "--dry-run", "DRY-RUN-MARK"],
        None,
    )
    .await;
    assert!(planned.success, "{}", planned.stdout);
    let plan = planned.of_type("result");
    assert_eq!(plan.len(), 2, "{}", planned.stdout);
    assert!(plan.iter().all(|record| record["outcome"] == "would-send"));

    // Project selection by folder name reaches only alpha's agent.
    let first = run_ilium(
        &dirs,
        &beta,
        &["broadcast", "--project", "alpha", "FIRST", "MARK"],
        None,
    )
    .await;
    assert!(first.success, "{} {}", first.stdout, first.stderr);
    let first_results = first.of_type("result");
    assert_eq!(first_results.len(), 1, "{}", first.stdout);
    assert_eq!(first_results[0]["outcome"], "delivered", "{}", first.stdout);
    assert!(
        wait_for_file(&alpha_received, "FIRST MARK").await,
        "alpha: {:?}",
        read(&alpha_received)
    );

    // An inverted regex over project paths reaches only beta's agent.
    let second = run_ilium(
        &dirs,
        &alpha,
        &[
            "broadcast",
            "--regex",
            "alpha$",
            "--field",
            "project",
            "--invert",
            "SECOND-MARK",
        ],
        None,
    )
    .await;
    assert!(second.success, "{} {}", second.stdout, second.stderr);
    assert_eq!(second.summary()["delivered"], 1, "{}", second.stdout);
    assert!(
        wait_for_file(&beta_received, "SECOND-MARK").await,
        "beta: {:?}",
        read(&beta_received)
    );

    // A selection that matches nothing fails without sending.
    let unmatched = run_ilium(
        &dirs,
        &alpha,
        &["broadcast", "-m", "no-such-pane", "LOST-MARK"],
        None,
    )
    .await;
    assert!(!unmatched.success);
    assert_eq!(unmatched.summary()["recipients"], 0);

    // Standard input reaches every agent.
    let third = run_ilium(&dirs, &alpha, &["broadcast", "-"], Some("THIRD-MARK\n")).await;
    assert!(third.success, "{} {}", third.stdout, third.stderr);
    assert_eq!(third.summary()["delivered"], 2, "{}", third.stdout);
    assert!(wait_for_file(&alpha_received, "THIRD-MARK").await);
    assert!(wait_for_file(&beta_received, "THIRD-MARK").await);

    let alpha_text = read(&alpha_received);
    let beta_text = read(&beta_received);
    assert!(!alpha_text.contains("SECOND-MARK"), "alpha: {alpha_text:?}");
    assert!(!beta_text.contains("FIRST MARK"), "beta: {beta_text:?}");
    for text in [&alpha_text, &beta_text] {
        assert!(
            !text.contains("DRY-RUN-MARK") && !text.contains("LOST-MARK"),
            "{text:?}"
        );
    }
    // Shells are never typed into.
    assert!(
        read(&shell_received).is_empty(),
        "shell: {:?}",
        read(&shell_received)
    );
}
