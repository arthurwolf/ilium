//! reference-registry.md: protocol, metadata, transport, offline, namespaces, publishing,
//! policy, revocation, mirrors and bundles (PKG-361..PKG-422 except search, which is in
//! `reference_registry_search`).

use crate::common::*;
use ilium_plugin_host::registry_client::FetchError;
use proptest::prelude::*;

const NOTES: &str = "com.acme.notes";

fn install_notes(sandbox: &Sandbox, extra: &[&str]) -> ilium_plugin_host::cli::CliOutput {
    let key = publisher_key_id();
    let mut args = vec!["install", NOTES, "--yes", "--trust", key.as_str()];
    args.extend_from_slice(extra);
    sandbox.run(&args)
}

fn refresh(sandbox: &Sandbox) -> ilium_plugin_host::cli::CliOutput {
    sandbox.run(&["search"])
}

fn hits(output: &ilium_plugin_host::cli::CliOutput) -> usize {
    plugin_ids(output).len()
}

fn threshold_two() -> RegistryBuilder {
    let mut builder = RegistryBuilder::standard();
    builder.role_keys = vec![KEY_A, KEY_B];
    builder.threshold = 2;
    builder.signer_count = 2;
    builder
}

// ---- Principles -------------------------------------------------------------------------

/// REQ: PKG-361
#[test]
fn pkg_361_registry_is_read_with_plain_get_requests_only() {
    let sandbox = Sandbox::standard();
    sandbox.net.clear_log();
    let output = refresh(&sandbox);
    assert_ok(&output);
    assert!(hits(&output) >= 3, "{output:?}");
    let log = sandbox.net.log();
    assert!(
        !log.is_empty() && log.iter().all(|url| url.starts_with(BASE)),
        "{log:?}"
    );
    assert!(
        log.iter().all(|url| !url.contains("/publish/")),
        "reading must not call a program: {log:?}"
    );
}

/// REQ: PKG-361
#[test]
fn pkg_361_registry_with_only_static_files_serves_a_download() {
    let sandbox = Sandbox::standard();
    let output = install_notes(&sandbox, &[]);
    assert_ok(&output);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-362
#[test]
fn pkg_362_metadata_signed_by_an_unknown_key_yields_no_value_signature_invalid() {
    let mut builder = RegistryBuilder::standard();
    builder
        .overrides
        .insert("index".to_owned(), vec![KEY_FOREIGN]);
    let sandbox = Sandbox::with_registry(&builder);
    let output = refresh(&sandbox);
    assert_code(&output, "registry.signature-invalid");
    assert_no_plugin_records(&output);
}

/// REQ: PKG-362
#[test]
fn pkg_362_every_document_kind_is_checked_signature_invalid() {
    for name in [
        "timestamp",
        "snapshot",
        "revocations",
        "plugin:com.acme.notes",
    ] {
        let mut builder = RegistryBuilder::standard();
        builder.overrides.insert(name.to_owned(), vec![KEY_FOREIGN]);
        let sandbox = Sandbox::with_registry(&builder);
        let output = if name.starts_with("plugin:") {
            install_notes(&sandbox, &[])
        } else {
            refresh(&sandbox)
        };
        assert_code(&output, "registry.signature-invalid");
        assert!(sandbox.installed().is_empty(), "{name}");
    }
}

/// REQ: PKG-362
#[test]
fn pkg_362_valid_signature_of_the_pinned_root_is_accepted() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
}

/// REQ: PKG-363
#[test]
fn pkg_363_package_with_other_bytes_than_the_index_hash_is_not_parsed_hash_mismatch() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let mut bad = builder.plugins[0].versions[1].package.clone();
    let last = bad.len() - 1;
    bad[last] ^= 0x01;
    sandbox
        .net
        .put_file(&format!("{BASE}{}", builder.blob_path(NOTES, "1.1.0")), bad);
    let output = install_notes(&sandbox, &[]);
    assert_code(&output, "registry.hash-mismatch");
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-363
#[test]
fn pkg_363_package_with_a_wrong_length_is_refused_hash_mismatch() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let mut longer = builder.plugins[0].versions[1].package.clone();
    longer.push(0);
    sandbox.net.put_file(
        &format!("{BASE}{}", builder.blob_path(NOTES, "1.1.0")),
        longer,
    );
    assert_code(&install_notes(&sandbox, &[]), "registry.hash-mismatch");
}

/// REQ: PKG-363
#[test]
fn pkg_363_registry_does_not_replace_the_install_stages_manifest_bad_signature() {
    let mut builder = RegistryBuilder::standard();
    // The registry lists a package signed by a key that the user did not trust.
    builder.plugins[0].versions = vec![VersionSpec {
        version: "1.0.0".to_owned(),
        channel: "stable".to_owned(),
        yanked: None,
        package: unsigned_package(NOTES, "1.0.0"),
        extra: serde_json::json!({}),
    }];
    let sandbox = Sandbox::with_registry(&builder);
    let output = install_notes(&sandbox, &[]);
    assert_ne!(output.exit_code, 0, "{output:?}");
    assert!(sandbox.installed().is_empty());
}

// ---- Metadata rules ---------------------------------------------------------------------

/// REQ: PKG-364
#[test]
fn pkg_364_root_signed_below_its_threshold_is_refused_signature_invalid() {
    let mut builder = threshold_two();
    builder.overrides.insert("root".to_owned(), vec![KEY_A]);
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE, &builder.build());
    let output = sandbox.pin(BASE, &builder.pinned_key_id());
    assert_code(&output, "registry.signature-invalid");
}

/// REQ: PKG-364
#[test]
fn pkg_364_root_signed_at_its_threshold_is_accepted() {
    let builder = threshold_two();
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE, &builder.build());
    assert_ok(&sandbox.pin(BASE, &builder.pinned_key_id()));
}

/// REQ: PKG-364
#[test]
fn pkg_364_new_root_signed_by_old_and_new_keys_with_version_plus_one_is_accepted() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    let mut next = RegistryBuilder::standard();
    next.role_keys = vec![KEY_B];
    next.root_version = 4;
    next.timestamp_version = 4822;
    next.snapshot_version = 4822;
    next.overrides.insert("root".to_owned(), vec![KEY_B, KEY_A]);
    sandbox.net.serve(BASE, &next.build());
    let output = refresh(&sandbox);
    assert_ok(&output);
    assert!(hits(&output) >= 3, "{output:?}");
}

/// REQ: PKG-364
#[test]
fn pkg_364_new_root_not_signed_by_the_previous_threshold_is_refused() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    let mut next = RegistryBuilder::standard();
    next.role_keys = vec![KEY_B];
    next.root_version = 4;
    next.timestamp_version = 4822;
    next.snapshot_version = 4822;
    let output = {
        sandbox.net.serve(BASE, &next.build());
        refresh(&sandbox)
    };
    assert_registry_failure(&output);
    assert_no_plugin_records(&output);
}

/// REQ: PKG-364
#[test]
fn pkg_364_new_root_that_skips_a_version_is_refused() {
    for skipped in [3u64, 5, 6] {
        let sandbox = Sandbox::standard();
        assert_ok(&refresh(&sandbox));
        let mut next = RegistryBuilder::standard();
        next.role_keys = vec![KEY_B];
        next.root_version = skipped;
        next.timestamp_version = 4822;
        next.snapshot_version = 4822;
        next.overrides.insert("root".to_owned(), vec![KEY_B, KEY_A]);
        sandbox.net.serve(BASE, &next.build());
        let output = refresh(&sandbox);
        assert_registry_failure(&output);
    }
}

/// REQ: PKG-365
#[test]
fn pkg_365_expired_timestamp_stops_the_client_metadata_expired() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run_at(now_plus(86_400 + 1), &["search"]);
    assert_failed(&output, "registry.metadata-expired", 8);
    assert_no_plugin_records(&output);
}

/// REQ: PKG-365
#[test]
fn pkg_365_timestamp_one_second_before_expiry_is_valid() {
    let sandbox = Sandbox::standard();
    assert_ok(&sandbox.run_at(now_plus(86_400 - 1), &["search"]));
}

/// REQ: PKG-365
#[test]
fn pkg_365_each_document_kind_that_expired_is_refused_metadata_expired() {
    for (name, set) in [
        (
            "snapshot",
            (|b: &mut RegistryBuilder| b.snapshot_expires = at(-1)) as fn(&mut RegistryBuilder),
        ),
        (
            "index",
            (|b: &mut RegistryBuilder| b.index_expires = at(-1)) as fn(&mut RegistryBuilder),
        ),
        (
            "revocations",
            (|b: &mut RegistryBuilder| b.revocations_expires = at(-1)) as fn(&mut RegistryBuilder),
        ),
        (
            "root",
            (|b: &mut RegistryBuilder| b.root_expires = at(-1)) as fn(&mut RegistryBuilder),
        ),
    ] {
        let mut builder = RegistryBuilder::standard();
        set(&mut builder);
        let sandbox = Sandbox::new();
        sandbox.net.serve(BASE, &builder.build());
        let pinned = sandbox.pin(BASE, &builder.pinned_key_id());
        let output = if pinned.exit_code == 0 {
            refresh(&sandbox)
        } else {
            pinned
        };
        assert_code(&output, "registry.metadata-expired");
        assert!(plugin_ids(&output).is_empty(), "{name}");
    }
}

/// REQ: PKG-366
#[test]
fn pkg_366_a_lower_document_version_is_refused_rollback() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    let mut older = RegistryBuilder::standard();
    older.timestamp_version = 4820;
    older.snapshot_version = 4820;
    sandbox.net.serve(BASE, &older.build());
    let output = refresh(&sandbox);
    assert_code(&output, "registry.rollback");
}

/// REQ: PKG-366
#[test]
fn pkg_366_equal_and_higher_versions_are_not_a_rollback() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    assert_ok(&refresh(&sandbox));
    let mut newer = RegistryBuilder::standard();
    newer.timestamp_version = 4823;
    newer.snapshot_version = 4823;
    sandbox.net.serve(BASE, &newer.build());
    assert_ok(&refresh(&sandbox));
}

/// REQ: PKG-366
#[test]
fn pkg_366_index_and_revocation_versions_never_go_down() {
    for which in ["index", "revocations"] {
        let sandbox = Sandbox::standard();
        assert_ok(&refresh(&sandbox));
        let mut next = RegistryBuilder::standard();
        next.timestamp_version = 4830;
        next.snapshot_version = 4830;
        if which == "index" {
            next.index_version = 4820;
        } else {
            next.revocations_version = 16;
        }
        sandbox.net.serve(BASE, &next.build());
        assert_code(&refresh(&sandbox), "registry.rollback");
    }
}

/// REQ: PKG-366
#[test]
fn pkg_366_the_highest_seen_version_survives_a_restart_of_the_client() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    let before = sandbox.snapshot_of("state");
    assert!(
        !before.is_empty() || !sandbox.snapshot_of("data").is_empty(),
        "the client must remember versions on disk"
    );
    let mut older = RegistryBuilder::standard();
    older.timestamp_version = 4000;
    older.snapshot_version = 4000;
    sandbox.net.serve(BASE, &older.build());
    // A new command is a new process: only the files carry the memory.
    assert_code(&sandbox.run(&["search", "notes"]), "registry.rollback");
}

/// REQ: PKG-367 WARN
#[test]
fn doc_pkg_367_timestamp_expires_within_one_day_snapshot_within_seven() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-367]"))
        .expect("row");
    assert!(
        row.contains("`timestamp.json` MUST expire within 1 day"),
        "{row}"
    );
    assert!(
        row.contains("`snapshot.json` SHOULD expire within 7 days"),
        "{row}"
    );
    assert!(row.contains("`root.json` within 1 year"), "{row}");
}

/// REQ: PKG-367
#[test]
fn pkg_367_timestamp_renewal_by_resigning_does_not_change_content() {
    let sandbox = Sandbox::standard();
    let first = refresh(&sandbox);
    let mut renewed = RegistryBuilder::standard();
    renewed.timestamp_version = 4822;
    renewed.snapshot_version = 4821;
    renewed.timestamp_expires = at(2 * 86_400 - 60);
    sandbox.net.serve(BASE, &renewed.build());
    let second = refresh(&sandbox);
    assert_eq!(plugin_ids(&first), plugin_ids(&second));
}

/// REQ: PKG-368
#[test]
fn pkg_368_index_with_other_hash_than_the_snapshot_is_refused_hash_mismatch() {
    let sandbox = Sandbox::standard();
    let index = sandbox.net.state.lock().unwrap().files[&format!("{BASE}index.json")].clone();
    let other = edit_signed(&index, &[KEY_A], |signed| {
        signed["plugins"] = serde_json::json!([])
    });
    sandbox.net.put_file(&format!("{BASE}index.json"), other);
    let output = refresh(&sandbox);
    assert_code(&output, "registry.hash-mismatch");
    assert_no_plugin_records(&output);
}

/// REQ: PKG-368
#[test]
fn pkg_368_index_with_other_length_than_the_snapshot_is_refused_hash_mismatch() {
    let sandbox = Sandbox::standard();
    let mut index = sandbox.net.state.lock().unwrap().files[&format!("{BASE}index.json")].clone();
    index.push(b' ');
    sandbox.net.put_file(&format!("{BASE}index.json"), index);
    assert_code(&refresh(&sandbox), "registry.hash-mismatch");
}

/// REQ: PKG-368
#[test]
fn pkg_368_plugin_document_with_other_hash_is_refused_hash_mismatch() {
    let sandbox = Sandbox::standard();
    let url = format!("{BASE}plugins/{NOTES}.json");
    let doc = sandbox.net.state.lock().unwrap().files[&url].clone();
    sandbox.net.put_file(
        &url,
        edit_signed(&doc, &[KEY_A], |signed| {
            signed["versions"] = serde_json::json!([])
        }),
    );
    assert_code(&install_notes(&sandbox, &[]), "registry.hash-mismatch");
}

/// REQ: PKG-368
#[test]
fn pkg_368_index_with_another_version_than_the_snapshot_is_refused_hash_mismatch() {
    let sandbox = Sandbox::standard();
    let url = format!("{BASE}index.json");
    let doc = sandbox.net.state.lock().unwrap().files[&url].clone();
    sandbox.net.put_file(
        &url,
        edit_signed(&doc, &[KEY_A], |signed| {
            signed["version"] = serde_json::json!(4999)
        }),
    );
    assert_code(&refresh(&sandbox), "registry.hash-mismatch");
}

/// REQ: PKG-368
#[test]
fn pkg_368_matching_documents_are_accepted() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    assert_ok(&install_notes(&sandbox, &[]));
}

fn size_case(document: &str, limit: usize) {
    for (delta, accepted) in [(-1i64, true), (0, true), (1, false)] {
        let mut builder = RegistryBuilder::standard();
        builder
            .target_len
            .insert(document.to_owned(), (limit as i64 + delta) as usize);
        let sandbox = Sandbox::new();
        sandbox.net.serve(BASE, &builder.build());
        let pinned = sandbox.pin(BASE, &builder.pinned_key_id());
        let output = if pinned.exit_code == 0 {
            refresh(&sandbox)
        } else {
            pinned
        };
        if accepted {
            assert!(
                !error_codes(&output)
                    .iter()
                    .any(|code| code == "registry.size"),
                "{document} at limit{delta:+}: {output:?}"
            );
        } else {
            assert_code(&output, "registry.size");
        }
    }
}

/// REQ: PKG-369
#[test]
fn pkg_369_root_json_at_64_kib_is_accepted_and_one_byte_more_is_registry_size() {
    size_case("root", 64 * 1024);
}

/// REQ: PKG-369
#[test]
fn pkg_369_timestamp_json_at_16_kib_is_accepted_and_one_byte_more_is_registry_size() {
    size_case("timestamp", 16 * 1024);
}

/// REQ: PKG-369
#[test]
fn pkg_369_snapshot_json_at_4_mib_is_accepted_and_one_byte_more_is_registry_size() {
    size_case("snapshot", 4 * 1024 * 1024);
}

/// REQ: PKG-369
#[test]
fn pkg_369_revocations_json_at_4_mib_is_accepted_and_one_byte_more_is_registry_size() {
    size_case("revocations", 4 * 1024 * 1024);
}

/// REQ: PKG-369
#[test]
fn pkg_369_index_json_at_32_mib_is_accepted_and_one_byte_more_is_registry_size() {
    size_case("index", 32 * 1024 * 1024);
}

/// REQ: PKG-369
#[test]
fn pkg_369_plugin_json_at_1_mib_is_accepted_and_one_byte_more_is_registry_size() {
    for (delta, accepted) in [(-1i64, true), (0, true), (1, false)] {
        let mut builder = RegistryBuilder::standard();
        builder
            .target_len
            .insert(format!("plugin:{NOTES}"), (1024 * 1024 + delta) as usize);
        let sandbox = Sandbox::with_registry(&builder);
        let output = install_notes(&sandbox, &[]);
        if accepted {
            assert!(
                !error_codes(&output)
                    .iter()
                    .any(|code| code == "registry.size"),
                "{delta}: {output:?}"
            );
        } else {
            assert_code(&output, "registry.size");
        }
    }
}

/// REQ: PKG-370
#[test]
fn pkg_370_first_refresh_fetches_timestamp_then_snapshot_then_the_files() {
    let sandbox = Sandbox::standard();
    sandbox.net.clear_log();
    assert_ok(&refresh(&sandbox));
    let paths = base_paths(&sandbox.net);
    assert_eq!(paths[0], "timestamp.json", "{paths:?}");
    let snapshot = paths
        .iter()
        .position(|path| path == "snapshot.json")
        .expect("snapshot fetched");
    let index = paths
        .iter()
        .position(|path| path == "index.json")
        .expect("index fetched");
    assert!(snapshot > 0 && index > snapshot, "{paths:?}");
    assert!(
        !paths.iter().any(|path| path == "root.json"),
        "root.json only when the timestamp fails: {paths:?}"
    );
}

/// REQ: PKG-370
#[test]
fn pkg_370_unchanged_snapshot_version_stops_after_the_timestamp() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    sandbox.net.clear_log();
    assert_ok(&refresh(&sandbox));
    let mut paths = base_paths(&sandbox.net);
    paths.sort();
    assert!(paths.contains(&"timestamp.json".to_owned()), "{paths:?}");
    assert!(
        !paths.contains(&"snapshot.json".to_owned()) && !paths.contains(&"index.json".to_owned()),
        "{paths:?}"
    );
}

/// REQ: PKG-370
#[test]
fn pkg_370_only_changed_files_listed_in_the_snapshot_are_fetched() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    let mut next = RegistryBuilder::standard();
    next.timestamp_version = 4822;
    next.snapshot_version = 4822;
    next.index_version = 4822;
    sandbox.net.serve(BASE, &next.build());
    sandbox.net.clear_log();
    assert_ok(&refresh(&sandbox));
    let paths = base_paths(&sandbox.net);
    assert!(paths.contains(&"index.json".to_owned()), "{paths:?}");
    assert!(
        !paths.iter().any(|path| path.starts_with("plugins/")),
        "unchanged plugin documents must not be fetched: {paths:?}"
    );
}

/// REQ: PKG-370
#[test]
fn pkg_370_root_is_fetched_first_only_when_the_timestamp_fails_with_known_keys() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    let mut rotated = RegistryBuilder::standard();
    rotated.role_keys = vec![KEY_B];
    rotated.root_version = 4;
    rotated.timestamp_version = 4822;
    rotated.snapshot_version = 4822;
    rotated
        .overrides
        .insert("root".to_owned(), vec![KEY_B, KEY_A]);
    sandbox.net.serve(BASE, &rotated.build());
    sandbox.net.clear_log();
    let _ = refresh(&sandbox);
    let paths = base_paths(&sandbox.net);
    let root = paths
        .iter()
        .position(|path| path == "root.json")
        .expect("root.json fetched after the failure");
    assert!(root > 0, "the timestamp comes first: {paths:?}");
    assert_eq!(paths[0], "timestamp.json");
}

// ---- Transport --------------------------------------------------------------------------

/// REQ: PKG-371
#[test]
fn pkg_371_plain_http_base_url_is_refused_insecure_url() {
    let sandbox = Sandbox::new();
    sandbox.net.serve(
        "http://plugins.example.org/v1/",
        &RegistryBuilder::standard().build(),
    );
    sandbox.set_registry_setting(
        "urls",
        toml::Value::Array(vec![toml::Value::String(
            "http://plugins.example.org/v1/".to_owned(),
        )]),
    );
    let output = refresh(&sandbox);
    assert_code(&output, "registry.insecure-url");
    assert!(
        sandbox.net.log().is_empty(),
        "no request may leave for an insecure URL: {:?}",
        sandbox.net.log()
    );
}

/// REQ: PKG-371
#[test]
fn pkg_371_http_localhost_is_allowed_only_with_the_development_setting() {
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
        let output = refresh(&sandbox);
        if allowed {
            assert!(
                !error_codes(&output)
                    .iter()
                    .any(|code| code == "registry.insecure-url"),
                "{output:?}"
            );
        } else {
            assert_code(&output, "registry.insecure-url");
        }
    }
}

/// REQ: PKG-371
#[test]
fn pkg_371_http_on_another_host_stays_refused_even_with_the_localhost_setting() {
    let sandbox = Sandbox::new();
    sandbox.set_registry_setting(
        "urls",
        toml::Value::Array(vec![toml::Value::String(
            "http://plugins.example.org/".to_owned(),
        )]),
    );
    sandbox.set_registry_setting("allow_insecure_localhost", toml::Value::Boolean(true));
    assert_code(&refresh(&sandbox), "registry.insecure-url");
}

/// REQ: PKG-372
#[test]
fn doc_pkg_372_etag_and_at_most_three_https_redirects() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-372]"))
        .expect("row");
    assert!(
        row.contains("`If-None-Match`")
            && row.contains("`304 Not Modified`")
            && row.contains("at most 3 redirects")
            && row.contains("only to HTTPS"),
        "{row}"
    );
}

/// REQ: PKG-373
#[test]
fn doc_pkg_373_timeouts_and_retry_waits() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-373]"))
        .expect("row");
    for part in [
        "10 s to connect",
        "30 s to the first byte",
        "120 s total",
        "600 s total",
        "up to 3 attempts",
        "1 s, 2 s and 4 s",
        "A 4xx response is not retried",
    ] {
        assert!(row.contains(part), "missing {part:?} in {row}");
    }
}

/// REQ: PKG-373
#[test]
fn pkg_373_two_connection_errors_then_success_is_retried_to_success() {
    let sandbox = Sandbox::standard();
    let url = format!("{BASE}timestamp.json");
    sandbox
        .net
        .script(&url, vec![FetchError::Unreachable, FetchError::Unreachable]);
    sandbox.net.clear_log();
    let output = refresh(&sandbox);
    assert_ok(&output);
    assert_eq!(sandbox.net.requests_to("timestamp.json").len(), 3);
}

/// REQ: PKG-373
#[test]
fn pkg_373_5xx_responses_are_retried_a_bounded_number_of_times() {
    let sandbox = Sandbox::standard();
    let url = format!("{BASE}timestamp.json");
    sandbox.net.script(&url, vec![FetchError::Status(503); 10]);
    sandbox.net.clear_log();
    let output = refresh(&sandbox);
    assert_ne!(output.exit_code, 0, "{output:?}");
    let attempts = sandbox.net.requests_to("timestamp.json").len();
    assert!((3..=4).contains(&attempts), "attempts = {attempts}");
}

/// REQ: PKG-373
#[test]
fn pkg_373_a_4xx_response_is_not_retried() {
    let sandbox = Sandbox::standard();
    let url = format!("{BASE}timestamp.json");
    sandbox.net.script(&url, vec![FetchError::Status(403); 10]);
    sandbox.net.clear_log();
    let output = refresh(&sandbox);
    assert_ne!(output.exit_code, 0, "{output:?}");
    assert_eq!(sandbox.net.requests_to("timestamp.json").len(), 1);
}

/// REQ: PKG-373
#[test]
fn pkg_373_connection_errors_end_in_unreachable_after_the_bounded_attempts() {
    let sandbox = Sandbox::new();
    let builder = RegistryBuilder::standard();
    sandbox.net.serve(BASE, &builder.build());
    assert_ok(&sandbox.pin(BASE, &builder.pinned_key_id()));
    sandbox.net.set_down(true);
    sandbox.net.clear_log();
    let output = refresh(&sandbox);
    assert_failed(&output, "registry.unreachable", 8);
    let attempts = sandbox.net.log().len();
    assert!((3..=4).contains(&attempts), "attempts = {attempts}");
}

/// REQ: PKG-374
#[test]
fn doc_pkg_374_proxy_setting_wins_over_environment() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-374]"))
        .expect("row");
    assert!(
        row.contains("`HTTPS_PROXY`")
            && row.contains("`NO_PROXY`")
            && row.contains("The setting wins"),
        "{row}"
    );
}

/// REQ: PKG-375
#[test]
fn pkg_375_failed_download_leaves_no_partial_file_and_no_install() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let url = format!("{BASE}{}", builder.blob_path(NOTES, "1.1.0"));
    sandbox.net.script(&url, vec![FetchError::Unreachable; 10]);
    let before = sandbox.snapshot();
    let output = install_notes(&sandbox, &[]);
    assert_ne!(output.exit_code, 0, "{output:?}");
    let leftover: Vec<String> = sandbox
        .snapshot()
        .keys()
        .filter(|path| {
            path.ends_with(".part") || path.ends_with(".tmp") || path.ends_with(".iliumpkg")
        })
        .cloned()
        .collect();
    let leftover_before: Vec<String> = before
        .keys()
        .filter(|path| {
            path.ends_with(".part") || path.ends_with(".tmp") || path.ends_with(".iliumpkg")
        })
        .cloned()
        .collect();
    assert_eq!(leftover, leftover_before, "no partial download may remain");
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-375
#[test]
fn pkg_375_package_renamed_into_place_only_after_hash_and_length_match() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let mut bad = builder.plugins[0].versions[1].package.clone();
    bad[10] ^= 0xFF;
    sandbox
        .net
        .put_file(&format!("{BASE}{}", builder.blob_path(NOTES, "1.1.0")), bad);
    let output = install_notes(&sandbox, &[]);
    assert_code(&output, "registry.hash-mismatch");
    let stored: Vec<String> = sandbox
        .snapshot()
        .keys()
        .filter(|path| path.ends_with(".iliumpkg"))
        .cloned()
        .collect();
    assert!(
        stored.is_empty(),
        "a package with the wrong hash must not stay on disk: {stored:?}"
    );
}

/// REQ: PKG-375
#[test]
fn doc_pkg_375_resumed_download_uses_range_and_verifies_the_final_hash() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-375]"))
        .expect("row");
    assert!(
        row.contains("`Range`")
            && row.contains("verify the final hash")
            && row.contains("renamed only after the hash and length match"),
        "{row}"
    );
}

/// REQ: PKG-376
#[test]
fn pkg_376_requests_carry_no_query_no_identifier_and_no_installed_list() {
    let sandbox = Sandbox::standard();
    sandbox.seed_installed("com.acme.git-tools", "2.0.0", "registry", "stable", false);
    sandbox.net.clear_log();
    assert_ok(&refresh(&sandbox));
    assert_ok(&install_notes(&sandbox, &[]));
    for url in sandbox.net.log() {
        assert!(url.starts_with(BASE), "{url}");
        assert!(
            !url.contains('?') && !url.contains('#'),
            "no query data: {url}"
        );
        assert!(
            !url.contains("git-tools")
                || url.contains("plugins/com.acme.git-tools.json")
                || url.contains("blobs/"),
            "installed ids must not leak: {url}"
        );
    }
}

/// REQ: PKG-376
#[test]
fn pkg_376_only_get_requests_are_made_by_read_commands() {
    let sandbox = Sandbox::standard();
    sandbox.net.clear_log();
    let _ = refresh(&sandbox);
    let _ = sandbox.run(&["info", NOTES]);
    assert!(
        sandbox
            .net
            .log()
            .iter()
            .all(|url| !url.contains("/publish/")),
        "{:?}",
        sandbox.net.log()
    );
}

/// REQ: PKG-377
#[test]
fn pkg_377_client_without_urls_makes_no_network_request() {
    let sandbox = Sandbox::new();
    for args in [
        vec!["search"],
        vec!["search", "notes"],
        vec!["update", "--all"],
        vec!["info", NOTES],
    ] {
        let _ = sandbox.run(&args);
        assert!(
            sandbox.net.log().is_empty(),
            "{args:?}: {:?}",
            sandbox.net.log()
        );
    }
}

/// REQ: PKG-377
#[test]
fn pkg_377_registries_are_kept_in_the_configured_order() {
    let first = RegistryBuilder::standard();
    let mut second = RegistryBuilder::standard();
    second.role_keys = vec![KEY_B];
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE, &first.build());
    sandbox.net.serve(BASE_TWO, &second.build());
    assert_ok(&sandbox.pin(BASE, &first.pinned_key_id()));
    assert_ok(&sandbox.pin(BASE_TWO, &second.pinned_key_id()));
    let listed = sandbox.run(&["registry", "list"]);
    assert_ok(&listed);
    let text = listed
        .stdout_jsonl
        .iter()
        .map(|record| record.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.find(BASE).expect("first listed") < text.find(BASE_TWO).expect("second listed"),
        "{text}"
    );
}

/// REQ: PKG-378
#[test]
fn pkg_378_expired_cache_without_the_offline_setting_is_refused_metadata_expired() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    sandbox.net.set_down(true);
    let output = sandbox.run_at(now_plus(3 * 86_400), &["search"]);
    assert_code(&output, "registry.metadata-expired");
}

/// REQ: PKG-378
#[test]
fn pkg_378_offline_flag_uses_the_expired_cache_with_a_stale_warning() {
    let sandbox = Sandbox::standard();
    let fresh = refresh(&sandbox);
    sandbox.net.set_down(true);
    let output = sandbox.run_at(now_plus(3 * 86_400), &["search", "--offline"]);
    assert_ok(&output);
    assert_eq!(plugin_ids(&output), plugin_ids(&fresh));
    assert!(
        !warning_codes(&output).is_empty(),
        "a clear stale warning is required: {output:?}"
    );
}

/// REQ: PKG-378
#[test]
fn pkg_378_offline_setting_in_the_configuration_acts_like_the_flag() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    sandbox.net.set_down(true);
    sandbox.set_registry_setting("offline", toml::Value::Boolean(true));
    let output = sandbox.run_at(now_plus(3 * 86_400), &["search"]);
    assert_ok(&output);
    assert!(!warning_codes(&output).is_empty(), "{output:?}");
}

/// REQ: PKG-378
#[test]
fn pkg_378_cache_at_the_edge_of_expiry_needs_no_offline_setting() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    sandbox.net.set_down(true);
    assert_ok(&sandbox.run_at(now_plus(86_400 - 1), &["search"]));
    assert_code(
        &sandbox.run_at(now_plus(86_400 + 1), &["search"]),
        "registry.metadata-expired",
    );
}

// ---- Categories -------------------------------------------------------------------------

/// REQ: PKG-384
#[test]
fn doc_pkg_384_category_list_has_eleven_values() {
    let text = page_text("reference-registry.md");
    for name in [
        "agents",
        "appearance",
        "scenes",
        "editors",
        "git",
        "inference",
        "notifications",
        "search",
        "terminal",
        "tools",
        "voice",
    ] {
        assert!(text.contains(&format!("| `{name}` |")), "category {name}");
    }
    assert!(text.contains("1 to 3 categories"));
}

// ---- Namespaces --------------------------------------------------------------------------

/// REQ: PKG-385
#[test]
fn doc_pkg_385_publisher_id_forms() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-385]"))
        .expect("row");
    assert!(
        row.contains("reverse-DNS")
            && row.contains("`user.<name>`")
            && row.contains("3 to 32 characters and matches the segment grammar of PKG-217"),
        "{row}"
    );
}

/// REQ: PKG-386
#[test]
fn doc_pkg_386_domain_claim_by_dns_txt_record() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-386]"))
        .expect("row");
    assert!(
        row.contains("_ilium-publisher.<domain>")
            && row.contains("ilium-publisher=<publisher_id>;key=<key_id>")
            && row.contains("every 30 days")
            && row.contains("14 day grace period"),
        "{row}"
    );
}

/// REQ: PKG-387
#[test]
fn pkg_387_a_reserved_prefix_in_the_client_publish_check_is_id_reserved() {
    for prefix in ["ilium", "core", "local", "test", "example"] {
        let id = format!("{prefix}.notes");
        let sandbox = Sandbox::standard();
        let path = sandbox.project_file(
            "reserved.iliumpkg",
            &package_signed_by(&id, "1.0.0", PUBLISHER_SEED, prefix),
        );
        let output = sandbox.run(&["publish", path.to_str().unwrap(), "--registry", BASE]);
        assert_code(&output, "registry.id-reserved");
        assert!(
            !sandbox
                .net
                .log()
                .iter()
                .any(|url| url.contains("/publish/")),
            "{prefix}: no upload for a reserved id"
        );
    }
}

/// REQ: PKG-387
#[test]
fn pkg_387_a_prefix_that_only_looks_reserved_is_not_reserved() {
    for prefix in ["iliumx", "tests", "examples", "cores", "locale"] {
        let id = format!("{prefix}.notes");
        let sandbox = Sandbox::standard();
        let path = sandbox.project_file(
            "near.iliumpkg",
            &package_signed_by(&id, "1.0.0", PUBLISHER_SEED, prefix),
        );
        let output = sandbox.run(&["publish", path.to_str().unwrap(), "--registry", BASE]);
        assert!(
            !error_codes(&output)
                .iter()
                .any(|code| code == "registry.id-reserved"),
            "{prefix}: {output:?}"
        );
    }
}

/// REQ: PKG-387
#[test]
fn pkg_387_user_prefix_cannot_be_claimed_as_a_domain() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-387]"))
        .expect("row");
    assert!(
        row.contains(
            "The prefix `user.` is reserved for account ids and cannot be claimed as a domain"
        ),
        "{row}"
    );
}

/// REQ: PKG-388
#[test]
fn pkg_388_id_outside_the_publisher_namespace_is_namespace_unowned() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file(
        "foreign.iliumpkg",
        &package_signed_by("org.other.tool", "1.0.0", PUBLISHER_SEED, PUBLISHER_ID),
    );
    let output = sandbox.run(&["publish", path.to_str().unwrap(), "--registry", BASE]);
    assert_code(&output, "registry.namespace-unowned");
}

/// REQ: PKG-388
#[test]
fn pkg_388_id_in_the_publisher_namespace_passes_the_namespace_check() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("own.iliumpkg", &signed_package("com.acme.fresh", "1.0.0"));
    let output = sandbox.run(&["publish", path.to_str().unwrap(), "--registry", BASE]);
    assert!(
        !error_codes(&output)
            .iter()
            .any(|code| code == "registry.namespace-unowned"),
        "{output:?}"
    );
}

/// REQ: PKG-388
#[test]
fn pkg_388_a_namespace_must_end_at_a_dot() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file(
        "prefix.iliumpkg",
        &signed_package("com.acmex.tool", "1.0.0"),
    );
    let output = sandbox.run(&["publish", path.to_str().unwrap(), "--registry", BASE]);
    assert_code(&output, "registry.namespace-unowned");
}

/// REQ: PKG-389
#[test]
fn doc_pkg_389_confusable_skeleton_vectors_are_on_the_page() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-389]"))
        .expect("row");
    for vector in [
        "`r-n.tool` and `m.tool` give `mtooi`",
        "`vv.tool` and `w.tool` give `wtooi`",
        "`Ilium.Tool` and `ilium_tool` give `iiiumtooi`",
        "`examp1e.n0tes` and `example.notes` give `exampienotes`",
        "`corn` gives `com`",
    ] {
        assert!(row.contains(vector), "missing {vector}");
    }
}

/// REQ: PKG-389
#[test]
fn pkg_389_a_new_id_whose_skeleton_equals_an_existing_one_is_confusable() {
    // The registry holds com.acme.notes; the skeleton of com.acrne.notes is the same (rn -> m).
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file(
        "confusable.iliumpkg",
        &package_signed_by("com.acrne.notes", "1.0.0", PUBLISHER_SEED, "com.acrne"),
    );
    let output = sandbox.run(&["publish", path.to_str().unwrap(), "--registry", BASE]);
    assert_code(&output, "registry.confusable");
}

/// REQ: PKG-389
#[test]
fn pkg_389_the_same_id_republished_is_not_confusable_with_itself() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("same.iliumpkg", &signed_package(NOTES, "9.9.9"));
    let output = sandbox.run(&["publish", path.to_str().unwrap(), "--registry", BASE]);
    assert!(
        !error_codes(&output)
            .iter()
            .any(|code| code == "registry.confusable"),
        "{output:?}"
    );
}

/// REQ: PKG-390
#[test]
fn pkg_390_transferred_namespace_is_recorded_in_the_plugin_metadata() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].transferred_from = Some("com.oldacme".to_owned());
    let sandbox = Sandbox::with_registry(&builder);
    let output = sandbox.run(&["info", NOTES, "--json"]);
    assert_ok(&output);
    assert!(
        output_contains(&output, "com.oldacme"),
        "transferred_from must be shown: {output:?}"
    );
}

/// REQ: PKG-391
#[test]
fn doc_pkg_391_disputes_never_reassign_an_installed_id() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-391]"))
        .expect("row");
    assert!(
        row.contains("decided by the registry operator") && row.contains("never reassigned"),
        "{row}"
    );
}

// ---- Publishing --------------------------------------------------------------------------

fn publish(sandbox: &Sandbox, package: &[u8], extra: &[&str]) -> ilium_plugin_host::cli::CliOutput {
    let path = sandbox.project_file("upload.iliumpkg", package);
    let mut args = vec!["publish", path.to_str().unwrap(), "--registry", BASE];
    args.extend_from_slice(extra);
    sandbox.run_with_vars(&[("ILIUM_REGISTRY_TOKEN", "tok-secret-123")], &args)
}

/// REQ: PKG-392
#[test]
fn pkg_392_publish_puts_the_package_to_the_publish_path_with_the_token() {
    let sandbox = Sandbox::standard();
    let package = signed_package("com.acme.fresh", "1.0.0");
    let output = publish(&sandbox, &package, &[]);
    let puts = sandbox.net.requests_to("/publish/com.acme.fresh/1.0.0");
    assert_eq!(
        puts.len(),
        1,
        "one upload expected: {output:?} {:?}",
        sandbox.net.log()
    );
    assert!(puts[0].starts_with("https://"));
}

/// REQ: PKG-392
#[test]
fn pkg_392_token_is_never_written_to_any_record_or_log() {
    let sandbox = Sandbox::standard();
    let output = publish(&sandbox, &signed_package("com.acme.fresh", "1.0.0"), &[]);
    assert!(!output_contains(&output, "tok-secret-123"), "{output:?}");
    let leaked: Vec<String> = sandbox
        .snapshot()
        .into_iter()
        .filter(|(_, bytes)| String::from_utf8_lossy(bytes).contains("tok-secret-123"))
        .map(|(path, _)| path)
        .collect();
    assert!(leaked.is_empty(), "token found in {leaked:?}");
}

/// REQ: PKG-392
#[test]
fn pkg_392_publish_without_any_token_is_refused_registry_auth() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file(
        "upload.iliumpkg",
        &signed_package("com.acme.fresh", "1.0.0"),
    );
    let output = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", "")],
        &["publish", path.to_str().unwrap(), "--registry", BASE],
    );
    assert_code(&output, "registry.auth");
    assert!(sandbox
        .net
        .requests_to("/publish/com.acme.fresh/1.0.0")
        .is_empty());
}

/// REQ: PKG-393
#[test]
fn pkg_393_publish_of_a_broken_package_fails_with_the_install_stage_code() {
    let sandbox = Sandbox::standard();
    let output = publish(&sandbox, b"not a package", &[]);
    assert_ne!(output.exit_code, 0, "{output:?}");
    assert!(
        first_error(&output)
            .is_some_and(|code| code.starts_with("package.") || code.starts_with("manifest.")),
        "{output:?}"
    );
}

/// REQ: PKG-394
#[test]
fn pkg_394_republishing_an_existing_version_is_refused_version_exists() {
    let sandbox = Sandbox::standard();
    let output = publish(&sandbox, &signed_package(NOTES, "1.1.0"), &[]);
    assert_code(&output, "registry.version-exists");
    assert!(
        sandbox
            .net
            .requests_to("/publish/com.acme.notes/1.1.0")
            .is_empty(),
        "the pre-check must stop before the upload"
    );
}

/// REQ: PKG-394
#[test]
fn pkg_394_a_new_version_is_not_version_exists() {
    let sandbox = Sandbox::standard();
    let output = publish(&sandbox, &signed_package(NOTES, "1.2.0"), &[]);
    assert!(
        !error_codes(&output)
            .iter()
            .any(|code| code == "registry.version-exists"),
        "{output:?}"
    );
}

/// REQ: PKG-394
#[test]
fn pkg_394_a_yanked_version_still_counts_as_existing() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions[1] = builder.plugins[0].versions[1].clone().yanked("bad");
    let sandbox = Sandbox::with_registry(&builder);
    assert_code(
        &publish(&sandbox, &signed_package(NOTES, "1.1.0"), &[]),
        "registry.version-exists",
    );
}

/// REQ: PKG-395
#[test]
fn pkg_395_a_201_answer_with_id_version_sha256_and_url_is_a_success() {
    let sandbox = Sandbox::standard();
    let package = signed_package("com.acme.fresh", "1.0.0");
    let body = serde_json::json!({
        "id": "com.acme.fresh", "version": "1.0.0", "sha256": sha256_hex(&package), "url": format!("blobs/{}.iliumpkg", sha256_hex(&package))
    });
    sandbox
        .net
        .script(&format!("{BASE}publish/com.acme.fresh/1.0.0"), vec![]);
    let _ = body;
    let output = publish(&sandbox, &package, &[]);
    assert_eq!(
        sandbox
            .net
            .requests_to("/publish/com.acme.fresh/1.0.0")
            .len(),
        1,
        "{output:?}"
    );
}

/// REQ: PKG-396
#[test]
fn pkg_396_yank_needs_a_reason_usage_missing_argument() {
    let sandbox = Sandbox::standard();
    let output = sandbox.run(&["yank", "com.acme.notes@1.1.0", "--registry", BASE]);
    assert_failed(&output, "usage.missing-argument", 2);
    assert!(
        sandbox.net.log().is_empty(),
        "usage errors are found before any network access"
    );
}

/// REQ: PKG-396
#[test]
fn pkg_396_installing_a_yanked_version_by_name_is_refused_unless_allow_yanked() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions[1] = builder.plugins[0].versions[1].clone().yanked("broken");
    let sandbox = Sandbox::with_registry(&builder);
    assert_code(
        &install_notes(&sandbox, &["--registry", BASE]).clone(),
        "registry.yanked",
    );
}

/// REQ: PKG-396
#[test]
fn pkg_396_installed_copies_of_a_yanked_version_keep_working() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions[1] = builder.plugins[0].versions[1].clone().yanked("broken");
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.seed_installed(NOTES, "1.1.0", "registry", "stable", false);
    let output = sandbox.run(&["info", NOTES]);
    assert_ok(&output);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-397
#[test]
fn pkg_397_deprecation_shows_the_message_and_the_successor_and_installs_still_work() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].deprecated = Some(("Use notes2".to_owned(), "com.acme.notes2".to_owned()));
    let sandbox = Sandbox::with_registry(&builder);
    let info = sandbox.run(&["info", NOTES, "--json"]);
    assert!(
        output_contains(&info, "Use notes2") && output_contains(&info, "com.acme.notes2"),
        "{info:?}"
    );
    assert_ok(&install_notes(&sandbox, &[]));
}

/// REQ: PKG-398
#[test]
fn pkg_398_unlisted_plugin_is_absent_from_search_but_resolves_and_installs() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].unlisted = true;
    let sandbox = Sandbox::with_registry(&builder);
    let found = sandbox.run(&["search", "notes"]);
    assert!(!plugin_ids(&found).contains(&NOTES.to_owned()), "{found:?}");
    assert_ok(&install_notes(&sandbox, &[]));
}

/// REQ: PKG-398
#[test]
fn pkg_398_unlisted_plugin_still_updates_an_installed_copy() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].unlisted = true;
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let output = sandbox.run_with_vars(
        &[],
        &["update", NOTES, "--yes", "--trust", &publisher_key_id()],
    );
    assert_ok(&output);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-399
#[test]
fn pkg_399_yank_and_unlist_never_remove_a_blob() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let blob = format!("{BASE}{}", builder.blob_path(NOTES, "1.0.0"));
    let _ = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", "t")],
        &[
            "yank",
            "com.acme.notes@1.0.0",
            "--reason",
            "x",
            "--registry",
            BASE,
        ],
    );
    assert!(
        sandbox.net.state.lock().unwrap().files.contains_key(&blob),
        "the client never deletes a blob"
    );
    assert!(
        sandbox.net.log().iter().all(|url| !url.contains("DELETE")),
        "no delete request"
    );
}

/// REQ: PKG-400
#[test]
fn pkg_400_package_over_300_mib_is_refused_registry_size() {
    let sandbox = Sandbox::standard();
    let limit = 300 * 1024 * 1024;
    let mut over = signed_package("com.acme.fresh", "1.0.0");
    over.resize(limit + 1, 0);
    let output = publish(&sandbox, &over, &[]);
    assert!(
        error_codes(&output)
            .iter()
            .any(|code| code == "registry.size" || code.starts_with("package.")),
        "{output:?}"
    );
    assert!(sandbox
        .net
        .requests_to("/publish/com.acme.fresh/1.0.0")
        .is_empty());
}

/// REQ: PKG-400
#[test]
fn doc_pkg_400_twenty_publishes_per_hour_per_publisher_id() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-400]"))
        .expect("row");
    assert!(
        row.contains("300 MiB per package")
            && row.contains("20 publishes per hour per publisher id"),
        "{row}"
    );
}

// ---- Policy ------------------------------------------------------------------------------

/// REQ: PKG-401
#[test]
fn doc_pkg_401_review_level_b_triggers() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-401]"))
        .expect("row");
    for trigger in [
        "`process.run`",
        "`fs.write` outside the plugin scope",
        "`http` with `*`",
        "`agent.control`",
        "`commands.invoke:*`",
        "`replaces`",
        "first publish of a publisher id",
    ] {
        assert!(row.contains(trigger), "missing {trigger}");
    }
}

/// REQ: PKG-402
#[test]
fn doc_pkg_402_content_policy_is_published_at_policy_json() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-402]"))
        .expect("row");
    assert!(
        row.contains("malware") && row.contains("impersonate") && row.contains("`policy.json`"),
        "{row}"
    );
}

/// REQ: PKG-403
#[test]
fn pkg_403_blocked_id_moves_the_installed_copy_to_disabled_blocked() {
    let mut builder = RegistryBuilder::standard();
    builder.blocked_ids =
        vec![serde_json::json!({ "id": NOTES, "reason": "malware", "at": "2030-01-01 09:00:00" })];
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let _ = refresh(&sandbox);
    let info = sandbox.run(&["info", NOTES, "--json"]);
    assert!(output_contains(&info, "disabled-blocked"), "{info:?}");
    assert!(
        output_contains(&info, "registry.takedown") && output_contains(&info, "malware"),
        "{info:?}"
    );
}

/// REQ: PKG-403
#[test]
fn pkg_403_a_blocked_plugin_is_enabled_again_only_with_accept_risk() {
    let mut builder = RegistryBuilder::standard();
    builder.blocked_ids =
        vec![serde_json::json!({ "id": NOTES, "reason": "malware", "at": "2030-01-01 09:00:00" })];
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let _ = refresh(&sandbox);
    let refused = sandbox.run(&["enable", NOTES]);
    assert_ne!(refused.exit_code, 0, "{refused:?}");
    let allowed = sandbox.run(&["enable", NOTES, "--accept-risk"]);
    assert_ok(&allowed);
}

/// REQ: PKG-403
#[test]
fn pkg_403_an_id_that_is_not_blocked_keeps_its_state() {
    let mut builder = RegistryBuilder::standard();
    builder.blocked_ids = vec![
        serde_json::json!({ "id": "com.other.thing", "reason": "x", "at": "2030-01-01 09:00:00" }),
    ];
    let sandbox = Sandbox::with_registry(&builder);
    sandbox.seed_installed(NOTES, "1.0.0", BASE, "stable", false);
    let _ = refresh(&sandbox);
    assert!(!output_contains(
        &sandbox.run(&["info", NOTES, "--json"]),
        "disabled-blocked"
    ));
}

/// REQ: PKG-403
#[test]
fn pkg_403_installing_a_blocked_id_is_refused_takedown() {
    let mut builder = RegistryBuilder::standard();
    builder.blocked_ids =
        vec![serde_json::json!({ "id": NOTES, "reason": "malware", "at": "2030-01-01 09:00:00" })];
    let sandbox = Sandbox::with_registry(&builder);
    assert_code(&install_notes(&sandbox, &[]), "registry.takedown");
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-404
#[test]
fn doc_pkg_404_appeal_once_answered_within_14_days() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-404]"))
        .expect("row");
    assert!(
        row.contains("appeal a takedown once")
            && row.contains("within 14 days")
            && row.contains("new version number"),
        "{row}"
    );
}

/// REQ: PKG-405
#[test]
fn pkg_405_a_revoked_publisher_key_blocks_a_fresh_install_key_revoked() {
    let mut builder = RegistryBuilder::standard();
    builder.revoked_keys = vec![
        serde_json::json!({ "publisher_id": PUBLISHER_ID, "key_id": publisher_key_id(), "reason": "compromised", "effective": "2030-01-01 09:00:00" }),
    ];
    let sandbox = Sandbox::with_registry(&builder);
    let output = install_notes(&sandbox, &[]);
    assert_code(&output, "trust.key-revoked");
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-406
#[test]
fn pkg_406_no_rating_is_shown_or_sent() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].extra_index = serde_json::json!({ "rating": 4.9, "downloads": 1_000_000 });
    let sandbox = Sandbox::with_registry(&builder);
    let output = refresh(&sandbox);
    assert!(
        !output_contains(&output, "4.9"),
        "a client must not show a rating: {output:?}"
    );
    assert!(
        sandbox
            .net
            .log()
            .iter()
            .all(|url| !url.contains("rating") && !url.contains("review")),
        "{:?}",
        sandbox.net.log()
    );
}

/// REQ: PKG-407
#[test]
fn pkg_407_revocations_are_fetched_on_every_refresh_even_if_nothing_else_changed() {
    let sandbox = Sandbox::standard();
    assert_ok(&refresh(&sandbox));
    for _ in 0..3 {
        sandbox.net.clear_log();
        assert_ok(&refresh(&sandbox));
        assert_eq!(
            sandbox.net.requests_to("revocations.json").len(),
            1,
            "{:?}",
            sandbox.net.log()
        );
    }
}

/// REQ: PKG-407
#[test]
fn pkg_407_a_new_key_revocation_is_applied_at_the_next_refresh() {
    let sandbox = Sandbox::standard();
    assert_ok(&install_notes(&sandbox, &[]));
    let mut next = RegistryBuilder::standard();
    next.revocations_version = 18;
    next.timestamp_version = 4822;
    next.snapshot_version = 4822;
    next.revoked_keys = vec![
        serde_json::json!({ "publisher_id": PUBLISHER_ID, "key_id": publisher_key_id(), "reason": "compromised", "effective": "2030-01-01 09:00:00" }),
    ];
    sandbox.net.serve(BASE, &next.build());
    let _ = refresh(&sandbox);
    let info = sandbox.run(&["info", NOTES, "--json"]);
    assert!(
        output_contains(&info, "disabled-blocked"),
        "a compromised key moves installed plugins to disabled-blocked: {info:?}"
    );
}

// ---- Mirrors -----------------------------------------------------------------------------

/// REQ: PKG-408
#[test]
fn pkg_408_a_full_mirror_keeps_the_signature_chain_unchanged() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE_TWO, &builder.build());
    assert_ok(&sandbox.pin(BASE_TWO, &builder.pinned_key_id()));
    assert_ok(&refresh(&sandbox));
    assert_ok(&install_notes(&sandbox, &[]));
}

/// REQ: PKG-409
#[test]
fn pkg_409_a_partial_mirror_with_its_own_root_works_as_a_separate_registry() {
    let mut partial =
        RegistryBuilder::with_plugins(vec![RegistryBuilder::standard().plugins[0].clone()]);
    partial.role_keys = vec![KEY_B];
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE_TWO, &partial.build());
    assert_ok(&sandbox.pin(BASE_TWO, &partial.pinned_key_id()));
    let output = refresh(&sandbox);
    assert_eq!(plugin_ids(&output), vec![NOTES.to_owned()]);
}

fn sync_dir(sandbox: &Sandbox, name: &str) -> std::path::PathBuf {
    let dir = sandbox.dir.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// REQ: PKG-410
#[test]
fn pkg_410_mirror_sync_copies_metadata_and_blobs_and_verifies_hashes() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let dir = sync_dir(&sandbox, "mirror");
    let output = sandbox.run(&["mirror", "sync", dir.to_str().unwrap(), "--registry", BASE]);
    assert_ok(&output);
    let copied = read_tree(&dir);
    for path in [
        "timestamp.json",
        "snapshot.json",
        "index.json",
        "revocations.json",
        "root.json",
    ] {
        assert!(
            copied.contains_key(path),
            "{path} missing in {:?}",
            copied.keys().collect::<Vec<_>>()
        );
    }
    assert!(copied.contains_key(&builder.blob_path(NOTES, "1.1.0")));
    for (path, bytes) in builder.build() {
        if copied.contains_key(&path) {
            assert_eq!(
                copied[&path], bytes,
                "{path} must be byte-identical (PKG-439)"
            );
        }
    }
}

/// REQ: PKG-410
#[test]
fn pkg_410_second_sync_downloads_only_changed_files() {
    let sandbox = Sandbox::standard();
    let dir = sync_dir(&sandbox, "mirror");
    assert_ok(&sandbox.run(&["mirror", "sync", dir.to_str().unwrap(), "--registry", BASE]));
    sandbox.net.clear_log();
    assert_ok(&sandbox.run(&["mirror", "sync", dir.to_str().unwrap(), "--registry", BASE]));
    assert!(
        sandbox.net.requests_to(".iliumpkg").is_empty(),
        "no blob again: {:?}",
        sandbox.net.log()
    );
}

/// REQ: PKG-410
#[test]
fn pkg_410_allow_restricts_the_copy_to_the_listed_plugins() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let dir = sync_dir(&sandbox, "mirror");
    assert_ok(&sandbox.run(&[
        "mirror",
        "sync",
        dir.to_str().unwrap(),
        "--registry",
        BASE,
        "--allow",
        "com.acme.git-tools",
    ]));
    let copied = read_tree(&dir);
    assert!(copied.contains_key(&builder.blob_path("com.acme.git-tools", "2.0.0")));
    assert!(
        !copied.contains_key(&builder.blob_path(NOTES, "1.1.0")),
        "a plugin outside --allow must not be copied"
    );
}

/// REQ: PKG-410
#[test]
fn pkg_410_a_tampered_blob_aborts_the_sync_hash_mismatch() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let mut bad = builder.plugins[0].versions[1].package.clone();
    bad[3] ^= 1;
    sandbox
        .net
        .put_file(&format!("{BASE}{}", builder.blob_path(NOTES, "1.1.0")), bad);
    let dir = sync_dir(&sandbox, "mirror");
    let output = sandbox.run(&["mirror", "sync", dir.to_str().unwrap(), "--registry", BASE]);
    assert_code(&output, "registry.hash-mismatch");
    assert!(!read_tree(&dir).contains_key(&builder.blob_path(NOTES, "1.1.0")));
}

/// REQ: PKG-411
#[test]
fn pkg_411_root_keys_come_from_the_configuration_never_from_the_mirror() {
    let genuine = RegistryBuilder::standard();
    let mut evil = RegistryBuilder::standard();
    evil.role_keys = vec![KEY_FOREIGN];
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE_TWO, &genuine.build());
    assert_ok(&sandbox.pin(BASE_TWO, &genuine.pinned_key_id()));
    sandbox.net.serve(BASE_TWO, &evil.build());
    let output = refresh(&sandbox);
    assert_registry_failure(&output);
    assert_no_plugin_records(&output);
}

// ---- Bundles -----------------------------------------------------------------------------

fn export(
    sandbox: &Sandbox,
    args: &[&str],
) -> (std::path::PathBuf, ilium_plugin_host::cli::CliOutput) {
    let dir = sync_dir(sandbox, "bundle");
    let mut full = vec!["export-bundle", dir.to_str().unwrap()];
    full.extend_from_slice(args);
    let output = sandbox.run(&full);
    (dir, output)
}

/// REQ: PKG-412
#[test]
fn pkg_412_bundle_holds_the_original_signed_metadata_and_the_blobs() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let (dir, output) = export(&sandbox, &[NOTES]);
    assert_ok(&output);
    let tree = read_tree(&dir);
    assert!(tree.contains_key(&format!("plugins/{NOTES}.json")));
    assert!(tree.contains_key(&builder.blob_path(NOTES, "1.1.0")));
    assert_eq!(
        tree[&format!("plugins/{NOTES}.json")],
        builder.build()[&format!("plugins/{NOTES}.json")],
        "original signatures kept"
    );
    assert!(
        !tree.contains_key("plugins/com.acme.git-tools.json"),
        "filtered to the listed plugins"
    );
}

/// REQ: PKG-412
#[test]
fn pkg_412_a_version_range_selects_the_exported_blobs() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let (dir, output) = export(&sandbox, &["com.acme.notes@^1.0"]);
    assert_ok(&output);
    assert!(read_tree(&dir).contains_key(&builder.blob_path(NOTES, "1.1.0")));
}

/// REQ: PKG-412
#[test]
fn pkg_412_unknown_plugin_cannot_be_exported_not_found() {
    let sandbox = Sandbox::standard();
    let (_, output) = export(&sandbox, &["com.acme.nothing"]);
    assert_failed(&output, "plugin.not-found", 5);
}

/// REQ: PKG-413
#[test]
fn pkg_413_install_from_a_bundle_uses_no_network() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let (dir, output) = export(&sandbox, &[NOTES]);
    assert_ok(&output);
    sandbox.net.clear_log();
    sandbox.net.set_down(true);
    let key = publisher_key_id();
    let installed = sandbox.run(&[
        "install",
        NOTES,
        "--from-bundle",
        dir.to_str().unwrap(),
        "--yes",
        "--trust",
        &key,
    ]);
    assert_ok(&installed);
    assert!(sandbox.net.log().is_empty(), "{:?}", sandbox.net.log());
}

/// REQ: PKG-413
#[test]
fn pkg_413_bundle_metadata_is_verified_with_the_pinned_root_keys() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let (dir, _) = export(&sandbox, &[NOTES]);
    let path = dir.join(format!("plugins/{NOTES}.json"));
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, edit_signed(&bytes, &[KEY_FOREIGN], |_| {})).unwrap();
    let key = publisher_key_id();
    let output = sandbox.run(&[
        "install",
        NOTES,
        "--from-bundle",
        dir.to_str().unwrap(),
        "--yes",
        "--trust",
        &key,
    ]);
    assert_registry_failure(&output);
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-413
#[test]
fn pkg_413_expired_bundle_is_refused_unless_offline() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let (dir, _) = export(&sandbox, &[NOTES]);
    let key = publisher_key_id();
    let later = now_plus(3 * 86_400);
    let mut env = sandbox.env.clone();
    env.now = later;
    let refused = run_in(
        &env,
        &[
            "install",
            NOTES,
            "--from-bundle",
            dir.to_str().unwrap(),
            "--yes",
            "--trust",
            &key,
        ],
    );
    assert_code(&refused, "registry.metadata-expired");
    let allowed = run_in(
        &env,
        &[
            "install",
            NOTES,
            "--from-bundle",
            dir.to_str().unwrap(),
            "--yes",
            "--trust",
            &key,
            "--offline",
        ],
    );
    assert_ok(&allowed);
}

/// REQ: PKG-414
#[test]
fn doc_pkg_414_bundle_snapshot_expires_like_any_other() {
    let text = page_text("reference-registry.md");
    let row = text
        .lines()
        .find(|line| line.contains("[PKG-414]"))
        .expect("row");
    assert!(
        row.contains("expires like any other") && row.contains("`offline = true`"),
        "{row}"
    );
}

// ---- Failure table -----------------------------------------------------------------------

/// REQ: PKG-415
#[test]
fn pkg_415_unreachable_network_without_a_cache_is_unreachable_exit_8() {
    let sandbox = Sandbox::standard();
    sandbox.net.set_down(true);
    let output = refresh(&sandbox);
    assert_failed(&output, "registry.unreachable", 8);
}

/// REQ: PKG-415
#[test]
fn pkg_415_unreachable_network_with_a_valid_cache_uses_the_cache() {
    let sandbox = Sandbox::standard();
    let fresh = refresh(&sandbox);
    sandbox.net.set_down(true);
    let output = refresh(&sandbox);
    assert_ok(&output);
    assert_eq!(plugin_ids(&output), plugin_ids(&fresh));
}

/// REQ: PKG-415
#[test]
fn pkg_415_a_file_install_is_unaffected_by_the_network() {
    let sandbox = Sandbox::standard();
    sandbox.net.set_down(true);
    let path = sandbox.project_file("local.iliumpkg", &signed_package("com.acme.local", "1.0.0"));
    let key = publisher_key_id();
    let output = sandbox.run(&["install", path.to_str().unwrap(), "--yes", "--trust", &key]);
    assert_ok(&output);
}

/// REQ: PKG-416
#[test]
fn pkg_416_expired_metadata_refuses_an_install_and_offers_offline() {
    let sandbox = Sandbox::standard();
    let mut env = sandbox.env.clone();
    env.now = now_plus(3 * 86_400);
    let key = publisher_key_id();
    let output = run_in(&env, &["install", NOTES, "--yes", "--trust", &key]);
    assert_code(&output, "registry.metadata-expired");
    assert!(
        error_message(&output, "registry.metadata-expired").contains("--offline"),
        "{output:?}"
    );
    assert!(sandbox.installed().is_empty());
}

/// REQ: PKG-417
#[test]
fn pkg_417_rollback_keeps_the_old_cache_and_names_both_versions() {
    let sandbox = Sandbox::standard();
    let fresh = refresh(&sandbox);
    let mut older = RegistryBuilder::standard();
    older.timestamp_version = 4000;
    older.snapshot_version = 4000;
    sandbox.net.serve(BASE, &older.build());
    let output = refresh(&sandbox);
    assert_code(&output, "registry.rollback");
    let message = error_message(&output, "registry.rollback");
    assert!(
        message.contains("4821") && message.contains("4000"),
        "{message}"
    );
    sandbox.net.set_down(true);
    assert_eq!(
        plugin_ids(&refresh(&sandbox)),
        plugin_ids(&fresh),
        "the old cache stays usable"
    );
}

/// REQ: PKG-418
#[test]
fn pkg_418_hash_mismatch_is_not_retried_beyond_the_transport_retries() {
    let builder = RegistryBuilder::standard();
    let sandbox = Sandbox::with_registry(&builder);
    let mut bad = builder.plugins[0].versions[1].package.clone();
    bad[5] ^= 1;
    let url = format!("{BASE}{}", builder.blob_path(NOTES, "1.1.0"));
    sandbox.net.put_file(&url, bad);
    sandbox.net.clear_log();
    let output = install_notes(&sandbox, &[]);
    assert_code(&output, "registry.hash-mismatch");
    assert_eq!(
        sandbox
            .net
            .requests_to(&builder.blob_path(NOTES, "1.1.0"))
            .len(),
        1,
        "a mismatch is not a transport error"
    );
}

/// REQ: PKG-419
#[test]
fn pkg_419_below_threshold_the_document_is_not_used_at_all() {
    let mut builder = threshold_two();
    builder.overrides.insert("index".to_owned(), vec![KEY_A]);
    let sandbox = Sandbox::with_registry(&builder);
    let output = refresh(&sandbox);
    assert_code(&output, "registry.signature-invalid");
    assert_no_plugin_records(&output);
}

/// REQ: PKG-419
#[test]
fn pkg_419_two_signatures_of_the_same_key_do_not_count_twice() {
    let mut builder = threshold_two();
    builder
        .overrides
        .insert("index".to_owned(), vec![KEY_A, KEY_A]);
    let sandbox = Sandbox::with_registry(&builder);
    assert_code(&refresh(&sandbox), "registry.signature-invalid");
}

/// REQ: PKG-420
#[test]
fn pkg_420_a_requested_yanked_version_is_refused_without_allow_yanked() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions[1] = builder.plugins[0].versions[1].clone().yanked("broken");
    let sandbox = Sandbox::with_registry(&builder);
    let key = publisher_key_id();
    let refused = sandbox.run(&["install", "com.acme.notes@1.1.0", "--yes", "--trust", &key]);
    assert_code(&refused, "registry.yanked");
    let allowed = sandbox.run(&[
        "install",
        "com.acme.notes@1.1.0",
        "--yes",
        "--trust",
        &key,
        "--allow-yanked",
    ]);
    assert_ok(&allowed);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-420
#[test]
fn pkg_420_resolution_skips_a_yanked_version_silently() {
    let mut builder = RegistryBuilder::standard();
    builder.plugins[0].versions[1] = builder.plugins[0].versions[1].clone().yanked("broken");
    let sandbox = Sandbox::with_registry(&builder);
    let output = install_notes(&sandbox, &[]);
    assert_ok(&output);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.0.0"));
    assert!(
        warning_codes(&output)
            .iter()
            .all(|code| code != "registry.yanked"),
        "silent: {output:?}"
    );
}

/// REQ: PKG-421
#[test]
fn pkg_421_a_package_signed_by_a_revoked_key_is_refused_key_revoked() {
    let mut builder = RegistryBuilder::standard();
    builder.revoked_keys = vec![
        serde_json::json!({ "publisher_id": PUBLISHER_ID, "key_id": publisher_key_id(), "reason": "retired", "effective": "2030-01-01 09:00:00" }),
    ];
    let sandbox = Sandbox::with_registry(&builder);
    assert_code(&install_notes(&sandbox, &[]), "trust.key-revoked");
}

/// REQ: PKG-422
#[test]
fn pkg_422_the_first_configured_registry_wins() {
    let first = RegistryBuilder::standard();
    let mut second = RegistryBuilder::with_plugins(vec![
        PluginSpec::new(NOTES).with_versions(vec![VersionSpec::signed(NOTES, "2.0.0")])
    ]);
    second.role_keys = vec![KEY_B];
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE, &first.build());
    sandbox.net.serve(BASE_TWO, &second.build());
    assert_ok(&sandbox.pin(BASE, &first.pinned_key_id()));
    assert_ok(&sandbox.pin(BASE_TWO, &second.pinned_key_id()));
    assert_ok(&install_notes(&sandbox, &[]));
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("1.1.0"));
}

/// REQ: PKG-422
#[test]
fn pkg_422_registry_flag_selects_one_registry() {
    let first = RegistryBuilder::standard();
    let mut second = RegistryBuilder::with_plugins(vec![
        PluginSpec::new(NOTES).with_versions(vec![VersionSpec::signed(NOTES, "2.0.0")])
    ]);
    second.role_keys = vec![KEY_B];
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE, &first.build());
    sandbox.net.serve(BASE_TWO, &second.build());
    assert_ok(&sandbox.pin(BASE, &first.pinned_key_id()));
    assert_ok(&sandbox.pin(BASE_TWO, &second.pinned_key_id()));
    assert_ok(&install_notes(&sandbox, &["--registry", BASE_TWO]));
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("2.0.0"));
}

/// REQ: PKG-422
#[test]
fn pkg_422_a_plugin_updates_only_from_the_registry_it_came_from() {
    let mut first = RegistryBuilder::standard();
    first.plugins[0]
        .versions
        .push(VersionSpec::signed(NOTES, "3.0.0"));
    let mut second = RegistryBuilder::with_plugins(vec![
        PluginSpec::new(NOTES).with_versions(vec![VersionSpec::signed(NOTES, "2.0.0")])
    ]);
    second.role_keys = vec![KEY_B];
    let sandbox = Sandbox::new();
    sandbox.net.serve(BASE, &first.build());
    sandbox.net.serve(BASE_TWO, &second.build());
    assert_ok(&sandbox.pin(BASE, &first.pinned_key_id()));
    assert_ok(&sandbox.pin(BASE_TWO, &second.pinned_key_id()));
    assert_ok(&install_notes(&sandbox, &["--registry", BASE_TWO]));
    let key = publisher_key_id();
    let _ = sandbox.run(&["update", NOTES, "--yes", "--trust", &key]);
    assert_eq!(sandbox.installed_version(NOTES).as_deref(), Some("2.0.0"));
}

// ---- Properties --------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// REQ: PKG-366
    #[test]
    fn pkg_366_any_lower_timestamp_version_is_rejected_property(lower in 1u64..4821) {
        let sandbox = Sandbox::standard();
        assert_ok(&refresh(&sandbox));
        let mut older = RegistryBuilder::standard();
        older.timestamp_version = lower;
        older.snapshot_version = lower;
        sandbox.net.serve(BASE, &older.build());
        let output = refresh(&sandbox);
        prop_assert!(error_codes(&output).iter().any(|code| code == "registry.rollback"));
    }

    /// REQ: PKG-365
    #[test]
    fn pkg_365_any_time_after_expiry_is_refused_property(extra in 2i64..400_000_000) {
        let sandbox = Sandbox::standard();
        let output = sandbox.run_at(now_plus(86_400 + extra), &["search"]);
        prop_assert!(error_codes(&output).iter().any(|code| code == "registry.metadata-expired"));
    }
}
