//! Shell completion scripts and `<id>` completion (DEV-907). Page: reference-cli-completion.

use std::time::{Duration, Instant};

use crate::common::*;
use crate::complete_plugin_ids;

/// REQ: DEV-907
#[test]
fn completions_print_a_script_for_each_shell_offline_and_refuse_an_unknown_shell() {
    let root = scratch();
    let cli_env = env(root.path());
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = run(&cli_env, &["completions", shell]);
        assert_eq!(out.exit_code, 0, "{shell}: {:?}", out.stdout_jsonl);
        assert!(
            !out.stdout_raw.trim().is_empty(),
            "{shell} prints the script to standard output"
        );
    }
    let unknown = run(&cli_env, &["completions", "tcsh"]);
    assert_eq!(unknown.exit_code, 2, "{:?}", unknown.stdout_jsonl);
    assert_eq!(error_code(&unknown).as_deref(), Some("usage.invalid-value"));
}

/// REQ: DEV-907
#[test]
fn id_completion_calls_plugin_list_json_and_times_out_after_500_ms_with_no_output() {
    let root = scratch();
    let cli_env = env(root.path());
    let started = Instant::now();
    let completion = complete_plugin_ids(&cli_env, "ilium-plugin-slow-list");
    let elapsed = started.elapsed();
    assert!(
        completion.candidates.is_empty(),
        "no candidates after the timeout"
    );
    assert!(
        completion.stderr.is_empty(),
        "no error output after the timeout"
    );
    assert!(
        completion.list_json_calls >= 1,
        "completion calls `ilium plugin list --json`"
    );
    assert!(
        elapsed >= Duration::from_millis(500),
        "the wait lasts the 500 ms timeout: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_millis(1500),
        "the wait stops at the timeout: {elapsed:?}"
    );
}
