//! G11 (plugin CLI router): the `ilium <name>` surface of the real binary and the commands the
//! bundled plugins contribute.
//!
//! Every run uses a temporary data, config and runtime directory, so the user's real session and
//! plugin registry are never read or written.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output};

use ilium::plugin_cli::{contributed_commands, prepare, resolve, Refusal};
use ilium::plugin_cmd::bundled_catalogue;

/// A run of the binary over throw-away directories.
fn ilium(scratch: &Path, args: &[&str]) -> Output {
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).expect("project dir");
    Command::new(env!("CARGO_BIN_EXE_ilium"))
        .args(["--cwd"])
        .arg(&project)
        .args(args)
        .env("XDG_DATA_HOME", scratch.join("data"))
        .env("XDG_CONFIG_HOME", scratch.join("config"))
        .env("XDG_RUNTIME_DIR", scratch.join("run"))
        .env_remove("ILIUM_PANE_ID")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the ilium binary runs")
}

/// REQ: API-005
#[test]
fn an_unknown_subcommand_is_a_clap_error() {
    let scratch = tempfile::tempdir().expect("scratch");
    let output = ilium(scratch.path(), &["no-such-plugin-command"]);
    assert_eq!(output.status.code(), Some(2), "clap's usage-error code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand 'no-such-plugin-command'"),
        "{stderr}"
    );
    assert!(output.stdout.is_empty());
}

/// REQ: API-005
#[test]
fn help_still_lists_the_built_in_subcommands() {
    let scratch = tempfile::tempdir().expect("scratch");
    let output = ilium(scratch.path(), &["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in ["plugin", "new-pane", "progress"] {
        assert!(stdout.contains(name), "{name} is listed in:\n{stdout}");
    }
}

/// REQ: API-005
#[test]
fn the_bundled_plugins_contribute_their_commands() {
    let commands = contributed_commands(&bundled_catalogue("server"), &BTreeSet::new());
    for name in [
        "chat",
        "panes",
        "broadcast",
        "new-pane",
        "progress",
        "recap",
        "resource",
        "voice",
    ] {
        assert!(resolve(&commands, name).is_some(), "{name} is contributed");
    }
    // `chat` runs without a server; the others need the live session.
    let needs_server = |name: &str| resolve(&commands, name).map(|command| command.requires_server);
    assert_eq!(needs_server("chat"), Some(false));
    assert_eq!(needs_server("recap"), Some(true));
    // Aliases resolve to the owning command.
    assert_eq!(
        resolve(&commands, "ressource").map(|command| command.name.as_str()),
        Some("resource")
    );
    assert_eq!(
        resolve(&commands, "wait").map(|command| command.name.as_str()),
        Some("progress")
    );
}

/// REQ: API-005
#[test]
fn a_disabled_bundled_plugin_contributes_nothing() {
    let disabled: BTreeSet<String> = ["ilium.core.recap".to_owned()].into();
    let commands = contributed_commands(&bundled_catalogue("server"), &disabled);
    assert!(resolve(&commands, "recap").is_none());
    assert_eq!(
        prepare(&commands, &["recap".to_owned()], false),
        Err(Refusal::Unknown("recap".to_owned()))
    );
    assert!(resolve(&commands, "panes").is_some(), "other plugins stay");
}

/// REQ: API-005
#[test]
fn the_pane_selector_group_of_a_bundled_command_is_checked_up_front() {
    let commands = contributed_commands(&bundled_catalogue("server"), &BTreeSet::new());
    let words = |text: &[&str]| {
        text.iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    assert!(prepare(&commands, &words(&["recap", "--pane", "3"]), false).is_ok());
    assert!(matches!(
        prepare(&commands, &words(&["recap", "--pane"]), false),
        Err(Refusal::Usage { .. })
    ));
}

/// REQ: API-005
#[tokio::test]
async fn running_an_unknown_name_reports_it_without_touching_a_session() {
    let outcome =
        ilium::plugin_cli::run(vec!["no-such-plugin-command".into()], Path::new(".")).await;
    assert_eq!(
        outcome,
        ilium::plugin_cli::Ran::Unknown("no-such-plugin-command".to_owned())
    );
}
