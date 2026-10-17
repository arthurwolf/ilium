//! `ilium:host/cli` (api-cli.md): the output, input and context calls of a CLI invocation.

use ilium_plugin_api::generated::host::cli::HostCli;
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["cli.contribute"];

fn jsonl_rig() -> (Rig, u64) {
    let rig = server_rig(GRANTS);
    let invocation = rig.kernel.start_cli_invocation("jsonl", "jsonl");
    (rig, invocation)
}

fn text_rig() -> (Rig, u64) {
    let rig = server_rig(GRANTS);
    let invocation = rig.kernel.start_cli_invocation("text", "text");
    (rig, invocation)
}

/// REQ: WIT:ilium:host/cli.write-line
#[test]
fn wit_cli_write_line_appends_one_json_line() {
    let (mut rig, call) = jsonl_rig();
    rig.host
        .cli()
        .write_line(call, "{\"type\":\"result\",\"n\":1}".to_owned())
        .expect("written");
    assert_eq!(
        rig.kernel.cli_stdout(call),
        vec!["{\"type\":\"result\",\"n\":1}".to_owned()]
    );
}

/// REQ: WIT:ilium:host/cli.write-line
#[test]
fn wit_cli_write_line_not_a_json_object_with_a_type_invalid_argument() {
    let (mut rig, call) = jsonl_rig();
    for line in [
        "[1]",
        "{\"n\":1}",
        "{\"type\":3}",
        "not json",
        "{\"type\":\"a\"}\n{\"type\":\"b\"}",
    ] {
        assert_eq!(
            code_of(rig.host.cli().write_line(call, line.to_owned())),
            ErrorCode::InvalidArgument,
            "{line}"
        );
    }
}

/// REQ: WIT:ilium:host/cli.write-line
#[test]
fn wit_cli_write_line_size_boundary_invalid_argument() {
    let (mut rig, call) = jsonl_rig();
    let wrap = |len: usize| format!("{{\"type\":\"{}\"}}", "a".repeat(len - 11));
    assert!(rig.host.cli().write_line(call, wrap(64 * KIB)).is_ok());
    assert_eq!(
        code_of(rig.host.cli().write_line(call, wrap(64 * KIB + 1))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/cli.write-line
#[test]
fn wit_cli_write_line_on_a_text_command_invalid_argument() {
    let (mut rig, call) = text_rig();
    assert_eq!(
        code_of(
            rig.host
                .cli()
                .write_line(call, "{\"type\":\"x\"}".to_owned())
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/cli.write-line
#[test]
fn wit_cli_write_line_after_finish_not_found() {
    let (mut rig, call) = jsonl_rig();
    rig.host.cli().finish(call, 0);
    assert_eq!(
        code_of(
            rig.host
                .cli()
                .write_line(call, "{\"type\":\"x\"}".to_owned())
        ),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/cli.write-text
#[test]
fn wit_cli_write_text_appends_to_stdout() {
    let (mut rig, call) = text_rig();
    rig.host
        .cli()
        .write_text(call, "hello\n".to_owned())
        .expect("written");
    assert_eq!(rig.kernel.cli_stdout_text(call), "hello\n");
}

/// REQ: WIT:ilium:host/cli.write-text
#[test]
fn wit_cli_write_text_size_boundary_invalid_argument() {
    let (mut rig, call) = text_rig();
    assert!(rig.host.cli().write_text(call, text_of(64 * KIB)).is_ok());
    assert_eq!(
        code_of(rig.host.cli().write_text(call, text_of(64 * KIB + 1))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/cli.write-text
#[test]
fn wit_cli_write_text_on_a_jsonl_command_invalid_argument() {
    let (mut rig, call) = jsonl_rig();
    assert_eq!(
        code_of(rig.host.cli().write_text(call, "x".to_owned())),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/cli.write-text
#[test]
fn wit_cli_write_text_unknown_invocation_not_found() {
    let (mut rig, _) = text_rig();
    assert_eq!(
        code_of(rig.host.cli().write_text(8_888_888, "x".to_owned())),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/cli.write-error
#[test]
fn wit_cli_write_error_appends_to_the_error_channel() {
    let (mut rig, call) = jsonl_rig();
    rig.host
        .cli()
        .write_error(call, "boom".to_owned())
        .expect("written");
    assert!(rig.kernel.cli_errors(call).contains(&"boom".to_owned()));
}

/// REQ: WIT:ilium:host/cli.write-error
#[test]
fn wit_cli_write_error_size_boundary_invalid_argument() {
    let (mut rig, call) = jsonl_rig();
    assert!(rig.host.cli().write_error(call, text_of(64 * KIB)).is_ok());
    assert_eq!(
        code_of(rig.host.cli().write_error(call, text_of(64 * KIB + 1))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/cli.write-error
#[test]
fn wit_cli_write_error_after_finish_not_found() {
    let (mut rig, call) = jsonl_rig();
    rig.host.cli().finish(call, 1);
    assert_eq!(
        code_of(rig.host.cli().write_error(call, "late".to_owned())),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/cli.write-stderr
#[test]
fn wit_cli_write_stderr_appends_to_stderr() {
    let (mut rig, call) = text_rig();
    rig.host
        .cli()
        .write_stderr(call, "warn\n".to_owned())
        .expect("written");
    assert_eq!(rig.kernel.cli_stderr_text(call), "warn\n");
}

/// REQ: WIT:ilium:host/cli.write-stderr
#[test]
fn wit_cli_write_stderr_size_boundary_invalid_argument() {
    let (mut rig, call) = text_rig();
    assert!(rig.host.cli().write_stderr(call, text_of(64 * KIB)).is_ok());
    assert_eq!(
        code_of(rig.host.cli().write_stderr(call, text_of(64 * KIB + 1))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/cli.write-stderr
#[test]
fn wit_cli_write_stderr_after_finish_not_found() {
    let (mut rig, call) = text_rig();
    rig.host.cli().finish(call, 0);
    assert_eq!(
        code_of(rig.host.cli().write_stderr(call, "x".to_owned())),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/cli.finish
#[test]
fn wit_cli_finish_records_the_exit_code() {
    let (mut rig, call) = text_rig();
    rig.host.cli().finish(call, 3);
    assert_eq!(rig.kernel.cli_exit_code(call), Some(3));
}

/// REQ: WIT:ilium:host/cli.finish
#[test]
fn wit_cli_finish_unknown_invocation_is_a_no_op() {
    let (mut rig, _) = text_rig();
    rig.host.cli().finish(8_888_888, 1);
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/cli.finish
#[test]
fn wit_cli_finish_twice_keeps_the_first_exit_code() {
    let (mut rig, call) = text_rig();
    rig.host.cli().finish(call, 2);
    rig.host.cli().finish(call, 9);
    assert_eq!(rig.kernel.cli_exit_code(call), Some(2));
}

/// REQ: WIT:ilium:host/cli.read-stdin
#[test]
fn wit_cli_read_stdin_returns_a_request_id_and_the_chunk_arrives() {
    let mut rig = server_rig(GRANTS);
    let call = rig
        .kernel
        .start_cli_invocation_with_stdin("text", "text", b"abcdef");
    let request = rig.host.cli().read_stdin(call, 4).expect("asked");
    rig.host.pump(8);
    assert_eq!(rig.host.cli().take_stdin(request), Some(b"abcd".to_vec()));
}

/// REQ: WIT:ilium:host/cli.read-stdin
#[test]
fn wit_cli_read_stdin_stdin_none_unsupported() {
    let mut rig = server_rig(GRANTS);
    let call = rig
        .kernel
        .start_cli_invocation_without_stdin("text", "text");
    assert_eq!(
        code_of(rig.host.cli().read_stdin(call, 4)),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/cli.read-stdin
#[test]
fn wit_cli_read_stdin_four_mib_total_quota_exceeded() {
    let mut rig = server_rig(GRANTS);
    let call = rig
        .kernel
        .start_cli_invocation_with_stdin("text", "text", &vec![1u8; 5 * MIB]);
    let first = rig
        .host
        .cli()
        .read_stdin(call, (4 * MIB) as u32)
        .expect("up to the total");
    rig.host.pump(8);
    assert_eq!(
        rig.host.cli().take_stdin(first).map(|chunk| chunk.len()),
        Some(4 * MIB)
    );
    assert_eq!(
        code_of(rig.host.cli().read_stdin(call, 1)),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/cli.take-stdin
#[test]
fn wit_cli_take_stdin_returns_the_chunk_once() {
    let mut rig = server_rig(GRANTS);
    let call = rig
        .kernel
        .start_cli_invocation_with_stdin("text", "text", b"xyz");
    let request = rig.host.cli().read_stdin(call, 10).expect("asked");
    rig.host.pump(8);
    assert_eq!(rig.host.cli().take_stdin(request), Some(b"xyz".to_vec()));
    assert_eq!(rig.host.cli().take_stdin(request), None);
}

/// REQ: WIT:ilium:host/cli.take-stdin
#[test]
fn wit_cli_take_stdin_unknown_request_is_none() {
    let mut rig = server_rig(GRANTS);
    assert_eq!(rig.host.cli().take_stdin(31_337), None);
}

/// REQ: WIT:ilium:host/cli.is-tty
#[test]
fn wit_cli_is_tty_reports_stdin_and_stdout() {
    let mut rig = server_rig(GRANTS);
    let call = rig
        .kernel
        .start_cli_invocation_with_tty("text", "text", (true, false));
    assert_eq!(rig.host.cli().is_tty(call), (true, false));
}

/// REQ: WIT:ilium:host/cli.is-tty
#[test]
fn wit_cli_is_tty_unknown_invocation_is_both_false() {
    let mut rig = server_rig(GRANTS);
    assert_eq!(rig.host.cli().is_tty(8_888_888), (false, false));
}

/// REQ: WIT:ilium:host/cli.caller-pane
#[test]
fn wit_cli_caller_pane_names_the_calling_pane() {
    let mut rig = server_rig(GRANTS);
    let call = rig
        .kernel
        .start_cli_invocation_from_pane("text", "text", PANE);
    assert_eq!(rig.host.cli().caller_pane(call), Some(PANE));
}

/// REQ: WIT:ilium:host/cli.caller-pane
#[test]
fn wit_cli_caller_pane_outside_a_pane_is_none() {
    let (mut rig, call) = text_rig();
    assert_eq!(rig.host.cli().caller_pane(call), None);
}

/// REQ: WIT:ilium:host/cli.selected-panes
#[test]
fn wit_cli_selected_panes_reports_the_selection() {
    let mut rig = server_rig(GRANTS);
    let call = rig
        .kernel
        .start_cli_invocation_with_selection("text", "text", vec![PANE], false);
    let selection = rig.host.cli().selected_panes(call).expect("selection");
    assert_eq!(selection.panes, vec![PANE]);
    assert!(!selection.all);
}

/// REQ: WIT:ilium:host/cli.selected-panes
#[test]
fn wit_cli_selected_panes_no_selection_is_none() {
    let (mut rig, call) = text_rig();
    assert!(rig.host.cli().selected_panes(call).is_none());
}
