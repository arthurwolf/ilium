//! End-to-end worktree creation through the loopback HTTP boundary.
//! The child server sees only an isolated fake `codex` executable on PATH.

#![cfg(unix)]

use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ilium_core::{NodeId, Tree};
use ilium_ipc::{write_frame, ClientRequest};
use ilium_transport::{Liveness, SessionEndpoint};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod common;
use common::read_initial_state;

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

struct IsolatedHttpServer {
    root: tempfile::TempDir,
    project: PathBuf,
    socket: PathBuf,
    launch_record: PathBuf,
    prompt_record: PathBuf,
    port: u16,
    child: Child,
}

/// A failed assertion must still release an in-flight Git hook before the
/// child server and its temporary checkout are torn down.
struct HookRelease {
    path: PathBuf,
}

impl HookRelease {
    fn release(&self) {
        std::fs::write(&self.path, b"go\n").expect("release isolated Git hook");
    }
}

impl Drop for HookRelease {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.path, b"go\n");
    }
}

impl Drop for IsolatedHttpServer {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl IsolatedHttpServer {
    async fn start() -> Self {
        let root = tempfile::Builder::new()
            .prefix("iwh")
            .tempdir_in("/tmp")
            .expect("short isolated test root");
        let project = ilium_platform::paths::canonicalize(root.path())
            .expect("canonical fixture root")
            .join("project");
        let bin = root.path().join("bin");
        let config = root.path().join("config");
        let home = root.path().join("home");
        let data = root.path().join("data");
        let runtime = root.path().join("runtime");
        for path in [&project, &bin, &config, &home, &data, &runtime] {
            std::fs::create_dir(path).expect("isolated server directory");
        }
        let port_guard = TcpListener::bind(("127.0.0.1", 0)).expect("reserve a loopback port");
        let port = port_guard
            .local_addr()
            .expect("reserved port address")
            .port();
        std::fs::write(
            config.join("config.toml"),
            format!(
                "[notifications]\nenabled = false\n[session]\nbackups_enabled = false\n[api]\nport = {port}\n"
            ),
        )
        .expect("isolated server config");
        assert_eq!(
            ilium_server::config::load(&config)
                .expect("isolated config is valid")
                .http_api
                .port,
            port
        );

        git(&project, &["init", "-q", "-b", "main"]);
        git(&project, &["config", "user.name", "Ilium Test"]);
        git(&project, &["config", "user.email", "ilium@example.invalid"]);
        std::fs::write(project.join("README.md"), "isolated fixture\n").unwrap();
        git(&project, &["add", "README.md"]);
        git(&project, &["commit", "-q", "-m", "initial"]);

        let launch_record = root.path().join("launches.txt");
        let prompt_record = root.path().join("prompts.txt");
        let fake_codex = bin.join("codex");
        std::fs::write(
            &fake_codex,
            "#!/bin/sh\nprintf '%s|%s\\n' \"$PWD\" \"${ILIUM_WORKTREE:-}\" >> \"$ILIUM_HTTP_TEST_LAUNCH_RECORD\"\nprintf '› '\nwhile IFS= read -r line; do\n  printf '%s\\n' \"$line\" >> \"$ILIUM_HTTP_TEST_PROMPT_RECORD\"\n  printf '\\r\\n› '\ndone\n",
        )
        .expect("fake Codex script");
        let mut permissions = std::fs::metadata(&fake_codex)
            .expect("fake Codex metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&fake_codex, permissions).expect("fake Codex permissions");

        let socket = runtime.join("http.sock");
        let snapshot = project.join(".ilium/sessions/workspace-http.json");
        let log = runtime.join("server.log");
        let search_path =
            std::env::join_paths([bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
                .expect("isolated provider and system command path");
        drop(port_guard);
        let server_binary = std::env::var("CARGO_BIN_EXE_ilium-server")
            .expect("Cargo provides the ilium-server binary path to integration tests");
        let child = Command::new(server_binary)
            .args([
                "--session-name",
                "workspace-http",
                "--socket-path",
                socket.to_str().unwrap(),
                "--snapshot-path",
                snapshot.to_str().unwrap(),
                "--session-cwd",
                project.to_str().unwrap(),
                "--log-path",
                log.to_str().unwrap(),
            ])
            .current_dir(&project)
            .env("PATH", search_path)
            .env("SHELL", "/bin/sh")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_DATA_HOME", &data)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env(ilium_server::paths::CONFIG_DIR_ENV, &config)
            .env("ILIUM_HTTP_TEST_LAUNCH_RECORD", &launch_record)
            .env("ILIUM_HTTP_TEST_PROMPT_RECORD", &prompt_record)
            .env(
                "ILIUM_HTTP_TEST_HOOK_ENTERED",
                root.path().join("post-checkout-entered"),
            )
            .env(
                "ILIUM_HTTP_TEST_HOOK_RELEASE",
                root.path().join("post-checkout-release"),
            )
            .env(
                "ILIUM_HTTP_TEST_HOOK_FINISHED",
                root.path().join("post-checkout-finished"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("isolated ilium-server process");
        let server = Self {
            root,
            project,
            socket,
            launch_record,
            prompt_record,
            port,
            child,
        };

        let endpoint = SessionEndpoint::from_path(&server.socket);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while endpoint.probe_liveness() != Liveness::Live
            || tokio::net::TcpStream::connect(("127.0.0.1", server.port))
                .await
                .is_err()
        {
            assert!(
                tokio::time::Instant::now() < deadline,
                "isolated server did not bind its IPC and HTTP listeners"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        server
    }

    async fn send_post(&self, request: Value) -> tokio::net::TcpStream {
        let body = request.to_string();
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", self.port))
            .await
            .expect("connect isolated loopback HTTP listener");
        let tokens: ilium_control::access::Tokens = serde_json::from_slice(
            &std::fs::read(self.socket.with_extension("ctl-tokens.json"))
                .expect("isolated control tokens"),
        )
        .unwrap();
        let head = format!(
            "POST /create_agent HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.port, tokens.act,
            body.len()
        );
        stream.write_all(head.as_bytes()).await.expect("HTTP head");
        stream.write_all(body.as_bytes()).await.expect("HTTP body");
        stream
    }

    async fn post(&self, request: Value) -> (u16, String) {
        let mut stream = self.send_post(request).await;
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(20), stream.read_to_end(&mut response))
            .await
            .expect("HTTP response before timeout")
            .expect("read HTTP response");
        let response = String::from_utf8(response).expect("HTTP response is UTF-8");
        let (head, body) = response
            .split_once("\r\n\r\n")
            .expect("HTTP headers and body");
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|status| status.parse::<u16>().ok())
            .expect("HTTP status code");
        (status, body.to_owned())
    }

    async fn snapshot_and_stop(&mut self) -> Tree {
        let mut stream = SessionEndpoint::from_path(&self.socket)
            .connect()
            .await
            .expect("connect isolated IPC observer");
        write_frame(
            &mut stream,
            &ClientRequest::Attach {
                session: "workspace-http".into(),
                hello: Default::default(),
            },
        )
        .await
        .expect("attach isolated observer");
        let (tree, _) = read_initial_state(&mut stream, Duration::from_secs(5)).await;
        write_frame(&mut stream, &ClientRequest::KillSession)
            .await
            .expect("stop isolated session");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if self
                .child
                .try_wait()
                .expect("isolated child status")
                .is_some()
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "isolated server did not stop"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tree
    }
}

async fn wait_for_file(path: &Path) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for isolated marker {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn monitoring_events_http_returns_a_bounded_quiet_response() {
    let server = IsolatedHttpServer::start().await;
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", server.port))
        .await
        .unwrap();
    stream.write_all(format!("GET /v1/events?max-items=12&max-chars=80 HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", server.port).as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8(bytes).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(body.starts_with("quiet cursor="), "{body}");
    assert!(body.chars().count() <= 80);
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", server.port))
        .await
        .unwrap();
    stream.write_all(format!("GET /v1/events?min-severity=unknown HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", server.port).as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8(bytes).unwrap();
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
}

#[tokio::test]
async fn monitoring_digest_http_returns_bounded_compact_and_json_responses() {
    let server = IsolatedHttpServer::start().await;
    for (format, budget) in [("compact", 80), ("json", 180)] {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", server.port))
            .await
            .unwrap();
        stream.write_all(format!("GET /v1/digest?format={format}&max-chars={budget} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", server.port).as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8(bytes).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(body.chars().count() <= budget);
        if format == "json" {
            let value: Value = serde_json::from_str(body).unwrap();
            assert_eq!(value["quiet"], true);
        } else {
            assert!(body.starts_with("quiet cursor="), "{body}");
        }
    }
}

#[tokio::test]
async fn http_workspace_uses_server_path_and_preserves_legacy_request() {
    let mut server = IsolatedHttpServer::start().await;
    let forbidden_path = server.root.path().join("caller-selected-path");
    let (status, _) = server
        .post(json!({
            "agent_type": "codex",
            "project": server.project,
            "prompt": "must not launch",
            "workspace": {
                "branch": "agent/path-override",
                "base": "main",
                "path": forbidden_path
            }
        }))
        .await;
    assert!(
        (400..500).contains(&status),
        "path override status {status}"
    );
    assert!(!forbidden_path.exists());
    assert!(git(
        &server.project,
        &["branch", "--list", "agent/path-override"]
    )
    .is_empty());

    let branch = "agent/http-boundary";
    let worktree_path = ilium_platform::paths::canonicalize(server.root.path())
        .expect("canonical fixture root")
        .join("project.worktrees/agent-http-boundary");
    let (status, body) = server
        .post(json!({
            "agent_type": "codex",
            "project": server.project,
            "prompt": "inspect isolated worktree",
            "workspace": { "branch": branch, "base": "main" }
        }))
        .await;
    assert_eq!(status, 200, "worktree HTTP response: {body}");
    let worktree_response: Value = serde_json::from_str(&body).expect("worktree response JSON");
    assert_eq!(worktree_response["prompt_delivered"], true);
    assert_eq!(
        worktree_response["project_path"],
        server.project.to_str().unwrap()
    );
    let worktree_pane = NodeId(
        worktree_response["pane_id"]
            .as_u64()
            .expect("worktree pane id"),
    );
    assert!(worktree_path.is_dir(), "server-derived sibling checkout");
    assert_eq!(git(&worktree_path, &["branch", "--show-current"]), branch);

    let (status, body) = server
        .post(json!({
            "agent_type": "codex",
            "project": server.project,
            "prompt": "inspect shared checkout"
        }))
        .await;
    assert_eq!(status, 200, "legacy HTTP response: {body}");
    let legacy_response: Value = serde_json::from_str(&body).expect("legacy response JSON");
    assert_eq!(legacy_response["prompt_delivered"], true);
    let legacy_pane = NodeId(legacy_response["pane_id"].as_u64().expect("legacy pane id"));
    assert_ne!(worktree_pane, legacy_pane);

    let tree = server.snapshot_and_stop().await;
    assert_eq!(tree.pane_cwd(worktree_pane), Some(worktree_path.as_path()));
    assert_eq!(tree.pane_cwd(legacy_pane), Some(server.project.as_path()));
    assert!(tree.pane_workspace(legacy_pane).is_none());
    let workspace = tree
        .pane_workspace(worktree_pane)
        .expect("worktree pane retained provenance");
    assert!(workspace.created_by_ilium);
    assert_eq!(workspace.worktree_root, worktree_path);
    assert_eq!(workspace.branch, branch);
    assert_eq!(workspace.base_ref, "refs/heads/main");
    let workspace_id = workspace.workspace_id.as_deref().expect("ownership ID");
    let git_metadata = git(&worktree_path, &["rev-parse", "--absolute-git-dir"]);
    let marker: Value = serde_json::from_slice(
        &std::fs::read(Path::new(&git_metadata).join("ilium-workspace-owner.json"))
            .expect("Git metadata ownership marker"),
    )
    .expect("ownership marker JSON");
    assert_eq!(marker["workspace_id"], workspace_id);
    assert_eq!(marker["worktree_root"], worktree_path.to_str().unwrap());

    let launches = std::fs::read_to_string(&server.launch_record).expect("fake agent launches");
    assert!(launches.contains(&format!(
        "{}|{}",
        worktree_path.display(),
        worktree_path.display()
    )));
    assert!(launches.contains(&format!("{}|\n", server.project.display())));
    let prompts = std::fs::read_to_string(&server.prompt_record).expect("fake agent prompts");
    assert!(prompts.contains("inspect isolated worktree"), "{prompts}");
    assert!(prompts.contains("inspect shared checkout"), "{prompts}");
}

#[tokio::test]
async fn disconnect_during_worktree_add_never_commits_a_pane() {
    let mut server = IsolatedHttpServer::start().await;
    let hook_entered = server.root.path().join("post-checkout-entered");
    let hook_finished = server.root.path().join("post-checkout-finished");
    let hook_release = HookRelease {
        path: server.root.path().join("post-checkout-release"),
    };
    let hook = server.project.join(".git/hooks/post-checkout");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf 'entered\\n' > \"$ILIUM_HTTP_TEST_HOOK_ENTERED\"\nwhile [ ! -f \"$ILIUM_HTTP_TEST_HOOK_RELEASE\" ]; do sleep 0.05; done\nprintf 'finished\\n' > \"$ILIUM_HTTP_TEST_HOOK_FINISHED\"\n",
    )
    .expect("controlled post-checkout hook");
    let mut permissions = std::fs::metadata(&hook)
        .expect("hook metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&hook, permissions).expect("hook permissions");

    let branch = "agent/disconnected-http";
    let worktree = ilium_platform::paths::canonicalize(server.root.path())
        .expect("canonical fixture root")
        .join("project.worktrees/agent-disconnected-http");
    let connection = server
        .send_post(json!({
            "agent_type": "codex",
            "project": server.project,
            "prompt": "must never reach provider",
            "workspace": { "branch": branch, "base": "main" }
        }))
        .await;
    wait_for_file(&hook_entered).await;
    assert!(worktree.is_dir(), "Git add reached post-checkout hook");
    assert!(
        git(&server.project, &["branch", "--list", branch]).ends_with(branch),
        "Git registered the linked branch before the hook returned"
    );

    drop(connection);
    tokio::time::sleep(Duration::from_millis(100)).await;
    hook_release.release();
    wait_for_file(&hook_finished).await;
    let common_dir = ilium_git::discover(&server.project)
        .await
        .unwrap()
        .common_dir;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if matches!(
            ilium_platform::process_control::try_workspace_repository_lease(&common_dir),
            Ok(Some(_))
        ) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "requester disconnect did not finish its repository transaction"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let branch_listing = git(&server.project, &["branch", "--list", branch]);
    if worktree.exists() {
        assert!(
            branch_listing.ends_with(branch),
            "retained checkout lost its branch: {branch_listing}"
        );
        let metadata = git(&worktree, &["rev-parse", "--absolute-git-dir"]);
        assert!(
            Path::new(&metadata)
                .join("ilium-workspace-owner.json")
                .is_file(),
            "retained checkout lost its owner marker"
        );
    } else {
        assert!(branch_listing.is_empty(), "rollback left a branch behind");
    }
    let tree = server.snapshot_and_stop().await;
    assert_eq!(tree.panes().count(), 0, "no disconnected HTTP pane");
    assert!(
        !server.launch_record.exists(),
        "fake provider never launched"
    );
    assert!(
        !server.prompt_record.exists(),
        "prompt never reached provider"
    );
}

#[tokio::test]
async fn monitoring_pane_brief_omits_full_metadata_and_labels_missing_message_evidence() {
    let server = IsolatedHttpServer::start().await;
    let (status, body) = server
        .post(json!({"agent_type":"codex","project":server.project,"prompt":"fixture brief"}))
        .await;
    assert_eq!(status, 200, "{body}");
    let created: Value = serde_json::from_str(&body).unwrap();
    let pane = created["pane_id"].as_u64().unwrap();
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", server.port))
        .await
        .unwrap();
    stream.write_all(format!("GET /v1/panes/{pane}?brief=true HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", server.port).as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8(bytes).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{response}");
    let brief: Value = serde_json::from_str(body).unwrap();
    assert_eq!(brief["pane"], pane);
    assert!(brief["state"].is_string());
    assert!(brief.get("last_message").is_some_and(Value::is_null));
    assert!(brief.get("state_age_seconds").is_some());
    assert!(
        brief.get("prompt_queue").is_none()
            && brief.get("screen").is_none()
            && brief.get("node").is_none()
    );
    assert!(body.chars().count() <= 1000);
}

async fn control_post(
    server: &IsolatedHttpServer,
    token: Option<&str>,
    operation: Value,
) -> (u16, String) {
    let body = operation.to_string();
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", server.port))
        .await
        .unwrap();
    stream.write_all(format!("POST /v1/ops HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{authorization}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", server.port, body.len()).as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8(bytes).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        body.into(),
    )
}

#[tokio::test]
async fn monitoring_http_permissions_default_to_read_and_require_admin_for_termination() {
    let server = IsolatedHttpServer::start().await;
    let rename = json!({"op":"rename","node":999,"title":"fixture"});
    let (status, body) = control_post(&server, None, rename.clone()).await;
    assert_eq!(status, 403, "outside unauthenticated write: {body}");
    let token_path = server.socket.with_extension("ctl-tokens.json");
    let file = ilium_platform::secure_fs::open_regular_file(&token_path).unwrap();
    ilium_platform::secure_fs::validate_private_open_file(&file).unwrap();
    let tokens: Value = serde_json::from_reader(file).unwrap();
    assert_eq!(
        control_post(&server, Some("wrong-token"), rename.clone())
            .await
            .0,
        401
    );
    for operation in [
        json!({"op":"terminate","pane":999}),
        json!({"op":"kill_session"}),
    ] {
        let (status, body) = control_post(&server, tokens["act"].as_str(), operation).await;
        assert_eq!(status, 403, "act termination: {body}");
    }
    let (status, _) = control_post(&server, tokens["admin"].as_str(), rename).await;
    assert_ne!(status, 401);
    assert_ne!(status, 403);
}

#[tokio::test]
async fn monitoring_control_write_audit_records_caller_operation_target_and_time() {
    let server = IsolatedHttpServer::start().await;
    // A missing target still records an authorized request, never a fake success.
    let token_path = server.socket.with_extension("ctl-tokens.json");
    let tokens: Value = serde_json::from_reader(
        std::fs::File::open(&token_path).expect("automation token file created at startup"),
    )
    .unwrap();
    let _ = control_post(
        &server,
        tokens["act"].as_str(),
        json!({"op":"rename","node":999,"title":"fixture"}),
    )
    .await;
    let path = server.project.join(".ilium/logs/workspace-http.ctl.jsonl");
    let file = ilium_platform::secure_fs::open_regular_file(&path).expect("durable write audit");
    ilium_platform::secure_fs::validate_private_open_file(&file).unwrap();
    use std::io::BufRead;
    let entries: Vec<Value> = std::io::BufReader::new(file)
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    let entry = entries
        .iter()
        .find(|entry| entry["operation"] == "rename")
        .expect("rename audit record");
    assert_eq!(entry["target"], 999);
    assert!(entry["caller"]["token_id"].is_string());
    assert!(entry["at_unix_millis"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn monitoring_audit_failure_refuses_an_authorized_mutation() {
    let mut server = IsolatedHttpServer::start().await;
    let tokens: ilium_control::access::Tokens = serde_json::from_slice(
        &std::fs::read(server.socket.with_extension("ctl-tokens.json")).unwrap(),
    )
    .unwrap();
    // A directory at the exact log filename makes append fail without changing
    // unrelated logs or relying on platform-specific permission behavior.
    std::fs::create_dir_all(server.project.join(".ilium/logs/workspace-http.ctl.jsonl")).unwrap();
    let (status, body) = control_post(
        &server,
        Some(&tokens.act),
        json!({"op":"new_group","parent":ilium_core::ROOT_ID.0,"name":"must not exist"}),
    )
    .await;
    assert_eq!(status, 500, "audit refusal: {body}");
    let tree = server.snapshot_and_stop().await;
    assert!(!tree.all_ids().any(|id| tree
        .get(id)
        .is_some_and(|node| node.name == "must not exist")));
}

#[tokio::test]
async fn review_fifo_audit_refuses_promptly_without_mutating() {
    let mut server = IsolatedHttpServer::start().await;
    let tokens: ilium_control::access::Tokens = serde_json::from_slice(
        &std::fs::read(server.socket.with_extension("ctl-tokens.json")).unwrap(),
    )
    .unwrap();
    let path = server.project.join(".ilium/logs/workspace-http.ctl.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    assert!(Command::new("mkfifo")
        .args(["-m", "600"])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        control_post(
            &server,
            Some(&tokens.act),
            json!({"op":"new_group","parent":ilium_core::ROOT_ID.0,"name":"must not exist"}),
        ),
    )
    .await;
    assert!(
        response.is_ok(),
        "audit FIFO must be rejected without waiting for a reader"
    );
    assert_eq!(response.unwrap().0, 500);
    let tree = server.snapshot_and_stop().await;
    assert!(!tree.all_ids().any(|id| tree
        .get(id)
        .is_some_and(|node| node.name == "must not exist")));
}
