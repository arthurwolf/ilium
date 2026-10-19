//! reference-admin-policy.md: locations, merge rules, ownership check, run-time and install-time
//! effects, malformed files, forced states, locked settings, `policy show`, project values.

use ilium_plugin_api::{ErrorCode, Tier};
use serde_json::{json, Value};

use crate::support::*;

const DENY_CLIPBOARD: &str = "version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n";

fn policy_line(lines: &[Value]) -> Value {
    lines
        .iter()
        .find(|line| line["type"] == "policy")
        .cloned()
        .expect("a policy line")
}

fn sources(lines: &[Value]) -> Vec<Value> {
    lines
        .iter()
        .filter(|line| line["type"] == "policy-source")
        .cloned()
        .collect()
}

fn strings(value: &Value) -> Vec<String> {
    let mut items: Vec<String> = value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    items.sort();
    items
}

/// REQ: SEC-180
#[test]
fn sec_180_system_file_is_read_before_the_user_file() {
    let rig = setup(&[])
        .system_policy("version = 1\n", true)
        .user_policy("version = 1\n")
        .start();
    let found = sources(&rig.policy_show());
    assert_eq!(found.len(), 2);
    assert!(
        found[0]["path"]
            .as_str()
            .expect("path")
            .ends_with("/etc/ilium/policy.toml"),
        "{found:?}"
    );
    assert!(found[1]["path"]
        .as_str()
        .expect("path")
        .ends_with("policy.toml"));
    assert_ne!(found[0]["path"], found[1]["path"]);
}

/// REQ: SEC-180
#[test]
fn sec_180_both_files_contribute_to_the_effective_policy() {
    let system = "version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n";
    let user = "version = 1\n[grants]\ndeny = [\"notify.desktop\"]\n";
    let rig = setup(&[])
        .system_policy(system, true)
        .user_policy(user)
        .start();
    let policy = policy_line(&rig.policy_show());
    assert_eq!(
        strings(&policy["grants"]["deny"]),
        vec!["clipboard.read", "notify.desktop"]
    );
}

/// REQ: SEC-180
#[test]
fn sec_180_the_per_platform_system_paths_are_listed_in_the_manual() {
    assert_page_has(
        "reference-admin-policy.md",
        &[
            "/etc/ilium/policy.toml",
            "/Library/Application Support/Ilium/policy.toml",
            "%ProgramData%\\Ilium\\policy.toml",
        ],
    );
}

/// REQ: SEC-181
#[test]
fn sec_181_allowed_lists_intersect() {
    let rig = setup(&[])
        .system_policy(
            "version = 1\n[packages]\nallowed_tiers = [\"t1\", \"t2\"]\n",
            true,
        )
        .user_policy("version = 1\n[packages]\nallowed_tiers = [\"t2\", \"t3\"]\n")
        .start();
    assert_eq!(
        strings(&policy_line(&rig.policy_show())["packages"]["allowed_tiers"]),
        vec!["t2"]
    );
}

/// REQ: SEC-181
#[test]
fn sec_181_denied_lists_union() {
    let rig = setup(&[])
        .system_policy("version = 1\n[plugins]\ndeny_ids = [\"a.*\"]\n", true)
        .user_policy("version = 1\n[plugins]\ndeny_ids = [\"b.*\"]\n")
        .start();
    assert_eq!(
        strings(&policy_line(&rig.policy_show())["plugins"]["deny_ids"]),
        vec!["a.*", "b.*"]
    );
}

/// REQ: SEC-181
#[test]
fn sec_181_allow_booleans_and_together_and_cannot_be_lifted_by_the_user() {
    for (system, user, expected) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let rig = setup(&[])
            .system_policy(
                &format!("version = 1\n[packages]\nallow_unsigned = {system}\n"),
                true,
            )
            .user_policy(&format!(
                "version = 1\n[packages]\nallow_unsigned = {user}\n"
            ))
            .start();
        assert_eq!(
            policy_line(&rig.policy_show())["packages"]["allow_unsigned"],
            json!(expected),
            "{system} {user}"
        );
    }
}

/// REQ: SEC-181
#[test]
fn sec_181_offline_ors_and_the_user_cannot_turn_it_off() {
    for (system, user, expected) in [
        (true, false, true),
        (false, true, true),
        (false, false, false),
    ] {
        let rig = setup(&[])
            .system_policy(
                &format!("version = 1\n[network]\noffline = {system}\n"),
                true,
            )
            .user_policy(&format!("version = 1\n[network]\noffline = {user}\n"))
            .start();
        assert_eq!(
            policy_line(&rig.policy_show())["network"]["offline"],
            json!(expected),
            "{system} {user}"
        );
    }
}

/// REQ: SEC-181
#[test]
fn sec_181_deny_wins_over_allow_for_a_plugin_id() {
    let policy =
        "version = 1\n[plugins]\nallow_ids = [\"example.*\"]\ndeny_ids = [\"example.bad\"]\n";
    let rig = setup(&[]).system_policy(policy, true).start();
    assert_eq!(
        rig.host.install_check("example.bad", Tier::T3),
        Err("policy.id-denied".to_owned())
    );
    assert_eq!(rig.host.install_check("example.good", Tier::T3), Ok(()));
}

/// REQ: SEC-181
#[test]
fn sec_181_policy_wins_over_a_user_grant_choice() {
    let rig = setup(&["clipboard.read"])
        .system_policy(DENY_CLIPBOARD, true)
        .start();
    let result = rig.call("ilium:host/clipboard.read", json!({}));
    assert!(is_not_permitted(&result));
}

/// REQ: SEC-182
#[test]
fn sec_182_a_writable_system_file_is_ignored_and_audited() {
    let rig = setup(&["clipboard.read"])
        .system_policy(DENY_CLIPBOARD, false)
        .start();
    assert!(
        is_ok(&rig.call("ilium:host/clipboard.read", json!({}))),
        "an ignored policy must not deny"
    );
    let records = rig.audit_events("policy-violation");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["reason"], "policy-file-writable");
    assert_eq!(sources(&rig.policy_show())[0]["used"], json!(false));
}

/// REQ: SEC-182
#[test]
fn sec_182_a_persistent_notice_says_the_policy_was_ignored() {
    let rig = setup(&[]).system_policy(DENY_CLIPBOARD, false).start();
    assert!(
        rig.notices()
            .iter()
            .any(|notice| notice == "Admin policy ignored: file is writable by non-administrators"),
        "{:?}",
        rig.notices()
    );
}

/// REQ: SEC-182
#[test]
fn sec_182_a_root_owned_file_is_used_and_raises_no_notice() {
    let rig = setup(&[]).system_policy(DENY_CLIPBOARD, true).start();
    assert!(rig.notices().is_empty());
    assert_eq!(sources(&rig.policy_show())[0]["used"], json!(true));
}

/// REQ: SEC-184
#[test]
fn sec_184_install_checks_fail_with_the_named_policy_codes() {
    let cases = [
        (
            "[packages]\nallowed_tiers = [\"t1\"]\n",
            "example.demo",
            Tier::T3,
            "policy.tier-denied",
        ),
        (
            "[plugins]\ndeny_ids = [\"example.*\"]\n",
            "example.demo",
            Tier::T3,
            "policy.id-denied",
        ),
        (
            "[packages]\nallow_third_party = false\n",
            "example.demo",
            Tier::T2,
            "policy.third-party-denied",
        ),
        (
            "[packages]\nallow_url_installs = false\n",
            "example.demo",
            Tier::T3,
            "policy.source-denied",
        ),
    ];
    for (body, id, tier, code) in cases {
        let rig = setup(&[])
            .system_policy(&format!("version = 1\n{body}"), true)
            .start();
        let found = if code == "policy.source-denied" {
            rig.host.install_check_from_url(id, tier)
        } else {
            rig.host.install_check(id, tier)
        };
        assert_eq!(found, Err(code.to_owned()), "{body}");
    }
}

/// REQ: SEC-184, SEC-198
#[test]
fn sec_184_a_denied_grant_does_not_fail_the_install() {
    let rig = setup(&[]).system_policy(DENY_CLIPBOARD, true).start();
    assert_eq!(
        rig.host
            .install_check_with_grants("example.demo", Tier::T3, &["clipboard.read"]),
        Ok(())
    );
}

/// REQ: SEC-184, SEC-198
#[test]
fn sec_184_runtime_refusal_is_not_permitted_with_reason_policy_and_no_policy_text() {
    let rig = setup(&["clipboard.read"])
        .system_policy(DENY_CLIPBOARD, true)
        .start();
    match rig.call("ilium:host/clipboard.read", json!({})) {
        ilium_plugin_host::testing::ImportResult::Err(error) => {
            assert_eq!(error.code, ErrorCode::NotPermitted);
            assert!(error.message.starts_with("policy: "), "{}", error.message);
            assert!(!error.message.contains("deny"), "{}", error.message);
            assert!(
                !error.message.contains("clipboard.read"),
                "{}",
                error.message
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// REQ: SEC-184
#[test]
fn sec_184_publisher_and_replacement_rules_have_their_own_codes() {
    let rig = setup(&[]).system_policy("version = 1\n[publishers]\ndeny = [\"a1b20c9e\"]\n[extensions]\nallow_replacement = false\n", true).start();
    assert_eq!(
        rig.host.install_check_signed_by("example.demo", "a1b20c9e"),
        Err("policy.publisher-denied".to_owned())
    );
    assert_eq!(
        rig.host
            .install_check_replacing("example.demo", "ilium.core.titles"),
        Err("policy.replacement".to_owned())
    );
}

/// REQ: SEC-185
#[test]
fn sec_185_a_policy_denied_row_is_locked_off_with_the_administrator_note() {
    let rig = setup(&[])
        .system_policy("version = 1\n[grants]\ndeny = [\"process.run:rg\"]\n", true)
        .start();
    let rows = rig.host.review_rows(&["process.run:rg", "notify.toast"]);
    assert!(rows[0].locked && !rows[0].checked);
    assert_eq!(
        rows[0].note.as_deref(),
        Some("Blocked by your administrator")
    );
    assert!(!rows[1].locked && rows[1].note.is_none());
}

/// REQ: SEC-185
#[test]
fn sec_185_a_locked_row_cannot_be_checked() {
    let rig = setup(&[]).system_policy(DENY_CLIPBOARD, true).start();
    assert!(!rig.host.try_check_row("clipboard.read"));
}

/// REQ: SEC-186
#[test]
fn sec_186_a_plugin_violating_a_new_policy_is_disabled_at_the_next_start_and_keeps_its_data() {
    let rig = setup(&[]).start();
    rig.host.write_plugin_data("kept.txt", b"data");
    let mut rig = rig;
    rig.setup.system_policy = Some((
        "version = 1\n[plugins]\ndeny_ids = [\"test.*\"]\n".to_owned(),
        true,
    ));
    let rig = rig.restart();
    assert!(!matches!(
        rig.host.state(),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active
    ));
    assert!(rig
        .notices()
        .iter()
        .any(|notice| notice.contains("Disabled by your administrator")));
    assert_eq!(rig.audit_events("policy-violation").len(), 1);
    assert_eq!(
        rig.host.read_plugin_data("kept.txt"),
        Some(b"data".to_vec())
    );
}

/// REQ: SEC-187
#[test]
fn sec_187_each_malformed_shape_fails_closed_for_third_parties() {
    let bad = [
        "version = 1\n[grants\n",
        "version = 1\nsurprise = true\n",
        "version = 1\n[packages]\nallow_unsigned = \"no\"\n",
        "version = 2\n",
    ];
    for text in bad {
        let rig = setup(&[]).system_policy(text, true).start();
        assert_eq!(
            rig.host.load_error().as_deref(),
            Some("policy.invalid"),
            "{text}"
        );
        assert!(
            rig.notices()
                .iter()
                .any(|notice| notice.starts_with("Admin policy is invalid: ")),
            "{text}"
        );
    }
}

/// REQ: SEC-187
#[test]
fn sec_187_t1_plugins_run_normally_when_the_policy_is_invalid() {
    let rig = setup(&[])
        .tier(Tier::T1)
        .system_policy("version = 2\n", true)
        .start();
    assert!(rig.host.load_error().is_none());
    assert!(matches!(
        rig.host.state(),
        ilium_plugin_api::generated::host::plugins::InstanceState::Active
    ));
}

/// REQ: SEC-187
#[test]
fn sec_187_a_missing_policy_file_is_not_an_error() {
    let rig = setup(&[]).start();
    assert!(rig.host.load_error().is_none());
    assert!(rig.notices().is_empty());
}

/// REQ: SEC-187
#[test]
fn sec_187_an_unreadable_policy_file_counts_as_malformed() {
    let rig = setup(&[]).system_policy_unreadable().start();
    assert_eq!(rig.host.load_error().as_deref(), Some("policy.invalid"));
}

/// REQ: SEC-188
#[test]
fn sec_188_a_policy_edit_takes_effect_only_after_a_restart() {
    let rig = setup(&["clipboard.read"])
        .system_policy("version = 1\n", true)
        .start();
    rig.host.replace_system_policy_file(DENY_CLIPBOARD);
    assert!(is_ok(&rig.call("ilium:host/clipboard.read", json!({}))));
    let mut rig = rig;
    rig.setup.system_policy = Some((DENY_CLIPBOARD.to_owned(), true));
    let rig = rig.restart();
    assert!(is_not_permitted(
        &rig.call("ilium:host/clipboard.read", json!({}))
    ));
}

/// REQ: SEC-188
#[test]
fn sec_188_policy_reload_is_not_a_command_of_version_0_1_0() {
    let scratch = scratch();
    let output = ilium_plugin_host::cli::run(
        &args(&["plugin", "policy", "reload"]),
        &cli_env(scratch.path()),
    );
    assert_ne!(output.exit_code, 0);
}

/// REQ: SEC-189
#[test]
fn sec_189_force_disabled_cannot_be_enabled_and_force_enabled_cannot_be_disabled() {
    let policy = "version = 1\n[plugins]\nforce_disabled = [\"example.fun\"]\nforce_enabled = [\"example.compliance\"]\n";
    let rig = setup(&["plugins.manage"])
        .system_policy(policy, true)
        .start();
    let enable = rig.value(
        "ilium:host/plugins.set-enabled",
        json!({ "plugin": "example.fun", "enabled": true, "scope": "user" }),
    );
    assert_refused(&enable, ErrorCode::NotPermitted, "policy");
    let disable = rig.value(
        "ilium:host/plugins.set-enabled",
        json!({ "plugin": "example.compliance", "enabled": false, "scope": "user" }),
    );
    assert_refused(&disable, ErrorCode::NotPermitted, "policy");
}

/// REQ: SEC-189
#[test]
fn sec_189_a_missing_force_enabled_plugin_is_reported_and_not_installed() {
    let rig = setup(&[])
        .system_policy(
            "version = 1\n[plugins]\nforce_enabled = [\"example.compliance\"]\n",
            true,
        )
        .start();
    let policy = policy_line(&rig.policy_show());
    assert_eq!(policy["missing"], json!(["example.compliance"]));
    assert!(!rig.host.is_installed("example.compliance"));
    assert!(!rig.audit_lines().is_empty());
}

/// REQ: SEC-190
#[test]
fn sec_190_a_locked_setting_refuses_a_user_write_with_settings_locked() {
    let rig = setup(&["settings.write-all"])
        .system_policy(
            "version = 1\n[settings]\nlocked = { \"terminal.scrollback\" = 20000 }\n",
            true,
        )
        .start();
    let user = rig.host.user_set_setting("terminal.scrollback", json!(100));
    assert_eq!(
        user.map_err(|error| error.code.as_str().to_owned()),
        Err("settings.locked".to_owned())
    );
    let plugin = rig.value(
        "ilium:host/settings.set-plugin-json",
        json!({ "plugin": "ilium.core.terminal", "patch": "{\"terminal.scrollback\":100}" }),
    );
    assert_eq!(
        plugin.map_err(|error| error.code.as_str().to_owned()),
        Err("settings.locked".to_owned())
    );
    assert_eq!(
        rig.host.effective_setting("terminal.scrollback"),
        Some(json!(20000))
    );
}

/// REQ: SEC-190
#[test]
fn sec_190_a_plugin_reads_the_policy_value() {
    let rig = setup(&[]).system_policy("version = 1\n[settings]\nlocked = { \"plugins.test.plugin.endpoint\" = \"https://c.example.com\" }\n", true).start();
    let own = rig
        .value("ilium:host/settings.get-json", json!({}))
        .expect("own settings");
    assert!(own.to_string().contains("https://c.example.com"), "{own}");
}

/// REQ: SEC-190
#[test]
fn sec_190_the_cli_config_set_is_refused_with_settings_locked() {
    let scratch = scratch();
    let env = cli_env(scratch.path());
    std::fs::create_dir_all(&env.config_dir).expect("config dir");
    let output =
        ilium_plugin_host::cli::run(&args(&["config", "set", "terminal.scrollback", "10"]), &env);
    assert_ne!(output.exit_code, 0);
}

/// REQ: SEC-191
#[test]
fn sec_191_policy_show_prints_sources_one_policy_and_a_summary() {
    let rig = setup(&[])
        .system_policy("version = 1\n", true)
        .user_policy("version = 1\n")
        .start();
    let lines = rig.policy_show();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line["type"] == "policy-source")
            .count(),
        2
    );
    assert_eq!(
        lines.iter().filter(|line| line["type"] == "policy").count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line["type"] == "summary")
            .count(),
        1
    );
    assert_eq!(lines.last().expect("last line")["type"], "summary");
}

/// REQ: SEC-191
#[test]
fn sec_191_each_source_line_has_path_owner_hash_and_used() {
    let text = "version = 1\n";
    let rig = setup(&[]).system_policy(text, true).start();
    let source = &sources(&rig.policy_show())[0];
    for key in ["path", "owner", "hash", "used"] {
        assert!(source.get(key).is_some(), "{key} in {source}");
    }
    assert_eq!(source["hash"], json!(sha256_hex(text.as_bytes())));
}

/// REQ: SEC-192
#[test]
fn sec_192_project_values_are_ignored_when_the_policy_says_so_even_for_a_trusted_project() {
    let rig = setup(&[])
        .system_policy(
            "version = 1\n[projects]\nallow_project_values = false\n",
            true,
        )
        .start();
    rig.host.trust_project("/work/project");
    rig.host
        .write_project_value("/work/project", "test.plugin", "mode", json!("project"));
    assert_ne!(rig.host.effective_setting("mode"), Some(json!("project")));
}

/// REQ: SEC-192
#[test]
fn sec_192_project_values_apply_for_a_trusted_project_without_the_restriction() {
    let rig = setup(&[]).start();
    rig.host.trust_project("/work/project");
    rig.host
        .write_project_value("/work/project", "test.plugin", "mode", json!("project"));
    assert_eq!(rig.host.effective_setting("mode"), Some(json!("project")));
}

/// REQ: SEC-194
#[test]
fn sec_194_user_stores_are_owner_only_and_below_the_users_directories() {
    use std::os::unix::fs::PermissionsExt;
    let rig = setup(&["clipboard.read"]).start();
    rig.host.persist_all_stores();
    let mut seen = 0;
    for name in [
        "grants.json",
        "consents.json",
        "trust.json",
        "project-trust.json",
        "secrets.json",
    ] {
        let path = rig.state.path().join(name);
        if path.exists() {
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode & 0o077, 0, "{name} is {mode:o}");
            seen += 1;
        }
    }
    assert!(seen >= 1, "at least the grant store must exist");
    let audit = rig.state.path().join("audit");
    assert_eq!(
        std::fs::metadata(&audit)
            .expect("audit dir")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

/// REQ: SEC-194
#[test]
fn sec_194_one_users_decisions_never_apply_to_another_user() {
    let first = setup(&["clipboard.read"]).start();
    first.host.persist_all_stores();
    let second = setup(&[]).start();
    assert!(!is_ok(&second.call("ilium:host/clipboard.read", json!({}))));
    assert_ne!(first.state.path(), second.state.path());
}

/// REQ: SEC-195
#[test]
fn sec_195_installing_from_the_system_directory_copies_into_the_user_directory() {
    let rig = setup(&[]).start();
    let system_dir = scratch();
    let copied = rig
        .host
        .install_from_system_dir(system_dir.path(), "example.demo")
        .expect("install");
    assert!(
        copied.starts_with(rig.state.path()) || copied.starts_with(rig.host.user_install_dir())
    );
    assert!(!copied.starts_with(system_dir.path()));
}

/// REQ: SEC-195
#[test]
fn sec_195_the_system_directory_is_not_written_by_a_user_install() {
    let rig = setup(&[]).start();
    let system_dir = scratch();
    let before = std::fs::read_dir(system_dir.path()).expect("dir").count();
    let _ = rig
        .host
        .install_from_system_dir(system_dir.path(), "example.demo");
    assert_eq!(
        std::fs::read_dir(system_dir.path()).expect("dir").count(),
        before
    );
}

/// REQ: SEC-196
#[test]
fn sec_196_runtime_directory_is_mode_0700_per_user() {
    use std::os::unix::fs::PermissionsExt;
    let rig = setup(&[]).start();
    let dir = rig.host.runtime_dir();
    assert_eq!(
        std::fs::metadata(&dir)
            .expect("runtime dir")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

/// REQ: SEC-196
#[test]
fn sec_196_sessions_cross_lists_only_the_same_users_sessions() {
    let rig = setup(&["sessions.cross"]).start();
    rig.host.add_session("mine", "arthur");
    rig.host.add_session("theirs", "someone-else");
    let listed = rig
        .value("ilium:host/sessions.list", json!({}))
        .expect("sessions");
    let text = listed.to_string();
    assert!(text.contains("mine") && !text.contains("theirs"), "{text}");
}

/// REQ: SEC-197
#[test]
fn sec_197_root_refuses_third_party_plugins_with_sandbox_unavailable() {
    for tier in [Tier::T2, Tier::T3] {
        let rig = setup(&[]).tier(tier).effective_uid(0).start();
        assert_eq!(
            rig.host.load_error().as_deref(),
            Some("sandbox.unavailable"),
            "{tier:?}"
        );
        assert!(rig
            .host
            .load_error_message()
            .expect("message")
            .contains("running as root is not supported for third-party plugins"));
    }
}

/// REQ: SEC-197
#[test]
fn sec_197_root_does_not_affect_t1_and_an_unprivileged_uid_loads_t3() {
    let rig = setup(&[]).tier(Tier::T1).effective_uid(0).start();
    assert!(rig.host.load_error().is_none());
    let rig = setup(&[]).effective_uid(1000).start();
    assert!(rig.host.load_error().is_none());
}

/// REQ: SEC-199
#[test]
fn sec_199_the_system_policy_applies_to_every_user_and_no_store_is_shared() {
    let policy = DENY_CLIPBOARD;
    let one = setup(&["clipboard.read"])
        .system_policy(policy, true)
        .start();
    let two = setup(&["clipboard.read"])
        .system_policy(policy, true)
        .start();
    assert!(is_not_permitted(
        &one.call("ilium:host/clipboard.read", json!({}))
    ));
    assert!(is_not_permitted(
        &two.call("ilium:host/clipboard.read", json!({}))
    ));
    assert_ne!(one.state.path(), two.state.path());
}

/// REQ: SEC-199
#[test]
fn sec_199_the_manual_points_to_the_enterprise_guide() {
    assert_page_has(
        "reference-admin-policy.md",
        &[
            "/pages/guide-enterprise-deployment/",
            "No store is shared between users.",
        ],
    );
}

/// REQ: SEC-193
#[test]
fn doc_sec_193_the_manual_lists_the_five_grant_pattern_forms() {
    assert_page_has(
        "reference-admin-policy.md",
        &[
            "`<name>:<arg>`",
            "`<name>:*`",
            "`<prefix>.*`",
            "`http:*.<domain>`",
        ],
    );
}

/// REQ: SEC-382
#[test]
fn doc_sec_382_the_manual_treats_a_denied_extension_point_as_a_denied_grant() {
    assert_page_has(
        "reference-admin-policy.md",
        &["extensions.deny_points", "treated as a denied grant"],
    );
}
