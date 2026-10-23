//! Determinism of `ilium plugin test` (DEV-888): two runs over the same inputs give the same output,
//! and the seed printed in the summary replays the run.

use crate::common::*;

/// A Rust server project made by `ilium plugin new`, with its environment and project folder.
fn new_rust_project() -> (
    tempfile::TempDir,
    ilium_plugin_host::cli::CliEnv,
    std::path::PathBuf,
) {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let made = run(
        &env,
        &[
            "new",
            dir.to_str().expect("utf8 path"),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    (root, env, dir)
}

/// REQ: DEV-888
#[test]
fn two_test_runs_give_identical_output_and_the_printed_seed_replays_them() {
    let (_root, env, _dir) = new_rust_project();
    let first = run_in_project(&env, "test", &[]);
    let second = run_in_project(&env, "test", &[]);
    assert_eq!(
        first.stdout_text, second.stdout_text,
        "two runs over the same inputs are byte-identical"
    );
    let summary = records_of(&first, "summary")
        .into_iter()
        .next()
        .expect("a summary record");
    let seed = summary.get("seed").expect("the summary prints the seed");
    let seed_text = seed
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| seed.to_string());
    let replay = run_in_project(&env, "test", &["--seed", seed_text.as_str()]);
    assert_eq!(
        replay.stdout_text, first.stdout_text,
        "the printed seed reproduces the run"
    );
}
