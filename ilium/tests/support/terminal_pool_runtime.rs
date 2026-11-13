//! Explicit release qualification, using real server delivery and PTY rendering.
use super::*;

// Qualification covers the user's 100+ pane load. PTY sessions are charged
// to their own per-pane quota (bounded only by the owned-worker slot table),
// not to the shared replay/output pool. Never lower this count to make the
// gate pass: a lowered gate hid the capacity cut that dropped 22 panes on
// 2026-10-09. The product target is at least 200 panes.
const PANE_COUNT: usize = 200;
// 16384 lines of 257 bytes is about 4 MiB of retained output per pane, the
// scale of a long agent transcript, not a token burst.
const BURST_LINES: usize = 16384;
const BURST_LINE_WIDTH: usize = 256;
const BURST_BYTES_PER_PANE: usize = BURST_LINES * (BURST_LINE_WIDTH + 1);
// The first HEAVY_PANE_COUNT panes retain 128000 lines of 257 bytes, about
// 32 MiB, which is the per-pane retained window of the PTY journal
// (`MAX_RETAINED_BYTES` in ilium-pty/src/session.rs). Only a finite pool
// evicts hidden panes, so these panes are what a re-selection must replay.
const HEAVY_PANE_COUNT: usize = 8;
const HEAVY_BURST_LINES: usize = 128_000;
const HEAVY_BURST_BYTES_PER_PANE: usize = HEAVY_BURST_LINES * (BURST_LINE_WIDTH + 1);
// The noise ends with a complete CSI (ESC [ 0 m) so the last ESC cannot swallow
// the first byte of the marker that follows.
const LARGE_FIRST_PANE_NOISE_BYTES: usize = 24 * 1024 * 1024;
const SELECTION_LIMIT: Duration = Duration::from_secs(5);
// Bounded window that records how late a missed marker arrived; it never counts as a pass.
const LATE_DISPLAY_DIAGNOSTIC_WINDOW: Duration = Duration::from_secs(20);

/// Each selection is appended as one JSON line beside the receipt, so a run that
/// stops early still shows every measurement it completed.
fn progress_receipt_path() -> Option<std::path::PathBuf> {
    let mut path = std::env::var_os("ILIUM_PANE_LATENCY_RECEIPT")?;
    path.push(".progress.jsonl");
    Some(std::path::PathBuf::from(path))
}
fn append_progress_sample(path: Option<&std::path::Path>, sample: &serde_json::Value) {
    let Some(path) = path else {
        return;
    };
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file, "{sample}").unwrap();
    file.sync_data().unwrap();
}
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
    // Sidebar hover diagnostics can cover the next pane's title. Keep the
    // pointer in the content area while locating rows, as a keyboard user can.
    tui.write(&sgr_mouse_move(100, 40)).unwrap();
    assert!(
        wait_until(
            || !tui.screen_text().contains("Plain terminal · F9 details"),
            WAIT_TIMEOUT,
        )
        .await,
        "sidebar hover diagnostic did not close before pane navigation"
    );
    // The sidebar holds PANE_COUNT rows plus group rows, so one pass in a
    // single direction can miss a title that sits above the viewport after a
    // selection moved the list. Search both directions with a budget that
    // covers the whole tree twice; the bound is a harness limit, not a timing one.
    let mut row = None;
    for step in 0..(PANE_COUNT * 4) {
        row = tui.with_screen(|screen| {
            rows_containing_before_column(screen, &title, TREE_TEXT_COLUMNS)
                .into_iter()
                .next()
        });
        if row.is_some() {
            break;
        }
        let scroll_up = backwards != (step / PANE_COUNT % 2 == 1);
        tui.write(&sgr_mouse_down(if scroll_up { 64 } else { 65 }, 8, 15))
            .unwrap_or_else(|error| panic!("scroll isolated tree: {error:?}"));
        tui.write(&sgr_mouse_move(100, 40)).unwrap();
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
    let buffered_tag = format!("BUFFERED-LINE-{number:03}");
    let first_content_ms = if number == 0 && pass == "burst-forward" {
        // The current screen (the final marker) may appear before the first
        // line: the replay's tail is previewed first. Either counts as content
        // within the same selection limit.
        let appeared = wait_until(
            || inner_marker_visible(tui, "FIRST-CONTENT-000") || inner_marker_visible(tui, &marker),
            SELECTION_LIMIT.saturating_sub(started.elapsed()),
        )
        .await;
        assert!(
            appeared,
            "the selected large-output pane showed no content within the selection limit"
        );
        Some(started.elapsed().as_secs_f64() * 1000.0)
    } else if number != 0 {
        // Every other pane shows buffered transcript lines before, or together
        // with, its final marker; measure when the first pane content appears.
        let appeared = wait_until(
            || inner_marker_visible(tui, &buffered_tag) || inner_marker_visible(tui, &marker),
            SELECTION_LIMIT.saturating_sub(started.elapsed()),
        )
        .await;
        appeared.then(|| started.elapsed().as_secs_f64() * 1000.0)
    } else {
        None
    };
    let displayed = wait_until(
        || inner_marker_visible(tui, &marker),
        SELECTION_LIMIT.saturating_sub(started.elapsed()),
    )
    .await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    // When the 5-second check misses, keep watching for the marker for a
    // bounded window and record when it arrived. The receipt then shows how
    // late a failing pane was. The pass criterion is still the 5-second limit
    // asserted below; this window never turns a miss into a pass.
    let late_display_ms = if displayed {
        None
    } else {
        let appeared = wait_until(
            || inner_marker_visible(tui, &marker),
            LATE_DISPLAY_DIAGNOSTIC_WINDOW,
        )
        .await;
        appeared.then(|| started.elapsed().as_secs_f64() * 1000.0)
    };
    let result = serde_json::json!({
        "type": "measurement", "pass": pass, "pane": number,
        "marker": marker, "displayed": displayed, "elapsed_ms": elapsed_ms,
        "late_display_ms": late_display_ms,
        "first_content_ms": first_content_ms,
        "burst_age_ms": burst_released_at.elapsed().as_secs_f64() * 1000.0,
        "already_visible_before_click": already_visible,
        "screen": tui.screen_text(),
    });
    println!("{result}");
    if let Some(path) = std::env::var_os("ILIUM_PANE_LATENCY_RECEIPT") {
        // Incremental JSONL so an interrupted run still leaves evidence.
        let mut jsonl = path;
        jsonl.push(".jsonl");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(jsonl)
        {
            let _ = std::io::Write::write_all(&mut file, format!("{result}\n").as_bytes());
        }
    }
    assert!(displayed, "inner content absent for {title}: {result}");
    assert!(
        elapsed_ms <= SELECTION_LIMIT.as_secs_f64() * 1000.0,
        "inner content for {title} exceeded the per-selection 5-second limit: {result}"
    );
    result
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "explicit 200-pane release runtime qualification"]
async fn no_pool_release_renders_200_panes_and_measures_click_latency() {
    run_click_latency_gate(0).await;
}

// Finite pool: hidden panes lose their parser engines under pressure, so a
// selection after the 200-pane load must rebuild from retained history. This
// is the path a user with a finite `engine_memory_budget_mib` takes; the
// minimum valid budget (256 MiB) forces eviction across the 200 panes.
#[cfg(unix)]
#[tokio::test]
#[ignore = "explicit 200-pane release runtime qualification with a finite 256 MiB parser pool"]
async fn finite_pool_256_mib_renders_200_panes_and_measures_click_latency() {
    run_click_latency_gate(256).await;
}

#[cfg(unix)]
async fn run_click_latency_gate(engine_memory_budget_mib: u32) {
    if cfg!(debug_assertions) {
        panic!("release qualification must launch the optimized client binary");
    }
    let client_binary = std::path::PathBuf::from(ilium_binary());
    let matching_server_binary = client_binary.with_file_name("ilium-server");
    let matching_server_binary = if matching_server_binary.is_file() {
        matching_server_binary
    } else {
        // `cargo test --bin ilium-server` builds a test harness, not the
        // executable the client launches. Build the release server under a
        // separate target directory so this nested Cargo invocation does not
        // contend with the outer test process's target lock.
        let workspace_manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("ilium package is inside the workspace")
            .join("Cargo.toml");
        let target_root = client_binary
            .parent()
            .and_then(std::path::Path::parent)
            .expect("release client binary is inside target/release");
        let server_target = target_root.join("blank-pane-server");
        let status = std::process::Command::new("cargo")
            .args(["build", "--release", "--manifest-path"])
            .arg(workspace_manifest)
            .args([
                "-p",
                "ilium-server",
                "--bin",
                "ilium-server",
                "--target-dir",
            ])
            .arg(&server_target)
            .status()
            .expect("start the matching release server build on the remote test host");
        assert!(
            status.success(),
            "matching release server build failed: {status}"
        );
        server_target.join("release/ilium-server")
    };
    assert!(
        matching_server_binary.is_file(),
        "release qualification needs the matching server executable: {}",
        matching_server_binary.display()
    );
    let temp_root = tempfile::tempdir().unwrap();
    let xdg = IsolatedXdgDirs::under(temp_root.path()).unwrap();
    seed_keyboard_config(&xdg);
    let project_dir = temp_root.path().join("no-pool-runtime");
    std::fs::create_dir_all(&project_dir).unwrap();
    seed_project_config(&xdg, &project_dir);
    let config_path = xdg.ilium_config_dir.join("config.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    if engine_memory_budget_mib == 0 {
        assert_eq!(
            ilium_client::config::load(&xdg.ilium_config_dir)
                .unwrap()
                .terminal
                .engine_memory_budget_mib,
            0,
            "qualification must exercise the omitted-setting no-pool default"
        );
    }
    let config = if engine_memory_budget_mib == 0 {
        config
    } else {
        // Insert into an existing [terminal] table when one was saved, so the
        // TOML never gains a duplicate table; the load check below proves it.
        let budget_line = format!("engine_memory_budget_mib = {engine_memory_budget_mib}");
        if config.contains("[terminal]\n") {
            config.replacen("[terminal]\n", &format!("[terminal]\n{budget_line}\n"), 1)
        } else {
            format!("{config}\n[terminal]\n{budget_line}\n")
        }
    };
    std::fs::write(&config_path, &config).unwrap();
    if engine_memory_budget_mib != 0 {
        std::fs::write(&config_path, &config).unwrap();
        assert_eq!(
            ilium_client::config::load(&xdg.ilium_config_dir)
                .unwrap()
                .terminal
                .engine_memory_budget_mib,
            engine_memory_budget_mib,
            "finite pool budget must load before the client starts"
        );
    }
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 8872);
    drop(listener);
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
    let mut server_search_paths = vec![matching_server_binary
        .parent()
        .expect("matching release server has a parent directory")
        .to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        server_search_paths.extend(std::env::split_paths(&path));
    }
    let server_search_path =
        std::env::join_paths(server_search_paths).expect("matching server path must be valid");
    let command = command.env("PATH", server_search_path.to_string_lossy().into_owned());
    let mut tui = PtySession::spawn(command).unwrap();
    let client_started =
        wait_until(|| tui.screen_text().contains(PROJECT_NAME), WAIT_TIMEOUT).await;
    assert!(
        client_started,
        "isolated release client did not render the project; screen: {:?}",
        tui.screen_text()
    );
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
        // Heavy panes retain the full per-pane window, so a later selection
        // must replay that much history through the serial parser.
        let burst_lines = if number < HEAVY_PANE_COUNT {
            HEAVY_BURST_LINES
        } else {
            BURST_LINES
        };
        let script = if number == 0 {
            // CAN terminates the deliberately unfinished escape run. A newline
            // alone leaves ESC pending and consumes the marker's initial `I`.
            format!(
                "while [ ! -e \"$1\" ]; do /bin/sleep 0.02; done; printf '\\n\\n\\n\\n\\n\\n\\n\\nFIRST-CONTENT-000\\n'; /usr/bin/head -c {} /dev/zero | /usr/bin/tr '\\000' '\\033'; printf '\\030\\n\\n%s\\n' \"$2\"; /bin/sleep 0.1; : > \"$3\"; exec /bin/cat",
                LARGE_FIRST_PANE_NOISE_BYTES
            )
        } else {
            format!(
                "while [ ! -e \"$1\" ]; do /bin/sleep 0.02; done; /usr/bin/yes \"$4 $(printf '%{BURST_LINE_WIDTH}s' '')\" | /usr/bin/head -n {burst_lines}; printf \"\\n\\n\\n%s\\n\" \"$2\"; exec /bin/cat"
            )
        };
        let command = format!(
            "/bin/sh -c {} ilium-pane-fixture {} {} {} {}",
            shell_quote(&script),
            shell_quote(&release_file.to_string_lossy()),
            shell_quote(&marker),
            shell_quote(&first_pane_output_complete.to_string_lossy()),
            shell_quote(&format!("BUFFERED-LINE-{number:03}")),
        );
        connection
            .requests
            .send(ClientRequest::NewPane {
                request_id: 1,
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
    // Keep all PTYs alive but silent until the full 200-pane load exists, then
    // release their first output together before timing the initial reveal pass.
    let burst_released_at = std::time::Instant::now();
    std::fs::write(&release_file, b"release").unwrap();
    assert!(
        wait_until(|| first_pane_output_complete.is_file(), WAIT_TIMEOUT,).await,
        "the large first-pane journal fixture did not finish before selection"
    );
    let mut measurements = Vec::with_capacity(PANE_COUNT * 3);
    let progress_path = progress_receipt_path();
    for number in 0..PANE_COUNT {
        let sample = select_and_measure(
            &mut tui,
            number,
            number == 0,
            "burst-forward",
            burst_released_at,
        )
        .await;
        append_progress_sample(progress_path.as_deref(), &sample);
        measurements.push(sample);
    }
    for number in (0..PANE_COUNT).rev() {
        let sample = select_and_measure(
            &mut tui,
            number,
            true,
            "initialized-reverse",
            burst_released_at,
        )
        .await;
        append_progress_sample(progress_path.as_deref(), &sample);
        measurements.push(sample);
    }
    for number in 0..PANE_COUNT {
        let sample = select_and_measure(
            &mut tui,
            number,
            false,
            "initialized-forward",
            burst_released_at,
        )
        .await;
        append_progress_sample(progress_path.as_deref(), &sample);
        measurements.push(sample);
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
        "heavy_pane_count": HEAVY_PANE_COUNT,
        "heavy_burst_bytes_per_pane": HEAVY_BURST_BYTES_PER_PANE,
        "burst_bytes_total": HEAVY_PANE_COUNT * HEAVY_BURST_BYTES_PER_PANE
            + (PANE_COUNT - HEAVY_PANE_COUNT) * BURST_BYTES_PER_PANE,
        "engine_memory_budget_mib": engine_memory_budget_mib,
        "measurements": measurements,
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

/// Fresh creation is distinct from selecting an existing, fully initialized
/// pane. The keyboard action is shared with the New Terminal toolbar button.
/// Fixtures are ordinary absolute-path scripts, never real agent installers.
#[cfg(unix)]
#[tokio::test]
#[ignore = "explicit 128-pane fresh-start release runtime qualification"]
async fn fresh_terminal_startup_with_128_idle_and_busy_panes() {
    if cfg!(debug_assertions) {
        panic!("startup qualification requires release binaries");
    }
    let mut accepted = true;
    for busy in [false, true] {
        accepted &= qualify_fresh_terminal_startup(busy).await;
    }
    assert!(
        accepted,
        "fresh terminal startup qualification failed; see numeric receipts"
    );
}

#[cfg(unix)]
async fn qualify_fresh_terminal_startup(busy: bool) -> bool {
    const TRIALS: usize = 32;
    const STARTUP_LIMIT: Duration = Duration::from_secs(2);
    let workload = if busy { "continuously-busy" } else { "idle" };
    let client_binary = std::path::PathBuf::from(ilium_binary());
    let server_binary = client_binary.with_file_name("ilium-server");
    assert!(
        server_binary.is_file(),
        "prebuild the matching release server"
    );
    let temp_root = tempfile::tempdir().unwrap();
    let xdg = IsolatedXdgDirs::under(temp_root.path()).unwrap();
    seed_keyboard_config(&xdg);
    let project_dir = temp_root.path().join("startup-runtime");
    std::fs::create_dir_all(&project_dir).unwrap();
    seed_project_config(&xdg, &project_dir);
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_ne!(port, 8872);
    drop(listener);
    let config_path = xdg.ilium_config_dir.join("config.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        config_path,
        format!("{config}\n[api]\nport = {port}\n[debug]\nfile_logging_enabled = true\n"),
    )
    .unwrap();
    let marker_path = temp_root.path().join("next-startup-marker");
    let fake_shell = temp_root.path().join("startup-fixture-shell");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = '-c' ]; then exec /bin/sh \"$@\"; fi\nmarker=$(/bin/cat {}); printf '\\n\\n\\n%s\\n' \"$marker\"; exec /bin/cat\n",
        shell_quote(&marker_path.to_string_lossy()),
    );
    std::fs::write(&fake_shell, script).unwrap();
    assert!(std::process::Command::new("chmod")
        .arg("700")
        .arg(&fake_shell)
        .status()
        .unwrap()
        .success());
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
    .arg(project_dir.to_string_lossy().to_string())
    .env("SHELL", fake_shell.to_string_lossy().into_owned())
    .env("RUST_LOG", "ilium_startup=debug");
    let command = xdg
        .as_pairs()
        .into_iter()
        .fold(command, |command, (key, value)| {
            command.env(key, value.to_string_lossy().to_string())
        });
    let mut paths = vec![server_binary.parent().unwrap().to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    let command = command.env(
        "PATH",
        std::env::join_paths(paths)
            .unwrap()
            .to_string_lossy()
            .into_owned(),
    );
    let mut tui = PtySession::spawn(command).unwrap();
    assert!(wait_until(|| tui.screen_text().contains(PROJECT_NAME), WAIT_TIMEOUT).await);
    let (socket, _server_pid) = isolated_server_identity(&xdg, &project_dir).await;
    let mut connection = Connection::connect(&socket, SESSION_NAME.to_owned())
        .await
        .unwrap();
    let initial = receive_initial_tree(&mut connection, "fresh startup initial tree").await;
    assert_eq!(
        initial.panes().count(),
        0,
        "fixture starts without preexisting panes"
    );
    // Exactly 127 existing PTYs plus the newly created PTY = 128 at every
    // measured launch. Busy sources remain active throughout all trials.
    let mut background_ids = Vec::with_capacity(PANE_COUNT - 1);
    for number in 0..PANE_COUNT - 1 {
        let source = if busy {
            "while :; do printf '%256s\\n' background; /bin/sleep 0.02; done"
        } else {
            "exec /bin/cat"
        };
        let command = format!("/bin/sh -c {}", shell_quote(source));
        connection
            .requests
            .send(ClientRequest::NewPane {
                request_id: number as u64 + 1,
                parent_group: ilium_core::ROOT_ID,
                kind: ilium_ipc::NewPaneKind::Command(command),
                working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
            })
            .await
            .unwrap();
        let Some(tree) = startup_tree_with_count(&mut connection, number + 1).await else {
            println!(
                "{}",
                serde_json::json!({
                    "type":"error", "workload":workload, "phase":"background_fixture_creation",
                    "completed_background_panes":number, "required_background_panes":PANE_COUNT - 1,
                    "unattempted_trials":TRIALS, "error":"fixture creation deadline exceeded",
                })
            );
            let output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
            cleanup.already_cleaned_up = output.status.success();
            if !wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await {
                tui.kill().unwrap();
            }
            return false;
        };
        background_ids = tree.pane_ids_in_tree_order();
    }
    let mut samples = Vec::with_capacity(TRIALS);
    for trial in 0..TRIALS {
        let marker = format!("FRESH-{workload}-{trial:03}");
        std::fs::write(&marker_path, &marker).unwrap();
        assert!(!inner_marker_visible(&tui, &marker));
        let key_unix_us = startup_wall_us();
        let started = std::time::Instant::now();
        tui.write(b"\x02c").unwrap();
        let displayed = wait_until(|| inner_marker_visible(&tui, &marker), STARTUP_LIMIT).await;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        samples.push(serde_json::json!({
            "trial": trial, "displayed": displayed, "elapsed_ms": elapsed_ms,
            "key_unix_us": key_unix_us, "observed_unix_us": startup_wall_us(),
            "censored": !displayed,
        }));
        if displayed {
            let echo = format!("ECHO-{workload}-{trial:03}");
            let echo_started = std::time::Instant::now();
            tui.write(format!("{echo}\n").as_bytes()).unwrap();
            let echo_displayed =
                wait_until(|| inner_marker_visible(&tui, &echo), STARTUP_LIMIT).await;
            samples.last_mut().unwrap()["echo_ms"] =
                serde_json::json!(echo_started.elapsed().as_secs_f64() * 1000.0);
            samples.last_mut().unwrap()["echo_failed"] = serde_json::json!(!echo_displayed);
            samples.last_mut().unwrap()["final_marker_visible"] =
                serde_json::json!(inner_marker_visible(&tui, &marker));
        }
        // Use authoritative state only after timing presentation. A control
        // connection must not replace the real keyboard creation under test.
        let Some(tree) = startup_tree_with_count(&mut connection, PANE_COUNT).await else {
            let sample = samples.last_mut().unwrap();
            sample["displayed"] = serde_json::json!(false);
            sample["censored"] = serde_json::json!(true);
            sample["error"] = serde_json::json!("creation tree absent before bounded deadline");
            break;
        };
        let pane_id = tree
            .pane_ids_in_tree_order()
            .into_iter()
            .find(|id| !background_ids.contains(id))
            .expect("one newly created pane");
        samples.last_mut().unwrap()["pane_id"] = serde_json::json!(pane_id.0);
        connection
            .requests
            .send(ClientRequest::ClosePane { pane_id })
            .await
            .unwrap();
        if startup_tree_with_count(&mut connection, PANE_COUNT - 1)
            .await
            .is_none()
        {
            samples.last_mut().unwrap()["error"] =
                serde_json::json!("owned pane close absent before bounded deadline");
            break;
        }
        println!(
            "{}",
            serde_json::json!({"type":"measurement", "workload":workload, "pane_count":PANE_COUNT, "sample":samples.last().unwrap()})
        );
    }
    let mut latencies: Vec<_> = samples
        .iter()
        .filter(|s| s["displayed"] == true)
        .map(|s| s["elapsed_ms"].as_f64().unwrap())
        .collect();
    latencies.sort_by(f64::total_cmp);
    let percentile = |percent: usize| {
        latencies
            .get((latencies.len() * percent).div_ceil(100).saturating_sub(1))
            .copied()
    };
    let failures = samples
        .iter()
        .filter(|s| {
            s["displayed"] != true
                || s["echo_failed"] == true
                || s["final_marker_visible"] == false
                || s.get("error").is_some()
        })
        .count();
    let censored = samples.iter().filter(|s| s["censored"] == true).count();
    let output = run_one_shot(&xdg, &project_dir, &["kill-session", SESSION_NAME]).await;
    assert!(
        output.status.success(),
        "isolated startup session cleanup failed"
    );
    cleanup.already_cleaned_up = true;
    let exited = wait_until(|| tui.has_exited(), WAIT_TIMEOUT).await;
    if !exited {
        tui.kill().unwrap();
    }
    assert!(exited);
    // Both isolated writers have exited before reading final numeric logs.
    let logs = walk_log_files(&xdg.debug_log_dir);
    let stage_measurements = correlate_startup_stages(&logs, &samples);
    let incomplete_stage_trials = stage_measurements
        .iter()
        .filter(|trial| trial["complete"] != true)
        .count();
    let receipt = serde_json::json!({
        "type":"result", "synthetic_terminal_fixtures":true,
        "measurement":"keyboard_new_terminal_to_inner_marker_presented", "workload":workload,
        "pane_count":PANE_COUNT, "trials":samples.len(), "requested_trials":TRIALS, "failures":failures, "censored_trials":censored,
        "p50_ms":percentile(50), "p95_ms":percentile(95), "p99_ms":percentile(99),
        "max_ms":latencies.last(), "limit_ms":STARTUP_LIMIT.as_millis(),
        "samples":samples, "stage_measurements":stage_measurements,
        "incomplete_stage_trials":incomplete_stage_trials, "unattempted_trials":TRIALS - samples.len(),
        "cross_process_clock":"unix_us; same-process durations use monotonic_us",
        "user_incident_attribution_proven":false,
    });
    println!("{receipt}");
    if let Some(path) = std::env::var_os("ILIUM_STARTUP_LATENCY_RECEIPT") {
        let path = std::path::PathBuf::from(path).with_extension(format!("{workload}.json"));
        std::fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    }
    failures == 0 && samples.len() == TRIALS && incomplete_stage_trials == 0
}

fn startup_wall_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros()
        .min(u128::from(u64::MAX)) as u64
}

async fn startup_tree_with_count(connection: &mut Connection, count: usize) -> Option<Tree> {
    tokio::time::timeout(WAIT_TIMEOUT, async {
        while let Some(event) = connection.events.recv().await {
            let (event, _retention) = event.into_parts();
            if let ServerEvent::TreeSnapshot(tree) = event {
                if tree.panes().count() == count {
                    return Some(tree);
                }
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

#[derive(Clone, Debug)]
struct StartupStage {
    stage: String,
    request_id: u64,
    pane_id: u64,
    child_process_id: u64,
    process_id: u64,
    sequence: u64,
    unix_us: u64,
    monotonic_us: u64,
}

fn parse_startup_stage(line: &str) -> Option<StartupStage> {
    if !line.contains("startup timing") {
        return None;
    }
    let fields: std::collections::HashMap<_, _> = line
        .split_whitespace()
        .filter_map(|word| word.split_once('='))
        .map(|(key, value)| (key, value.trim_matches('"')))
        .collect();
    let number = |key| fields.get(key)?.parse::<u64>().ok();
    Some(StartupStage {
        stage: fields.get("stage")?.to_string(),
        request_id: number("request_id")?,
        pane_id: number("pane_id")?,
        child_process_id: number("child_process_id")?,
        process_id: number("process_id")?,
        sequence: number("sequence")?,
        unix_us: number("unix_us")?,
        monotonic_us: number("monotonic_us")?,
    })
}

fn correlate_startup_stages(
    logs: &[(PathBuf, String)],
    samples: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    // Scan without retaining background frame traffic. At most ten numeric
    // records per measured trial survive, regardless of workload frame rate.
    samples.iter().map(|sample| {
        let pane = sample["pane_id"].as_u64().unwrap_or(0);
        let key_us = sample["key_unix_us"].as_u64().unwrap();
        let records = || logs.iter().flat_map(|(_, text)| text.lines()).filter_map(parse_startup_stage);
        let first = |stage: &str, predicate: &dyn Fn(&StartupStage) -> bool| {
            records().filter(|record| record.stage == stage && predicate(record))
                .min_by_key(|record| record.unix_us)
        };
        let finish = first("creation_finished", &|r| pane != 0 && r.pane_id == pane && r.unix_us >= key_us);
        let receipt = first("creation_receipt", &|r| pane != 0 && r.pane_id == pane && r.unix_us >= key_us);
        let ready = first("pty_ready", &|r| pane != 0 && r.pane_id == pane && r.unix_us >= key_us);
        let launch = ready.as_ref().and_then(|ready| first("process_launched", &|r| {
            r.process_id == ready.process_id && r.child_process_id == ready.child_process_id
                && r.unix_us >= key_us
        }));
        let spawn = first("spawn_begin", &|r| pane != 0 && r.pane_id == pane && r.unix_us >= key_us);
        let intent = receipt.as_ref().and_then(|receipt| first("creation_intent", &|r| {
            r.process_id == receipt.process_id && r.request_id == receipt.request_id
                && r.unix_us >= key_us && r.unix_us <= receipt.unix_us
        }));
        let received = finish.as_ref().zip(spawn.as_ref()).and_then(|(finish, spawn)| {
            first("request_received", &|r| r.process_id == finish.process_id
                && r.request_id == finish.request_id && r.unix_us >= key_us
                && r.unix_us <= spawn.unix_us)
        });
        let raw = launch.as_ref().and_then(|launch| first("first_pty_read", &|r| {
            r.process_id == launch.process_id && r.child_process_id == launch.child_process_id
                && r.unix_us >= key_us
        }));
        let parsed = launch.as_ref().and_then(|launch| first("first_parsed_output", &|r| {
            r.process_id == launch.process_id && r.child_process_id == launch.child_process_id
                && r.unix_us >= key_us
        }));
        let emitted = receipt.as_ref().zip(parsed.as_ref()).and_then(|(receipt, parsed)| {
            first("frame_emitted", &|r| r.process_id == receipt.process_id && r.pane_id == pane
                && r.sequence >= parsed.sequence && r.unix_us >= parsed.unix_us)
        });
        let chain = [&intent, &received, &spawn, &launch, &ready, &raw, &parsed, &finish, &receipt, &emitted];
        let missing: Vec<_> = ["creation_intent", "request_received", "spawn_begin", "process_launched",
            "pty_ready", "first_pty_read", "first_parsed_output", "creation_finished", "creation_receipt", "frame_emitted"]
            .into_iter().zip(chain).filter_map(|(name, record)| record.is_none().then_some(name)).collect();
        let mono_ms = |from: &Option<StartupStage>, to: &Option<StartupStage>| {
            from.as_ref().zip(to.as_ref()).and_then(|(a,b)| (a.process_id == b.process_id)
                .then_some((i128::from(b.monotonic_us) - i128::from(a.monotonic_us)) as f64 / 1000.0))
        };
        let wall_ms = |from: &Option<StartupStage>, to: &Option<StartupStage>| {
            from.as_ref().zip(to.as_ref()).map(|(a,b)| (i128::from(b.unix_us) - i128::from(a.unix_us)) as f64 / 1000.0)
        };
        let numeric_records: Vec<_> = chain.into_iter().filter_map(|r| r.as_ref()).map(|r| serde_json::json!({
            "stage":r.stage, "request_id":r.request_id, "pane_id":r.pane_id,
            "child_process_id":r.child_process_id,"process_id":r.process_id,
            "sequence":r.sequence,"unix_us":r.unix_us,"monotonic_us":r.monotonic_us,
        })).collect();
        serde_json::json!({
            "trial":sample["trial"], "pane_id":pane, "complete":missing.is_empty(), "missing_stages":missing,
            "key_to_intent_wall_ms":intent.as_ref().map(|r| (i128::from(r.unix_us)-i128::from(key_us)) as f64 / 1000.0),
            "intent_to_request_wall_ms":wall_ms(&intent,&received),
            "request_to_spawn_begin_ms":mono_ms(&received,&spawn),
            "spawn_begin_to_process_launch_ms":mono_ms(&spawn,&launch),
            "process_launch_to_first_read_ms":mono_ms(&launch,&raw),
            "process_launch_to_pty_ready_ms":mono_ms(&launch,&ready),
            "spawn_begin_to_first_read_ms":mono_ms(&spawn,&raw),
            "first_read_to_first_parse_ms":mono_ms(&raw,&parsed),
            "first_parse_to_frame_wall_ms":wall_ms(&parsed,&emitted),
            "intent_to_frame_ms":mono_ms(&intent,&emitted),
            "key_to_marker_ms":sample["elapsed_ms"], "records":numeric_records,
        })
    }).collect()
}

#[test]
fn startup_numeric_stage_parser_rejects_unrelated_or_incomplete_logs() {
    assert!(parse_startup_stage("terminal content request_id=1").is_none());
    assert!(parse_startup_stage("startup timing stage=frame_emitted").is_none());
    let record = parse_startup_stage("DEBUG ilium_startup: startup timing stage=\"first_pty_read\" request_id=0 pane_id=0 child_process_id=22 sequence=0 process_id=33 monotonic_us=44 unix_us=55").unwrap();
    assert_eq!(record.stage, "first_pty_read");
    assert_eq!(record.child_process_id, 22);
    assert_eq!(record.monotonic_us, 44);
}

#[test]
fn startup_stage_correlation_fences_requests_and_ignores_blank_and_background_frames() {
    let line = |stage: &str,
                request: u64,
                pane: u64,
                child: u64,
                process: u64,
                sequence: u64,
                time: u64| {
        format!("startup timing stage={stage} request_id={request} pane_id={pane} child_process_id={child} process_id={process} sequence={sequence} monotonic_us={time} unix_us={time}\n")
    };
    let mut log = String::new();
    for (stage, request, pane, child, process, sequence, time) in [
        ("request_received", 22, 0, 0, 7, 0, 900), // same request ID, older control request
        ("creation_intent", 22, 0, 0, 8, 0, 1010),
        ("request_received", 22, 0, 0, 7, 0, 1020),
        ("spawn_begin", 0, 9, 0, 7, 0, 1030),
        ("process_launched", 0, 0, 44, 7, 0, 1040),
        ("pty_ready", 0, 9, 44, 7, 0, 1065),
        ("first_pty_read", 0, 0, 44, 7, 0, 1050),
        ("first_parsed_output", 0, 0, 44, 7, 1, 1060),
        ("creation_finished", 22, 9, 0, 7, 0, 1070),
        ("creation_receipt", 22, 9, 0, 8, 0, 1080),
        ("frame_emitted", 0, 9, 0, 8, 0, 1085), // blank initial state is not first output
        ("frame_emitted", 0, 99, 0, 8, 1, 1086), // unrelated focused pane
        ("frame_emitted", 0, 9, 0, 8, 1, 1090),
    ] {
        log.push_str(&line(stage, request, pane, child, process, sequence, time));
    }
    let sample = serde_json::json!({"trial":0,"pane_id":9,"key_unix_us":1000,"elapsed_ms":0.1});
    let result = correlate_startup_stages(&[(PathBuf::from("fixture"), log)], &[sample]);
    assert_eq!(result[0]["complete"], true);
    assert_eq!(result[0]["request_to_spawn_begin_ms"], 0.01);
    assert_eq!(result[0]["first_parse_to_frame_wall_ms"], 0.03);
    assert_eq!(result[0]["records"].as_array().unwrap().len(), 10);
}
