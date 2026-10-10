use std::process::{Command, Output, Stdio};

const DIAGNOSTICS_ENV: &str = "ILIUM_ANIMATION_SANDBOX_DIAGNOSTICS";

fn run_helper(diagnostics: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ilium-animation-helper"));
    command
        .arg("--ipc")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if diagnostics {
        command.env(DIAGNOSTICS_ENV, "1");
    } else {
        command.env_remove(DIAGNOSTICS_ENV);
    }

    let mut child = command.spawn().unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

#[test]
fn helper_startup_errors_are_reported_only_to_opted_in_stderr() {
    let detailed = run_helper(true);
    assert_eq!(detailed.status.code(), Some(70));
    assert!(
        String::from_utf8_lossy(&detailed.stderr)
            .contains("animation helper initialization failed"),
        "expected an opt-in startup diagnostic on stderr"
    );
    assert!(
        detailed.stdout.is_empty(),
        "diagnostics must not corrupt IPC stdout"
    );

    let quiet = run_helper(false);
    assert_eq!(quiet.status.code(), Some(70));
    assert!(quiet.stderr.is_empty(), "production startup remains silent");
    assert!(
        quiet.stdout.is_empty(),
        "startup failures must not emit IPC bytes"
    );
}
