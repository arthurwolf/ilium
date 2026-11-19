//! Startup must remain available when optional automation credentials cannot be admitted.
#![cfg(unix)]
use ilium_transport::{Liveness, SessionEndpoint};
use std::time::Duration;

#[tokio::test]
async fn review_nonregular_credentials_do_not_block_or_prevent_server_startup() {
    let mut outcomes = Vec::new();
    for kind in ["directory", "fifo"] {
        let root = tempfile::Builder::new()
            .prefix("ilium-monitoring-startup-")
            .tempdir_in("/data")
            .unwrap();
        std::fs::write(root.path().join(".scratch-reaper"), b"").unwrap();
        let socket = root.path().join("s.sock");
        let tokens = socket.with_extension("ctl-tokens.json");
        if kind == "directory" {
            std::fs::create_dir(&tokens).unwrap();
        } else {
            assert!(std::process::Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(&tokens)
                .status()
                .unwrap()
                .success());
        }
        std::fs::create_dir(root.path().join("config")).unwrap();
        std::fs::write(
            root.path().join("config/config.toml"),
            "[debug]\nfile_logging_enabled = true\n[api]\nport = 0\n",
        )
        .unwrap();
        let diagnostics = root.path().join("stderr.log");
        let binary = std::env::var("CARGO_BIN_EXE_ilium-server").unwrap();
        let mut child = tokio::process::Command::new(binary)
            .arg("--session-name")
            .arg("review")
            .arg("--socket-path")
            .arg(&socket)
            .arg("--snapshot-path")
            .arg(root.path().join(".ilium/sessions/review.json"))
            .arg("--session-cwd")
            .arg(root.path())
            .arg("--log-path")
            .arg(root.path().join("server.log"))
            .env(
                ilium_server::paths::CONFIG_DIR_ENV,
                root.path().join("config"),
            )
            .env("XDG_DATA_HOME", root.path().join("data"))
            .current_dir(root.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(&diagnostics).unwrap())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let started = loop {
            if SessionEndpoint::from_path(&socket).probe_liveness() == Liveness::Live {
                break true;
            }
            if child.try_wait().unwrap().is_some() || tokio::time::Instant::now() >= deadline {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let mut observed = Vec::new();
        let response = tokio::time::timeout(Duration::from_secs(2), async {
            if !started {
                return Err("socket never became live".to_owned());
            }
            let mut stream = SessionEndpoint::from_path(&socket)
                .connect()
                .await
                .map_err(|error| error.to_string())?;
            ilium_ipc::write_frame(
                &mut stream,
                &ilium_ipc::ClientRequest::Attach {
                    session: "review".into(),
                    hello: Default::default(),
                },
            )
            .await
            .map_err(|error| error.to_string())?;
            loop {
                let event = ilium_ipc::read_frame::<ilium_ipc::ServerEvent, _>(&mut stream)
                    .await
                    .map_err(|error| error.to_string())?;
                observed.push(format!("{event:?}").chars().take(300).collect::<String>());
                if matches!(
                    event,
                    ilium_ipc::ServerEvent::TreeSnapshot(_)
                        | ilium_ipc::ServerEvent::PaneStateSnapshot { .. }
                ) {
                    return Ok::<(), String>(());
                }
            }
        })
        .await;
        let started = matches!(response, Ok(Ok(())));
        let exit_before_cleanup = child.try_wait().unwrap();
        let evidence = format!(
            "response={response:?}; events={observed:?}; exit={exit_before_cleanup:?}; stderr={}; log={}",
            std::fs::read_to_string(&diagnostics).unwrap_or_default(),
            std::fs::read_to_string(root.path().join("server.log")).unwrap_or_default(),
        );
        let _ = child.kill().await;
        let _ = child.wait().await;
        outcomes.push((kind, started, evidence));
    }
    assert!(
        outcomes.iter().all(|(_, started, _)| *started),
        "startup outcomes: {outcomes:?}"
    );
}
