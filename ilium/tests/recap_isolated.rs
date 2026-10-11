//! Real CLI/server + fake loopback inference. Never reads the user's config or sessions.
#![cfg(unix)]
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Session {
    root: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    binary: PathBuf,
}
impl Session {
    fn command(&self, args: &[&str]) -> Output {
        Command::new(&self.binary)
            .arg("--cwd")
            .arg(&self.root)
            .args(args)
            .env("ILIUM_SOCKET_DIR", &self.socket)
            .env("ILIUM_CONFIG_DIR", &self.config)
            .env("ILIUM_DEBUG_LOG_DIR", self.root.join("logs"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_CONFIG_HOME", self.root.join("xdg-config"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("ILIUM_AGENT_SETUP_HOME", self.root.join("agent-home"))
            .env("SHELL", "/bin/sh")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy")
            .env_remove("ILIUM_SESSION_SOCKET")
            .env_remove("ILIUM_PANE_ID")
            .output()
            .expect("isolated CLI executes")
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.command(&["kill-session", "recap-offline"]);
    }
}

fn fake_provider() -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let (mut socket, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => return,
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let read = socket.read(&mut chunk).unwrap();
                if read == 0 {
                    return;
                }
                request.extend_from_slice(&chunk[..read]);
                assert!(request.len() <= 128 * 1024, "bounded fake request");
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..end]);
                    let length: usize = header
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        let body: serde_json::Value =
                            serde_json::from_slice(&request[end + 4..end + 4 + length]).unwrap();
                        tx.send(body["messages"][1]["content"].as_str().unwrap().to_owned())
                            .unwrap();
                        let answer = serde_json::json!({"short":{"text":"Selected shell shows the factual marker.","pointers":[]},"long":{"text":"The selected shell produced the marker and is idle.","pointers":[]}}).to_string();
                        let response =
                            serde_json::json!({"message":{"content":answer}}).to_string();
                        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                        return;
                    }
                }
            }
        }
    });
    (address, rx, worker)
}

#[tokio::test]
async fn recap_selected_shell_screen_reaches_fake_inference_with_matching_binaries() {
    // Short, isolated runtime paths are required by Unix socket length bounds.
    let scratch = if Path::new("/data").is_dir() {
        "/data"
    } else {
        "/media/arthur/build"
    };
    let root = tempfile::Builder::new()
        .prefix("ilium-recap-")
        .tempdir_in(scratch)
        .unwrap();
    std::fs::write(root.path().join(".scratch-reaper"), "").unwrap();
    let (endpoint, requests, worker) = fake_provider();
    let binary = std::env::var_os("ILIUM_PTY_SMOKE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_ilium")));
    assert!(
        binary.parent().unwrap().join("ilium-server").is_file(),
        "build matching sibling server first"
    );
    let session = Session {
        root: root.path().to_owned(),
        socket: root.path().join("s"),
        config: root.path().join("c"),
        binary,
    };
    std::fs::create_dir_all(&session.config).unwrap();
    std::fs::create_dir_all(session.root.join(".ilium")).unwrap();
    std::fs::write(
        session.root.join(".ilium/config.yaml"),
        "project name: recap offline fixture\n",
    )
    .unwrap();
    std::fs::write(session.config.join("config.toml"), format!("[agent_setup]\nnever_ask_global = true\n[onboarding]\ncompleted = true\nai_enabled = true\n[inference]\nselected_provider = 'ollama'\n[inference.ollama]\nbase_url = '{endpoint}'\nmodel = 'fake-offline'\n")).unwrap();
    for marker in ["SELECTED_RECAP_MARKER", "UNSELECTED_RECAP_MARKER"] {
        let command = format!("printf '{marker}\\nTOKEN_VALUE=fakecredential12345\\n'; exec cat");
        let created = session.command(&[
            "new-pane",
            "--session-name",
            "recap-offline",
            "--keep-open",
            "--",
            "/bin/sh",
            "-c",
            &command,
        ]);
        assert!(
            created.status.success(),
            "{}",
            String::from_utf8_lossy(&created.stderr)
        );
    }
    let listed = session.command(&["panes", "--here", "--session", "recap-offline"]);
    assert!(listed.status.success());
    let panes: Vec<serde_json::Value> = String::from_utf8(listed.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|v: &serde_json::Value| v["type"] == "pane")
        .collect();
    let selected = panes
        .iter()
        .find(|pane| {
            pane["name"]
                .as_str()
                .unwrap_or("")
                .contains("SELECTED_RECAP_MARKER")
                && !pane["name"].as_str().unwrap_or("").contains("UNSELECTED")
        })
        .expect("selected shell exists");
    let id = selected["pane_id"].to_string();
    // Wait on bounded current-screen replies rather than assuming PTY output
    // has been parsed merely because pane creation was acknowledged.
    let socket = std::fs::read_dir(&session.socket)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "sock"))
        .unwrap();
    let mut connection =
        ilium_client::connection::Connection::connect(&socket, "recap-offline".to_owned())
            .await
            .unwrap();
    let pane_id = ilium_core::NodeId(selected["pane_id"].as_u64().unwrap());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            connection
                .requests
                .send(ilium_ipc::ClientRequest::GetPaneRecapScreen { pane_id })
                .await
                .unwrap();
            loop {
                let event = connection.events.recv().await.unwrap();
                let (event, _retention) = event.into_parts();
                if let ilium_ipc::ServerEvent::PaneRecapScreen {
                    pane_id: replied,
                    result,
                } = event
                {
                    assert_eq!(replied, pane_id);
                    if result.is_ok_and(|text| text.contains("SELECTED_RECAP_MARKER")) {
                        return;
                    }
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("selected fake shell reaches the current parser");
    connection
        .requests
        .send(ilium_ipc::ClientRequest::Detach)
        .await
        .unwrap();
    drop(connection);

    let result = session.command(&[
        "recap",
        "--here",
        "--session",
        "recap-offline",
        "--pane",
        &id,
        "--max-panes",
        "1",
        "--timeout-seconds",
        "10",
    ]);
    assert!(
        result.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let prompt = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("one fake inference request");
    assert!(
        prompt.contains("SELECTED_RECAP_MARKER"),
        "selected current screen reaches inference"
    );
    assert!(
        !prompt.contains("UNSELECTED_RECAP_MARKER"),
        "unselected screen never enters evidence"
    );
    assert!(
        !prompt.contains("fakecredential12345"),
        "credential value is redacted before inference"
    );
    assert!(prompt.contains("[redacted]"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("Selected shell shows"));
    assert!(
        !String::from_utf8_lossy(&result.stdout).contains("fakecredential12345"),
        "pane names are redacted in output too"
    );
    worker.join().unwrap();
    // With the fake service already gone, AI-off must produce a denied record
    // without attempting another inference request.
    std::fs::write(session.config.join("config.toml"), "[agent_setup]\nnever_ask_global = true\n[onboarding]\ncompleted = true\nai_enabled = false\n").unwrap();
    let denied = session.command(&[
        "recap",
        "--here",
        "--session",
        "recap-offline",
        "--pane",
        &id,
        "--timeout-seconds",
        "3",
    ]);
    assert!(denied.status.success());
    let records: Vec<serde_json::Value> = String::from_utf8(denied.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    assert!(records.iter().any(|record| record["state"] == "denied"));
}
