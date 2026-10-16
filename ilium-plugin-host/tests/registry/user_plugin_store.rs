//! user-plugin-store.md: the Browse tab (PKG-530..541). The tab has no view model in the
//! skeleton, so the behaviour is checked through its CLI equivalents (`search`, `info`,
//! `install`, `update`) and the page text. See API_GAPS_p2.md.

use crate::common::*;
use crate::pages_p2::*;
use serde_json::json;

const PAGE: &str = "user-plugin-store.md";
const NOTES: &str = "com.acme.notes";

const CATEGORIES: [&str; 11] = ["agents", "appearance", "scenes", "editors", "git", "inference", "notifications", "search", "terminal", "tools", "voice"];

/// REQ: PKG-530
#[test]
fn doc_pkg_530_one_row_per_plugin_with_name_publisher_version_tier_description() {
    assert_row(PAGE, "PKG-530", &["one row per plugin id", "name, publisher, latest version, tier badge", "short description"]);
}

/// REQ: PKG-530
#[test]
fn pkg_530_search_lists_each_plugin_id_once_with_its_latest_version() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["search"]);
    assert_ok(&output);
    let ids = plugin_ids(&output);
    for id in [NOTES, "com.acme.git-tools", "org.zeta.cafe-timer"] {
        assert_eq!(ids.iter().filter(|found| found.as_str() == id).count(), 1, "{id} once: {ids:?}");
    }
    assert!(output_contains(&output, "1.1.0"), "latest version of notes: {output:?}");
}

/// REQ: PKG-530
#[test]
fn pkg_530_each_row_carries_name_and_publisher() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["search", "notes"]);
    assert_ok(&output);
    assert!(output_contains(&output, "Acme"), "{output:?}");
    assert!(output_contains(&output, NOTES), "{output:?}");
}

/// REQ: PKG-530
#[test]
fn pkg_530_a_query_without_a_match_gives_no_rows() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["search", "zzzz-no-such-plugin"]);
    assert_ok(&output);
    assert!(plugin_ids(&output).is_empty(), "{output:?}");
}

/// REQ: PKG-531
#[test]
fn doc_pkg_531_the_filter_offers_only_the_closed_category_list() {
    assert_row(PAGE, "PKG-531", &["closed category list"]);
    let registry = page_text("reference-registry.md");
    for category in CATEGORIES {
        assert!(registry.contains(category), "category {category} is missing from the registry page");
    }
}

/// REQ: PKG-531
#[test]
fn pkg_531_a_known_category_filter_is_accepted() {
    for category in CATEGORIES {
        let sandbox = Sandbox::standard();
        let output = sandbox.run(&["search", "--category", category]);
        assert_ne!(output.exit_code, 2, "{category}: {output:?}");
    }
}

/// REQ: PKG-531
#[test]
fn pkg_531_a_category_outside_the_closed_list_is_refused_invalid_value() {
    let sandbox = Sandbox::standard();
    assert_failed(&sandbox.run(&["search", "--category", "games"]), "usage.invalid-value", 2);
}

/// REQ: PKG-532
#[test]
fn doc_pkg_532_empty_state_names_the_add_command_and_no_network() {
    assert_row(PAGE, "PKG-532", &["No registry is set up", "MUST NOT contact the network"]);
}

/// REQ: PKG-532
#[test]
fn pkg_532_without_a_registry_search_reports_none_configured_and_exit_1() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["search"]);
    assert_failed(&output, "registry.none-configured", 1);
}

/// REQ: PKG-532
#[test]
fn pkg_532_without_a_registry_no_request_leaves_the_machine() {
    let sandbox = Sandbox::new();
    let _ = sandbox.run(&["search"]);
    let _ = sandbox.run(&["search", "notes"]);
    assert!(sandbox.net.log().is_empty(), "{:?}", sandbox.net.log());
}

/// REQ: PKG-533
#[test]
fn doc_pkg_533_yanked_versions_are_hidden_and_deprecation_shows_the_successor() {
    assert_row(PAGE, "PKG-533", &["Yanked versions MUST NOT appear", "badge", "successor"]);
}

/// REQ: PKG-533
#[test]
fn pkg_533_a_yanked_latest_version_is_not_offered_and_the_previous_one_is() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions[1].yanked = Some("withdrawn".to_owned());
    let sandbox = Sandbox::with_registry(&builder);
    let output = sandbox.run(&["search", "notes"]);
    assert_ok(&output);
    assert!(!output_contains(&output, "1.1.0"), "yanked version shown: {output:?}");
    assert!(output_contains(&output, "1.0.0"), "{output:?}");
}

/// REQ: PKG-533
#[test]
fn pkg_533_a_deprecated_plugin_shows_its_message_and_successor() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].deprecated = Some(("Use the new plugin".to_owned(), "com.acme.notes2".to_owned()));
    let sandbox = Sandbox::with_registry(&builder);
    let output = sandbox.run(&["info", NOTES, "--json"]);
    assert_ok(&output);
    assert!(output_contains(&output, "com.acme.notes2"), "{output:?}");
    assert!(output_contains(&output, "Use the new plugin"), "{output:?}");
}

/// REQ: PKG-534
#[test]
fn doc_pkg_534_ratings_are_not_shown() {
    assert_row(PAGE, "PKG-534", &["ratings are not shown", "not in 0.1"].map(|s| if s == "ratings are not shown" { "User ratings are not shown" } else { s }));
}

/// REQ: PKG-534
#[test]
fn pkg_534_search_and_info_output_has_no_rating_field() {
    let sandbox = Sandbox::standard();
    for args in [vec!["search"], vec!["info", NOTES, "--json"]] {
        let output = sandbox.run(&args);
        assert_ok(&output);
        assert!(!output_contains(&output, "\"rating\""), "{args:?}: {output:?}");
        assert!(!output_contains(&output, "\"stars\""), "{args:?}: {output:?}");
    }
}

/// REQ: PKG-535
#[test]
fn doc_pkg_535_detail_lists_every_grant_with_its_risk_level() {
    assert_row(PAGE, "PKG-535", &["every requested grant", "risk level"]);
}

/// REQ: PKG-535
#[test]
fn pkg_535_info_lists_the_grants_of_the_latest_version() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["info", NOTES, "--json"]);
    assert_ok(&output);
    assert!(output_contains(&output, "grants"), "{output:?}");
}

/// REQ: PKG-536
#[test]
fn doc_pkg_536_detail_shows_publisher_id_and_key_status() {
    assert_row(PAGE, "PKG-536", &["publisher id", "`trusted`", "`unknown`", "`revoked`"]);
}

/// REQ: PKG-536
#[test]
fn pkg_536_info_shows_the_publisher_id_and_an_unknown_key_before_trust() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["info", NOTES, "--json"]);
    assert_ok(&output);
    assert!(output_contains(&output, PUBLISHER_ID), "{output:?}");
    assert!(output_contains(&output, "unknown"), "{output:?}");
}

/// REQ: PKG-536
#[test]
fn pkg_536_info_shows_trusted_after_the_key_was_trusted() {
    let sandbox = Sandbox::standard();
    assert_ok(&install_notes(&sandbox, &[]));
    let output = sandbox.run(&["info", NOTES, "--json"]);
    assert!(output_contains(&output, "trusted"), "{output:?}");
}

/// REQ: PKG-536
#[test]
fn pkg_536_info_shows_revoked_for_a_revoked_key() {
    let mut builder = RegistryBuilder::standard();
    builder.revoked_keys = vec![json!({ "publisher_id": PUBLISHER_ID, "key_id": publisher_key_id(), "reason": "compromised", "effective": at(-3600) })];
    let sandbox = Sandbox::with_registry(&builder);
    let output = sandbox.run(&["info", NOTES, "--json"]);
    assert!(output_contains(&output, "revoked"), "{output:?}");
}

/// REQ: PKG-537
#[test]
fn doc_pkg_537_revoked_key_or_blocked_id_has_no_install_button() {
    assert_row(PAGE, "PKG-537", &["revoked key or a blocked id", "Install button", "reason"]);
}

/// REQ: PKG-537
#[test]
fn pkg_537_a_blocked_id_cannot_be_installed_and_the_reason_is_shown() {
    let mut builder = RegistryBuilder::standard();
    builder.blocked_ids = vec![json!({ "id": NOTES, "reason": "malware", "at": at(-3600) })];
    let sandbox = Sandbox::with_registry(&builder);
    let output = install_notes(&sandbox, &[]);
    assert_ne!(output.exit_code, 0, "{output:?}");
    assert!(output_contains(&output, "malware"), "{output:?}");
    assert_eq!(sandbox.installed_version(NOTES), None);
}

/// REQ: PKG-537
#[test]
fn pkg_537_a_revoked_key_cannot_be_installed() {
    let mut builder = RegistryBuilder::standard();
    builder.revoked_keys = vec![json!({ "publisher_id": PUBLISHER_ID, "key_id": publisher_key_id(), "reason": "compromised", "effective": at(-3600) })];
    let sandbox = Sandbox::with_registry(&builder);
    assert_code(&install_notes(&sandbox, &[]), "trust.key-revoked");
    assert_eq!(sandbox.installed_version(NOTES), None);
}

/// REQ: PKG-537
#[test]
fn pkg_537_a_block_of_another_id_does_not_affect_this_plugin() {
    let mut builder = RegistryBuilder::standard();
    builder.blocked_ids = vec![json!({ "id": "com.other.thing", "reason": "x", "at": at(-3600) })];
    let sandbox = Sandbox::with_registry(&builder);
    assert_ok(&install_notes(&sandbox, &[]));
}

/// REQ: PKG-538
#[test]
fn doc_pkg_538_store_install_uses_the_file_install_pipeline() {
    assert_row(PAGE, "PKG-538", &["same pipeline and dialog as a file install"]);
}

/// REQ: PKG-538
#[test]
fn pkg_538_a_registry_install_and_a_file_install_leave_the_same_installed_files() {
    let key = publisher_key_id();
    let from_registry = Sandbox::standard();
    assert_ok(&install_notes(&from_registry, &[]));
    let from_file = Sandbox::new();
    let path = from_file.project_file("notes.iliumpkg", &signed_package(NOTES, "1.1.0"));
    assert_ok(&from_file.run(&["install", path.to_str().expect("utf-8 path"), "--yes", "--trust", key.as_str()]));
    assert_eq!(from_registry.installed_version(NOTES), from_file.installed_version(NOTES));
}

/// REQ: PKG-538
#[test]
fn pkg_538_a_registry_install_without_trust_needs_the_same_decision_as_a_file() {
    let sandbox = Sandbox::standard();
    let registry = sandbox.run(&["install", NOTES, "--yes"]);
    let path = sandbox.project_file("notes.iliumpkg", &signed_package(NOTES, "1.1.0"));
    let file = sandbox.run(&["install", path.to_str().expect("utf-8 path"), "--yes"]);
    assert_eq!(registry.exit_code, file.exit_code, "{registry:?} / {file:?}");
    assert_eq!(error_codes(&registry), error_codes(&file));
}

/// REQ: PKG-539
#[test]
fn doc_pkg_539_update_badge_and_no_install_without_a_choice() {
    assert_row(PAGE, "PKG-539", &["update badge", "MUST NOT install it unless the user chooses", "`auto_update = install`"]);
}

/// REQ: PKG-539
#[test]
fn pkg_539_search_and_info_never_install_a_newer_version() {
    let sandbox = Sandbox::standard();
    let key = publisher_key_id();
    assert_ok(&sandbox.run(&["install", "com.acme.notes@1.0.0", "--yes", "--trust", key.as_str()]));
    let _ = sandbox.run(&["search"]);
    let _ = sandbox.run(&["info", NOTES]);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: PKG-539
#[test]
fn pkg_539_an_explicit_update_installs_the_newer_version() {
    let sandbox = Sandbox::standard();
    let key = publisher_key_id();
    assert_ok(&sandbox.run(&["install", "com.acme.notes@1.0.0", "--yes", "--trust", key.as_str()]));
    assert_ok(&sandbox.run(&["update", NOTES, "--yes"]));
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-539
#[test]
fn pkg_539_an_already_latest_plugin_has_nothing_to_update() {
    let sandbox = Sandbox::standard();
    assert_ok(&install_notes(&sandbox, &[]));
    let _ = sandbox.run(&["update", NOTES, "--yes"]);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-540
#[test]
fn doc_pkg_540_offline_shows_cache_age_and_marks_stale() {
    assert_row(PAGE, "PKG-540", &["offline", "cache age", "stale"].map(|s| if s == "offline" { "When offline" } else { s }));
}

/// REQ: PKG-540
#[test]
fn pkg_540_offline_search_after_a_refresh_succeeds_with_a_warning() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["search"]));
    let output = sandbox.run_at(now_plus(3600), &["search", "--offline"]);
    assert_ok(&output);
    assert!(!warning_codes(&output).is_empty(), "stale marker expected: {output:?}");
    assert!(!plugin_ids(&output).is_empty());
}

/// REQ: PKG-540
#[test]
fn pkg_540_offline_without_any_cache_has_no_rows() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["search", "--offline"]);
    assert!(plugin_ids(&output).is_empty(), "{output:?}");
}

/// REQ: PKG-541
#[test]
fn doc_pkg_541_expired_metadata_is_refused_with_metadata_expired() {
    assert_row(PAGE, "PKG-541", &["older than the expiry rule", "`registry.metadata-expired`"]);
}

/// REQ: PKG-541
#[test]
fn pkg_541_metadata_past_the_expiry_is_refused_exit_8() {
    let sandbox = Sandbox::standard();
    sandbox.net.set_down(true);
    let output = sandbox.run_at(now_plus(86_400 + 1), &["search"]);
    assert_eq!(output.exit_code, 8, "{output:?}");
    assert!(has_error(&output, "registry.metadata-expired") || has_error(&output, "registry.stale"), "{output:?}");
    assert!(plugin_ids(&output).is_empty());
}

/// REQ: PKG-541
#[test]
fn pkg_541_metadata_just_inside_the_expiry_is_shown() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run_at(now_plus(86_400 - 1), &["search"]);
    assert_ok(&output);
    assert!(!plugin_ids(&output).is_empty());
}

/// REQ: PKG-541
#[test]
fn pkg_541_a_registry_that_serves_an_already_expired_timestamp_is_refused() {
    let mut builder = RegistryBuilder::standard();
    builder.timestamp_expires = at(-60);
    let sandbox = Sandbox::with_registry(&builder);
    let output = sandbox.run(&["search"]);
    assert_eq!(output.exit_code, 8, "{output:?}");
    assert!(plugin_ids(&output).is_empty());
}
