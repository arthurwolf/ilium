//! guide-plugin-administration.md: stances, pre-trust, private registry, mirror, policy keys,
//! automatic updates (PKG-460..PKG-477). Lane `registry-publish`, part 2.

use crate::common::*;
use crate::pages_p2::*;
use ilium_plugin_host::registry_client::FetchError;
use serde_json::json;

const ADMIN: &str = "guide-plugin-administration.md";
const SLUG: &str = "guide-plugin-administration";
const NOTES: &str = "com.acme.notes";

fn trust_store_with(sandbox: &Sandbox, added_by: &str) -> std::path::PathBuf {
    let path = sandbox.project_dir().join("trust.json");
    let store = json!({
        "format": "ilium-trust/1",
        "keys": [{
            "key_id": publisher_key_id(),
            "publisher_id": PUBLISHER_ID,
            "publisher": "Acme Labs",
            "public_key": public_key_b64(PUBLISHER_SEED),
            "added": "2030-01-01 10:00:00",
            "added_by": added_by,
            "status": "trusted",
            "revoked": null
        }]
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&store).expect("store"))
        .expect("write trust store");
    path
}

// ---- Choose a stance (PKG-460..464) --------------------------------------------------------

/// REQ: PKG-460 WARN
#[test]
fn doc_pkg_460_company_only_uses_publishers_allow_and_allowed_sources() {
    assert_row(
        ADMIN,
        "PKG-460",
        &[
            "Allow only plugins from your company",
            "`publishers.allow`",
            "`packages.allowed_sources`",
        ],
    );
}

/// REQ: PKG-460 WARN
#[test]
fn pkg_460_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-460", SLUG, "MUST");
}

/// REQ: PKG-460 WARN
#[test]
fn pkg_460_publisher_not_in_publishers_allow_is_blocked_publisher_denied() {
    let sandbox = Sandbox::standard();
    let other = key_id_of(KEY_FOREIGN);
    write_policy(
        &sandbox,
        &format!("version = 1\n[publishers]\nallow = [\"{other}\"]\n"),
    );
    let output = install_notes(&sandbox, &[]);
    assert_policy_block(&output, "policy.publisher-denied");
    assert_eq!(sandbox.installed_version(NOTES), None);
}

/// REQ: PKG-460 WARN
#[test]
fn pkg_460_company_key_and_company_registry_together_allow_the_install() {
    let sandbox = Sandbox::standard();
    let key = publisher_key_id();
    write_policy(&sandbox, &format!("version = 1\n[publishers]\nallow = [\"{key}\"]\n[packages]\nallowed_sources = [\"{BASE}\"]\n"));
    assert_ok(&install_notes(&sandbox, &[]));
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-461 WARN
#[test]
fn doc_pkg_461_bundled_only_uses_allow_third_party_false() {
    assert_row(
        ADMIN,
        "PKG-461",
        &[
            "Allow only bundled plugins",
            "`packages.allow_third_party = false`",
        ],
    );
}

/// REQ: PKG-461 WARN
#[test]
fn pkg_461_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-461", SLUG, "MUST");
}

/// REQ: PKG-461 WARN
#[test]
fn pkg_461_third_party_install_is_blocked_third_party_denied() {
    let sandbox = Sandbox::standard();
    write_policy(
        &sandbox,
        "version = 1\n[packages]\nallow_third_party = false\n",
    );
    let output = install_notes(&sandbox, &[]);
    assert_policy_block(&output, "policy.third-party-denied");
    assert_eq!(sandbox.installed_version(NOTES), None);
}

/// REQ: PKG-462 WARN
#[test]
fn doc_pkg_462_one_plugin_is_blocked_by_deny_ids() {
    assert_row(
        ADMIN,
        "PKG-462",
        &[
            "Block one plugin",
            "`plugins.deny_ids = [\"com.example.bad\"]`",
        ],
    );
}

/// REQ: PKG-462 WARN
#[test]
fn pkg_462_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-462", SLUG, "MUST");
}

/// REQ: PKG-462 WARN
#[test]
fn pkg_462_denied_id_is_blocked_and_another_id_still_installs() {
    let sandbox = Sandbox::standard();
    write_policy(
        &sandbox,
        &format!("version = 1\n[plugins]\ndeny_ids = [\"{NOTES}\"]\n"),
    );
    assert_policy_block(&install_notes(&sandbox, &[]), "policy.id-denied");
    assert_eq!(sandbox.installed_version(NOTES), None);
    let key = publisher_key_id();
    let other = sandbox.run(&[
        "install",
        "com.acme.git-tools",
        "--yes",
        "--trust",
        key.as_str(),
    ]);
    assert_ok(&other);
}

/// REQ: PKG-463 WARN
#[test]
fn doc_pkg_463_risky_grant_is_blocked_by_grants_deny() {
    assert_row(
        ADMIN,
        "PKG-463",
        &[
            "Block a risky grant everywhere",
            "`grants.deny = [\"process.run:*\"]`",
        ],
    );
}

/// REQ: PKG-463 WARN
#[test]
fn pkg_463_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-463", SLUG, "MUST");
}

/// REQ: PKG-463 WARN
#[test]
fn pkg_463_a_denied_grant_never_fails_the_install_of_a_plugin_without_it() {
    let sandbox = Sandbox::standard();
    write_policy(
        &sandbox,
        "version = 1\n[grants]\ndeny = [\"process.run:*\"]\n",
    );
    assert_ok(&install_notes(&sandbox, &[]));
}

fn registry_with_beta() -> RegistryBuilder {
    let mut builder = RegistryBuilder::standard();
    let mut beta = VersionSpec::signed(NOTES, "1.2.0-beta.1");
    beta.channel = "beta".to_owned();
    builder.plugins[0].versions.push(beta);
    builder
}

/// REQ: PKG-464 WARN
#[test]
fn doc_pkg_464_beta_builds_are_stopped_by_allowed_channels() {
    assert_row(
        ADMIN,
        "PKG-464",
        &[
            "Stop beta builds",
            "`packages.allowed_channels = [\"stable\"]`",
        ],
    );
}

/// REQ: PKG-464 WARN
#[test]
fn pkg_464_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-464", SLUG, "MUST");
}

/// REQ: PKG-464 WARN, PKG-472 WARN
#[test]
fn pkg_464_beta_channel_install_is_refused_by_the_stable_only_policy_and_allowed_without_it() {
    let builder = registry_with_beta();
    let free = Sandbox::with_registry(&builder);
    assert_ok(&install_notes(&free, &["--channel", "beta"]));
    let sandbox = Sandbox::with_registry(&builder);
    write_policy(
        &sandbox,
        "version = 1\n[packages]\nallowed_channels = [\"stable\"]\n",
    );
    let refused = install_notes(&sandbox, &["--channel", "beta"]);
    assert_eq!(refused.exit_code, 7, "{refused:?}");
    assert_eq!(sandbox.installed_version(NOTES), None);
    assert_ok(&install_notes(&sandbox, &["--channel", "stable"]));
}

// ---- Pre-trust your key (PKG-465, 466) -----------------------------------------------------

/// REQ: PKG-465 MAY
#[test]
fn doc_pkg_465_admin_key_is_added_with_system_and_cannot_be_removed_by_the_user() {
    assert_row(
        ADMIN,
        "PKG-465",
        &[
            "ilium plugin trust add <file> --tier T2 --system",
            "`added_by: admin`",
            "the user cannot remove it",
        ],
    );
}

/// REQ: PKG-465 MAY
#[test]
fn pkg_465_a_key_marked_admin_is_not_removed_by_trust_remove() {
    let sandbox = Sandbox::new();
    let store = trust_store_with(&sandbox, "admin");
    let key = publisher_key_id();
    let path = store.to_str().expect("utf-8 path").to_owned();
    let output = sandbox.run_with_vars(
        &[("ILIUM_TRUST_STORE", path.as_str())],
        &["trust", "remove", key.as_str()],
    );
    assert_ne!(
        output.exit_code, 0,
        "the user cannot remove an admin key: {output:?}"
    );
    let kept = std::fs::read_to_string(&store).expect("store");
    assert!(
        kept.contains(&key) && kept.contains("\"added_by\": \"admin\""),
        "{kept}"
    );
}

/// REQ: PKG-465 MAY
#[test]
fn pkg_465_a_key_the_user_added_can_be_removed_so_the_admin_rule_is_not_general() {
    let sandbox = Sandbox::new();
    let store = trust_store_with(&sandbox, "user");
    let key = publisher_key_id();
    let path = store.to_str().expect("utf-8 path").to_owned();
    let output = sandbox.run_with_vars(
        &[("ILIUM_TRUST_STORE", path.as_str())],
        &["trust", "remove", key.as_str()],
    );
    assert_ok(&output);
    let after = std::fs::read_to_string(&store).expect("store");
    assert!(
        !after.contains(&key),
        "the user entry must be gone: {after}"
    );
}

/// REQ: PKG-465 MAY
#[test]
fn pkg_465_without_any_admin_key_the_install_still_works_with_the_user_trust_flag() {
    let sandbox = Sandbox::standard();
    assert_ok(&install_notes(&sandbox, &[]));
}

/// REQ: PKG-466 WARN
#[test]
fn doc_pkg_466_a_trusted_key_never_overrides_a_revocation() {
    assert_row(
        ADMIN,
        "PKG-466",
        &[
            "never overrides a revocation",
            "A revoked key stays revoked",
        ],
    );
}

/// REQ: PKG-466 WARN
#[test]
fn pkg_466_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-466", SLUG, "never overrides a revocation");
}

/// REQ: PKG-466 WARN
#[test]
fn pkg_466_a_key_trusted_by_flag_is_still_refused_after_the_registry_revokes_it() {
    let mut builder = RegistryBuilder::standard();
    builder.revoked_keys = vec![
        json!({ "publisher_id": PUBLISHER_ID, "key_id": publisher_key_id(), "reason": "compromised", "effective": at(-3600) }),
    ];
    let sandbox = Sandbox::with_registry(&builder);
    let output = install_notes(&sandbox, &[]);
    assert_code(&output, "trust.key-revoked");
    assert_eq!(sandbox.installed_version(NOTES), None);
}

// ---- Run a private registry (PKG-467..469) -------------------------------------------------

/// REQ: PKG-467
#[test]
fn doc_pkg_467_registry_must_be_served_over_https() {
    assert_row(
        ADMIN,
        "PKG-467",
        &["MUST be served over HTTPS", "`localhost`", "PKG-371"],
    );
}

/// REQ: PKG-467
#[test]
fn pkg_467_registry_add_with_a_plain_http_url_is_refused_invalid_value() {
    let sandbox = Sandbox::new();
    sandbox.net.serve(
        "http://plugins.example.org/v1/",
        &RegistryBuilder::standard().build(),
    );
    let output = sandbox.pin(
        "http://plugins.example.org/v1/",
        &RegistryBuilder::standard().pinned_key_id(),
    );
    assert_code(&output, "usage.invalid-value");
    assert!(
        sandbox.net.log().is_empty(),
        "no request for a refused URL: {:?}",
        sandbox.net.log()
    );
}

/// REQ: PKG-467
#[test]
fn pkg_467_a_configured_http_url_on_another_host_is_refused_insecure_url() {
    let sandbox = Sandbox::new();
    sandbox.set_registry_setting(
        "urls",
        toml::Value::Array(vec![toml::Value::String(
            "http://plugins.example.org/v1/".to_owned(),
        )]),
    );
    assert_code(&sandbox.run(&["search"]), "registry.insecure-url");
    assert!(sandbox.net.log().is_empty());
}

/// REQ: PKG-467
#[test]
fn pkg_467_https_registry_is_accepted_by_registry_add() {
    let sandbox = Sandbox::new();
    let builder = RegistryBuilder::standard();
    sandbox.net.serve(BASE, &builder.build());
    assert_ok(&sandbox.pin(BASE, &builder.pinned_key_id()));
}

/// REQ: PKG-467
#[test]
fn pkg_467_http_localhost_is_accepted_only_with_the_development_setting() {
    for allowed in [false, true] {
        let builder = RegistryBuilder::standard();
        let sandbox = Sandbox::new();
        sandbox
            .net
            .serve("http://localhost:8080/", &builder.build());
        sandbox.set_registry_setting(
            "urls",
            toml::Value::Array(vec![toml::Value::String(
                "http://localhost:8080/".to_owned(),
            )]),
        );
        sandbox.set_registry_setting("allow_insecure_localhost", toml::Value::Boolean(allowed));
        sandbox.set_registry_setting(
            "root",
            toml::Value::Array(vec![toml::Value::String(builder.pinned_key_id())]),
        );
        let output = sandbox.run(&["search"]);
        assert_eq!(
            !has_error(&output, "registry.insecure-url"),
            allowed,
            "allowed={allowed}: {output:?}"
        );
    }
}

/// REQ: PKG-468
#[test]
fn doc_pkg_468_timestamp_expires_after_one_day_and_must_be_resigned_daily() {
    assert_row(
        ADMIN,
        "PKG-468",
        &[
            "expires after one day",
            "MUST re-sign it at least daily",
            "`registry.stale`",
        ],
    );
}

/// REQ: PKG-468
#[test]
fn pkg_468_a_timestamp_one_second_past_one_day_is_reported_with_exit_8() {
    let sandbox = Sandbox::standard();
    sandbox.net.set_down(true);
    let output = sandbox.run_at(now_plus(86_400 + 1), &["search"]);
    assert_eq!(output.exit_code, 8, "{output:?}");
    // The guide names registry.stale, the registry page names registry.metadata-expired (PKG-365).
    assert!(
        has_error(&output, "registry.stale") || has_error(&output, "registry.metadata-expired"),
        "{output:?}"
    );
}

/// REQ: PKG-468
#[test]
fn pkg_468_a_timestamp_one_second_before_one_day_is_still_fresh() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run_at(now_plus(86_400 - 1), &["search"]));
}

/// REQ: PKG-468
#[test]
fn pkg_468_an_operator_who_re_signs_daily_keeps_clients_working() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run(&["search"]));
    let mut renewed = RegistryBuilder::standard();
    renewed.timestamp_version = 4822;
    renewed.timestamp_expires = at(2 * 86_400);
    sandbox.net.serve(BASE, &renewed.build());
    assert_ok(&sandbox.run_at(now_plus(86_400 + 60), &["search"]));
}

/// REQ: PKG-469 WARN
#[test]
fn doc_pkg_469_root_rotation_document_is_signed_by_old_and_new_key() {
    assert_row(
        ADMIN,
        "PKG-469",
        &[
            "rotation document signed by the old and the new key",
            "/pages/reference-registry/#rootjson",
        ],
    );
    let registry = page_text("reference-registry.md");
    assert!(
        registry.contains("### root.json"),
        "the linked section must exist"
    );
}

/// REQ: PKG-469 WARN
#[test]
fn pkg_469_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-469", SLUG, "MUST");
}

// ---- Policy keys (PKG-470..472, 475, 476) ---------------------------------------------------

/// REQ: PKG-470 WARN, PKG-471 WARN, PKG-472 WARN
#[test]
fn doc_pkg_470_to_472_policy_key_names_are_listed_with_their_ids() {
    let line = row(ADMIN, "PKG-470");
    for part in [
        "`packages.allowed_sources` [PKG-470]",
        "`packages.allow_url_installs` [PKG-471]",
        "`packages.allowed_channels` [PKG-472]",
    ] {
        assert!(line.contains(part), "missing {part:?} in {line}");
    }
}

/// REQ: PKG-470 WARN
#[test]
fn pkg_470_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-470", SLUG, "is refused with `policy.blocked`");
}

/// REQ: PKG-471 WARN
#[test]
fn pkg_471_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-471", SLUG, "is refused with `policy.blocked`");
}

/// REQ: PKG-472 WARN
#[test]
fn pkg_472_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-472", SLUG, "is refused with `policy.blocked`");
}

/// REQ: PKG-470 WARN
#[test]
fn pkg_470_a_registry_outside_allowed_sources_is_refused_source_denied() {
    let sandbox = Sandbox::standard();
    write_policy(
        &sandbox,
        &format!("version = 1\n[packages]\nallowed_sources = [\"{BASE_TWO}\"]\n"),
    );
    let output = install_notes(&sandbox, &[]);
    assert_policy_block(&output, "policy.source-denied");
    assert_eq!(sandbox.installed_version(NOTES), None);
}

/// REQ: PKG-471 WARN
#[test]
fn pkg_471_url_installs_off_refuses_a_bare_https_url_but_not_a_registry_id() {
    let sandbox = Sandbox::standard();
    let url = "https://downloads.example.org/notes.iliumpkg";
    sandbox.net.put_file(url, signed_package(NOTES, "1.0.0"));
    write_policy(
        &sandbox,
        "version = 1\n[packages]\nallow_url_installs = false\n",
    );
    let key = publisher_key_id();
    let refused = sandbox.run(&["install", url, "--yes", "--trust", key.as_str()]);
    assert_policy_block(&refused, "policy.source-denied");
    assert_ok(&install_notes(&sandbox, &[]));
}

/// REQ: PKG-471 WARN
#[test]
fn pkg_471_url_installs_default_to_allowed() {
    let sandbox = Sandbox::standard();
    let url = "https://downloads.example.org/notes.iliumpkg";
    sandbox.net.put_file(url, signed_package(NOTES, "1.0.0"));
    let key = publisher_key_id();
    assert_ok(&sandbox.run(&["install", url, "--yes", "--trust", key.as_str()]));
}

fn install_with_sources(source: &str) -> ilium_plugin_host::cli::CliOutput {
    let sandbox = Sandbox::standard();
    write_policy(
        &sandbox,
        &format!("version = 1\n[packages]\nallowed_sources = [\"{source}\"]\n"),
    );
    install_notes(&sandbox, &[])
}

/// REQ: PKG-475 WARN
#[test]
fn doc_pkg_475_allowed_sources_compares_scheme_host_and_path_prefix() {
    assert_row(
        ADMIN,
        "PKG-475",
        &[
            "compares the scheme, host and path prefix",
            "A trailing slash does not matter",
        ],
    );
}

/// REQ: PKG-475 WARN
#[test]
fn pkg_475_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-475", SLUG, "SHOULD");
}

/// REQ: PKG-475 WARN
#[test]
fn pkg_475_a_trailing_slash_in_the_policy_or_the_url_does_not_matter() {
    for source in [
        "https://plugins.example.org/ilium/v1",
        "https://plugins.example.org/ilium/v1/",
    ] {
        assert_ok(&install_with_sources(source));
    }
}

/// REQ: PKG-475 WARN
#[test]
fn pkg_475_a_shorter_path_prefix_of_the_registry_url_matches() {
    assert_ok(&install_with_sources("https://plugins.example.org/ilium/"));
    assert_ok(&install_with_sources("https://plugins.example.org/"));
}

/// REQ: PKG-475 WARN
#[test]
fn pkg_475_another_scheme_host_or_path_does_not_match() {
    for source in [
        "http://plugins.example.org/ilium/v1/",
        "https://mirror.example.org/ilium/v1/",
        "https://plugins.example.org/other/",
        "https://plugins.example.org/ilium/v2/",
    ] {
        let output = install_with_sources(source);
        assert_policy_block(&output, "policy.source-denied");
    }
}

/// REQ: PKG-476 WARN
#[test]
fn doc_pkg_476_a_package_installed_before_the_policy_stays_and_is_disabled_by_policy() {
    assert_row(
        ADMIN,
        "PKG-476",
        &[
            "installed before the policy changed is not removed",
            "`disabled-by-policy`",
        ],
    );
}

/// REQ: PKG-476 WARN
#[test]
fn pkg_476_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-476", SLUG, "SHOULD");
}

/// REQ: PKG-476 WARN
#[test]
fn pkg_476_a_later_deny_does_not_remove_the_record_or_rewrite_installed_json() {
    let sandbox = Sandbox::standard();
    assert_ok(&install_notes(&sandbox, &[]));
    let before = sandbox.snapshot_of("data");
    write_policy(
        &sandbox,
        &format!("version = 1\n[plugins]\ndeny_ids = [\"{NOTES}\"]\n"),
    );
    let _ = sandbox.run(&["info", NOTES]);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
    let after = sandbox.snapshot_of("data");
    assert_eq!(
        before.get("plugins/installed.json"),
        after.get("plugins/installed.json"),
        "the state is not written to installed.json"
    );
}

// ---- Mirror and air-gap (PKG-473, 474) -----------------------------------------------------

fn mirror_copy(builder: &RegistryBuilder) -> Tree {
    let sandbox = Sandbox::with_registry(builder);
    let dir = sandbox.dir.path().join("mirror");
    std::fs::create_dir_all(&dir).expect("mirror folder");
    assert_ok(&sandbox.run(&[
        "mirror",
        "sync",
        dir.to_str().expect("utf-8 path"),
        "--registry",
        BASE,
    ]));
    read_tree(&dir)
}

/// REQ: PKG-473
#[test]
fn doc_pkg_473_a_mirror_is_never_signed_by_its_operator() {
    assert_row(
        ADMIN,
        "PKG-473",
        &[
            "MUST NOT be signed by the mirror operator",
            "signatures of the original registry",
        ],
    );
}

/// REQ: PKG-473
#[test]
fn pkg_473_every_mirrored_metadata_file_is_byte_identical_to_the_original() {
    let builder = RegistryBuilder::standard();
    let original = builder.build();
    let copied = mirror_copy(&builder);
    for name in [
        "timestamp.json",
        "snapshot.json",
        "index.json",
        "revocations.json",
    ] {
        assert_eq!(
            copied.get(name),
            original.get(name),
            "{name} must not change a signed byte"
        );
    }
}

/// REQ: PKG-473
#[test]
fn pkg_473_the_mirror_verifies_with_the_root_key_of_the_original_registry() {
    let builder = RegistryBuilder::standard();
    let copied = mirror_copy(&builder);
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE_TWO, &copied);
    assert_ok(&sandbox.pin(BASE_TWO, &builder.pinned_key_id()));
    let output = sandbox.run(&["search"]);
    assert_ok(&output);
    assert!(
        plugin_ids(&output).contains(&NOTES.to_owned()),
        "{output:?}"
    );
}

/// REQ: PKG-473
#[test]
fn pkg_473_a_mirror_re_signed_by_its_operator_is_refused() {
    let genuine = RegistryBuilder::standard();
    let mut operator = RegistryBuilder::standard();
    operator.role_keys = vec![KEY_FOREIGN];
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE_TWO, &operator.build());
    let _ = sandbox.pin(BASE_TWO, &genuine.pinned_key_id());
    let output = sandbox.run(&["search"]);
    assert_registry_failure(&output);
    assert_no_plugin_records(&output);
}

/// REQ: PKG-474 WARN
#[test]
fn doc_pkg_474_an_unreachable_mirror_never_falls_back_to_another_registry() {
    assert_row(
        ADMIN,
        "PKG-474",
        &[
            "`packages.allowed_sources` set to the mirror",
            "`registry.unreachable`",
            "never falls back to another registry",
        ],
    );
}

/// REQ: PKG-474 WARN
#[test]
fn pkg_474_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-474", SLUG, "SHOULD");
}

/// REQ: PKG-474 WARN
#[test]
fn pkg_474_unreachable_mirror_reports_registry_unreachable_and_never_asks_the_original() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE, &builder.build());
    sandbox.net.serve(BASE_TWO, &builder.build());
    assert_ok(&sandbox.pin(BASE, &builder.pinned_key_id()));
    assert_ok(&sandbox.pin(BASE_TWO, &builder.pinned_key_id()));
    write_policy(
        &sandbox,
        &format!("version = 1\n[packages]\nallowed_sources = [\"{BASE_TWO}\"]\n"),
    );
    sandbox.net.script(
        &format!("{BASE_TWO}timestamp.json"),
        vec![FetchError::Unreachable; 10],
    );
    sandbox.net.clear_log();
    let output = sandbox.run(&["search"]);
    assert_code(&output, "registry.unreachable");
    assert!(
        sandbox.net.paths(BASE).is_empty(),
        "no fallback to the original: {:?}",
        sandbox.net.log()
    );
}

// ---- Automatic updates (PKG-477) -----------------------------------------------------------

/// REQ: PKG-477
#[test]
fn doc_pkg_477_automatic_update_runs_all_checks_and_never_prompts() {
    assert_row(
        ADMIN,
        "PKG-477",
        &[
            "MUST run all install checks",
            "MUST NOT install when new grants or a new signing key would need a prompt",
            "notifies only",
        ],
    );
}

fn registry_with_new_signing_key() -> RegistryBuilder {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions[1].package = package_signed_by(NOTES, "1.1.0", 77, PUBLISHER_ID);
    builder
}

/// REQ: PKG-477
#[test]
fn pkg_477_an_update_signed_by_a_new_key_is_not_installed_without_a_trust_decision() {
    let sandbox = Sandbox::with_registry(&registry_with_new_signing_key());
    let output = install_notes(&sandbox, &[]);
    assert_ne!(
        output.exit_code, 0,
        "the latest version is signed by a key nobody trusted: {output:?}"
    );
    assert_eq!(sandbox.installed_version(NOTES), None);
}

/// REQ: PKG-477
#[test]
fn pkg_477_yes_does_not_trust_the_new_key_of_an_update() {
    let sandbox = Sandbox::with_registry(&registry_with_new_signing_key());
    let key = publisher_key_id();
    assert_ok(&sandbox.run(&[
        "install",
        "com.acme.notes@1.0.0",
        "--yes",
        "--trust",
        key.as_str(),
    ]));
    let output = sandbox.run(&["update", NOTES, "--yes"]);
    assert_ne!(output.exit_code, 0, "a new key needs --trust: {output:?}");
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: PKG-477
#[test]
fn pkg_477_an_update_without_a_prompt_answer_in_ci_needs_input_and_installs_nothing() {
    let sandbox = Sandbox::with_registry(&registry_with_new_signing_key());
    let key = publisher_key_id();
    assert_ok(&sandbox.run(&[
        "install",
        "com.acme.notes@1.0.0",
        "--yes",
        "--trust",
        key.as_str(),
    ]));
    let output = sandbox.run(&["update", NOTES]);
    assert_ne!(output.exit_code, 0, "{output:?}");
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.0.0"));
}

/// REQ: PKG-477
#[test]
fn pkg_477_an_update_with_the_known_key_and_no_new_grants_installs() {
    let sandbox = Sandbox::standard();
    let key = publisher_key_id();
    assert_ok(&sandbox.run(&[
        "install",
        "com.acme.notes@1.0.0",
        "--yes",
        "--trust",
        key.as_str(),
    ]));
    assert_ok(&sandbox.run(&["update", NOTES, "--yes"]));
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}
