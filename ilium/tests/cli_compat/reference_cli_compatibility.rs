//! docs/plugins-manual reference-cli-compatibility: the baseline commands stay, owned by core plugins (COR-040, COR-042).

use crate::common::*;

const OWNERS: [(&str, &str, &[&str]); 5] = [
    ("chatroom", "ilium.core.chatroom", &["chat"]),
    (
        "fleet",
        "ilium.core.fleet",
        &["panes", "broadcast", "new-pane"],
    ),
    ("progress", "ilium.core.progress", &["progress"]),
    ("recap", "ilium.core.recap", &["recap"]),
    ("voice", "ilium.core.voice", &["voice"]),
];

fn manifest(dir: &str) -> serde_json::Value {
    let path = workspace_root()
        .join("plugins/core")
        .join(dir)
        .join("manifest.json");
    serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .expect("manifest json")
}

fn declared_cli_names(manifest: &serde_json::Value) -> Vec<String> {
    let cli = &manifest["contributes"]["cli"];
    let items: Vec<&serde_json::Value> = match cli {
        serde_json::Value::Array(items) => items.iter().collect(),
        serde_json::Value::Null => vec![],
        single => vec![single],
    };
    items
        .iter()
        .filter_map(|item| item["name"].as_str().map(str::to_owned))
        .collect()
}

/// REQ: COR-040
#[test]
fn cor_040_each_owner_manifest_declares_its_baseline_commands() {
    for (dir, id, commands) in OWNERS {
        let m = manifest(dir);
        assert_eq!(m["id"], id);
        let names = declared_cli_names(&m);
        for command in commands {
            assert!(
                names.iter().any(|n| n == command),
                "{id} must declare {command}: {names:?}"
            );
        }
    }
}

/// REQ: COR-040
#[test]
fn cor_040_wait_is_declared_as_an_alias_of_progress() {
    let m = manifest("progress");
    let aliases = &m["contributes"]["cli"];
    let text = aliases.to_string();
    assert!(text.contains("\"wait\""), "{text}");
}

/// REQ: COR-040
#[test]
fn cor_040_no_owner_declares_a_kernel_command_name() {
    for (dir, id, _) in OWNERS {
        for name in declared_cli_names(&manifest(dir)) {
            assert!(
                !["new-session", "ls", "kill-session", "help"].contains(&name.as_str()),
                "{id} declares kernel name {name}"
            );
        }
    }
}

/// REQ: COR-040
#[test]
fn cor_040_each_command_is_declared_by_exactly_one_owner() {
    let mut seen = std::collections::BTreeMap::new();
    for (dir, id, _) in OWNERS {
        for name in declared_cli_names(&manifest(dir)) {
            if let Some(previous) = seen.insert(name.clone(), id) {
                panic!("{name} declared by {previous} and {id}");
            }
        }
    }
    assert_eq!(seen.len(), 8);
}

/// REQ: COR-040
#[test]
fn cor_040_declared_output_modes_match_the_baseline_formats() {
    let expected = [
        ("chat", "text"),
        ("panes", "jsonl"),
        ("broadcast", "jsonl"),
        ("new-pane", "mixed"),
        ("progress", "jsonl"),
        ("recap", "jsonl"),
        ("voice", "jsonl"),
    ];
    for (dir, _, _) in OWNERS {
        let m = manifest(dir);
        let cli = &m["contributes"]["cli"];
        let items: Vec<&serde_json::Value> = cli
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_else(|| vec![cli]);
        for item in items {
            let name = item["name"].as_str().expect("name");
            let want = expected
                .iter()
                .find(|(n, _)| *n == name)
                .expect("known command")
                .1;
            assert_eq!(item["output"], want, "{name}");
        }
    }
}

/// REQ: COR-042
#[test]
fn cor_042_disabled_owner_makes_its_command_exit_one_with_cli_owner_disabled() {
    let sandbox = Sandbox::new();
    let disabled = sandbox.ilium(&["plugin", "disable", "ilium.core.chatroom"]);
    assert_eq!(disabled.code, 0, "{}", disabled.stderr);
    let run = sandbox.ilium(&["chat", "tail"]);
    assert_eq!(run.code, 1);
    let combined = format!("{}{}", run.stdout, run.stderr);
    assert!(combined.contains("cli.owner-disabled"), "{combined}");
    assert!(combined.contains("ilium.core.chatroom"), "{combined}");
}

/// REQ: COR-042
#[test]
fn cor_042_disabling_the_fleet_owner_fails_new_pane_but_not_chat() {
    let sandbox = Sandbox::new();
    assert_eq!(
        sandbox
            .ilium(&["plugin", "disable", "ilium.core.fleet"])
            .code,
        0
    );
    let pane = sandbox.ilium(&["new-pane", "--", "sh"]);
    assert_eq!(pane.code, 1);
    assert!(format!("{}{}", pane.stdout, pane.stderr).contains("cli.owner-disabled"));
    let chat = sandbox.ilium(&["chat", "init"]);
    assert_eq!(chat.code, 0, "{}", chat.stderr);
}

/// REQ: COR-042
#[test]
fn cor_042_kernel_commands_keep_working_with_every_core_plugin_disabled() {
    let sandbox = Sandbox::new();
    for (_, id, _) in OWNERS {
        assert_eq!(sandbox.ilium(&["plugin", "disable", id]).code, 0, "{id}");
    }
    let ls = sandbox.ilium(&["ls"]);
    assert_eq!(ls.code, 0, "{}", ls.stderr);
    assert_eq!(ls.stdout, "no sessions\n");
}

/// REQ: COR-042
#[test]
fn cor_042_help_marks_commands_of_disabled_owners() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["plugin", "disable", "ilium.core.voice"]);
    let help = sandbox.ilium(&["--help"]).stdout;
    let voice_line = help
        .lines()
        .find(|l| l.trim_start().starts_with("voice"))
        .expect("voice listed");
    assert!(voice_line.contains("(disabled)"), "{voice_line}");
    let chat_line = help
        .lines()
        .find(|l| l.trim_start().starts_with("chat"))
        .expect("chat listed");
    assert!(!chat_line.contains("(disabled)"), "{chat_line}");
}

/// REQ: COR-042
#[test]
fn cor_042_re_enabling_the_owner_restores_the_command() {
    let sandbox = Sandbox::new();
    sandbox.ilium(&["plugin", "disable", "ilium.core.chatroom"]);
    assert_eq!(sandbox.ilium(&["chat", "init"]).code, 1);
    sandbox.ilium(&["plugin", "enable", "ilium.core.chatroom"]);
    assert_eq!(sandbox.ilium(&["chat", "init"]).code, 0);
}

/// REQ: COR-042
#[test]
fn cor_042_a_failed_owner_reports_cli_owner_failed() {
    let sandbox = Sandbox::new();
    let run = sandbox.ilium_env(
        &["chat", "tail"],
        &[("ILIUM_TEST_FAIL_PLUGIN", "ilium.core.chatroom")],
    );
    assert_eq!(run.code, 1);
    assert!(format!("{}{}", run.stdout, run.stderr).contains("cli.owner-failed"));
}

/// REQ: COR-042
#[test]
fn cor_042_unknown_command_is_exit_two_not_an_owner_error() {
    let run = Sandbox::new().ilium(&["frobnicate"]);
    assert_eq!(run.code, 2);
    assert!(!format!("{}{}", run.stdout, run.stderr).contains("cli.owner"));
}

/// REQ: COR-042
#[test]
fn cor_042_safe_start_keeps_the_baseline_commands_available() {
    let sandbox = Sandbox::new();
    let run = sandbox.ilium_env(&["chat", "init"], &[("ILIUM_SAFE_PLUGINS", "1")]);
    assert_eq!(run.code, 0, "{}", run.stderr);
}
