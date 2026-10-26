//! CAP-001..CAP-017 of reference-requirements-capabilities.md, api half: grant syntax, the
//! grant table, install-time refusals and the tier x side x risk matrix of reference-capabilities.md.

use std::collections::BTreeSet;

use ilium_plugin_api::grants::{check_grant, info, risk_of, table, Grant, Risk};
use ilium_plugin_api::{Side, Tier};
use proptest::prelude::*;

use crate::support::{
    codes_for, manual_dir, manual_page, reports, reports_any_grant_code, sample_grant, table_rows,
    MATRIX_EXCLUDED, T1_ONLY,
};

fn grant(text: &str) -> Grant {
    Grant::parse(text).unwrap_or_else(|diagnostic| panic!("{text} parses: {diagnostic}"))
}

fn code_of(result: Result<(), ilium_plugin_api::Diagnostic>) -> String {
    result
        .expect_err("the grant is refused")
        .code
        .as_str()
        .to_owned()
}

// ---------------------------------------------------------------- CAP-001 grant syntax

/// REQ: CAP-001
#[test]
fn cap_001_first_colon_splits_name_and_later_colons_belong_to_argument() {
    let parsed = grant("services.use:ilium:inference/provider");
    assert_eq!(parsed.family(), "services.use");
    assert_eq!(parsed.argument(), Some("ilium:inference/provider"));
    assert_eq!(parsed.as_str(), "services.use:ilium:inference/provider");
}

/// REQ: CAP-001
#[test]
fn cap_001_grant_without_colon_has_no_argument() {
    let parsed = grant("tree.read");
    assert_eq!(parsed.family(), "tree.read");
    assert_eq!(parsed.argument(), None);
}

/// REQ: CAP-001
#[test]
fn cap_001_loopback_host_with_port_keeps_port_in_argument() {
    let parsed = grant("http:localhost:8080");
    assert_eq!(parsed.family(), "http");
    assert_eq!(parsed.argument(), Some("localhost:8080"));
}

/// REQ: CAP-001
#[test]
fn cap_001_picked_with_extra_argument_is_judged_on_the_whole_argument() {
    // fs.read:picked:x -> the argument is "picked:x", which is not a scope.
    assert!(reports(
        Tier::T2,
        Side::Client,
        &["fs.read:picked:x"],
        "manifest.grant-scope"
    ));
    assert_eq!(
        Grant::parse("fs.read:picked:x")
            .expect_err("the whole argument is an invalid scope")
            .code
            .as_str(),
        "manifest.grant-scope"
    );
}

proptest! {
    // REQ: CAP-001
    #[test]
    fn cap_001_property_argument_is_everything_after_the_first_colon(
        tail in "[a-z0-9.]{1,8}(:[a-z0-9.]{1,8}){0,3}"
    ) {
        let text = format!("services.use:{tail}");
        let parsed = Grant::parse(&text).expect("a services.use grant with any argument text parses");
        prop_assert_eq!(parsed.family(), "services.use");
        prop_assert_eq!(parsed.argument(), Some(tail.as_str()));
        prop_assert_eq!(parsed.as_str(), text.as_str());
    }
}

// ---------------------------------------------------------------- CAP-002 unknown names

/// REQ: CAP-002
#[test]
fn cap_002_unknown_grant_name_is_grant_unknown_on_parse() {
    for text in ["files.read", "tree.reed", "fs", "Tree.read", "nothing"] {
        let diagnostic = Grant::parse(text).expect_err("an unknown grant name is refused");
        assert_eq!(diagnostic.code.as_str(), "manifest.grant-unknown", "{text}");
    }
}

/// REQ: CAP-002
#[test]
fn cap_002_unknown_grant_name_fails_the_install_check() {
    for side in [Side::Server, Side::Client] {
        assert!(
            reports(Tier::T2, side, &["files.read"], "manifest.grant-unknown"),
            "{side}"
        );
    }
}

/// REQ: CAP-002
#[test]
fn cap_002_known_names_are_not_reported_unknown() {
    assert!(!reports(
        Tier::T2,
        Side::Server,
        &["tree.read", "fs.read:project"],
        "manifest.grant-unknown"
    ));
}

// ---------------------------------------------------------------- CAP-003 wildcards

/// REQ: CAP-003
#[test]
fn cap_003_plugin_wide_command_wildcard_is_valid() {
    assert!(!reports_any_grant_code(
        Tier::T2,
        Side::Server,
        &["commands.invoke:com.example.notes/*"]
    ));
    assert!(!reports_any_grant_code(
        Tier::T2,
        Side::Server,
        &["commands.invoke:ilium.git/status"]
    ));
}

/// REQ: CAP-003
#[test]
fn cap_003_wildcard_before_the_slash_is_refused() {
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["commands.invoke:*/notes"],
        "manifest.grant-wildcard"
    ));
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["commands.invoke:*/status"],
        "manifest.grant-wildcard"
    ));
}

/// REQ: CAP-003
#[test]
fn cap_003_partial_wildcard_in_a_command_id_is_refused() {
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["commands.invoke:ilium.git/prefix*"],
        "manifest.grant-wildcard"
    ));
}

/// REQ: CAP-003
#[test]
fn cap_003_wildcards_are_refused_where_the_table_forbids_them() {
    let refused = [
        "http:*",
        "messages.peer:com.example.*",
        "extensions.provide:com.example.notes/*",
        "extensions.contribute:com.example.notes/*",
        "topics.subscribe:a*b",
        "process.run:git*",
    ];
    for text in refused {
        // PKG-056: a process.run argument must be a program name, so its wildcard is
        // grant-program; every other forbidden wildcard is grant-wildcard.
        let expected = if text.starts_with("process.run") {
            "manifest.grant-program"
        } else {
            "manifest.grant-wildcard"
        };
        let codes = codes_for(Tier::T2, Side::Server, &[text]);
        assert!(
            codes.iter().any(|code| code == expected),
            "{text}: {codes:?}"
        );
    }
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["http:*"],
        "manifest.grant-wildcard"
    ));
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["messages.peer:com.example.*"],
        "manifest.grant-wildcard"
    ));
}

/// REQ: CAP-003
#[test]
fn cap_003_wildcards_the_table_allows_are_not_refused() {
    for text in [
        "http:*.openstreetmap.org",
        "commands.invoke:*",
        "topics.subscribe:ilium.progress/*",
        "topics.subscribe:*",
    ] {
        assert!(
            !reports(Tier::T2, Side::Server, &[text], "manifest.grant-wildcard"),
            "{text}"
        );
    }
}

// ---------------------------------------------------------------- CAP-004 argument presence

/// REQ: CAP-004
#[test]
fn cap_004_grant_that_takes_no_argument_refuses_one() {
    for text in [
        "tree.read:x",
        "fs.pick:project",
        "sessions.cross:all",
        "notify.toast:x",
    ] {
        assert!(
            reports(Tier::T1, Side::Server, &[text], "manifest.grant-argument")
                || reports(Tier::T1, Side::Client, &[text], "manifest.grant-argument"),
            "{text}"
        );
    }
}

/// REQ: CAP-004
#[test]
fn cap_004_grant_that_needs_an_argument_refuses_none() {
    for text in [
        "process.run",
        "commands.invoke",
        "fs.read",
        "http",
        "services.use",
        "messages.peer",
        "http-api.route",
    ] {
        assert!(
            reports(
                Tier::T2,
                Side::Server,
                &[text],
                "manifest.grant-argument-missing"
            ),
            "{text}"
        );
    }
}

/// REQ: CAP-004
#[test]
fn cap_004_loopback_host_without_port_is_an_argument_error() {
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["http:localhost"],
        "manifest.grant-argument"
    ));
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["http:127.0.0.1"],
        "manifest.grant-argument"
    ));
    assert!(!reports(
        Tier::T2,
        Side::Server,
        &["http:localhost:8080"],
        "manifest.grant-argument"
    ));
}

/// REQ: CAP-004
#[test]
fn cap_004_argument_rules_match_the_table_flag() {
    for row in table_rows() {
        for form in &row.forms {
            let family = form.split(':').next().expect("family");
            if MATRIX_EXCLUDED.contains(&family) {
                continue;
            }
            let row_info = info(family).unwrap_or_else(|| panic!("table row for {family}"));
            assert_eq!(row_info.takes_argument, form.contains(':'), "{form}");
        }
    }
}

// ---------------------------------------------------------------- CAP-005 tier-limited grants

/// REQ: CAP-005
#[test]
fn cap_005_t1_only_grants_fail_in_t2_and_t3_with_grant_tier() {
    for family in T1_ONLY {
        let text = sample_grant(family);
        let parsed = grant(&text);
        let side = info(family).expect("table row").sides[0];
        for tier in [Tier::T2, Tier::T3] {
            assert_eq!(
                code_of(check_grant(&parsed, tier, side)),
                "manifest.grant-tier",
                "{text} {tier}"
            );
        }
        assert!(check_grant(&parsed, Tier::T1, side).is_ok(), "{text} at T1");
    }
}

/// REQ: CAP-005
#[test]
fn cap_005_install_check_reports_grant_tier_for_a_t2_package() {
    assert!(reports(
        Tier::T2,
        Side::Client,
        &["plugins.manage"],
        "manifest.grant-tier"
    ));
    assert!(reports(
        Tier::T2,
        Side::Client,
        &["settings.read-all"],
        "manifest.grant-tier"
    ));
    assert!(reports(
        Tier::T3,
        Side::Server,
        &["settings.write-kernel:api"],
        "manifest.grant-tier"
    ));
    assert!(!reports(
        Tier::T1,
        Side::Client,
        &["plugins.manage"],
        "manifest.grant-tier"
    ));
}

/// REQ: CAP-005
#[test]
fn cap_005_ordinary_grants_pass_the_tier_check_at_every_tier() {
    for tier in Tier::ALL {
        assert!(
            check_grant(&grant("tree.read"), tier, Side::Server).is_ok(),
            "{tier}"
        );
    }
}

// ---------------------------------------------------------------- CAP-006 sides

/// REQ: CAP-006
#[test]
fn cap_006_client_only_grant_on_the_server_side_is_grant_side() {
    for text in [
        "ui.panels",
        "editor.read",
        "terminal.pointer",
        "keymap.read",
    ] {
        assert_eq!(
            code_of(check_grant(&grant(text), Tier::T2, Side::Server)),
            "manifest.grant-side",
            "{text}"
        );
    }
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["ui.panels"],
        "manifest.grant-side"
    ));
}

/// REQ: CAP-006
#[test]
fn cap_006_server_only_grant_on_the_client_side_is_grant_side() {
    for text in [
        "panes.signal",
        "panes.lifecycle",
        "terminal.output-filter",
        "cli.contribute",
        "workspace.manage",
    ] {
        assert_eq!(
            code_of(check_grant(&grant(text), Tier::T2, Side::Client)),
            "manifest.grant-side",
            "{text}"
        );
    }
    assert!(reports(
        Tier::T2,
        Side::Client,
        &["cli.contribute"],
        "manifest.grant-side"
    ));
}

/// REQ: CAP-006
#[test]
fn cap_006_scene_side_accepts_only_scene_grants() {
    assert_eq!(
        code_of(check_grant(&grant("tree.read"), Tier::T2, Side::Scene)),
        "manifest.grant-side"
    );
    assert!(check_grant(&grant("fs.read:project"), Tier::T2, Side::Scene).is_ok());
    assert!(check_grant(&grant("jobs.run"), Tier::T2, Side::Scene).is_ok());
}

/// REQ: CAP-006
#[test]
fn cap_006_sessions_cross_is_refused_on_the_client_side() {
    assert_eq!(
        code_of(check_grant(
            &grant("sessions.cross"),
            Tier::T2,
            Side::Client
        )),
        "manifest.grant-side"
    );
}

// ---------------------------------------------------------------- CAP-007 dependencies

/// REQ: CAP-007
#[test]
fn cap_007_fs_pick_alone_is_a_dependency_error() {
    assert!(reports(
        Tier::T2,
        Side::Client,
        &["fs.pick"],
        "manifest.grant-dependency"
    ));
}

/// REQ: CAP-007
#[test]
fn cap_007_fs_pick_with_a_picked_grant_has_no_dependency_error() {
    assert!(!reports(
        Tier::T2,
        Side::Client,
        &["fs.pick", "fs.read:picked"],
        "manifest.grant-dependency"
    ));
    assert!(!reports(
        Tier::T2,
        Side::Client,
        &["fs.pick", "fs.write:picked"],
        "manifest.grant-dependency"
    ));
}

/// REQ: CAP-007
#[test]
fn cap_007_fs_pick_with_an_unrelated_fs_grant_is_still_a_dependency_error() {
    assert!(reports(
        Tier::T2,
        Side::Client,
        &["fs.pick", "fs.read:project"],
        "manifest.grant-dependency"
    ));
}

/// REQ: CAP-007
#[test]
fn cap_007_settings_write_all_needs_settings_read_all() {
    assert!(reports(
        Tier::T1,
        Side::Client,
        &["settings.write-all"],
        "manifest.grant-dependency"
    ));
    assert!(!reports(
        Tier::T1,
        Side::Client,
        &["settings.read-all", "settings.write-all"],
        "manifest.grant-dependency"
    ));
}

/// REQ: CAP-007
#[test]
fn cap_007_panes_create_command_needs_panes_create_on_the_same_side() {
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["panes.create-command:htop"],
        "manifest.grant-dependency"
    ));
    assert!(!reports(
        Tier::T2,
        Side::Server,
        &["panes.create", "panes.create-command:htop"],
        "manifest.grant-dependency"
    ));
}

/// REQ: CAP-007
#[test]
fn cap_007_process_long_lived_needs_process_run() {
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["process.long-lived"],
        "manifest.grant-dependency"
    ));
    assert!(!reports(
        Tier::T2,
        Side::Server,
        &["process.run:git", "process.long-lived"],
        "manifest.grant-dependency"
    ));
}

// ---------------------------------------------------------------- CAP-008 fs scopes

/// REQ: CAP-008
#[test]
fn cap_008_write_to_read_only_scopes_is_grant_scope() {
    for text in [
        "fs.write:package",
        "fs.write:agent-sessions:claude",
        "fs.watch:agent-config:claude",
    ] {
        assert!(
            reports(Tier::T2, Side::Server, &[text], "manifest.grant-scope"),
            "{text}"
        );
    }
}

/// REQ: CAP-008; reference-capabilities file scopes explicitly permit H read/write.
#[test]
fn cap_008_agent_configuration_write_is_a_high_risk_valid_scope() {
    let parsed = grant("fs.write:agent-config:claude");
    assert_eq!(risk_of(std::slice::from_ref(&parsed)), Risk::High);
    assert!(check_grant(&parsed, Tier::T2, Side::Server).is_ok());
}

/// REQ: CAP-008
#[test]
fn cap_008_unknown_scope_and_bare_absolute_path_are_grant_scope() {
    for text in ["fs.read:nowhere", "fs.read:/srv/logs", "fs.write:/srv/logs"] {
        let codes = codes_for(Tier::T2, Side::Server, &[text]);
        assert!(
            codes.iter().any(|code| code == "manifest.grant-scope"),
            "{text}: {codes:?}"
        );
    }
}

/// REQ: CAP-008; reference-capabilities.md file scopes permit a named path or /**.
#[test]
fn cap_008_explicit_absolute_named_path_is_bundled_only() {
    for text in ["fs.read:path:/srv/logs", "fs.read:path:/srv/logs/**"] {
        let parsed = grant(text);
        assert!(
            check_grant(&parsed, Tier::T1, Side::Server).is_ok(),
            "{text}"
        );
        assert!(
            !reports_any_grant_code(Tier::T1, Side::Server, &[text]),
            "{text}"
        );
        for tier in [Tier::T2, Tier::T3] {
            assert_eq!(
                code_of(check_grant(&parsed, tier, Side::Server)),
                "manifest.grant-tier",
                "{text} {tier}"
            );
            assert!(
                reports(tier, Side::Server, &[text], "manifest.grant-tier"),
                "{text} {tier}"
            );
        }
    }
}

/// REQ: CAP-008
#[test]
fn cap_008_valid_scopes_are_accepted() {
    for text in [
        "fs.read:project",
        "fs.write:project",
        "fs.read:path:{home}/Pictures/**",
        "fs.read:agent-sessions:claude",
        "fs.read:package",
        "fs.write:data",
    ] {
        assert!(
            !reports(Tier::T2, Side::Server, &[text], "manifest.grant-scope"),
            "{text}"
        );
    }
}

// ---------------------------------------------------------------- CAP-009 programs

/// REQ: CAP-009
#[test]
fn cap_009_process_run_with_arguments_is_grant_program() {
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["process.run:git status"],
        "manifest.grant-program"
    ));
}

/// REQ: CAP-009
#[test]
fn cap_009_wildcard_relative_path_and_dotdot_are_grant_program() {
    for text in [
        "process.run:git*",
        "process.run:./bin/tool",
        "process.run:bin/tool",
        "process.run:../x",
        "process.run:",
    ] {
        assert!(
            reports(Tier::T2, Side::Server, &[text], "manifest.grant-program"),
            "{text}"
        );
    }
}

/// REQ: CAP-009
#[test]
fn cap_009_program_name_and_absolute_path_are_accepted() {
    for text in ["process.run:git", "process.run:/usr/bin/git"] {
        assert!(
            !reports(Tier::T2, Side::Server, &[text], "manifest.grant-program"),
            "{text}"
        );
    }
}

/// REQ: CAP-009
#[test]
fn cap_009_panes_create_command_uses_the_same_program_validation() {
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["panes.create", "panes.create-command:htop -d"],
        "manifest.grant-program"
    ));
    assert!(reports(
        Tier::T2,
        Side::Server,
        &["panes.create", "panes.create-command:a/b"],
        "manifest.grant-program"
    ));
    assert!(!reports(
        Tier::T2,
        Side::Server,
        &["panes.create", "panes.create-command:htop"],
        "manifest.grant-program"
    ));
}

// ---------------------------------------------------------------- CAP-010 risk

/// REQ: CAP-010
#[test]
fn cap_010_risk_of_names_the_table_risk_for_pure_rows() {
    for row in table_rows() {
        let expected = if row.risk_text == "H" || row.risk_text == "H, T1 only" {
            Risk::High
        } else if row.risk_text == "normal" || row.risk_text == "normal (unstable)" {
            Risk::Low
        } else {
            continue;
        };
        for family in &row.families {
            if MATRIX_EXCLUDED.contains(&family.as_str()) {
                continue;
            }
            let text = sample_grant(family);
            assert_eq!(info(family).expect("table row").risk, expected, "{family}");
            assert_eq!(risk_of(&[grant(&text)]), expected, "{text}");
        }
    }
}

/// REQ: CAP-010
#[test]
fn cap_010_mixed_rows_depend_on_the_argument() {
    assert_eq!(risk_of(&[grant("commands.invoke:*")]), Risk::High);
    assert_eq!(
        risk_of(&[grant("commands.invoke:ilium.git/status")]),
        Risk::Low
    );
    assert_eq!(risk_of(&[grant("http:localhost:8080")]), Risk::High);
    assert_eq!(risk_of(&[grant("http:api.example.com")]), Risk::Low);
    assert_eq!(risk_of(&[grant("clipboard.read")]), Risk::High);
    assert_eq!(risk_of(&[grant("clipboard.write")]), Risk::Low);
    assert_eq!(risk_of(&[grant("fs.read:project")]), Risk::Low);
    assert_eq!(
        risk_of(&[grant("fs.read:agent-sessions:claude")]),
        Risk::High
    );
    assert_eq!(risk_of(&[grant("fs.write:project")]), Risk::High);
    assert_eq!(risk_of(&[grant("fs.write:data")]), Risk::Low);
    assert_eq!(risk_of(&[grant("topics.subscribe:*")]), Risk::High);
    assert_eq!(risk_of(&[grant("settings.read-kernel:api")]), Risk::High);
}

/// REQ: CAP-010
#[test]
fn cap_010_risk_of_a_list_is_the_highest_member() {
    assert_eq!(
        risk_of(&[grant("tree.read"), grant("terminal.history")]),
        Risk::High
    );
    assert_eq!(
        risk_of(&[grant("tree.read"), grant("tree.write")]),
        Risk::Low
    );
    assert_eq!(
        risk_of(&[grant("terminal.history"), grant("tree.read")]),
        Risk::High
    );
}

/// REQ: CAP-010
#[test]
fn cap_010_ten_newer_high_risk_grants_are_high_at_every_tier() {
    for text in [
        "terminal.history",
        "terminal.input-lock",
        "terminal.raw-input",
        "panes.lifecycle",
        "tree.restructure",
        "sessions.cross",
        "process.long-lived",
        "agent.transcript",
        "settings.read-kernel:api",
        "settings.write-kernel:api",
    ] {
        assert_eq!(risk_of(&[grant(text)]), Risk::High, "{text}");
    }
}

proptest! {
    // REQ: CAP-010
    #[test]
    fn cap_010_property_risk_of_is_the_max_over_any_selection(mask in proptest::collection::vec(any::<bool>(), 4)) {
        let pool = ["tree.read", "terminal.history", "notify.toast", "process.run:git"];
        let chosen: Vec<Grant> = pool.iter().zip(&mask).filter(|(_, keep)| **keep).map(|(text, _)| grant(text)).collect();
        let expected = chosen.iter().map(|one| risk_of(std::slice::from_ref(one))).max();
        if let Some(expected) = expected {
            prop_assert_eq!(risk_of(&chosen), expected);
        }
    }
}

// ---------------------------------------------------------------- CAP-011 always granted

/// REQ: CAP-011
#[test]
fn doc_cap_011_page_lists_the_always_granted_capabilities() {
    let page = manual_page("reference-capabilities.md");
    let section = page
        .split("## Always granted")
        .nth(1)
        .and_then(|rest| rest.split("\n## Table").next())
        .expect("the Always granted section");
    for name in [
        "log",
        "clock",
        "timers",
        "storage",
        "settings",
        "codec",
        "fs.read:package",
        "fs.read:data",
        "fs.write:data",
        "fs.read:cache",
        "fs.write:cache",
        "fs.watch:package",
        "fs.watch:data",
        "fs.watch:cache",
        "messages",
        "ui.invalidate",
    ] {
        assert!(
            section.contains(&format!("`{name}`")),
            "always granted: {name}"
        );
    }
}

/// REQ: CAP-011
#[test]
fn cap_011_always_granted_fs_grants_in_a_manifest_are_not_errors() {
    for text in [
        "fs.read:data",
        "fs.write:data",
        "fs.read:cache",
        "fs.write:cache",
        "fs.read:package",
    ] {
        assert!(
            !reports_any_grant_code(Tier::T2, Side::Server, &[text]),
            "{text}"
        );
    }
}

// ---------------------------------------------------------------- CAP-017 table

/// REQ: CAP-017
#[test]
fn doc_cap_017_table_families_match_the_api_table() {
    let from_manual: BTreeSet<String> = table_rows()
        .into_iter()
        .flat_map(|row| row.families)
        .collect();
    let from_api: BTreeSet<String> = table().iter().map(|row| row.family.to_owned()).collect();
    assert_eq!(from_api, from_manual);
}

/// REQ: CAP-017
#[test]
fn cap_017_every_table_row_has_a_unique_family() {
    let families: Vec<&str> = table().iter().map(|row| row.family).collect();
    let unique: BTreeSet<&str> = families.iter().copied().collect();
    assert_eq!(families.len(), unique.len());
    assert!(!families.is_empty());
}

/// REQ: CAP-017
#[test]
fn doc_cap_017_every_grant_used_in_capability_pages_exists_in_the_table() {
    let known: BTreeSet<String> = table_rows()
        .into_iter()
        .flat_map(|row| row.families)
        .collect();
    let mut missing = Vec::new();
    let directory = manual_dir();
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .expect("manual directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("capability-") && name.ends_with(".md"))
        .collect();
    names.sort();
    for name in names {
        let page = manual_page(&name);
        for line in page.lines().filter(|line| {
            line.contains("\"capabilities\"")
                || line.contains("\"server\": [")
                || line.contains("\"client\": [")
        }) {
            for quoted in line.split('"').skip(1).step_by(2) {
                let family = quoted.split(':').next().unwrap_or("");
                if family.contains('.')
                    && family
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '.' || c == '-')
                    && !known.contains(family)
                    && !quoted.contains('/')
                    && !quoted.ends_with(".wasm")
                    && !family.starts_with("ilium")
                    && !family.starts_with("com")
                    && !family.starts_with("example")
                {
                    missing.push(format!("{name}: {quoted}"));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "grants missing from the table: {missing:?}"
    );
}

/// REQ: CAP-017
#[test]
fn doc_cap_017_requirement_states_87_forms() {
    let page = manual_page("reference-requirements-capabilities.md");
    assert!(page.contains("MUST list 87 forms"));
}

// ---------------------------------------------------------------- matrix: tier x side x risk

fn matrix_for_side(side: Side) {
    for row in table_rows() {
        for family in &row.families {
            if MATRIX_EXCLUDED.contains(&family.as_str()) {
                continue;
            }
            let text = sample_grant(family);
            let parsed = grant(&text);
            let side_allowed = row.sides.contains(&side);
            let t1_only = T1_ONLY.contains(&family.as_str());
            for tier in Tier::ALL {
                let tier_allowed = if t1_only {
                    tier == Tier::T1
                } else if family == "sessions.cross" {
                    tier != Tier::T3
                } else {
                    true
                };
                let result = check_grant(&parsed, tier, side);
                match (side_allowed, tier_allowed) {
                    (true, true) => assert!(result.is_ok(), "{text} {tier} {side}"),
                    (false, true) => assert_eq!(
                        code_of(result),
                        "manifest.grant-side",
                        "{text} {tier} {side}"
                    ),
                    (true, false) => assert_eq!(
                        code_of(result),
                        "manifest.grant-tier",
                        "{text} {tier} {side}"
                    ),
                    (false, false) => assert!(result.is_err(), "{text} {tier} {side}"),
                }
            }
        }
    }
}

/// REQ: CAP-005, CAP-006, CAP-017
#[test]
fn cap_017_matrix_server_side_tier_cells() {
    matrix_for_side(Side::Server);
}

/// REQ: CAP-005, CAP-006, CAP-017
#[test]
fn cap_017_matrix_client_side_tier_cells() {
    matrix_for_side(Side::Client);
}

/// REQ: CAP-005, CAP-006, CAP-017
#[test]
fn cap_017_matrix_scene_side_tier_cells() {
    matrix_for_side(Side::Scene);
}

/// REQ: CAP-006, CAP-017
#[test]
fn cap_017_api_table_sides_equal_the_manual_sides() {
    for row in table_rows() {
        for family in &row.families {
            if MATRIX_EXCLUDED.contains(&family.as_str()) {
                continue;
            }
            let mut expected = row.sides.clone();
            expected.sort_by_key(|side| side.as_str());
            let mut actual = info(family).expect("table row").sides.to_vec();
            actual.sort_by_key(|side| side.as_str());
            assert_eq!(actual, expected, "{family}");
        }
    }
}

/// REQ: CAP-005, CAP-017
#[test]
fn cap_017_api_table_tiers_exclude_t2_and_t3_for_t1_only_families() {
    for family in T1_ONLY {
        assert_eq!(
            info(family).expect("table row").tiers,
            &[Tier::T1],
            "{family}"
        );
    }
    assert!(info("tree.read")
        .expect("table row")
        .tiers
        .contains(&Tier::T3));
}

proptest! {
    // REQ: CAP-005, CAP-017
    #[test]
    fn cap_017_property_check_grant_is_monotone_in_tier(row_index in 0usize..70, side_index in 0usize..3) {
        let rows = table_rows();
        let row = &rows[row_index % rows.len()];
        let family = &row.families[0];
        prop_assume!(!MATRIX_EXCLUDED.contains(&family.as_str()));
        let parsed = grant(&sample_grant(family));
        let side = Side::ALL[side_index];
        // Allowed at T3 implies allowed at T2 and T1.
        if check_grant(&parsed, Tier::T3, side).is_ok() {
            prop_assert!(check_grant(&parsed, Tier::T2, side).is_ok());
        }
        if check_grant(&parsed, Tier::T2, side).is_ok() {
            prop_assert!(check_grant(&parsed, Tier::T1, side).is_ok());
        }
    }
}

proptest! {
    // REQ: CAP-017
    #[test]
    fn cap_017_property_covers_is_reflexive(row_index in 0usize..70) {
        let rows = table_rows();
        let row = &rows[row_index % rows.len()];
        let parsed = grant(&sample_grant(&row.families[0]));
        prop_assert!(parsed.covers(&parsed));
    }
}

/// REQ: CAP-003
#[test]
fn cap_003_host_wildcard_covers_a_subdomain_and_exact_host_does_not_cover_others() {
    assert!(grant("http:*.example.com").covers(&grant("http:api.example.com")));
    assert!(!grant("http:api.example.com").covers(&grant("http:*.example.com")));
    assert!(!grant("http:api.example.com").covers(&grant("http:other.example.com")));
}
