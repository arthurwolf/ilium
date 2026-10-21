//! Administrator failure runbook (guide-admin-failure-runbook.md): USR-230 to USR-235.
//! Text checks read the manual pages; rig tests drive the `ilium plugin` commands.

use serde_json::Value;

use crate::support::*;

const RUNBOOK: &str = "guide-admin-failure-runbook";

/// The text between a `### heading` line and the next `### ` heading.
fn runbook_section(text: &str, heading: &str) -> String {
    let marker = format!("### {heading}\n");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("runbook has no section `{heading}`"));
    let body = &text[start + marker.len()..];
    let end = body.find("\n### ").unwrap_or(body.len());
    body[..end].to_string()
}

/// The section whose heading slug (lowercase, spaces to hyphens) equals `anchor`.
fn section_for_anchor(text: &str, anchor: &str) -> String {
    let heading = text
        .lines()
        .filter_map(|line| line.strip_prefix("### "))
        .find(|heading| heading.to_lowercase().replace(' ', "-") == anchor)
        .unwrap_or_else(|| panic!("no runbook section for anchor `{anchor}`"));
    runbook_section(text, heading)
}

/// The cells of every data row of the first table in `section` (header and separator skipped).
fn table_body(section: &str) -> Vec<Vec<String>> {
    section
        .lines()
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<String>>()
        })
        .collect()
}

/// The data row whose first cell equals `first_cell`, with backticks ignored.
fn table_row(section: &str, first_cell: &str) -> Option<Vec<String>> {
    table_body(section).into_iter().find(|cells| {
        cells
            .first()
            .is_some_and(|cell| cell.trim_matches('`') == first_cell)
    })
}

/// The text of every backticked span in `text`.
fn backticked(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The anchors (`#...`) of the links in `cell`.
fn anchors(cell: &str) -> Vec<String> {
    cell.split("](#")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .map(str::to_string)
        .collect()
}

/// The triage table rows: symptom, section links, first command.
fn triage_rows(text: &str) -> Vec<Vec<String>> {
    table_body(&runbook_section(text, "Triage by symptom"))
}

/// The policy keys of the table in "Policy keys used in this runbook".
fn policy_keys(text: &str) -> Vec<String> {
    table_body(&runbook_section(text, "Policy keys used in this runbook"))
        .iter()
        .filter_map(|cells| backticked(&cells[0]).into_iter().next())
        .collect()
}

/// The event names of the "Audit events" table; a row may name several, comma-separated.
fn audit_events(text: &str) -> Vec<String> {
    table_body(&runbook_section(text, "Audit events"))
        .iter()
        .flat_map(|cells| {
            cells[0]
                .split(',')
                .map(|name| name.trim().trim_matches('`').to_string())
                .collect::<Vec<String>>()
        })
        .collect()
}

/// The CLI words that identify a command in the CLI reference, e.g. `plugin status`.
fn command_needle(command: &str) -> String {
    command
        .trim_start_matches("ilium ")
        .split_whitespace()
        .map(|token| token.split('=').next().unwrap_or(token))
        .take_while(|token| !token.starts_with('<') && *token != "--json")
        .collect::<Vec<&str>>()
        .join(" ")
}

/// The text of every manual page except the runbook itself.
fn source_pages_text() -> String {
    let pages_dir = workspace_root().join("docs/plugins-manual/pages");
    std::fs::read_dir(&pages_dir)
        .expect("read the manual pages folder")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            name.ends_with(".md") && name != format!("{RUNBOOK}.md")
        })
        .map(|path| std::fs::read_to_string(&path).expect("read a manual page"))
        .collect::<Vec<String>>()
        .join("\n")
}

// ---- USR-230 ----

/// REQ: USR-230
#[test]
fn usr_230_each_triage_row_has_a_cli_command_and_a_section_with_keys_and_events() {
    let runbook = manual_page(RUNBOOK);
    let cli_reference = manual_page("reference-plugin-cli");
    let keys = policy_keys(&runbook);
    let events = audit_events(&runbook);
    let rows = triage_rows(&runbook);
    assert_eq!(rows.len(), 5, "the triage table has five symptoms");
    for row in &rows {
        let command = backticked(&row[2])
            .into_iter()
            .next()
            .expect("first command of the triage row");
        let needle = command_needle(&command);
        assert!(
            cli_reference.contains(&needle),
            "first command `{command}` is not in the CLI reference"
        );
        for anchor in anchors(&row[1]) {
            let section = section_for_anchor(&runbook, &anchor);
            assert!(
                keys.iter().any(|key| section.contains(&format!("`{key}`"))),
                "section `{anchor}` lists no policy key"
            );
            assert!(
                events
                    .iter()
                    .any(|event| section.contains(&format!("`{event}`"))),
                "section `{anchor}` lists no audit event"
            );
        }
    }
    let policy_reference = format!(
        "{}{}",
        manual_page("reference-admin-policy"),
        manual_page("reference-plugin-policy")
    );
    for key in &keys {
        assert!(
            policy_reference.contains(key.as_str()),
            "policy key `{key}` is not in the policy reference"
        );
    }
}

// ---- USR-231 ----

/// REQ: USR-231
#[test]
fn usr_231_deny_ids_stops_the_plugin_for_all_users_and_keeps_its_data() {
    let mut rig = UserRig::new();
    let id = "com.example.stopme";
    install_ok(&mut rig, &spec(id, "1.0.0", &[]));
    rig.start_server();
    rig.write_storage_bucket(id);
    let data_folder = rig.data_dir().join("plugins/data").join(id);
    let data_before = tree_snapshot(&data_folder);
    assert!(
        !data_before.is_empty(),
        "the plugin has data before the policy edit"
    );
    rig.write_system_policy(&format!("version = 1\n[plugins]\ndeny_ids = [\"{id}\"]\n"));
    rig.restart_server();
    let row = list_row(&mut rig, id);
    assert_eq!(row.state, "Blocked by policy");
    assert!(data_folder.is_dir(), "the data folder was deleted");
    assert_eq!(
        tree_snapshot(&data_folder),
        data_before,
        "the data folder changed"
    );
}

// ---- USR-232 ----

/// REQ: USR-232
#[test]
fn usr_232_compromised_key_is_revoked_with_reason_and_its_plugin_is_disabled_blocked() {
    let runbook = manual_page(RUNBOOK);
    let section = runbook_section(&runbook, "Compromised signing key");
    let first_step = section
        .lines()
        .find(|line| line.starts_with("1. "))
        .expect("step 1 of the compromised signing key section");
    assert!(
        first_step.contains("ilium plugin trust revoke <key_id> --reason compromised"),
        "{first_step}"
    );
    assert!(!first_step.contains("--force"), "{first_step}");

    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.trust(&key);
    let id = "com.example.signed";
    install_ok(&mut rig, &spec(id, "1.0.0", &[]).signed_with(&key));
    rig.start_server();
    let fingerprint = key.fingerprint();
    let output = rig.cli(&[
        "plugin",
        "trust",
        "revoke",
        fingerprint.as_str(),
        "--reason",
        "compromised",
    ]);
    assert_eq!(result_exit_code(&output), 0);
    let row = list_row(&mut rig, id);
    assert_eq!(row.state, "disabled-blocked");
    assert_eq!(row.reason.as_deref(), Some("trust.key-revoked"));
}

// ---- USR-233 ----

/// REQ: USR-233
#[test]
fn usr_233_failure_window_kill_sequence_crash_loop_guard_and_install_lock_match_source_pages() {
    let runbook = manual_page(RUNBOOK);
    let limits = runbook_section(&runbook, "Time limits");
    // (limit row, value the runbook must give, source page, fragments the source must contain)
    let cases: [(&str, &str, &str, &[&str]); 4] = [
        (
            "Failure window",
            "10 minutes",
            "lifecycle",
            &["10 minutes"][..],
        ),
        (
            "Kill sequence",
            "SIGTERM, then SIGKILL after 2 s",
            "reference-host-helper",
            &["SIGTERM", "SIGKILL", "2 s"][..],
        ),
        (
            "Crash-loop guard trigger",
            "3 crashes during start within 5 minutes",
            "user-safe-mode-and-troubleshooting",
            &["3 crashes", "5 minutes"][..],
        ),
        (
            "Install lock wait",
            "30 s",
            "reference-install-and-update",
            &["30 s", "install.locked"][..],
        ),
    ];
    for (limit, expected_value, source_page, fragments) in cases {
        let cells = table_row(&limits, limit)
            .unwrap_or_else(|| panic!("the time-limits table has no row `{limit}`"));
        assert!(
            cells[1].contains(expected_value),
            "`{limit}`: runbook value is `{}`, expected `{expected_value}`",
            cells[1]
        );
        let source = manual_page(source_page);
        for fragment in fragments {
            assert!(
                source.contains(fragment),
                "`{limit}`: {source_page} does not contain `{fragment}`"
            );
        }
    }
}

// ---- USR-234 ----

/// REQ: USR-234
#[test]
fn usr_234_audit_path_jq_command_and_every_event_name_match_the_source_pages() {
    let runbook = manual_page(RUNBOOK);
    let path = "$XDG_STATE_HOME/ilium/audit/audit.jsonl";
    assert!(
        runbook.contains(path),
        "the runbook does not name the Linux audit file"
    );
    assert!(
        manual_page("reference-audit-log").contains("ilium/audit/audit.jsonl"),
        "the audit log reference gives no Linux audit path"
    );

    // The jq filter is emulated with serde_json on a fixture log: jq is not a test dependency.
    let bad_plugin = runbook_section(&runbook, "Bad plugin");
    let jq_line = bad_plugin
        .lines()
        .find(|line| line.contains("jq -r"))
        .expect("the Bad plugin section has a jq command");
    assert!(
        jq_line.contains("select(.plugin_id==\"<id>\")"),
        "{jq_line}"
    );
    assert!(jq_line.contains(&format!("\"{path}\"")), "{jq_line}");
    let rig = UserRig::new();
    std::fs::create_dir_all(rig.scratch_dir()).expect("create the scratch folder");
    let fixture_path = rig.scratch_dir().join("audit-fixture.jsonl");
    std::fs::write(
        &fixture_path,
        "{\"plugin_id\":\"com.example.bad\",\"event\":\"policy-violation\"}\n\
         {\"plugin_id\":\"com.example.other\",\"event\":\"grant-revoked\"}\n",
    )
    .expect("write the fixture audit log");
    let fixture = std::fs::read_to_string(&fixture_path).expect("read the fixture audit log");
    let selected: Vec<Value> = fixture
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("fixture line is JSON"))
        .filter(|record| record["plugin_id"] == "com.example.bad")
        .collect();
    assert_eq!(
        selected.len(),
        1,
        "the filter must select only the named plugin"
    );

    let source_text = source_pages_text();
    let events = audit_events(&runbook);
    assert!(!events.is_empty(), "the runbook names no audit events");
    for event in &events {
        assert!(
            source_text.contains(&format!("`{event}`")),
            "event `{event}` appears in no source page"
        );
    }
}

// ---- USR-235 ----

/// REQ: USR-235
#[test]
fn usr_235_escalation_runs_publisher_then_project_then_blocklist_with_placeholder_open() {
    let runbook = manual_page(RUNBOOK);
    let escalation = runbook_section(&runbook, "Escalation path");
    let steps: Vec<&str> = escalation
        .lines()
        .filter(|line| line.starts_with(|character: char| character.is_ascii_digit()))
        .collect();
    assert_eq!(steps.len(), 3, "{escalation}");
    assert!(steps[0].contains("The publisher"), "{}", steps[0]);
    assert!(steps[1].contains("The Ilium project"), "{}", steps[1]);
    assert!(steps[2].contains("The release blocklist"), "{}", steps[2]);

    let open_decisions = runbook_section(&runbook, "Open decisions");
    let row = table_row(&open_decisions, "Security contact address")
        .expect("the open decisions table has a security contact row");
    assert!(row[1].contains("security@ilium.example"), "{}", row[1]);
    assert!(row[1].contains("placeholder"), "{}", row[1]);
}
