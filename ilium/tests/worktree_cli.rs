//! Real `ilium new-pane --worktree` against one isolated server and Git repo.
//! The only `codex` on the child server's PATH is a test script; this test
//! never launches an installed agent or connects to a user session.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ilium_client::connection::Connection;
use ilium_ipc::ServerEvent;
use serde_json::Value;

const SESSION_NAME: &str = "default";
const BRANCH: &str = "agent/cli-test";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .expect("Git is installed for this integration test");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Git fixture output is UTF-8")
        .trim()
        .to_owned()
}

struct IsolatedSession {
    root: tempfile::TempDir,
    project: PathBuf,
    socket: PathBuf,
    active_log: PathBuf,
    launch_record: PathBuf,
    server: Child,
}

impl Drop for IsolatedSession {
    fn drop(&mut self) {
        if self.server.try_wait().ok().flatten().is_none() {
            let _ = self.server.kill();
            let _ = self.server.wait();
        }
    }
}

impl IsolatedSession {
    async fn start() -> Self {
        let root = tempfile::Builder::new()
            .prefix("iwc")
            .tempdir_in("/tmp")
            .expect("short isolated test root");
        let project = root.path().join("project");
        let bin = root.path().join("bin");
        let home = root.path().join("home");
        let data = root.path().join("data");
        let config = root.path().join("config");
        let agent_setup = root.path().join("agent-setup");
        let runtime = root.path().join("runtime");
        let socket_dir = runtime.join("ilium");
        let log_root = root.path().join("logs");
        for directory in [
            &project,
            &bin,
            &home,
            &data,
            &config,
            &agent_setup,
            &socket_dir,
            &log_root,
        ] {
            std::fs::create_dir_all(directory).expect("create isolated test directory");
        }
        std::fs::write(
            config.join("config.toml"),
            "[notifications]\nenabled = false\n[http_api]\nport = 0\n",
        )
        .expect("write isolated server config");

        git(&project, &["init", "-q", "-b", "main"]);
        git(&project, &["config", "user.name", "Ilium Test"]);
        git(&project, &["config", "user.email", "ilium@example.invalid"]);
        std::fs::write(project.join("README.md"), "isolated Git fixture\n").unwrap();
        std::fs::write(project.join(".gitignore"), ".local.env\n").unwrap();
        std::fs::write(project.join(".worktreeinclude"), ".local.env\n").unwrap();
        std::fs::write(project.join(".local.env"), "only in this test\n").unwrap();
        git(
            &project,
            &["add", "README.md", ".gitignore", ".worktreeinclude"],
        );
        git(&project, &["commit", "-q", "-m", "initial"]);

        let launch_record = root.path().join("fake-agent-launch");
        let fake_codex = bin.join("codex");
        let script = format!(
            "#!/bin/sh\ntest -f setup.done || exit 4\nprintf '%s|%s\\n' \"$PWD\" \"$ILIUM_WORKTREE\" > '{}'\n",
            launch_record.display()
        );
        std::fs::write(&fake_codex, script).expect("write fake Codex");
        let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&fake_codex, permissions).unwrap();

        let socket = ilium::session::socket_path_in(&socket_dir, &project, SESSION_NAME);
        let socket_key = ilium::session::socket_key_in(&socket_dir, &project, SESSION_NAME);
        let log_directory = log_root.join(socket_key);
        std::fs::create_dir_all(&log_directory).expect("create isolated log directory");
        let log = log_directory.join("server.log");
        let active_log = log_directory.join(".active-log-path");
        let snapshot = project.join(".ilium/sessions/default.json");
        std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
        let server_binary = Path::new(env!("CARGO_BIN_EXE_ilium")).with_file_name("ilium-server");
        assert!(
            server_binary.is_file(),
            "build the matching ilium-server binary before this integration test: {}",
            server_binary.display()
        );
        let path = std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
            .expect("isolated PATH");
        let server = Command::new(server_binary)
            .args([
                "--session-name",
                SESSION_NAME,
                "--socket-path",
                socket.to_str().unwrap(),
                "--snapshot-path",
                snapshot.to_str().unwrap(),
                "--session-cwd",
                project.to_str().unwrap(),
                "--log-path",
                log.to_str().unwrap(),
                "--active-log-path-file",
                active_log.to_str().unwrap(),
            ])
            .current_dir(&project)
            .env("PATH", &path)
            .env("SHELL", "/bin/sh")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env(ilium_platform::runtime_dir::SOCKET_DIR_ENV, &socket_dir)
            .env(ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV, &log_root)
            .env(ilium_platform::paths::CONFIG_DIR_ENV, &config)
            .env(ilium_client::AGENT_SETUP_HOME_ENV, &agent_setup)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start isolated ilium-server");
        let mut session = Self {
            root,
            project,
            socket,
            active_log,
            launch_record,
            server,
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !session.socket.exists() || !session.active_log.exists() {
            assert!(
                session.server.try_wait().unwrap().is_none(),
                "isolated server exited before readiness"
            );
            assert!(
                tokio::time::Instant::now() < deadline,
                "server did not start"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        session
    }

    async fn run_cli(&self, arguments: &[&str]) -> (bool, String, String) {
        let bin = self.root.path().join("bin");
        let path = std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
            .expect("isolated PATH");
        let output = tokio::time::timeout(
            Duration::from_secs(20),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_ilium"))
                .args(arguments)
                .current_dir(&self.project)
                .env("PATH", path)
                .env("SHELL", "/bin/sh")
                .env("HOME", self.root.path().join("home"))
                .env("XDG_DATA_HOME", self.root.path().join("data"))
                .env("XDG_CONFIG_HOME", self.root.path().join("config"))
                .env("XDG_RUNTIME_DIR", self.root.path().join("runtime"))
                .env(
                    ilium_platform::runtime_dir::SOCKET_DIR_ENV,
                    self.root.path().join("runtime/ilium"),
                )
                .env(
                    ilium_platform::runtime_dir::DEBUG_LOG_DIR_ENV,
                    self.root.path().join("logs"),
                )
                .env(
                    ilium_platform::paths::CONFIG_DIR_ENV,
                    self.root.path().join("config"),
                )
                .env(
                    ilium_client::AGENT_SETUP_HOME_ENV,
                    self.root.path().join("agent-setup"),
                )
                .env_remove(ilium_ipc::pane_env::PANE_ID)
                .env_remove(ilium_ipc::pane_env::SESSION_NAME)
                .env_remove(ilium_ipc::pane_env::SESSION_SOCKET)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("isolated CLI finished before timeout")
        .expect("run isolated CLI");
        (
            output.status.success(),
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        )
    }

    async fn tree(&self) -> ilium_core::Tree {
        let mut connection = Connection::connect(&self.socket, SESSION_NAME.to_string())
            .await
            .expect("connect isolated snapshot observer");
        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match connection.events.recv().await {
                    Some(ServerEvent::TreeSnapshot(tree)) => return tree,
                    Some(ServerEvent::Error { message }) => panic!("snapshot error: {message}"),
                    Some(_) => {}
                    None => panic!("server closed before snapshot"),
                }
            }
        })
        .await
        .expect("snapshot before timeout");
        let _ = connection
            .requests
            .send(ilium_ipc::ClientRequest::Detach)
            .await;
        event
    }

    async fn stop(mut self) -> PathBuf {
        let project_path = self.root.path().to_path_buf();
        let (success, stdout, stderr) = self.run_cli(&["kill-session", SESSION_NAME]).await;
        assert!(success, "kill-session: {stdout} {stderr}");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while self.server.try_wait().unwrap().is_none() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "server did not exit"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        drop(self);
        project_path
    }
}

fn jsonl(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line).expect("CLI stdout is JSONL");
            assert!(record["type"].is_string(), "JSONL type is required");
            record
        })
        .collect()
}

#[tokio::test]
async fn new_pane_worktree_cli_creates_owned_workspace_and_rejects_collision() {
    let session = IsolatedSession::start().await;
    std::fs::write(
        session.root.path().join("config/config.toml"),
        "[notifications]\nenabled = false\n[http_api]\nport = 0\n[git]\nsetup_command = \"test -r .local.env && printf setup-ready > setup.done\"\n",
    )
    .expect("configure isolated worktree setup");
    let arguments = ["new-pane", "--worktree", "--branch", BRANCH, "--", "codex"];
    let (success, stdout, stderr) = session.run_cli(&arguments).await;
    assert!(success, "new-pane failed: {stdout} {stderr}");
    let records = jsonl(&stdout);
    let result = records
        .iter()
        .find(|record| record["type"] == "result")
        .expect("correlated creation result");
    let request_id = result["request_id"].as_u64().expect("request ID");
    assert!(records
        .iter()
        .all(|record| record["request_id"] == request_id));
    let stages: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "progress")
        .filter_map(|record| record["stage"].as_str())
        .collect();
    assert_eq!(
        stages,
        [
            "querying-repository",
            "creating-worktree",
            "preparing",
            "running-setup",
            "starting"
        ]
    );
    let pane_id = result["pane_id"].as_u64().expect("pane ID");
    let linked = session.root.path().join("project.worktrees/agent-cli-test");
    assert_eq!(result["worktree_path"], linked.to_str().unwrap());
    assert_eq!(result["branch"], BRANCH);
    assert_eq!(result["base"], "main");
    assert_eq!(git(&linked, &["branch", "--show-current"]), BRANCH);
    assert_eq!(
        std::fs::read(linked.join(".local.env")).unwrap(),
        b"only in this test\n"
    );
    assert_eq!(
        std::fs::read(linked.join("setup.done")).unwrap(),
        b"setup-ready"
    );
    let metadata_head = PathBuf::from(git(&linked, &["rev-parse", "--git-path", "HEAD"]));
    let marker_path = metadata_head
        .parent()
        .unwrap()
        .join("ilium-workspace-owner.json");
    let marker: Value = serde_json::from_slice(&std::fs::read(marker_path).unwrap()).unwrap();
    assert_eq!(marker["worktree_root"], linked.to_str().unwrap());
    assert_eq!(marker["branch"], BRANCH);
    let marker_id = marker["workspace_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .expect("durable workspace identity");
    let tree = session.tree().await;
    let pane_id = ilium_core::NodeId(pane_id);
    assert_eq!(tree.pane_cwd(pane_id), Some(linked.as_path()));
    let workspace = tree.pane_workspace(pane_id).expect("workspace on pane");
    assert!(workspace.created_by_ilium);
    assert_eq!(workspace.workspace_id.as_deref(), Some(marker_id));
    assert_eq!(workspace.worktree_root, linked);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !session.launch_record.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "fake Codex did not launch"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        std::fs::read_to_string(&session.launch_record)
            .unwrap()
            .trim(),
        format!("{}|{}", linked.display(), linked.display())
    );

    let (second_success, second_stdout, second_stderr) = session.run_cli(&arguments).await;
    assert!(
        !second_success,
        "duplicate branch unexpectedly succeeded: {second_stdout} {second_stderr}"
    );
    let second_records = jsonl(&second_stdout);
    assert!(
        second_records.iter().any(|record| {
            record["type"] == "error"
                && record["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("branch"))
        }),
        "duplicate branch has no JSONL error: {second_stdout}"
    );
    assert_eq!(
        git(
            &session.project,
            &["rev-parse", "--verify", "refs/heads/agent/cli-test"]
        ),
        git(&session.project, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&session.project, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        2
    );
    assert_eq!(session.tree().await.panes().count(), 1);

    let root_path = session.stop().await;
    assert!(!root_path.exists(), "isolated test tree was not removed");
}
