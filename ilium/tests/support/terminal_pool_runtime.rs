//! Explicit release qualification, using real server delivery and PTY rendering.
use super::*;

// Qualification covers the user's 100+ pane load. The server's shared
// 2 GiB worker budget admits 128 Unix PTYs (five 2 MiB workers each), the two
// 32 MiB CPU workers, and leaves replay/output headroom.
const PANE_COUNT: usize = 128;
const BURST_LINES: usize = 1024;
const BURST_LINE_WIDTH: usize = 256;
const BURST_BYTES_PER_PANE: usize = BURST_LINES * (BURST_LINE_WIDTH + 1);
const LARGE_FIRST_PANE_NOISE_BYTES: usize = 24 * 1024 * 1024;
const SELECTION_LIMIT: Duration = Duration::from_secs(5);

fn shell_quote(argument: &str) -> String {
    let mut quoted = String::from("'");
    for character in argument.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

fn inner_marker_visible(tui: &PtySession, marker: &str) -> bool {
    tui.with_screen(|screen| {
        let (rows, columns) = screen.size();
        (3..rows.saturating_sub(3)).any(|row| {
            // Fixture markers never occur in tree titles or pane chrome.
            let text: String = (TREE_TEXT_COLUMNS..columns)
                .filter_map(|column| screen.cell(row, column))
                .map(vt100::Cell::contents)
                .collect();
            text.contains(marker)
        })
    })
}

async fn select_and_measure(
    tui: &mut PtySession,
    number: usize,
    backwards: bool,
    pass: &str,
    burst_released_at: std::time::Instant,
) -> serde_json::Value {
    let title = format!("pool-{number:03}");
    let marker = format!("INNER-PANE-{number:03}");
    tui.write(b"\x02w").expect("focus isolated tree");
    let mut row = None;
    for _ in 0..PANE_COUNT {
        row = tui.with_screen(|screen| {
            rows_containing_before_column(screen, &title, TREE_TEXT_COLUMNS)
                .into_iter()
                .next()
        });
        if row.is_some() {
            break;
        }
        tui.write(&sgr_mouse_down(if backwards { 64 } else { 65 }, 8, 15))
            .expect("scroll isolated tree");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(
        row.is_some(),
        "test pane {title} absent from isolated sidebar"
    );
    let row = wait_for_stable_value(
        || {
            tui.with_screen(|screen| {
                rows_containing_before_column(screen, &title, TREE_TEXT_COLUMNS)
                    .into_iter()
                    .next()
            })
        },
        SETTLED_LAYOUT_DURATION,
        WAIT_TIMEOUT,
    )
    .await
    .expect("test pane row settles before clicking");
    let already_visible = inner_marker_visible(tui, &marker);
    let started = std::time::Instant::now();
    tui.write(&sgr_mouse_down(0, 8, row)).unwrap();
    tui.write(&sgr_mouse_up(8, row)).unwrap();
    tui.write(b"\r").unwrap();
    let first_content_ms = if number == 0 && pass == "burst-forward" {
        let appeared = wait_until(
            || {
                inner_marker_visible(tui, "FIRST-CONTENT-000")
                    && !inner_marker_visible(tui, &marker)
            },
            SELECTION_LIMIT.saturating_sub(started.elapsed()),
        )
        .await;
        assert!(
            appeared,
            "the selected large-output pane showed no first content before completion"
        );
        Some(started.elapsed().as_secs_f64() * 1000.0)
    } else {
        None
    };
    let displayed = wait_until(
        || inner_marker_visible(tui, &marker),
        SELECTION_LIMIT.saturating_sub(started.elapsed()),
    )
    .await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let result = serde_json::json!({
        "type": "measurement", "pass": pass, "pane": number,
        "marker": marker, "displayed": displayed, "elapsed_ms": elapsed_ms,
        "first_content_ms": first_content_ms,
        "burst_age_ms": burst_released_at.elapsed().as_secs_f64() * 1000.0,
        "already_visible_before_click": already_visible,
        "screen": tui.screen_text(),
    });
    println!("{result}");
    assert!(displayed, "inner content absent for {title}: {result}");
    assert!(
        elapsed_ms <= SELECTION_LIMIT.as_secs_f64() * 1000.0,
        "inner content for {title} exceeded the per-selection 5-second limit: {result}"
    );
    result
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "explicit 128-pane release runtime qualification"]
async fn no_pool_release_renders_128_panes_and_measures_click_latency() {
    assert!(
        !cfg!(debug_assertions),
        "release qualification must launch the optimized client binary"
    );
    let client_binary = std::path::PathBuf::from(ilium_binary());
    let matching_server_binary = client_binary.with_file_name("ilium-server");
    assert!(
        matching_server_binary.is_file(),
        "release qualification needs the matching server beside the client: {}",
        matching_server_binary.display()
    );
    let temp_root = tempfile::tempdir().unwrap();
    let xdg = IsolatedXdgDirs::under(temp_root.path()).unwrap();
    seed_keyboard_config(&xdg);
    let project_dir = temp_root.path().join("no-pool-runtime");
    std::fs::create_dir_all(&project_dir).unwrap();
    seed_project_config(&xdg, &project_dir);
    assert_eq!(
        ilium_client::config::load(&xdg.ilium_config_dir)
            .unwrap()
            .terminal
            .engine_memory_budget_mib,
        0,
        "qualification must exercise the omitted-setting no-pool default"
    );
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 8872);
    drop(listener);
    let config_path = xdg.ilium_config_dir.join("config.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    assert!(!config.contains("[api]"));
    std::fs::write(&config_path, format!("{config}\n[api]\nport = {port}\n")).unwrap();
    let mut cleanup = KillSessionOnDrop {
        xdg: &xdg,
        cwd: project_dir.clone(),
        session_name: SESSION_NAME,
        already_cleaned_up: false,
    };
    let command = PtyCommand::new(
        client_binary.to_string_lossy().into_owned(),
        &project_dir,
        44,
        140,
    )
    .arg("--cwd")
    .arg(project_dir.to_string_lossy().to_string());
    let command = xdg
        .as_pairs()
        .into_iter()
        .fold(command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut tui = PtySession::spawn(command).unwrap();
    assert!(wait_until(|| tui.screen_text().contains(PROJECT_NAME), WAIT_TIMEOUT).await);
    let (socket, server_pid) = isolated_server_identity(&xdg, &project_dir).await;
    #[cfg(target_os = "linux")]
    assert_eq!(
        std::fs::read_link(format!("/proc/{server_pid}/exe")).unwrap(),
        matching_server_binary.canonicalize().unwrap(),
        "the isolated PTY must use the matching release server"
    );
    let mut connection = Connection::connect(&socket, SESSION_NAME.to_owned())
        .await
        .unwrap();
    let initial = receive_initial_tree(&mut connection, "no-pool runtime initial state").await;
    let mut known = initial.pane_ids_in_tree_order();
    let release_file = temp_root.path().join("release-all-terminal-output");
    let first_pane_output_complete = temp_root.path().join("first-pane-output-complete");
    for number in 0..PANE_COUNT {
        let marker = format!("INNER-PANE-{number:03}");
        let script = if number == 0 {
            format!(
                "while [ ! -e \"$1\" ]; do /bin/sleep 0.02; done; printf '\\n\\n\\n\\n\\n\\n\\n\\nFIRST-CONTENT-000\\n'; /usr/bin/head -c {} /dev/zero | /usr/bin/tr '\\000' '\\033'; printf '\\n\\n%s\\n' \"$2\"; /bin/sleep 0.1; : > \"$3\"; exec /bin/cat",
                LARGE_FIRST_PANE_NOISE_BYTES
            )
        } else {
            format!(
                "while [ ! -e \"$1\" ]; do /bin/sleep 0.02; done; i=0; while [ \"$i\" -lt {BURST_LINES} ]; do printf \"%{BURST_LINE_WIDTH}s\\n\" \"\"; i=$((i + 1)); done; printf \"\\n\\n\\n%s\\n\" \"$2\"; exec /bin/cat"
            )
        };
        let command = format!(
            "/bin/sh -c {} ilium-pane-fixture {} {} {}",
            shell_quote(&script),
            shell_quote(&release_file.to_string_lossy()),
            shell_quote(&marker),
            shell_quote(&first_pane_output_complete.to_string_lossy()),
        );
        connection
            .requests
            .send(ClientRequest::NewPane {
                parent_group: ilium_core::ROOT_ID,
                kind: ilium_ipc::NewPaneKind::Command(command),
                working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
            })
            .await
            .unwrap();
        let tree = receive_tree_snapshot(&mut connection, "create isolated marker pane").await;
        let id = tree
            .pane_ids_in_tree_order()
            .into_iter()
            .find(|id| !known.contains(id))
            .expect("one new isolated pane");
        known.push(id);
        connection
            .requests
            .send(ClientRequest::RenameNode {
                node_id: id,
                title: format!("pool-{number:03}"),
                short_title: None,
                inferred_icon: None,
            })
            .await
            .unwrap();
        receive_tree_snapshot(&mut connection, "name isolated marker pane").await;
    }
    // Keep all PTYs alive but silent until the full 128-pane load exists, then
    // release their first output together before timing the initial reveal pass.
    let burst_released_at = std::time::Instant::now();
    std::fs::write(&release_file, b"release").unwrap();
    assert!(
        wait_until(|| first_pane_output_complete.is_file(), WAIT_TIMEOUT,).await,
        "the large first-pane journal fixture did not finish before selection"
    );
    let mut measurements = Vec::with_capacity(PANE_COUNT * 3);
    for number in 0..PANE_COUNT {
        measurements.push(
            select_and_measure(
                &mut tui,
                number,
                number == 0,
                "burst-forward",
                burst_released_at,
            )
            .await,
        );
    }
    for number in (0..PANE_COUNT).rev() {
        measurements.push(
            select_and_measure(
                &mut tui,
                number,
                true,
                "initialized-reverse",
                burst_released_at,
            )
            .await,
        );
    }
    for number in 0..PANE_COUNT {
        measurements.push(
            select_and_measure(
                &mut tui,
                number,
                false,
                "initialized-forward",
                burst_released_at,
            )
            .await,
        );
    }
    let mut summaries = Vec::new();
    for pass in [
        "burst-forward",
        "initialized-reverse",
        "initialized-forward",
    ] {
        let mut latencies: Vec<f64> = measurements
            .iter()
            .filter(|sample| {
                sample["pass"] == pass && sample["already_visible_before_click"] == false
            })
            .map(|sample| sample["elapsed_ms"].as_f64().unwrap())
            .collect();
        latencies.sort_by(f64::total_cmp);
        assert!(!latencies.is_empty(), "pass must contain real pane changes");
        let count = latencies.len();
        summaries.push(serde_json::json!({
            "pass": pass, "selection_changes": count,
            "p50_ms": latencies[(count - 1) / 2],
            "p95_ms": latencies[(count * 95).div_ceil(100) - 1],
            "max_ms": latencies[count - 1],
        }));
    }
    let latency_accepted = summaries
        .iter()
        .all(|pass| pass["max_ms"].as_f64().unwrap() <= 5000.0);
    let receipt = serde_json::json!({
        "type": "result", "synthetic_terminal_fixtures": true,
        "binary": ilium_binary(), "server_pid": server_pid,
        "isolated_http_port": port, "pane_count": PANE_COUNT,
        "burst_bytes_per_pane": BURST_BYTES_PER_PANE,
        "burst_bytes_total": BURST_BYTES_PER_PANE * PANE_COUNT,
        "pool_default_mib": 0, "measurements": measurements,
        "latency_summaries": summaries, "selection_limit_ms": 5000,
        "latency_accepted": latency_accepted,
        "user_incident_attribution_proven": false,
    });
    if let Some(path) = std::env::var_os("ILIUM_PANE_LATENCY_RECEIPT") {
        std::fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    }
    println!("{receipt}");
    let output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(
        output.status.success(),
        "isolated session cleanup: {output:?}"
    );
    cleanup.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().unwrap();
    }
    assert!(exited, "isolated client exits after its server closes");
    assert!(
        latency_accepted,
        "pane selection exceeds the 5-second qualification limit: {receipt}"
    );
}
