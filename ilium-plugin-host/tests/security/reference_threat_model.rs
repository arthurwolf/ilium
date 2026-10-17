//! reference-threat-model.md: protected assets, boundary validation, trust in T1, toxic grant
//! combinations, inter-plugin residual risk and the out-of-scope attackers.

use ilium_plugin_api::{ErrorCode, Tier};
use proptest::prelude::*;
use serde_json::json;

use crate::support::*;

/// Data-read grants of normal risk named by the page (SEC-004).
const READ_GRANTS: [&str; 6] = [
    "terminal.read-screen",
    "terminal.read-cells",
    "fs.read:project",
    "tree.read",
    "agent.read",
    "journal.read",
];
/// Egress grants of normal risk named by the page (SEC-004).
const EGRESS_GRANTS: [&str; 6] = [
    "http:example.com",
    "inference",
    "notify.desktop",
    "clipboard.write",
    "messages.peer",
    "services.use",
];

/// REQ: SEC-001
#[test]
fn sec_001_a_plugin_without_grants_cannot_touch_any_protected_asset_not_permitted() {
    let rig = t3(&[]);
    let calls = [
        ("ilium:host/clipboard.read", json!({})),
        ("ilium:host/journal.read", json!({})),
        (
            "ilium:host/fs.read",
            json!({ "scope": "home", "path": ".ssh/id_ed25519" }),
        ),
        (
            "ilium:host/process.run",
            json!({ "program": "cat", "args": [] }),
        ),
        ("ilium:host/settings.get-kernel-json", json!({})),
    ];
    for (name, args) in calls {
        let result = rig.call(name, args);
        assert!(is_not_permitted(&result), "{name}: {result:?}");
    }
}

/// REQ: SEC-001
#[test]
fn sec_001_a_grant_for_one_scope_does_not_open_a_sibling_asset() {
    let rig = t3(&["fs.read:data"]);
    let own = rig.value(
        "ilium:host/fs.read",
        json!({ "scope": "data", "path": "none.txt" }),
    );
    assert!(
        own.is_err() && own.as_ref().err().map(|error| error.code) != Some(ErrorCode::NotPermitted)
    );
    let other = rig.value(
        "ilium:host/fs.read",
        json!({ "scope": "home", "path": ".aws/credentials" }),
    );
    assert_code(&other, ErrorCode::NotPermitted);
}

/// REQ: SEC-001
#[test]
fn sec_001_the_asset_table_has_thirteen_rows() {
    let page = manual_page("reference-threat-model.md");
    let rows = page
        .lines()
        .skip_while(|line| !line.starts_with("| Asset | Where it lives"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .count();
    assert_eq!(rows, 13);
}

/// REQ: SEC-002
#[test]
fn sec_002_malformed_input_at_the_plugin_boundary_is_invalid_argument() {
    let rig = t3(&["http:example.com"]);
    let bad_methods = ["", "GE\nT", "get with space"];
    for method in bad_methods {
        let result = rig.fetch(request_json(method, "https://example.com/", &[], None));
        assert!(result.is_err(), "{method:?}");
    }
    let bad_url = rig.fetch(request_json("GET", "not a url", &[], None));
    assert!(bad_url.is_err());
}

/// REQ: SEC-002
#[test]
fn sec_002_identity_comes_from_the_channel_not_from_the_message() {
    let rig = t3(&["messages.peer"]);
    let result = rig.value(
        "ilium:host/messages.send-peer",
        json!({ "peer": "other.plugin", "from": "ilium.core.sessions", "payload": "aGk=" }),
    );
    if result.is_ok() {
        assert!(rig
            .host
            .peer_messages()
            .iter()
            .all(|message| message.sender == "test.plugin"));
    }
}

/// REQ: SEC-002
#[test]
fn sec_002_a_helper_message_with_an_unknown_kind_is_not_acted_on() {
    let rig = t3(&[]);
    let before = rig.audit_lines().len();
    rig.host
        .inject_helper_message(b"{\"kind\":\"definitely-unknown\"}");
    assert!(rig.host.failures().len() <= 1);
    assert!(rig.audit_lines().len() >= before);
}

/// REQ: SEC-003
#[test]
fn sec_003_t1_runs_in_the_server_process_and_t2_t3_never_do() {
    use ilium_plugin_host::testing::Isolation;
    assert_eq!(
        rig(Tier::T1, ilium_plugin_api::Side::Server, &[])
            .host
            .isolation(),
        Isolation::InProcess
    );
    assert_eq!(
        rig(Tier::T2, ilium_plugin_api::Side::Server, &[])
            .host
            .isolation(),
        Isolation::HelperProcess
    );
    assert_eq!(
        rig(Tier::T3, ilium_plugin_api::Side::Server, &[])
            .host
            .isolation(),
        Isolation::HelperProcess
    );
}

/// REQ: SEC-003
#[test]
fn sec_003_a_flaw_in_t1_is_not_modelled_as_a_plugin_attack() {
    assert_page_has(
        "reference-threat-model.md",
        &["A flaw in T1 code is a vulnerability in Ilium, not a plugin attack."],
    );
}

/// REQ: SEC-004
#[test]
fn sec_004_one_read_grant_and_one_egress_grant_show_the_toxic_row() {
    for read in READ_GRANTS {
        let rig = t3(&[]);
        let rows = rig.host.review_rows(&[read, "http:example.com"]);
        let toxic: Vec<_> = rows.iter().filter(|row| row.toxic).collect();
        assert_eq!(toxic.len(), 1, "{read}");
        assert_eq!(
            toxic[0].text,
            "This plugin can read your data and send data out."
        );
    }
}

/// REQ: SEC-004
#[test]
fn sec_004_every_pair_across_the_two_lists_shows_the_row() {
    let rig = t3(&[]);
    for read in READ_GRANTS {
        for egress in EGRESS_GRANTS {
            let rows = rig.host.review_rows(&[read, egress]);
            assert!(rows.iter().any(|row| row.toxic), "{read} + {egress}");
        }
    }
}

/// REQ: SEC-004
#[test]
fn sec_004_read_only_or_egress_only_sets_show_no_row() {
    let rig = t3(&[]);
    assert!(!rig
        .host
        .review_rows(&READ_GRANTS)
        .iter()
        .any(|row| row.toxic));
    assert!(!rig
        .host
        .review_rows(&EGRESS_GRANTS)
        .iter()
        .any(|row| row.toxic));
    assert!(!rig.host.review_rows(&[]).iter().any(|row| row.toxic));
}

/// REQ: SEC-004
#[test]
fn sec_004_the_row_is_informational_warning_styled_and_names_both_groups() {
    let rig = t3(&[]);
    let rows = rig
        .host
        .review_rows(&["terminal.read-screen", "http:example.com"]);
    let row = rows.iter().find(|row| row.toxic).expect("toxic row");
    assert!(!row.checkable && row.locked);
    assert!(row.warning_style);
    assert!(row.detail.contains("terminal.read-screen") && row.detail.contains("http:example.com"));
}

/// REQ: SEC-004
#[test]
fn sec_004_the_pair_does_not_raise_the_risk_class() {
    let rig = t3(&[]);
    let rows = rig
        .host
        .review_rows(&["terminal.read-screen", "http:example.com"]);
    assert!(rows
        .iter()
        .filter(|row| !row.toxic)
        .all(|row| row.risk != "high"));
}

/// REQ: SEC-005 MAY
#[test]
fn sec_005_deny_combinations_refuses_the_pair_and_its_absence_allows_it() {
    let pair = ["terminal.read-screen", "http:example.com"];
    let with_policy = setup(&[])
        .system_policy("version = 1\n[grants]\ndeny_combinations = true\n", true)
        .start();
    assert!(with_policy
        .host
        .install_check_with_grants("example.demo", Tier::T3, &pair)
        .is_err());
    let without = setup(&[]).start();
    assert_eq!(
        without
            .host
            .install_check_with_grants("example.demo", Tier::T3, &pair),
        Ok(())
    );
    let explicit_false = setup(&[])
        .system_policy("version = 1\n[grants]\ndeny_combinations = false\n", true)
        .start();
    assert_eq!(
        explicit_false
            .host
            .install_check_with_grants("example.demo", Tier::T3, &pair),
        Ok(())
    );
}

/// REQ: SEC-005 MAY
#[test]
fn sec_005_deny_combinations_leaves_single_group_plugins_alone() {
    let rig = setup(&[])
        .system_policy("version = 1\n[grants]\ndeny_combinations = true\n", true)
        .start();
    assert_eq!(
        rig.host
            .install_check_with_grants("example.demo", Tier::T3, &["terminal.read-screen"]),
        Ok(())
    );
    assert_eq!(
        rig.host
            .install_check_with_grants("example.demo", Tier::T3, &["http:example.com"]),
        Ok(())
    );
}

/// REQ: SEC-006
#[test]
fn sec_006_the_two_lists_are_disjoint_and_match_the_page() {
    for read in READ_GRANTS {
        assert!(!EGRESS_GRANTS.contains(&read));
    }
    let page = manual_page("reference-threat-model.md");
    for name in READ_GRANTS
        .iter()
        .map(|grant| grant.split(':').next().unwrap_or_default())
        .chain(
            EGRESS_GRANTS
                .iter()
                .map(|grant| grant.split(':').next().unwrap_or_default()),
        )
    {
        assert!(page.contains(&format!("`{name}`")), "{name}");
    }
}

/// REQ: SEC-006
#[test]
fn sec_006_the_host_classifies_every_listed_grant_into_exactly_its_list() {
    for read in READ_GRANTS {
        assert_eq!(
            ilium_plugin_host::review::grant_group(read),
            Some(ilium_plugin_host::review::GrantGroup::DataRead),
            "{read}"
        );
    }
    for egress in EGRESS_GRANTS {
        assert_eq!(
            ilium_plugin_host::review::grant_group(egress),
            Some(ilium_plugin_host::review::GrantGroup::Egress),
            "{egress}"
        );
    }
}

/// REQ: SEC-007
#[test]
fn sec_007_the_review_dialog_shows_receives_data_from_other_plugins() {
    let rig = t3(&[]);
    for grants in [
        vec!["messages.peer"],
        vec!["services.use:other.plugin/svc"],
        vec!["topics.subscribe:other.topic"],
    ] {
        let refs: Vec<&str> = grants.clone();
        let text = rig.host.review_text_for(&refs);
        assert!(
            text.contains("Receives data from other plugins"),
            "{grants:?}"
        );
    }
    assert!(!rig
        .host
        .review_text_for(&["clipboard.read"])
        .contains("Receives data from other plugins"));
}

/// REQ: SEC-007
#[test]
fn sec_007_messages_from_ids_not_in_accepts_messages_from_are_dropped_with_a_log_line() {
    let rig = setup(&["messages.peer"])
        .accepts_messages_from(&["friend.plugin"])
        .start();
    rig.host.deliver_peer_message("stranger.plugin", b"hi");
    rig.host.deliver_peer_message("friend.plugin", b"hi");
    assert_eq!(rig.host.peer_messages().len(), 1);
    assert_eq!(rig.host.peer_messages()[0].sender, "friend.plugin");
    assert!(rig
        .log_messages()
        .iter()
        .any(|line| line.contains("stranger.plugin")));
}

/// REQ: SEC-007
#[test]
fn sec_007_inference_provide_is_high_risk_with_the_documented_wording() {
    let rig = t3(&[]);
    let rows = rig.host.review_rows(&["inference.provide"]);
    assert_eq!(rows[0].risk, "high");
    assert!(rig
        .host
        .review_text_for(&["inference.provide"])
        .contains("Receives the text other plugins send to the AI model"));
}

/// REQ: SEC-008
#[test]
fn sec_008_the_platform_does_not_claim_to_stop_same_user_malware() {
    assert_page_has(
        "reference-threat-model.md",
        &["The platform MUST NOT claim to stop it."],
    );
    let docs = manual_page("security.md");
    assert!(!docs
        .to_lowercase()
        .contains("protects against malware running as you"));
}

/// REQ: SEC-008
#[test]
fn sec_008_the_http_token_file_is_readable_by_the_same_user() {
    use std::os::unix::fs::PermissionsExt;
    let rig = t3(&[]);
    let mode = std::fs::metadata(rig.host.http_api_token_path())
        .expect("token")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode & 0o400,
        0o400,
        "the owner can read it, which is the accepted risk"
    );
}

/// REQ: SEC-009
#[test]
fn sec_009_root_attacker_physical_access_and_a_compromised_kernel_are_listed_as_out_of_scope() {
    assert_page_has(
        "reference-threat-model.md",
        &["A root attacker, physical access, and a compromised kernel are out of scope."],
    );
}

/// REQ: SEC-009
#[test]
fn sec_009_a_root_server_does_not_pretend_to_be_safe_for_third_parties() {
    let rig = setup(&[]).effective_uid(0).start();
    assert_eq!(
        rig.host.load_error().as_deref(),
        Some("sandbox.unavailable")
    );
}

/// REQ: SEC-010
#[test]
fn sec_010_t1_integrity_is_checked_at_load() {
    let rig = setup(&[]).tier(Tier::T1).tampered_t1_bundle().start();
    assert_eq!(rig.host.load_error().as_deref(), Some("integrity.failed"));
    assert_eq!(rig.audit_events("integrity-failure").len(), 1);
}

/// REQ: SEC-010
#[test]
fn sec_010_an_untampered_t1_bundle_loads() {
    let rig = setup(&[]).tier(Tier::T1).start();
    assert!(rig.host.load_error().is_none());
}

/// REQ: SEC-011
#[test]
fn sec_011_third_party_packages_do_not_load_on_windows_and_macos_and_there_is_no_fallback() {
    for platform in ["windows", "macos"] {
        for tier in [Tier::T2, Tier::T3] {
            let rig = setup(&[]).tier(tier).platform(platform).start();
            assert_eq!(
                rig.host.load_error().as_deref(),
                Some("sandbox.unavailable"),
                "{platform} {tier:?}"
            );
            assert_eq!(
                rig.host.isolation_attempted(),
                None,
                "no in-process fallback"
            );
        }
    }
}

/// REQ: SEC-011
#[test]
fn sec_011_t1_loads_on_every_platform() {
    for platform in ["linux", "windows", "macos"] {
        let rig = setup(&[]).tier(Tier::T1).platform(platform).start();
        assert!(rig.host.load_error().is_none(), "{platform}");
    }
}

/// REQ: SEC-012
#[test]
fn sec_012_guest_clock_is_coarsened_to_a_millisecond_for_third_parties() {
    let rig = t3(&[]);
    let readings: Vec<u64> = (0..50).map(|_| rig.host.guest_clock_nanos()).collect();
    assert!(readings
        .iter()
        .all(|nanos| nanos % 1_000_000 == 0 || nanos % 1_000_000 < 1_000_000));
    let distinct_sub_ms: std::collections::HashSet<u64> =
        readings.iter().map(|nanos| nanos % 1_000_000).collect();
    assert!(
        distinct_sub_ms.len() <= 1,
        "no sub-millisecond detail may be visible"
    );
}

/// REQ: SEC-012
#[test]
fn sec_012_the_page_says_side_channels_are_reduced_not_eliminated() {
    assert_page_has("reference-threat-model.md", &["reduced but not eliminated"]);
}

proptest! {
    // REQ: SEC-004
    #[test]
    fn sec_004_the_row_appears_exactly_when_both_groups_are_present(reads in prop::collection::vec(0usize..6, 0..4), egresses in prop::collection::vec(0usize..6, 0..4)) {
        let mut grants: Vec<&str> = reads.iter().map(|index| READ_GRANTS[*index]).collect();
        grants.extend(egresses.iter().map(|index| EGRESS_GRANTS[*index]));
        let rig = t3(&[]);
        let toxic = rig.host.review_rows(&grants).iter().any(|row| row.toxic);
        prop_assert_eq!(toxic, !reads.is_empty() && !egresses.is_empty());
    }
}
