//! Kernel-side twins for the fleet plugin's ported originals (core-plugin-fleet.md,
//! capability-panes-create.md, capability-terminal-write.md, background-jobs.md). The kernel keeps
//! the delivery readiness of `initial-input` and the admission of the work behind the HTTP route;
//! the twins drive them through the public `ServerKernel` test world.

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use ilium_plugin_api::generated::types::{CreatePaneSpec, Intent, NodeId};
use ilium_plugin_api::kernel::KernelFacade;
use serde_json::json;

use crate::common::*;

/// A terminal pane running `command` that is created with `initial_input`.
fn terminal_with_initial_input(kernel: &ilium_server::plugin_host::ServerKernel, command: &[&str], initial_input: &str) -> NodeId {
    created(apply(
        kernel,
        Intent::CreatePane(CreatePaneSpec {
            parent: None,
            title: "initial-input".to_owned(),
            content_kind: "ilium.terminal".to_owned(),
            cwd: None,
            command: Some(command.iter().map(|part| (*part).to_owned()).collect()),
            initial_input: Some(initial_input.to_owned()),
        }),
    ))
}

/// REQ: PORT:ilium-server/src/agent_delivery.rs::initial_prompt_waits_for_owned_process_even_when_composer_is_visible
#[test]
fn port_004_initial_input_waits_for_an_owned_agent_process_even_when_a_composer_is_visible() {
    let delivered = in_both_modes(|kernel| {
        // The screen shows a ready Codex composer, but the foreground process is plain `cat`:
        // no agent process is owned by the pane, so the body must not be typed.
        let pane = terminal_with_initial_input(
            kernel,
            &["sh", "-c", "printf '>_ OpenAI Codex\\n\u{203a} '; exec cat"],
            "must wait for ownership",
        );
        kernel.settle();
        kernel.advance_clock_ms(5_000);
        kernel.settle();
        kernel.pane_input_log(pane)
    });
    assert_eq!(delivered, Vec::<u8>::new());
}

/// REQ: PORT:ilium-server/src/agent_delivery.rs::initial_prompt_waits_past_codex_loading_composer
#[test]
fn port_005_initial_input_waits_past_the_codex_loading_composer_then_arrives() {
    let (while_loading, after_ready) = in_both_modes(|kernel| {
        let pane = terminal_with_initial_input(
            kernel,
            &[
                "bash",
                "-c",
                "printf '>_ OpenAI Codex\\nmodel:     loading   /model to change\\n\u{203a} Ask Codex to do anything'; exec -a codex cat",
            ],
            "after startup",
        );
        kernel.settle();
        kernel.advance_clock_ms(3_000);
        kernel.settle();
        let while_loading = kernel.pane_input_log(pane);
        // Clear the screen and show the finished banner: the model name replaces `loading`.
        kernel.feed_output(
            pane,
            "\u{1b}[2J\u{1b}[H>_ OpenAI Codex\r\nmodel:     GPT-6-Luna low   /model to change\r\n\u{203a} Ask Codex to do anything".as_bytes(),
        );
        kernel.settle();
        kernel.advance_clock_ms(3_000);
        kernel.settle();
        let after_ready = String::from_utf8_lossy(&kernel.pane_input_log(pane)).contains("after startup");
        (while_loading, after_ready)
    });
    assert_eq!(while_loading, Vec::<u8>::new());
    assert!(after_ready, "the body must be typed once the loading banner is gone");
}

/// REQ: PORT:ilium-server/src/http_api.rs::lookup_refusal_precedes_callback_and_allows_original_input_retry
#[test]
fn port_007_an_oversized_project_is_refused_before_any_pane_and_the_same_input_can_retry() {
    on_plugin_build(|kernel| {
        let nodes_before = kernel.tree().nodes().len();
        let oversized = "x".repeat(8_193);
        let (status, body) = kernel.http_post(
            "/create_agent",
            json!({"agent_type": "claude", "project": oversized, "prompt": "retry me"}),
        );
        assert_eq!(status, 400, "{body}");
        assert!(body["error"].as_str().is_some_and(|message| !message.is_empty()), "{body}");
        kernel.settle();
        assert_eq!(kernel.tree().nodes().len(), nodes_before, "a refused lookup creates nothing");

        let (retry_status, retry_body) = kernel.http_post(
            "/create_agent",
            json!({"agent_type": "claude", "project": kernel.project_root(), "prompt": "retry me"}),
        );
        assert_eq!(retry_status, 200, "{retry_body}");
        assert_eq!(retry_body["project_path"], json!(kernel.project_root()));
        assert_eq!(retry_body["prompt_delivered"], json!(true));
    });
}

/// REQ: PORT:ilium-server/src/http_api.rs::cancelled_lookup_waiter_keeps_native_callback_admitted_until_return
#[test]
fn port_006_a_client_that_hangs_up_mid_request_leaves_the_route_and_job_banks_healthy() {
    on_plugin_build(|kernel| {
        let address = kernel.http_listener_address().expect("the local listener is up");
        let workers_before = kernel.bank_worker_count("bank-a");
        let body = json!({"agent_type": "claude", "project": kernel.project_root(), "prompt": "abandoned"}).to_string();
        let request = format!(
            "POST /create_agent HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        {
            let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).expect("connect to the listener");
            stream.set_write_timeout(Some(Duration::from_secs(5))).expect("bounded write");
            stream.write_all(request.as_bytes()).expect("send the request");
            // Dropped here without reading: the waiter is cancelled while the lookup may run.
        }
        kernel.settle();

        let (status, reply) = kernel.http_post(
            "/create_agent",
            json!({"agent_type": "claude", "project": kernel.project_root(), "prompt": "after hang-up"}),
        );
        assert_eq!(status, 200, "{reply}");
        assert_eq!(reply["prompt_delivered"], json!(true));

        let ticket = kernel.run_finite_job("bank-a", Box::new(|| 7 * 2));
        kernel.settle();
        assert_eq!(kernel.job_result(ticket), Some(14));
        assert_eq!(kernel.bank_worker_count("bank-a"), workers_before);
    });
}
