//! Requirement coverage for the publish lane: rights claims and counter-notices (PUB-050..PUB-056),
//! licence changes (PUB-057..PUB-060), sanctions screening (PUB-061..PUB-063), export
//! classification (PUB-064), non-code assets and notices (PUB-065..PUB-068), and paid plugins and
//! licence keys (PUB-069..PUB-072).
//!
//! Behaviour that is not implemented yet is exercised through the closest existing entry point
//! (`cli::run` for the `ilium plugin` verbs, `FakeRegistry` for the registry-side publish checks).
//! Where the manual names a verb that does not exist, the test calls that verb and asserts the
//! documented result, so it fails at runtime with the reason in its assertion message.

use ilium_plugin_host::cli::{self, CliEnv, CliOutput};
use ilium_plugin_host::package::write_package;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::signing::{sign, SignedAt, SigningKey};
use ilium_plugin_host::testing::{wat_component, FakeRegistry};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const PLUGIN: &str = "example.agent-tools";
const REGISTRY: &str = "https://registry.test";
/// 2030-01-01 12:00:00 UTC, the fixed "now" of the test environment.
const NOW_SECS: u64 = 1_893_499_200;
const SBOM: &[u8] = br#"{"components":[]}"#;
const LICENSE_TEXT: &[u8] = b"MIT License\n";

/// A fetcher that reaches no network: every registry call fails as unreachable.
struct NoNetwork;

impl Fetcher for NoNetwork {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECS)
}

fn env_at(dir: &Path, now: SystemTime) -> CliEnv {
    CliEnv::new(
        dir.join("data"),
        dir.join("state"),
        dir.join("config"),
        dir.to_path_buf(),
        now,
        true,
        Arc::new(NoNetwork),
    )
}

/// Runs `ilium plugin <args>` in `dir` at the fixed time.
fn run(dir: &Path, args: &[&str]) -> CliOutput {
    run_at(dir, now(), args)
}

/// Runs `ilium plugin <args>` in `dir` at `now`, for the time-based rules.
fn run_at(dir: &Path, now: SystemTime, args: &[&str]) -> CliOutput {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    cli::run(&owned, &env_at(dir, now))
}

/// Everything a command printed, joined for substring checks.
fn text_of(out: &CliOutput) -> String {
    format!("{}\n{}", out.stdout_raw, out.stderr)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write fixture file");
    path.to_str().expect("utf-8 path").to_owned()
}

/// The manifest of the test plugin. `license` and `description` are the fields the rules vary.
fn manifest(version: &str, license: &str, description: &str) -> Value {
    json!({
        "api": "ilium:plugin@0.1.0",
        "id": PLUGIN,
        "name": "Agent tools",
        "version": version,
        "publisher": "example",
        "description": description,
        "license": license,
        "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": [] },
        "capabilities": { "server": ["tree.read", "tree.decorate"] },
        "contributes": {
            "decoration_slots": [{ "id": "work", "placement": "badge", "priority": 50 }]
        }
    })
}

/// An unsigned package: the manifest with its file table, the component and each extra file.
fn package_with(mut manifest_value: Value, extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let component = wat_component("(component)");
    let mut files: Vec<(String, Vec<u8>)> = vec![("server.wasm".to_owned(), component)];
    files.extend(extra.iter().map(|(path, bytes)| ((*path).to_owned(), bytes.clone())));
    let listing: Vec<Value> = files
        .iter()
        .map(|(path, bytes)| json!({ "path": path, "length": bytes.len(), "sha256": sha256_hex(bytes) }))
        .collect();
    manifest_value["files"] = Value::Array(listing);
    let manifest_bytes = serde_json::to_vec_pretty(&manifest_value).expect("manifest json");
    let mut entries = vec![("manifest.json".to_owned(), manifest_bytes)];
    entries.extend(files);
    write_package(&entries).expect("unsigned package")
}

/// The same package signed with a throwaway Ed25519 seed.
fn signed(package: Vec<u8>) -> Vec<u8> {
    sign(
        &package,
        &SigningKey(vec![7u8; 32]),
        SignedAt("2030-01-01 12:00:00".to_owned()),
    )
    .expect("signed package")
}

/// A signed release of the test plugin with an SBOM and a LICENSE file.
fn release_bytes(version: &str, license: &str) -> Vec<u8> {
    signed(package_with(
        manifest(version, license, "Marks agents that start working."),
        &[("sbom.json", SBOM.to_vec()), ("LICENSE", LICENSE_TEXT.to_vec())],
    ))
}

/// A registry that requires level B inputs at publish time.
fn level_b_registry() -> FakeRegistry {
    let mut registry = FakeRegistry::new(REGISTRY);
    registry.require_level_b();
    registry
}

// ---- PUB-050..PUB-056: rights claims and counter-notices ---------------------------------------

/// REQ: PUB-050
#[test]
fn pub_050_claim_without_proof_of_ownership_is_answered_with_a_request_and_starts_no_window() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &[
            "claim", "submit", "--plugin", PLUGIN, "--legal-name", "Rights Holder Ltd",
            "--contact", "claims@rights.test", "--good-faith",
        ],
    );
    let text = text_of(&out);
    assert!(
        text.contains("proof-of-ownership") && !text.contains("decision_due"),
        "PUB-050 not implemented: no claim intake verb exists in `ilium plugin`; a claim without \
         proof of ownership must be answered with a request for that item and start no decision \
         window (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-051
#[test]
fn pub_051_complete_claim_is_acknowledged_with_a_claim_id_and_decided_within_the_window() {
    let dir = tempfile::tempdir().expect("tempdir");
    let proof = write_file(dir.path(), "proof.pdf", b"%PDF-1.4 proof of ownership");
    let out = run(
        dir.path(),
        &[
            "claim", "submit", "--plugin", PLUGIN, "--version", "0.1.0", "--work", "Agent tools",
            "--proof", proof.as_str(), "--legal-name", "Rights Holder Ltd",
            "--contact", "claims@rights.test", "--good-faith",
        ],
    );
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("claim_id") && text.contains("decision_due"),
        "PUB-051 not implemented: a complete claim must be acknowledged with a claim id within 1 \
         business day and carry a decision due within 10 business days (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-052
#[test]
fn pub_052_open_claim_quarantines_only_the_version_it_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["claim", "quarantine", "--plugin", PLUGIN, "--version", "0.2.0"],
    );
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("0.2.0") && !text.contains("0.1.0"),
        "PUB-052 not implemented: quarantine through `quarantine.json` must name only the claimed \
         version 0.2.0 and leave 0.1.0 resolving (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-053
#[test]
fn pub_053_counter_notice_is_forwarded_and_the_publisher_receives_the_forwarding_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &[
            "claim", "counter-notice", "--plugin", PLUGIN, "--version", "0.2.0",
            "--reason", "The work is mine", "--contact", "publisher@example.test",
        ],
    );
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("forwarded_at"),
        "PUB-053 not implemented: a counter-notice must be forwarded to the claimant within 1 \
         business day and the publisher receipt must carry the forwarding time (exit {}, output: \
         {text})",
        out.exit_code
    );
}

/// REQ: PUB-054
#[test]
fn pub_054_quarantine_is_withdrawn_after_14_days_without_filing_but_takedown_stays() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fifteen_days_later = now() + Duration::from_secs(15 * 86_400);
    let out = run_at(dir.path(), fifteen_days_later, &["claim", "sweep", "--plugin", PLUGIN]);
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("quarantine withdrawn") && !text.contains("disabled-blocked"),
        "PUB-054 not implemented: 14 calendar days after forwarding with no court filing, a \
         quarantine must be withdrawn while a takedown stays disabled-blocked (exit {}, output: \
         {text})",
        out.exit_code
    );
}

/// REQ: PUB-055
#[test]
fn pub_055_three_upheld_claims_in_twelve_months_suspend_the_publisher_for_90_days() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut last = None;
    for _ in 0..3 {
        last = Some(run(
            dir.path(),
            &["claim", "uphold", "--publisher", "example", "--plugin", PLUGIN],
        ));
    }
    let out = last.expect("three uphold runs");
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("suspended") && text.contains("90"),
        "PUB-055 not implemented: the third upheld claim within 12 months must suspend publisher \
         `example` for 90 days and tell its maintainers the count and the date of the last claim \
         (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-056
#[test]
fn pub_056_claim_decision_is_logged_without_claimant_contact_or_counter_notice_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &[
            "claim", "decide", "--plugin", PLUGIN, "--version", "0.1.0", "--decision", "take-down",
            "--contact", "claims@rights.test",
        ],
    );
    let text = text_of(&out);
    assert!(
        out.exit_code == 0
            && text.contains("transparency-log")
            && text.contains("claim_id")
            && text.contains("decision")
            && !text.contains("claims@rights.test"),
        "PUB-056 not implemented: every decision must be logged in the transparency log with the \
         claim id, plugin id, version, decision and time, and never the claimant's contact address \
         (exit {}, output: {text})",
        out.exit_code
    );
}

// ---- PUB-057..PUB-060: licence changes ------------------------------------------------------------

/// REQ: PUB-057
#[test]
fn pub_057_licence_change_release_is_held_until_release_notes_name_both_identifiers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package = write_file(dir.path(), "example.agent-tools-0.2.0.iliumpkg", &release_bytes("0.2.0", "Apache-2.0"));
    let notes = write_file(dir.path(), "notes.md", b"Now under Apache-2.0.");
    let out = run(dir.path(), &["publish", package.as_str(), "--notes", notes.as_str(), "--json"]);
    let text = text_of(&out);
    assert!(
        text.contains("held") && text.contains("MIT") && text.contains("Apache-2.0"),
        "PUB-057 not implemented: a release changing `license` from MIT to Apache-2.0 whose notes \
         name only one identifier must be held (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-057
#[test]
fn pub_057_plugin_info_prints_both_licence_identifiers_of_a_licence_change() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(dir.path(), &["info", "example.agent-tools@0.2.0"]);
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("MIT") && text.contains("Apache-2.0"),
        "PUB-057 not implemented: `ilium plugin info example.agent-tools@0.2.0` must print the old \
         and the new SPDX identifier (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-058
#[test]
fn pub_058_auto_update_install_shows_a_licence_change_notice_and_waits_for_update_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = dir.path().join("config").join("ilium");
    std::fs::create_dir_all(&config).expect("config dir");
    write_file(&config, "config.toml", b"[plugins]\nauto_update = \"install\"\n");
    let out = run(dir.path(), &["update", PLUGIN, "--json"]);
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("license") && text.contains("notice") && text.contains("not installed"),
        "PUB-058 not implemented: with auto_update = install a version whose licence differs must \
         not be installed; a notice must show and it installs only after `ilium plugin update` \
         (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-059
#[test]
fn pub_059_licence_of_a_published_version_entry_is_not_changed_by_a_later_release() {
    let mut registry = FakeRegistry::new(REGISTRY);
    registry
        .try_publish(release_bytes("0.1.0", "MIT"), "stable")
        .expect("0.1.0 publishes under MIT");
    registry
        .try_publish(release_bytes("0.2.0", "Apache-2.0"), "stable")
        .expect("0.2.0 publishes under Apache-2.0");
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(dir.path(), &["info", "example.agent-tools@0.1.0"]);
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("MIT") && !text.contains("Apache-2.0"),
        "PUB-059 not implemented: the signed entry of 0.1.0 must keep license MIT after the \
         Apache-2.0 release, and no client-side view may show the later licence for it (exit {}, \
         output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-060
#[test]
fn pub_060_licence_change_without_consent_from_every_contributor_is_held() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package = write_file(dir.path(), "example.agent-tools-0.2.0.iliumpkg", &release_bytes("0.2.0", "Apache-2.0"));
    let out = run(
        dir.path(),
        &["publish", package.as_str(), "--contributor-consent", "alice", "--json"],
    );
    let text = text_of(&out);
    assert!(
        text.contains("held") && text.contains("contributor-consent"),
        "PUB-060 not implemented: a licence-changing release with consent from only one of two \
         contributors must be held (exit {}, output: {text})",
        out.exit_code
    );
}

// ---- PUB-061..PUB-063: sanctions screening -----------------------------------------------------------

/// REQ: PUB-061
#[test]
fn pub_061_sanctioned_name_is_refused_with_account_refused_and_no_token() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["account", "create", "--name", "Sanctioned Fixture Name", "--claims-contact", "claims@registry.test"],
    );
    let text = text_of(&out);
    assert!(
        text.contains("registry.account-refused") && !text.contains("token"),
        "PUB-061 not implemented: an account matching the fixture sanctions list must be refused \
         with registry.account-refused and no token (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-062
#[test]
fn pub_062_sanctions_refusal_names_neither_the_list_nor_the_country() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["account", "create", "--name", "Sanctioned Fixture Name", "--claims-contact", "claims@registry.test"],
    );
    let text = text_of(&out);
    assert!(
        text.contains("registry.account-refused")
            && text.contains("claims@registry.test")
            && !text.contains("sanctions")
            && !text.contains("Sanctioned Fixture Name")
            && !text.contains("country"),
        "PUB-062 not implemented: the refusal must show only registry.account-refused and the \
         claims contact, never the list, the matched entry or the country (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-063
#[test]
fn pub_063_screening_result_stays_in_the_operator_record_and_out_of_public_documents() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(dir.path(), &["account", "screen", "--name", "Fixture Account"]);
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("screened_at") && text.contains("private-record"),
        "PUB-063 not implemented: the screening result and its date must be kept in the operator's \
         private record only (exit {}, output: {text})",
        out.exit_code
    );
}

// ---- PUB-064: export classification ----------------------------------------------------------------

/// REQ: PUB-064
#[test]
fn pub_064_cryptographic_dependency_without_export_classification_row_fails_the_release_check() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_file(
        dir.path(),
        "sbom.json",
        br#"{"components":[{"name":"ed25519-dalek","algorithm":"Ed25519"}]}"#,
    );
    write_file(dir.path(), "export-classification.md", b"# Export classification\n");
    let out = run(dir.path(), &["check"]);
    let text = text_of(&out);
    assert!(
        out.exit_code != 0 && text.contains("export-classification-missing"),
        "PUB-064 not implemented: the release check must fail when an encryption dependency \
         (ed25519-dalek, Ed25519) has no row in the export classification statement (exit {}, \
         output: {text})",
        out.exit_code
    );
}

// ---- PUB-065..PUB-068: non-code assets and notices -------------------------------------------------

/// REQ: PUB-065
#[test]
fn pub_065_level_b_publish_refuses_a_bundled_asset_that_the_notice_file_does_not_list() {
    let mut registry = level_b_registry();
    let font = b"font bytes".to_vec();
    let without_notice = signed(package_with(
        manifest("0.1.0", "MIT", "Agent tools."),
        &[("sbom.json", SBOM.to_vec()), ("LICENSE", LICENSE_TEXT.to_vec()), ("assets/font.ttf", font.clone())],
    ));
    let refused = registry.try_publish(without_notice, "stable");
    assert!(
        format!("{refused:?}").contains("license-notice-missing"),
        "PUB-065: a package with a font and no NOTICE must be refused with license-notice-missing: {refused:?}"
    );
    let notice_without_font = signed(package_with(
        manifest("0.1.1", "MIT", "Agent tools."),
        &[
            ("sbom.json", SBOM.to_vec()),
            ("LICENSE", LICENSE_TEXT.to_vec()),
            ("NOTICE", b"Third-party notices: none.\n".to_vec()),
            ("assets/font.ttf", font),
        ],
    ));
    let result = registry.try_publish(notice_without_font, "stable");
    assert!(
        result.is_err(),
        "PUB-065 not implemented: NOTICE does not list assets/font.ttf with its path, source, \
         licence and attribution, so the level B publish must be refused with \
         license-notice-missing (got {result:?})"
    );
}

/// REQ: PUB-066
#[test]
fn pub_066_level_b_publish_refuses_an_asset_whose_licence_forbids_redistribution() {
    let mut registry = level_b_registry();
    let restricted = b"licence: no redistribution".to_vec();
    let package = signed(package_with(
        manifest("0.1.0", "MIT", "Agent tools."),
        &[
            ("sbom.json", SBOM.to_vec()),
            ("LICENSE", LICENSE_TEXT.to_vec()),
            ("NOTICE", b"assets/model.bin: no redistribution\n".to_vec()),
            ("assets/model.bin", restricted),
        ],
    ));
    let result = registry.try_publish(package, "stable");
    let detail = format!("{result:?}");
    assert!(
        detail.contains("registry.policy") && detail.contains("asset-licence-missing"),
        "PUB-066 not implemented: a level B review must refuse a version bundling a non-redistributable \
         asset with registry.policy and asset-licence-missing (got {detail})"
    );
}

/// REQ: PUB-067
#[test]
fn pub_067_plugin_info_shows_the_manifest_licence_and_not_an_asset_licence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(dir.path(), &["info", "example.agent-tools@0.1.0"]);
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("license: MIT") && !text.contains("CC-BY-NC"),
        "PUB-067 not implemented: `ilium plugin info` must show the manifest licence MIT as the \
         plugin licence, with bundled asset licences only in NOTICE (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-068
#[test]
fn pub_068_level_b_review_refuses_an_asset_whose_bytes_no_longer_match_the_notice_sha256() {
    let mut registry = level_b_registry();
    let original = b"original font".to_vec();
    let notice = format!("assets/font.ttf sha256={}\n", sha256_hex(&original));
    let tampered = b"changed font!".to_vec();
    let package = signed(package_with(
        manifest("0.1.0", "MIT", "Agent tools."),
        &[
            ("sbom.json", SBOM.to_vec()),
            ("LICENSE", LICENSE_TEXT.to_vec()),
            ("NOTICE", notice.into_bytes()),
            ("assets/font.ttf", tampered),
        ],
    ));
    let result = registry.try_publish(package, "stable");
    assert!(
        format!("{result:?}").contains("sha256"),
        "PUB-068 not implemented: an asset whose bytes differ from the SHA-256 listed in NOTICE \
         must fail the level B SHA-256 comparison (got {result:?})"
    );
}

/// REQ: PUB-068
#[test]
fn pub_068_review_record_carries_the_number_of_assets_checked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(dir.path(), &["review", "example.agent-tools@0.1.0"]);
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("assets_checked"),
        "PUB-068 not implemented: the level B review record must carry assets_checked (exit {}, \
         output: {text})",
        out.exit_code
    );
}

// ---- PUB-069..PUB-072: paid plugins and licence keys -----------------------------------------------

/// REQ: PUB-069
#[test]
fn pub_069_registry_accepts_a_paid_plugin_that_states_a_price_and_a_licence_key_check() {
    let mut registry = FakeRegistry::new(REGISTRY);
    let package = signed(package_with(
        manifest("0.1.0", "LicenseRef-Proprietary", "Requires a licence key from the seller."),
        &[
            ("sbom.json", SBOM.to_vec()),
            ("LICENSE", b"Proprietary terms.\n".to_vec()),
            ("README.md", b"Price: 5 EUR. Refunds: seller@example.test\n".to_vec()),
        ],
    ));
    let result = registry.try_publish(package, "stable");
    assert!(
        result.is_ok(),
        "PUB-069: the registry must not refuse a publish because the plugin charges for use (got {result:?})"
    );
}

/// REQ: PUB-070
#[test]
fn pub_070_paid_plugin_without_a_terms_file_is_reported_by_the_check() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_file(
        dir.path(),
        "manifest.json",
        serde_json::to_string_pretty(&manifest("0.1.0", "LicenseRef-Proprietary", "Paid plugin."))
            .expect("json")
            .as_bytes(),
    );
    write_file(dir.path(), "README.md", b"Price: 5 EUR per year. Refunds: seller@example.test\n");
    let out = run(dir.path(), &["check"]);
    let text = text_of(&out);
    assert!(
        out.exit_code != 0 && text.contains("terms") && text.contains("missing"),
        "PUB-070 not implemented: a paid plugin without a terms file in LICENSE or named in the \
         README must be reported by the check (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-071
#[test]
fn pub_071_plugin_page_of_a_paid_plugin_shows_the_seller_contact_address() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(dir.path(), &["info", "example.agent-tools@0.2.0"]);
    let text = text_of(&out);
    assert!(
        out.exit_code == 0 && text.contains("seller contact"),
        "PUB-071 not implemented: the page of a paid plugin must show the seller's contact address \
         for refund and support requests (exit {}, output: {text})",
        out.exit_code
    );
}

/// REQ: PUB-072
#[test]
fn pub_072_publish_refuses_a_package_that_contains_a_licence_key() {
    let mut registry = FakeRegistry::new(REGISTRY);
    let package = signed(package_with(
        manifest("0.1.0", "LicenseRef-Proprietary", "Requires a licence key from the seller."),
        &[
            ("sbom.json", SBOM.to_vec()),
            ("LICENSE", b"Proprietary terms.\n".to_vec()),
            ("README.md", b"Activate with LIC-TEST-KEY-7F3A9C\n".to_vec()),
        ],
    ));
    let result = registry.try_publish(package, "stable");
    assert!(
        result.is_err(),
        "PUB-072 not implemented: a package containing a licence key string must not be published \
         (got {result:?})"
    );
}

/// REQ: PUB-072
#[test]
fn pub_072_licence_key_plugin_must_declare_its_key_check_in_the_manifest() {
    let mut registry = FakeRegistry::new(REGISTRY);
    let package = signed(package_with(
        manifest("0.1.0", "LicenseRef-Proprietary", "Agent tools."),
        &[
            ("sbom.json", SBOM.to_vec()),
            ("LICENSE", b"Proprietary terms.\n".to_vec()),
            ("README.md", b"Enter your licence key in the settings.\n".to_vec()),
        ],
    ));
    let result = registry.try_publish(package, "stable");
    assert!(
        result.is_err(),
        "PUB-072 not implemented: a plugin that checks a licence key must declare the check in its \
         manifest (description `Requires a licence key`), otherwise the publish must be refused \
         (got {result:?})"
    );
}
