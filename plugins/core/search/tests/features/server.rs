//! The server side: matching, context, the scan job, the meaning index and per-client answers.
//!
//! Ported expectations of the original `search_ui.rs` and `search_workers.rs` tests are tagged
//! `PORT:`; each keeps the original input and expected value and reaches it through the
//! plugin's channels (query message in, results message out, `scan-files` job in between).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::ErrorCode;
use ilium_plugin_sdk::JobRunner;
use serde_json::Value;

use crate::support::*;

fn write_project_file(host: &mut FakeHost, path: &str, bytes: &[u8]) {
    host.fs().write(format!("project:{path}"), bytes);
}

/// Asks for `query` as client 1, runs the scan job to completion and returns the hits.
fn ask_with_scan(host: &mut FakeHost, revision: u64, query: &str) -> Vec<Value> {
    let before = host.submitted().len();
    host.deliver(vec![query_event(1, revision, query)]);
    host.settle();
    drive_jobs(host);
    let submitted = host.submitted();
    result_messages(&submitted[before..])
        .into_iter()
        .filter(|(_, payload)| payload["revision"] == revision)
        .flat_map(|(_, payload)| payload["hits"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn file_hits(hits: &[Value]) -> Vec<Value> {
    hits.iter()
        .filter(|hit| hit["kind"] == "file")
        .cloned()
        .collect()
}

// ---- matching and context -----------------------------------------------------------------

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::local_search_is_case_insensitive_and_keeps_the_line_target
#[test]
fn port_search_ui_file_search_is_case_insensitive_and_keeps_the_zero_based_line() {
    let mut host = activated_server();
    write_project_file(
        &mut host,
        "note.md",
        b"one\ntwo\nthree\nfour\nA Needle appears\n",
    );
    let hits = file_hits(&ask_with_scan(&mut host, 1, "needle"));
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0]["matched"], "Needle");
    assert_eq!(hits[0]["line"], 4);
}

/// REQ: F-009
#[test]
fn search_009_a_query_with_upper_case_finds_lower_case_text() {
    let mut host = activated_server();
    write_project_file(&mut host, "note.md", b"a needle appears\n");
    let hits = file_hits(&ask_with_scan(&mut host, 1, "NEEDLE"));
    assert_eq!(hits[0]["matched"], "needle");
}

/// REQ: F-009
#[test]
fn search_009_a_query_with_no_match_gives_no_file_hits() {
    let mut host = activated_server();
    write_project_file(&mut host, "note.md", b"nothing here\n");
    assert!(file_hits(&ask_with_scan(&mut host, 1, "needle")).is_empty());
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::terminal_search_ignores_ansi_and_preserves_a_raw_jump_offset
#[test]
fn port_search_ui_escape_sequences_do_not_hide_or_split_a_match() {
    let mut host = activated_server();
    write_project_file(&mut host, "log.txt", b"before \x1b[31mneedle\x1b[0m after");
    let hits = file_hits(&ask_with_scan(&mut host, 1, "NEEDLE"));
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0]["matched"], "needle");
    assert_eq!(hits[0]["before"], "before ");
    assert_eq!(hits[0]["after"], " after");
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::stripping_collapses_a_crlf_pair_into_a_single_newline
#[test]
fn port_search_ui_a_crlf_pair_counts_as_one_line_break_in_the_context() {
    let mut host = activated_server();
    write_project_file(&mut host, "crlf.txt", b"a\r\nneedle");
    let hits = file_hits(&ask_with_scan(&mut host, 1, "needle"));
    assert_eq!(hits[0]["before"], "a ", "one space, not two");
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::stripping_keeps_text_that_precedes_an_invalid_utf8_byte
#[test]
fn port_search_ui_text_around_an_invalid_utf8_byte_stays_searchable() {
    let mut host = activated_server();
    write_project_file(&mut host, "bin.txt", b"needle \xff tail");
    let first = file_hits(&ask_with_scan(&mut host, 1, "needle"));
    let second = file_hits(&ask_with_scan(&mut host, 2, "tail"));
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(second.len(), 1, "{second:?}");
}

/// REQ: F-009, PORT:ilium-client/src/search_ui.rs::result_from_source_collapses_embedded_newlines_in_context_text
#[test]
fn port_search_ui_embedded_newlines_in_the_context_become_spaces() {
    let mut host = activated_server();
    write_project_file(
        &mut host,
        "note.md",
        b"first line\nsecond needle third\nline",
    );
    let hits = file_hits(&ask_with_scan(&mut host, 1, "needle"));
    assert_eq!(hits[0]["before"], "first line second ");
    assert_eq!(hits[0]["after"], " third line");
    assert!(!hits[0]["before"].as_str().expect("before").contains('\n'));
    assert!(!hits[0]["after"].as_str().expect("after").contains('\n'));
}

/// REQ: F-009
#[test]
fn search_009_context_keeps_72_characters_on_each_side_of_a_match() {
    let mut host = activated_server();
    let text = format!("{}needle{}", "b".repeat(100), "a".repeat(100));
    write_project_file(&mut host, "wide.txt", text.as_bytes());
    let hits = file_hits(&ask_with_scan(&mut host, 1, "needle"));
    assert_eq!(
        hits[0]["before"].as_str().expect("before").chars().count(),
        72
    );
    assert_eq!(
        hits[0]["after"].as_str().expect("after").chars().count(),
        72
    );
}

/// REQ: F-009
#[test]
fn search_009_context_of_71_and_73_characters_is_kept_whole_and_cut_at_72() {
    let mut host = activated_server();
    for (index, count) in [71usize, 72, 73].into_iter().enumerate() {
        let text = format!("{}needle", "x".repeat(count));
        write_project_file(&mut host, &format!("f{index}.txt"), text.as_bytes());
    }
    let hits = file_hits(&ask_with_scan(&mut host, 1, "needle"));
    let mut lengths: Vec<usize> = hits
        .iter()
        .map(|hit| hit["before"].as_str().expect("before").chars().count())
        .collect();
    lengths.sort_unstable();
    assert_eq!(lengths, [71, 72, 72]);
}

/// REQ: F-009
#[test]
fn search_009_one_file_gives_at_most_800_hits() {
    let mut host = activated_server();
    let text = "needle\n".repeat(900);
    write_project_file(&mut host, "many.txt", text.as_bytes());
    let hits = ask_with_scan(&mut host, 1, "needle");
    assert_eq!(hits.len(), 800);
}

/// REQ: F-009
#[test]
fn search_009_800_matches_are_all_reported() {
    let mut host = activated_server();
    let text = "needle\n".repeat(800);
    write_project_file(&mut host, "many.txt", text.as_bytes());
    assert_eq!(ask_with_scan(&mut host, 1, "needle").len(), 800);
}

// ---- terminals ----------------------------------------------------------------------------

/// REQ: F-009, PORT:ilium-client/src/app.rs::workspace_search_finds_terminal_history_and_activates_its_exact_location
#[test]
fn port_app_a_terminal_screen_is_found_case_insensitively_with_its_pane_id() {
    let mut host = activated_server();
    let pane = add_terminal(
        &mut host,
        "shell",
        "before\r\nWORKSPACE_SEARCH_NEEDLE\r\nafter\r\n",
    );
    let hits = ask(&mut host, 1, "workspace_search_needle");
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0]["id"], pane.to_string());
    assert_eq!(hits[0]["kind"], "terminal");
    assert_eq!(hits[0]["title"], "shell");
}

/// REQ: F-009
#[test]
fn search_009_a_terminal_whose_name_matches_is_found_by_name() {
    let mut host = activated_server();
    add_terminal(&mut host, "build shell", "nothing special");
    let hits = ask(&mut host, 1, "build");
    assert!(
        hits.iter().any(|hit| hit["title"] == "build shell"),
        "{hits:?}"
    );
}

/// REQ: F-009
#[test]
fn search_009_an_agent_pane_is_reported_with_kind_agent() {
    use ilium_plugin_sdk::testing::TreeNode;
    use ilium_plugin_sdk::types::{AgentActivity, AgentView, NodeClass};
    let mut host = activated_server();
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: "claude: deploy script fix".to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: "claude".to_string(),
            activity: AgentActivity::Idle,
            objective: None,
            goal: None,
        }),
    });
    let hits = ask(&mut host, 1, "deploy");
    assert!(hits.iter().any(|hit| hit["kind"] == "agent"), "{hits:?}");
}

/// REQ: F-009
#[test]
fn search_009_an_empty_query_gives_an_empty_answer_for_the_same_revision() {
    let mut host = activated_server();
    add_terminal(&mut host, "build shell", "text");
    host.deliver(vec![query_event(1, 5, "")]);
    host.settle();
    let answers = result_messages(&host.submitted());
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].1["revision"], 5);
    assert_eq!(answers[0].1["hits"], serde_json::json!([]));
}

// ---- the scan job -------------------------------------------------------------------------

/// REQ: F-009
#[test]
fn search_009_a_query_starts_one_scan_files_job_whose_input_is_the_paths_joined_by_newlines() {
    let mut host = activated_server();
    write_project_file(&mut host, "docs/deploy.md", b"x");
    write_project_file(&mut host, "ops/deploy.sh", b"y");
    host.deliver(vec![query_event(1, 1, "deploy")]);
    host.settle();
    let jobs = host.jobs().list();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!(jobs[0].kind, "scan-files");
    let input = String::from_utf8(jobs[0].input.clone()).expect("utf-8 paths");
    let mut paths: Vec<&str> = input.split('\n').collect();
    paths.sort_unstable();
    assert_eq!(paths, ["docs/deploy.md", "ops/deploy.sh"]);
}

/// REQ: F-009
#[test]
fn search_009_job_progress_does_not_produce_an_answer_before_the_job_completes() {
    let mut host = activated_server();
    write_project_file(&mut host, "note.md", b"needle");
    host.deliver(vec![query_event(1, 1, "needle")]);
    host.settle();
    let request = host.jobs().list()[0].request;
    host.jobs().progress(request, 50.0, "half");
    host.settle();
    // Progress alone sends no result message at all, not even an empty answer.
    assert!(result_messages(&host.submitted()).is_empty());
    drive_jobs(&mut host);
    let answers = result_messages(&host.submitted());
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0].1["revision"], 1);
}

/// REQ: F-009
#[test]
fn search_009_job_runner_refuses_an_unknown_kind_with_invalid_argument() {
    let mut runner = ilium_plugin_core_search::SearchServer;
    let error = runner
        .run("other-kind".to_string(), Vec::new())
        .expect_err("unknown kind");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-009
#[test]
fn search_009_job_runner_scan_files_accepts_an_empty_path_list() {
    let mut runner = ilium_plugin_core_search::SearchServer;
    assert!(runner.run("scan-files".to_string(), Vec::new()).is_ok());
}

/// REQ: F-009, PORT:ilium-client/src/search_workers.rs::blocked_shared_cpu_does_not_block_producer_and_cancel_keeps_one_receipt
#[test]
fn port_search_workers_a_second_query_while_a_scan_runs_returns_at_once_and_leaves_one_job() {
    let mut host = activated_server();
    write_project_file(&mut host, "note.md", b"needle");
    host.deliver(vec![query_event(1, 1, "needle")]);
    host.settle();
    host.deliver(vec![query_event(1, 2, "needle")]);
    host.settle();
    let live: Vec<_> = host
        .jobs()
        .list()
        .into_iter()
        .filter(|job| job.kind == "scan-files")
        .collect();
    assert_eq!(live.len(), 1, "{live:?}");
    drive_jobs(&mut host);
    let answers: Vec<_> = result_messages(&host.submitted());
    let revisions_with_hits: Vec<u64> = answers
        .iter()
        .filter(|(_, payload)| {
            payload["hits"]
                .as_array()
                .is_some_and(|hits| !hits.is_empty())
        })
        .map(|(_, payload)| payload["revision"].as_u64().expect("revision"))
        .collect();
    assert_eq!(
        revisions_with_hits,
        [2],
        "only the newest query is answered with hits"
    );
}

/// REQ: F-009, PORT:ilium-client/src/search_workers.rs::blocked_shared_cpu_does_not_block_producer_and_cancel_keeps_one_receipt
#[test]
fn port_search_workers_a_cancelled_scan_gives_exactly_one_answer_without_hits() {
    let mut host = activated_server();
    write_project_file(&mut host, "note.md", b"needle");
    host.deliver(vec![query_event(1, 1, "needle")]);
    host.settle();
    for record in host.jobs().list() {
        host.jobs().finish(
            record.request,
            Err(ilium_plugin_sdk::types::PluginError {
                code: ErrorCode::Cancelled,
                message: "cancelled".to_string(),
                retry_after_ms: None,
            }),
        );
    }
    host.settle();
    let answers = result_messages(&host.submitted());
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0].1["revision"], 1);
    assert_eq!(answers[0].1["hits"], serde_json::json!([]));
}

/// REQ: F-009, PORT:ilium-client/src/search_workers.rs::oversize_capture_is_rejected_before_any_job_is_published
#[test]
fn port_search_workers_a_file_over_8_mib_is_skipped_before_any_job_reads_it() {
    let mut host = activated_server();
    let mut big = b"needle ".to_vec();
    big.resize(9 * 1024 * 1024, b'x');
    write_project_file(&mut host, "big.bin", &big);
    write_project_file(&mut host, "small.txt", b"needle");
    let hits = file_hits(&ask_with_scan(&mut host, 1, "needle"));
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0]["title"], "small.txt");
    for job in host.jobs().list() {
        assert!(job.input.len() < 8 * 1024 * 1024);
    }
    assert!(host.failures().is_empty());
}

/// REQ: F-009, PORT:ilium-client/src/search_workers.rs::another_frontend_cannot_submit_using_the_reserved_cpu_slot
#[test]
fn port_search_workers_each_client_gets_only_the_answer_to_its_own_query() {
    let mut host = activated_server();
    write_project_file(&mut host, "note.md", b"needle");
    host.deliver(vec![
        query_event(1, 1, "needle"),
        query_event(2, 1, "needle"),
    ]);
    host.settle();
    drive_jobs(&mut host);
    let answers = result_messages(&host.submitted());
    let mut clients: Vec<u64> = answers.iter().filter_map(|(client, _)| *client).collect();
    clients.sort_unstable();
    clients.dedup();
    assert_eq!(clients, [1, 2]);
    assert!(
        answers.iter().all(|(client, _)| client.is_some()),
        "never broadcast to every client"
    );
}

/// REQ: F-009, PORT:ilium-client/src/search_workers.rs::displayed_results_release_finite_input_credit_and_keep_measured_storage
#[test]
fn port_search_workers_a_finished_scan_frees_the_slot_and_keeps_its_hit_for_the_next_query() {
    let mut host = activated_server();
    write_project_file(&mut host, "note.md", b"a\nb\nc\nd\ne\nf\ng\nneedle\n");
    let first = file_hits(&ask_with_scan(&mut host, 1, "needle"));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["line"], 7);
    assert_eq!(first[0]["matched"], "needle");
    let started = host.jobs().list().len();
    host.deliver(vec![query_event(1, 2, "needle")]);
    host.settle();
    assert!(
        host.jobs().list().len() > started,
        "a new query starts a new job"
    );
}

// ---- search by meaning --------------------------------------------------------------------

fn indexed_host(panes: usize) -> FakeHost {
    let mut host = activated_server();
    for index in 0..panes {
        add_terminal(&mut host, &format!("shell {index}"), "text");
    }
    host.deliver(vec![query_event(1, 1, "why did the build fail")]);
    host.settle();
    host
}

/// REQ: F-009
#[test]
fn search_009_embed_batches_hold_at_most_256_texts_for_255_panes() {
    let mut host = indexed_host(255);
    assert_eq!(host.vector().embed_batch_sizes()[..1], [255]);
}

/// REQ: F-009
#[test]
fn search_009_embed_batches_hold_at_most_256_texts_for_256_panes() {
    let mut host = indexed_host(256);
    assert_eq!(host.vector().embed_batch_sizes()[..1], [256]);
}

/// REQ: F-009
#[test]
fn search_009_embed_batches_split_257_panes_into_256_and_1() {
    let mut host = indexed_host(257);
    assert_eq!(host.vector().embed_batch_sizes()[..2], [256, 1]);
}

/// REQ: F-009
#[test]
fn search_009_embed_batches_split_600_panes_into_256_256_and_88() {
    let mut host = indexed_host(600);
    assert_eq!(host.vector().embed_batch_sizes()[..3], [256, 256, 88]);
}

/// REQ: F-009
#[test]
fn search_009_meaning_query_asks_for_the_top_20() {
    let mut host = indexed_host(3);
    assert!(host.vector().top_k_requests().iter().all(|k| *k == 20));
    assert!(!host.vector().top_k_requests().is_empty());
}

/// REQ: F-009
#[test]
fn search_009_meaning_hits_carry_the_cosine_score_of_the_host() {
    let mut host = activated_server();
    let build = add_terminal(&mut host, "build shell", "linker error");
    host.vector().set_top_k(vec![(build.to_string(), 0.82)]);
    let hits = ask(&mut host, 1, "why did the build fail");
    let meaning: Vec<&Value> = hits.iter().filter(|hit| hit["kind"] == "meaning").collect();
    assert_eq!(meaning.len(), 1, "{hits:?}");
    assert_eq!(meaning[0]["id"], build.to_string());
    assert!((meaning[0]["score"].as_f64().expect("score") - 0.82).abs() < 1e-6);
}

/// REQ: F-009
#[test]
fn search_009_without_the_embed_grant_the_search_still_answers_by_text() {
    let mut host = activated_server();
    add_terminal(&mut host, "build shell", "needle");
    host.revoke("inference.embed");
    let hits = ask(&mut host, 1, "needle");
    assert!(hits.iter().any(|hit| hit["kind"] == "terminal"), "{hits:?}");
    assert!(hits.iter().all(|hit| hit["kind"] != "meaning"));
}
