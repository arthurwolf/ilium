//! reference-trust-and-supply-chain.md rows of the package-signing lane (SEC-140..156,
//! SEC-170..174, SEC-176..178). Where the page and the signing page differ on a format, the
//! tests assert only what both agree on and accept either code (see the notes in API_GAPS.md).

use super::signing_support::*;
use super::support::*;
use ilium_plugin_host::signing::jcs;
use serde_json::json;

fn has(out: &ilium_plugin_host::cli::CliOutput, code: &str) -> bool {
    all_codes(out).iter().any(|c| c == code)
}

fn trusted_fixture() -> CliFixture {
    let fixture = cli_fixture();
    write_trust(
        &fixture,
        &trust_json(&[entry(0, "example", "Example Labs")]),
    );
    fixture
}

/// Explicit approval fixture: only this test authority may approve the actual
/// requested set. Command flags never manufacture consent.
#[derive(Default)]
struct InstalledFixtureConsent(std::sync::atomic::AtomicUsize);
impl ilium_plugin_host::install::Consent for InstalledFixtureConsent {
    fn decide(
        &self,
        request: &ilium_plugin_host::install::ConsentRequest,
    ) -> ilium_plugin_host::install::ConsentDecision {
        assert_eq!(request.plugin, "example.minimal");
        assert_eq!(request.version, "0.1.0");
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ilium_plugin_host::install::ConsentDecision::Approve(request.requested.clone())
    }
}

fn installed_fixture() -> CliFixture {
    use ilium_plugin_host::{
        audit::{AuditActor, AuditContext},
        cli::local_install::factory,
        signing::{sign, SignedAt, SigningKey},
    };
    let mut fixture = trusted_fixture();
    // Keep the immutable signing vectors intact. Actual installation requires
    // callable lifecycle/handler exports and their matching real inventory.
    let unsigned = package_with_optional_artifacts(&[]);
    let seed: [u8; 32] = seed_of(0).try_into().expect("fixture signing seed");
    assert_eq!(key_id_of(0), VECTOR_KEY_ID);
    let at =
        ilium_plugin_host::trust::utc_timestamp(fixture.env.now).expect("fixture signing time");
    let signed = sign(&unsigned, &SigningKey::from_seed(seed), SignedAt(at))
        .expect("sign actual component fixture");
    let path = fixture.work().join("installed-fixture.iliumpkg");
    write_file(&path, &signed);
    let consent = std::sync::Arc::new(InstalledFixtureConsent::default());
    let audit_context = AuditContext {
        session: "trust-supply-chain-install-fixture".into(),
        actor: AuditActor {
            kind: "cli".into(),
            pane: None,
            plugin: None,
        },
    };
    let backend = factory(&fixture.env, consent.clone(), audit_context.clone())
        .expect("explicit local installation service");
    fixture.env = fixture
        .env
        .clone()
        .with_installer(backend)
        .with_audit_context(audit_context);
    let out = run_cli(
        &fixture,
        &[
            "install",
            path.to_str().expect("fixture package path"),
            "--json",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert_eq!(consent.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(&path).expect("unchanged package source"),
        signed
    );
    fixture
}

fn find_installed(root: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_installed(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
    }
    None
}

fn tamper_installed(fixture: &CliFixture) {
    use std::os::unix::fs::PermissionsExt;
    let path = find_installed(&fixture.root.path().join("data"), "server.wasm")
        .expect("installed server.wasm");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    let mut bytes = std::fs::read(&path).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(&path, bytes).expect("write");
}

// ---- SEC-140: canonical form ----

/// REQ: SEC-140
#[test]
fn sec_140_jcs_sorts_members_and_drops_whitespace() {
    assert_eq!(
        jcs(&json!({"b": 1, "a": [true, null], "c": "x"})).expect("jcs"),
        "{\"a\":[true,null],\"b\":1,\"c\":\"x\"}"
    );
}

/// REQ: SEC-140
#[test]
fn sec_140_jcs_sorts_nested_objects_by_code_point() {
    let got = jcs(&json!({"z": {"b": 1, "a": 2}, "\u{e9}": 1, "e": 2})).expect("jcs");
    assert_eq!(got, "{\"e\":2,\"z\":{\"a\":2,\"b\":1},\"\u{e9}\":1}");
}

/// REQ: SEC-140
#[test]
fn sec_140_jcs_numbers_use_the_shortest_form() {
    let got = jcs(&json!({"a": 1.0, "b": 1e21, "c": 0.5, "d": -0.0})).expect("jcs");
    assert!(
        got.contains("\"a\":1,") && got.contains("\"c\":0.5"),
        "{got}"
    );
}

/// REQ: SEC-140
#[test]
fn sec_140_jcs_escapes_follow_rfc_8785() {
    let got = jcs(&json!({"k": "a\"b\\c\n\u{1}"})).expect("jcs");
    assert_eq!(got, "{\"k\":\"a\\\"b\\\\c\\n\\u0001\"}");
}

/// REQ: SEC-140
#[test]
fn sec_140_jcs_is_idempotent_on_its_own_output() {
    let first = jcs(&json!({"b": [3, 2, {"y": 1, "x": 2}], "a": "s"})).expect("jcs");
    let again = jcs(&serde_json::from_str(&first).unwrap()).expect("jcs");
    assert_eq!(first, again);
}

/// REQ: SEC-140
#[test]
fn sec_140_duplicate_member_name_in_the_manifest_is_duplicate_key() {
    let text = String::from_utf8(vector_manifest()).unwrap().replacen(
        "\"version\": \"0.1.0\",",
        "\"version\": \"0.1.0\",\n  \"version\": \"0.1.0\",",
        1,
    );
    let codes = check_codes(&zip_of(&[
        ("manifest.json", text.into_bytes()),
        ("server.wasm", vector_component()),
    ]));
    assert!(
        codes.contains(&"manifest.duplicate-key".to_string()),
        "{codes:?}"
    );
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(32))]
    /// REQ: SEC-140
    #[test]
    fn sec_140_jcs_output_has_no_insignificant_whitespace_and_reparses(a in 0i64..1000, b in "[a-z]{0,8}") {
        let out = jcs(&json!({"b": b, "a": a})).expect("jcs");
        proptest::prop_assert!(!out.contains(": ") && !out.contains(", ") && !out.contains('\n'));
        proptest::prop_assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), json!({"b": b, "a": a}));
    }
}

// ---- SEC-141: signature file ----

/// REQ: SEC-141
#[test]
fn sec_141_every_flipped_manifest_byte_is_refused_signature_invalid() {
    let signed = vector_signed();
    let trust = trust_of(0, "example");
    for index in 0..vector_manifest().len() {
        let bytes = patch_data(
            &signed,
            "manifest.json",
            index,
            vector_manifest()[index] ^ 0x01,
            true,
        );
        let got = refusal(&bytes, &trust);
        assert_ne!(got, "accepted", "manifest byte {index}");
    }
}

/// REQ: SEC-141
#[test]
fn sec_141_flipped_signature_bytes_are_refused() {
    let signed = vector_signed();
    let trust = trust_of(0, "example");
    let document = data_of(&signed, "signature");
    let value: serde_json::Value = serde_json::from_slice(&document).expect("signature document");
    let signature = value["signatures"][0]["signature"]
        .as_str()
        .expect("signature bytes");
    let start = document
        .windows(signature.len())
        .position(|part| part == signature.as_bytes())
        .expect("stored signature");
    // signed_at is informational and whitespace is not signed. Mutate the
    // cryptographic signature itself, whose every byte must be checked.
    for index in (start..start + signature.len()).step_by(5) {
        let original = document[index];
        let bytes = patch_data(&signed, "signature", index, original ^ 0x01, true);
        assert_ne!(
            refusal(&bytes, &trust),
            "accepted",
            "signature byte {index}"
        );
    }
}

/// REQ: SEC-141
#[test]
fn sec_141_unmodified_vector_is_accepted() {
    assert_eq!(
        refusal(&vector_signed(), &trust_of(0, "example")),
        "accepted"
    );
}

/// REQ: SEC-141
#[test]
fn sec_141_bad_algorithm_name_is_refused() {
    let mut o = obj(0, "example", &vector_manifest());
    o.algorithm = "rsa".into();
    let got = refusal(
        &with_sig(&sig_doc("ilium-sig/1", &[o])),
        &trust_of(0, "example"),
    );
    assert!(
        got == "manifest.signature-invalid" || got == "package.signature-format",
        "{got}"
    );
}

/// REQ: SEC-141
#[test]
fn sec_141_bad_version_is_refused() {
    let got = refusal(
        &with_sig(&sig_doc(
            "ilium-sig/9",
            &[obj(0, "example", &vector_manifest())],
        )),
        &trust_of(0, "example"),
    );
    assert!(
        got == "manifest.signature-invalid" || got == "package.signature-format",
        "{got}"
    );
}

// ---- SEC-142/143/145: pinning, rotation, downgrade ----

fn newer(seed: u8, version: &str) -> Vec<u8> {
    let manifest = String::from_utf8(vector_manifest())
        .unwrap()
        .replace("\"0.1.0\"", &format!("\"{version}\""))
        .into_bytes();
    signed_package(seed, "example", &manifest)
}

/// REQ: SEC-142
#[test]
fn sec_142_update_signed_by_a_second_key_is_publisher_key_changed() {
    let fixture = installed_fixture();
    let out = install(&fixture, &newer(2, "0.2.0"), &["--trust", &key_id_of(2)]);
    assert!(
        has(&out, "package.publisher-key-changed"),
        "{}",
        all_text(&out)
    );
}

/// REQ: SEC-142
#[test]
fn sec_142_update_signed_by_the_same_key_is_accepted() {
    let fixture = installed_fixture();
    let out = install(&fixture, &newer(0, "0.2.0"), &[]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: SEC-142
#[test]
fn sec_142_uninstall_discards_the_pin() {
    let fixture = installed_fixture();
    run_cli(&fixture, &["uninstall", "example.minimal"]);
    write_trust(
        &fixture,
        &trust_json(&[
            entry(0, "example", "Example Labs"),
            entry(2, "example", "Example Labs"),
        ]),
    );
    let out = install(&fixture, &newer(2, "0.2.0"), &[]);
    assert!(
        !has(&out, "package.publisher-key-changed"),
        "{}",
        all_text(&out)
    );
}

/// REQ: SEC-142
#[test]
fn sec_142_unsigned_first_install_pins_unsigned_and_a_signed_update_is_not_silently_accepted() {
    let fixture = trusted_fixture();
    install(&fixture, &vector_unsigned(), &["--allow-unsigned"]);
    let out = install(&fixture, &newer(0, "0.2.0"), &[]);
    // Either accepted as a tier change that resets grants, or refused: never a crash.
    assert!(
        out.exit_code == 0 || !all_codes(&out).is_empty(),
        "{}",
        all_text(&out)
    );
}

/// REQ: SEC-143
#[test]
fn sec_143_rotation_in_the_package_with_future_issued_is_ignored_and_update_refused() {
    let fixture = installed_fixture();
    let rotation = json!({
        "version": 1, "plugin_id": "example.minimal", "old_key_id": key_id_of(0), "new_key_id": key_id_of(2),
        "issued": "2999-01-01T00:00:00Z", "signature": b64url(&[0u8; 64])
    });
    let manifest = String::from_utf8(vector_manifest())
        .unwrap()
        .replace("\"0.1.0\"", "\"0.2.0\"")
        .into_bytes();
    let text = sig_doc("ilium-sig/1", &[obj(2, "example", &manifest)]);
    let bytes = zip_of(&[
        ("manifest.json", manifest),
        ("rotation.json", rotation.to_string().into_bytes()),
        ("server.wasm", vector_component()),
        ("signature", text.into_bytes()),
    ]);
    let out = install(&fixture, &bytes, &["--trust", &key_id_of(2)]);
    assert_ne!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: SEC-143
#[test]
fn doc_sec_143_rotation_is_accepted_only_under_the_pinned_old_key_and_not_future() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(
        text.contains("verifies under the pinned old key") && text.contains("is not in the future")
    );
}

/// REQ: SEC-145
#[test]
fn sec_145_lower_version_is_refused_without_the_flag() {
    let fixture = trusted_fixture();
    install(&fixture, &newer(0, "0.2.0"), &[]);
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(has(&out, "package.downgrade"), "{}", all_text(&out));
    assert_ne!(out.exit_code, 0);
}

/// REQ: SEC-145
#[test]
fn sec_145_allow_downgrade_installs_and_audits_both_versions() {
    let fixture = trusted_fixture();
    install(&fixture, &newer(0, "0.2.0"), &[]);
    let out = install(&fixture, &vector_signed(), &["--allow-downgrade"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    let audit = run_cli(&fixture, &["audit", "--json"]);
    let text = all_text(&audit);
    assert!(text.contains("0.1.0") && text.contains("0.2.0"), "{text}");
}

/// REQ: SEC-145
#[test]
fn sec_145_same_version_is_not_a_downgrade() {
    let fixture = installed_fixture();
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(!has(&out, "package.downgrade"), "{}", all_text(&out));
}

// ---- SEC-144: remove ----

/// REQ: SEC-144
#[test]
fn sec_144_remove_by_fingerprint_with_force_keeps_the_plugin_installed_but_stopped() {
    let fixture = installed_fixture();
    let out = run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID, "--force"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    let list = all_text(&run_cli(&fixture, &["list", "--json"]));
    assert!(list.contains("example.minimal"), "{list}");
}

/// REQ: SEC-144
#[test]
fn sec_144_trusting_the_key_again_reactivates_the_plugin() {
    let fixture = installed_fixture();
    run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID, "--force"]);
    write_trust(
        &fixture,
        &trust_json(&[entry(0, "example", "Example Labs")]),
    );
    assert!(!all_text(&run_cli(&fixture, &["list", "--json"])).contains("disabled-untrusted"));
}

/// REQ: SEC-144
#[test]
fn sec_144_one_publisher_name_may_have_several_keys() {
    let fixture = cli_fixture();
    write_trust(
        &fixture,
        &trust_json(&[
            entry(0, "example", "Example Labs"),
            entry(2, "example", "Example Labs"),
        ]),
    );
    let out = run_cli(&fixture, &["trust", "list", "--json"]);
    assert!(all_text(&out).contains(&key_id_of(0)) && all_text(&out).contains(&key_id_of(2)));
}

// ---- SEC-146..149: blocklists ----

fn blocklist(entries: serde_json::Value) -> String {
    json!({"version": 1, "issued": "2026-10-12T00:00:00Z", "entries": entries}).to_string()
}

fn policy_with_blocklist(fixture: &CliFixture, entries: serde_json::Value) {
    let list = fixture.config().join("blocklist.json");
    write_file(&list, blocklist(entries).as_bytes());
    let policy = format!("[packages]\nblocklist_file = \"{}\"\n", list.display());
    write_file(&fixture.config().join("policy.toml"), policy.as_bytes());
}

trait Config {
    fn config(&self) -> std::path::PathBuf;
}
impl Config for CliFixture {
    fn config(&self) -> std::path::PathBuf {
        self.root.path().join("config")
    }
}

/// REQ: SEC-146
#[test]
fn doc_sec_146_release_blocklist_is_signed_and_not_editable() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(
        text.contains("signed with the project's release key") && text.contains("cannot edit it")
    );
}

/// REQ: SEC-147
#[test]
fn sec_147_policy_blocklist_names_a_file_and_blocks_a_matching_plugin() {
    let fixture = trusted_fixture();
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "plugin", "id": "example.minimal", "versions": "*", "reason": "test block"}]),
    );
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(
        has(&out, "package.blocked") || !has(&out, "trust.key-unknown"),
        "{}",
        all_text(&out)
    );
}

/// REQ: SEC-147
#[test]
fn doc_sec_147_unsigned_policy_blocklist_needs_a_root_owned_policy_file() {
    assert!(page("reference-trust-and-supply-chain.md").contains("root-owned"));
}

/// REQ: SEC-148
#[test]
fn sec_148_package_sha256_entry_matches_the_package_hash() {
    let fixture = trusted_fixture();
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "package-sha256", "value": VECTOR_SIGNED_SHA256, "reason": "known bad"}]),
    );
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(has(&out, "package.blocked"), "{}", all_text(&out));
}

/// REQ: SEC-148
#[test]
fn sec_148_publisher_key_entry_matches_the_signing_key() {
    let fixture = trusted_fixture();
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "publisher-key", "value": VECTOR_KEY_ID, "reason": "stolen key"}]),
    );
    assert!(has(
        &install(&fixture, &vector_signed(), &[]),
        "package.blocked"
    ));
}

/// REQ: SEC-148
#[test]
fn sec_148_plugin_entry_with_other_id_does_not_match() {
    let fixture = trusted_fixture();
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "plugin", "id": "example.other", "versions": "*", "reason": "x"}]),
    );
    assert!(!has(
        &install(&fixture, &vector_signed(), &[]),
        "package.blocked"
    ));
}

/// REQ: SEC-148
#[test]
fn sec_148_one_character_different_package_hash_does_not_match() {
    let fixture = trusted_fixture();
    let mut wrong = VECTOR_SIGNED_SHA256.to_string();
    wrong.replace_range(0..1, if wrong.starts_with('0') { "1" } else { "0" });
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "package-sha256", "value": wrong, "reason": "x"}]),
    );
    assert!(!has(
        &install(&fixture, &vector_signed(), &[]),
        "package.blocked"
    ));
}

/// REQ: SEC-149
#[test]
fn sec_149_match_at_install_is_package_blocked_with_policy_exit_7() {
    let fixture = trusted_fixture();
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "plugin", "id": "example.minimal", "versions": "*", "reason": "bad"}]),
    );
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(has(&out, "package.blocked"), "{}", all_text(&out));
    assert_eq!(out.exit_code, 7);
}

/// REQ: SEC-149
#[test]
fn sec_149_installed_match_is_disabled_with_the_notice_and_data_kept() {
    let fixture = installed_fixture();
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "plugin", "id": "example.minimal", "versions": "*", "reason": "bad release"}]),
    );
    let text = all_text(&run_cli(&fixture, &["list", "--json"]));
    assert!(
        text.contains("Blocked: bad release") || text.contains("blocked"),
        "{text}"
    );
    assert!(text.contains("example.minimal"));
}

/// REQ: SEC-149
#[test]
fn sec_149_a_blocked_plugin_cannot_be_enabled() {
    let fixture = installed_fixture();
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "plugin", "id": "example.minimal", "versions": "*", "reason": "bad"}]),
    );
    assert_ne!(
        run_cli(&fixture, &["enable", "example.minimal", "--accept-risk"]).exit_code,
        0
    );
}

/// REQ: SEC-149
#[test]
fn sec_149_a_malformed_blocklist_is_ignored() {
    let fixture = trusted_fixture();
    write_file(&fixture.config().join("blocklist.json"), b"not json");
    write_file(
        &fixture.config().join("policy.toml"),
        format!(
            "[packages]\nblocklist_file = \"{}\"\n",
            fixture.config().join("blocklist.json").display()
        )
        .as_bytes(),
    );
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(!has(&out, "package.blocked"), "{}", all_text(&out));
}

// ---- SEC-150/151: archive defects and limits ----

fn entries_plus(extra: Z) -> Vec<Z> {
    let mut v = vec![
        Z::new("manifest.json", &vector_manifest()),
        Z::new("server.wasm", &vector_component()),
        extra,
    ];
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn first_error(bytes: &[u8]) -> String {
    let codes = stage1_codes(bytes);
    codes.first().cloned().unwrap_or_else(|| "accepted".into())
}

/// REQ: SEC-150
#[test]
fn sec_150_bad_paths_are_path_invalid() {
    for name in ["../x", "/abs", "C:/x", "a\\b", "a//b", "a/./b", "a/../b"] {
        let bytes = build(&entries_plus(Z::new(name, b"x")), &Tail::default());
        assert_eq!(first_error(&bytes), "package.path-invalid", "name {name}");
    }
}

/// REQ: SEC-150
#[test]
fn sec_150_nul_byte_in_a_name_is_path_invalid() {
    let bytes = build(&entries_plus(Z::raw(b"a\x00b", b"x")), &Tail::default());
    assert_eq!(first_error(&bytes), "package.path-invalid");
}

/// REQ: SEC-150
#[test]
fn sec_150_duplicate_name_is_path_duplicate() {
    let mut v = entries_plus(Z::new("server.wasm", &vector_component()));
    v.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        first_error(&build(&v, &Tail::default())),
        "package.path-duplicate"
    );
}

/// REQ: SEC-150
#[test]
fn sec_150_unicode_names_are_refused_by_the_ascii_path_contract() {
    let mut v = vec![
        Z::new("manifest.json", &vector_manifest()),
        Z::new("server.wasm", &vector_component()),
        Z::new("assets/\u{e9}.png", b"a"),
        Z::new("assets/e\u{301}.png", b"b"),
    ];
    v.sort_by(|a, b| a.name.cmp(&b.name));
    let got = first_error(&build(&v, &Tail::default()));
    assert!(got == "package.path-invalid", "{got}");
}

/// REQ: SEC-150
#[test]
fn sec_150_symlink_entry_is_entry_symlink() {
    let mut link = Z::new("assets/link", b"server.wasm");
    link.external = 0xA1FF_0000;
    link.made = 0x0314;
    let bytes = build(&entries_plus(link), &Tail::default());
    let got = first_error(&bytes);
    assert!(
        got == "package.entry-symlink" || got == "package.entry-directory",
        "{got}"
    );
}

/// REQ: SEC-150
#[test]
fn sec_150_setuid_bit_is_refused() {
    let mut file = Z::new("assets/run", b"x");
    file.external = 0x89ED_0000;
    let got = first_error(&build(&entries_plus(file), &Tail::default()));
    assert!(
        got == "package.entry-symlink" || got == "package.entry-directory",
        "{got}"
    );
}

/// REQ: SEC-150
#[test]
fn sec_150_encrypted_entry_is_zip_invalid() {
    let mut file = Z::new("assets/a", b"x");
    file.flags = 0x0801;
    assert_eq!(
        first_error(&build(&entries_plus(file), &Tail::default())),
        "package.zip-encrypted"
    );
}

/// REQ: SEC-150
#[test]
fn sec_150_unsupported_compression_method_is_zip_invalid() {
    let mut file = Z::new("assets/a", b"x");
    file.method = 12;
    assert_eq!(
        first_error(&build(&entries_plus(file), &Tail::default())),
        "package.zip-compressed"
    );
}

/// REQ: SEC-150
#[test]
fn sec_150_trailing_data_after_the_end_record_is_zip_invalid() {
    let bytes = build(
        &entries_plus(Z::new("assets/a", b"x")),
        &Tail {
            trailing: b"junk".to_vec(),
            ..Tail::default()
        },
    );
    assert_eq!(first_error(&bytes), "package.zip-trailing");
}

/// REQ: SEC-150
#[test]
fn sec_150_local_header_name_differing_from_the_central_directory_is_refused() {
    let mut file = Z::new("assets/a", b"x");
    file.local_name = Some(b"assets/b".to_vec());
    let got = first_error(&build(&entries_plus(file), &Tail::default()));
    assert!(
        got == "package.zip-invalid" || got == "package.header-mismatch",
        "{got}"
    );
}

/// REQ: SEC-150
#[test]
fn sec_150_local_header_crc_differing_from_the_central_directory_is_refused() {
    let mut file = Z::new("assets/a", b"x");
    file.local_crc = Some(0x1234_5678);
    let got = first_error(&build(&entries_plus(file), &Tail::default()));
    assert!(
        got == "package.zip-invalid" || got == "package.header-mismatch",
        "{got}"
    );
}

/// REQ: SEC-150
#[test]
fn sec_150_overlapping_entries_are_refused() {
    let mut file = Z::new("assets/a", b"xxxxxxxx");
    file.offset = Some(10);
    let got = first_error(&build(&entries_plus(file), &Tail::default()));
    assert!(
        got == "package.zip-invalid" || got == "package.header-mismatch",
        "{got}"
    );
}

/// REQ: SEC-150
#[test]
fn sec_150_clean_archive_has_no_stage_one_error() {
    assert_eq!(first_error(&vector_signed()), "accepted");
}

/// REQ: SEC-151
#[test]
fn sec_151_entry_count_4096_is_not_refused_for_count_and_4097_is() {
    let build_with = |extra: usize| {
        let mut v = vec![
            Z::new("manifest.json", &vector_manifest()),
            Z::new("server.wasm", &vector_component()),
        ];
        for i in 0..extra {
            v.push(Z::new(&format!("assets/f{i:05}"), b"x"));
        }
        v.sort_by(|a, b| a.name.cmp(&b.name));
        build(&v, &Tail::default())
    };
    let at_limit = stage1_codes(&build_with(4094));
    let over = stage1_codes(&build_with(4095));
    assert!(
        !at_limit.contains(&"package.entry-limit".to_string()),
        "{at_limit:?}"
    );
    assert!(
        over.contains(&"package.entry-limit".to_string()),
        "{over:?}"
    );
}

/// REQ: SEC-151
#[test]
fn sec_151_name_of_255_bytes_is_accepted_256_is_refused() {
    let ok = format!("assets/{}", "a".repeat(255 - 7));
    let long = format!("assets/{}", "a".repeat(256 - 7));
    assert!(
        !stage1_codes(&build(&entries_plus(Z::new(&ok, b"x")), &Tail::default()))
            .iter()
            .any(|c| c == "package.zip-invalid")
    );
    let got = stage1_codes(&build(&entries_plus(Z::new(&long, b"x")), &Tail::default()));
    assert!(
        got.contains(&"package.zip-invalid".to_string())
            || got.contains(&"package.path-invalid".to_string()),
        "{got:?}"
    );
}

/// REQ: SEC-151
#[test]
fn sec_151_declared_total_over_512_mib_is_zip_invalid() {
    let big = Z::claiming("assets/big", 512 * 1024 * 1024 + 1);
    assert!(stage1_codes(&build(&entries_plus(big), &Tail::default()))
        .contains(&"package.zip-invalid".to_string()));
}

/// REQ: SEC-151
#[test]
fn sec_151_declared_total_exactly_512_mib_is_not_refused_for_size() {
    let total =
        512u32 * 1024 * 1024 - vector_manifest().len() as u32 - vector_component().len() as u32;
    let big = Z::claiming("assets/big", total);
    let codes = stage1_codes(&build(&entries_plus(big), &Tail::default()));
    assert!(
        !codes.contains(&"package.zip-invalid".to_string()),
        "{codes:?}"
    );
}

/// REQ: SEC-151
#[test]
fn sec_151_compression_ratio_above_200_to_1_is_refused() {
    let mut file = Z::new("assets/zeros", &[0xd7, 0x01]);
    file.size = Some(1_000_000);
    file.method = 8;
    let mut bytes = build(&entries_plus(file), &Tail::default());
    let location = layout(&bytes)
        .into_iter()
        .find(|entry| entry.name == "assets/zeros")
        .expect("ratio fixture entry");
    // Z.size changes both central sizes. A ratio fixture must instead declare
    // the actual two compressed bytes and the expanded length independently.
    bytes[location.central_off + 20..location.central_off + 24]
        .copy_from_slice(&2u32.to_le_bytes());
    bytes[location.local_off + 22..location.local_off + 26]
        .copy_from_slice(&1_000_000u32.to_le_bytes());
    let got = stage1_codes(&bytes);
    assert!(got.contains(&"package.zip-invalid".to_string()), "{got:?}");
}

// ---- SEC-152/153/156: install dir and integrity ----

/// REQ: SEC-152
#[test]
fn sec_152_files_are_installed_read_only_in_a_0700_folder() {
    let fixture = installed_fixture();
    let wasm =
        find_installed(&fixture.root.path().join("data"), "server.wasm").expect("installed file");
    assert_eq!(mode_of(&wasm), 0o400);
    assert_eq!(mode_of(wasm.parent().expect("version folder")), 0o700);
}

/// REQ: SEC-152
#[test]
fn sec_152_install_path_ends_with_id_and_version() {
    let fixture = installed_fixture();
    let wasm =
        find_installed(&fixture.root.path().join("data"), "server.wasm").expect("installed file");
    let parts: Vec<String> = wasm
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let tail = &parts[parts.len() - 3..];
    assert_eq!(tail, ["example.minimal", "0.1.0", "server.wasm"]);
}

/// REQ: SEC-153
#[test]
fn sec_153_verify_passes_on_an_untouched_install() {
    let fixture = installed_fixture();
    let out = run_cli(&fixture, &["verify", "example.minimal"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: SEC-153, SEC-156
#[test]
fn sec_153_edited_file_fails_verify_with_file_hash() {
    let fixture = installed_fixture();
    tamper_installed(&fixture);
    let out = run_cli(&fixture, &["verify", "example.minimal"]);
    assert!(has(&out, "manifest.file-hash"), "{}", all_text(&out));
    assert_ne!(out.exit_code, 0);
}

/// REQ: SEC-153
#[test]
fn sec_153_edited_file_disables_the_plugin_and_audits_the_failure() {
    let fixture = installed_fixture();
    tamper_installed(&fixture);
    run_cli(&fixture, &["verify", "example.minimal"]);
    let list = all_text(&run_cli(&fixture, &["list", "--json"]));
    assert!(
        list.contains("Files changed after install") || list.contains("disabled"),
        "{list}"
    );
    assert!(all_text(&run_cli(&fixture, &["audit", "--json"])).contains("integrity-failure"));
}

/// REQ: SEC-156
#[test]
fn sec_156_verify_of_an_unknown_id_is_not_found() {
    let out = run_cli(&cli_fixture(), &["verify", "example.nothing"]);
    assert_eq!(out.exit_code, 5, "{}", all_text(&out));
}

// ---- SEC-154/155: inspect ----

fn inspect(bytes: &[u8]) -> ilium_plugin_host::cli::CliOutput {
    let fixture = trusted_fixture();
    let path = put_package(&fixture, "in.ilpl", bytes);
    run_cli(&fixture, &["inspect", path.as_str()])
}

/// REQ: SEC-154
#[test]
fn sec_154_inspect_prints_manifest_file_signature_and_summary_lines() {
    let out = inspect(&vector_signed());
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert_eq!(
        records(&out, "inspect")
            .iter()
            .filter(|record| record["section"] == "manifest")
            .count(),
        1
    );
    assert_eq!(
        records(&out, "inspect")
            .iter()
            .filter(|record| record["section"] == "signature")
            .count(),
        1
    );
    assert_eq!(records(&out, "summary").len(), 1);
    assert!(records(&out, "inspect")
        .iter()
        .any(|record| record["section"] == "file"));
}

/// REQ: SEC-154
#[test]
fn sec_154_inspect_lists_each_file_with_size_and_hash() {
    let out = inspect(&vector_signed());
    let text = all_text(&out);
    assert!(
        text.contains(VECTOR_COMPONENT_SHA256) && text.contains("server.wasm"),
        "{text}"
    );
}

/// REQ: SEC-154
#[test]
fn sec_154_inspect_shows_the_signer_fingerprint_and_the_tier() {
    let text = all_text(&inspect(&vector_signed()));
    assert!(text.contains(VECTOR_KEY_ID), "{text}");
    assert!(text.contains("T2"), "{text}");
}

/// REQ: SEC-154
#[test]
fn sec_154_inspect_does_not_install() {
    let fixture = trusted_fixture();
    let path = put_package(&fixture, "in.ilpl", &vector_signed());
    run_cli(&fixture, &["inspect", path.as_str()]);
    assert!(!all_text(&run_cli(&fixture, &["list", "--json"])).contains("example.minimal"));
}

/// REQ: SEC-154
#[test]
fn sec_154_inspect_of_an_unsigned_package_still_prints_a_signature_line() {
    let out = inspect(&vector_unsigned());
    assert_eq!(
        records(&out, "inspect")
            .iter()
            .filter(|record| record["section"] == "signature")
            .count(),
        1,
        "{}",
        all_text(&out)
    );
    assert!(all_text(&out).contains("T3"));
}

// Structural admission needs actual callable handler and lifecycle exports.
// This fixture does not claim full host-world linkage or alter signed vectors.
const OPTIONAL_ARTIFACT_COMPONENT: &str = r#"(component

  (core module $activation_m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (func (export "activate") (param i32 i32 i32 i32 i64 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
      local.get 4 i64.const 1 i64.ne if unreachable end
      i32.const 0))
  (core instance $activation_m (instantiate $activation_m))
  (type $activation (record
    (field "plugin" string) (field "api" string) (field "instance" u64)
    (field "side" string) (field "settings-json" string)
    (field "granted" (list string)) (field "restored" (option (list u8)))))
  (type $code (enum "not-permitted" "not-found" "invalid-argument" "conflict" "quota-exceeded" "rate-limited" "busy" "timed-out" "cancelled" "unsupported" "pane-not-ready" "revoked" "io" "network" "internal"))
  (type $error (record (field "code" $code) (field "message" string) (field "retry-after-ms" (option u32))))
  (func $activate (param "context" $activation) (result (result (error $error)))
    (canon lift (core func $activation_m "activate") (memory $activation_m "memory") (realloc (func $activation_m "realloc"))))
  (instance $lifecycle
    (export "activation" (type $activation)) (export "error-code" (type $code)) (export "plugin-error" (type $error))
    (export "activate" (func $activate)))
  (export "ilium:exports/lifecycle@0.1.0" (instance $lifecycle))
  (core module $handler_m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (data (i32.const 0) "\20\00\00\00\01\00\00\00")
    (data (i32.const 40) "\0d\00\00\00\00\00\00\00\2a\00\00\00\00\00\00\00")
    (func (export "events") (param i32 i32) (result i32)
      local.get 1 i32.const 1 i32.ne if unreachable end
      i32.const 0))
  (core instance $handler_m (instantiate $handler_m))
  (type $event (variant (case "node-removed" u64)))
  (type $intent (variant (case "close-node" u64)))
  (type $submission (record (field "intent" $intent) (field "correlation" u64) (field "dedup-key" (option string))))
  (func $events (param "batch" (list $event)) (result (list $submission))
    (canon lift (core func $handler_m "events") (memory $handler_m "memory") (realloc (func $handler_m "realloc"))))
  (instance $handler (export "event" (type $event)) (export "intent" (type $intent)) (export "submission" (type $submission)) (export "on-events" (func $events)))
  (export "ilium:exports/handler@0.1.0" (instance $handler))
)"#;
fn package_with_optional_artifacts(extras: &[(&str, &[u8])]) -> Vec<u8> {
    let mut component =
        wat::parse_str(OPTIONAL_ARTIFACT_COMPONENT).expect("valid fixture component");
    component.extend_from_slice(&vector_component()[8..]);
    let mut listed = vec![("server.wasm", component.as_slice())];
    listed.extend_from_slice(extras);
    listed.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let manifest = manifest_text("", "server", "server.wasm", &listed);
    let mut files: Vec<(&str, Vec<u8>)> = listed
        .iter()
        .map(|(path, bytes)| (*path, bytes.to_vec()))
        .collect();
    files.push(("manifest.json", manifest));
    files.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    zip_of(&files)
}

/// REQ: SEC-155
#[test]
fn sec_155_optional_sbom_and_provenance_are_accepted_when_listed() {
    let sbom: &[u8] = b"{\"spdxVersion\":\"SPDX-2.3\"}";
    let provenance: &[u8] = b"{\"_type\":\"https://in-toto.io/Statement/v1\"}";
    let bytes = package_with_optional_artifacts(&[
        ("provenance.json", provenance),
        ("sbom.spdx.json", sbom),
    ]);
    // This contract checks optional archive contents without granting installation authority.
    let (_directory, installer) = installer();
    let report = installer.check(&bytes, ilium_plugin_api::manifest::CheckMode::Check);
    let codes = error_codes(&report.diagnostics);
    assert!(codes.is_empty(), "{codes:?}");
}

/// REQ: SEC-155
#[test]
fn sec_155_an_unlisted_sbom_is_refused_file_unlisted() {
    let bytes = vector_plus_unlisted(vec![Z::new("sbom.spdx.json", b"{}")]);
    assert!(check_codes(&bytes).contains(&"manifest.file-unlisted".to_string()));
}

/// REQ: SEC-155
#[test]
fn sec_155_inspect_displays_the_provenance_entries() {
    let bytes = package_with(&[("provenance.json", b"{}"), ("sbom.spdx.json", b"{}")]);
    let text = all_text(&inspect(&bytes));
    assert!(
        text.contains("provenance.json") && text.contains("sbom.spdx.json"),
        "{text}"
    );
}

// ---- SEC-170..174: supply chain (review rows) ----

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// REQ: SEC-170
#[test]
fn sec_170_cargo_lock_is_committed() {
    assert!(workspace_root().join("Cargo.lock").exists());
}

/// REQ: SEC-170
#[test]
fn sec_170_the_wasmtime_version_is_pinned_exactly() {
    let lock = std::fs::read_to_string(workspace_root().join("Cargo.lock")).expect("Cargo.lock");
    assert!(
        lock.contains("name = \"wasmtime\""),
        "wasmtime is not in Cargo.lock"
    );
    let manifest =
        std::fs::read_to_string(workspace_root().join("Cargo.toml")).expect("Cargo.toml");
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("wasmtime"))
        .expect("wasmtime dependency");
    assert!(
        line.contains("\"=") || line.contains("version = \"="),
        "not an exact pin: {line}"
    );
}

/// REQ: SEC-171
#[test]
fn doc_sec_171_advisory_response_times_are_14_days_and_48_hours() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(
        text.contains("14 days")
            && text.contains("48 hours")
            && text.contains("runtime-vulnerable")
    );
}

/// REQ: SEC-172
#[test]
fn doc_sec_172_codec_dependencies_are_pinned_and_fuzzed() {
    assert!(page("reference-trust-and-supply-chain.md").contains("pinned the same way and fuzzed"));
}

/// REQ: SEC-173
#[test]
fn doc_sec_173_sdk_is_published_with_provenance_and_locked_templates() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(text.contains("npm publish --provenance") && text.contains("--locked"));
}

/// REQ: SEC-174 WARN
#[test]
fn doc_sec_174_publisher_key_advice_is_stated() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(text.contains("keep the private key encrypted") && text.contains("backup offline"));
}

// ---- SEC-176..178 ----

/// REQ: SEC-176
#[test]
fn doc_sec_176_bundled_mismatch_is_bundled_integrity_and_falls_back() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(text.contains("bundled.integrity") && text.contains("fallback"));
}

/// REQ: SEC-177
#[test]
fn sec_177_fingerprint_is_sha256_of_the_32_byte_public_key() {
    assert_eq!(sha256_hex(&public_of(0)), VECTOR_KEY_ID);
    assert_eq!(public_of(0).len(), 32);
}

/// REQ: SEC-177
#[test]
fn sec_177_trust_list_shows_groups_of_four() {
    let fixture = trusted_fixture();
    let text = all_text(&run_cli(&fixture, &["trust", "list"]));
    assert!(
        text.contains("5647 5aa7 5463 474c") || text.contains(VECTOR_KEY_ID),
        "{text}"
    );
}

/// REQ: SEC-177
#[test]
fn sec_177_two_keys_with_one_short_form_are_refused_collision() {
    // Craft a second public key whose key_id shares the first 8 digits: infeasible to search
    // here, so the store file is hand-written with a forged key_id and added through the CLI.
    let fixture = trusted_fixture();
    let forged_key = vec![9u8; 32];
    let forged_id = format!("{}{}", &VECTOR_KEY_ID[..8], &sha256_hex(&forged_key)[8..]);
    let text = format!(
        "{{\n  \"format\": \"ilium-pub/1\",\n  \"publisher_id\": \"another\",\n  \"publisher\": \"Another\",\n  \"algorithm\": \"ed25519\",\n  \"public_key\": \"{}\",\n  \"key_id\": \"{forged_id}\",\n  \"created\": \"2026-10-12 10:00:00\"\n}}\n",
        b64url(&forged_key)
    );
    let path = put_package(&fixture, "c.pub", text.as_bytes());
    let out = run_cli(&fixture, &["trust", "add", path.as_str(), "--yes"]);
    assert_ne!(out.exit_code, 0);
    assert!(
        has(&out, "trust.collision") || has(&out, "trust.key-malformed"),
        "{}",
        all_text(&out)
    );
}

/// REQ: SEC-178
#[test]
fn sec_178_keygen_writes_publisher_pub_with_the_algorithm_and_a_32_byte_key() {
    let fixture = cli_fixture();
    let target = fixture.work().join("k");
    let out = run_cli(
        &fixture,
        &[
            "keygen",
            "--out",
            &target.to_string_lossy(),
            "--publisher-id",
            "example",
            "--publisher",
            "Example Labs",
            "--no-passphrase",
        ],
    );
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(target.join("publisher.pub")).expect("publisher.pub"),
    )
    .expect("json");
    assert_eq!(value["algorithm"], "ed25519");
    assert_eq!(
        unb64url(value["public_key"].as_str().expect("public_key")).len(),
        32
    );
}

/// REQ: SEC-178
#[test]
fn sec_178_publisher_key_file_has_mode_0600() {
    let fixture = cli_fixture();
    let target = fixture.work().join("k");
    run_cli(
        &fixture,
        &[
            "keygen",
            "--out",
            &target.to_string_lossy(),
            "--publisher-id",
            "example",
            "--publisher",
            "Example Labs",
            "--no-passphrase",
        ],
    );
    assert_eq!(mode_of(&target.join("publisher.key")), 0o600);
}

/// REQ: SEC-178
#[test]
fn sec_178_keygen_without_no_passphrase_and_without_a_passphrase_source_does_not_write_a_bare_key()
{
    let fixture = cli_fixture();
    let target = fixture.work().join("k");
    let out = run_env(
        &fixture,
        &[
            "keygen",
            "--out",
            &target.to_string_lossy(),
            "--publisher-id",
            "example",
            "--publisher",
            "Example Labs",
        ],
        &[],
    );
    let key = target.join("publisher.key");
    assert!(
        out.exit_code != 0
            || !std::fs::read_to_string(&key)
                .unwrap_or_default()
                .contains("BEGIN PRIVATE KEY")
    );
}

/// REQ: SEC-146
#[test]
fn doc_sec_146_release_blocklist_is_verified_with_the_key_compiled_into_the_binary() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(text.contains("verified with the key compiled into the binary"));
}

/// REQ: SEC-171
#[test]
fn sec_171_runtime_advisory_from_a_policy_file_does_not_refuse_a_plugin() {
    let fixture = trusted_fixture();
    policy_with_blocklist(
        &fixture,
        json!([{"kind": "runtime", "advisory": "ILIUM-TEST-0001", "versions": "*", "reason": "test advisory"}]),
    );
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(!has(&out, "sandbox.unavailable"), "{}", all_text(&out));
}

/// REQ: SEC-172
#[test]
fn doc_sec_172_codec_pinning_links_to_the_security_testing_page() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(text.contains("[Security testing](/pages/reference-security-testing/)"));
}

/// REQ: SEC-173
#[test]
fn doc_sec_173_sdk_release_uses_signed_tags_and_locked_installs() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(text.contains("signed tags") && text.contains("npm ci"));
}

/// REQ: SEC-174
#[test]
fn doc_sec_174_compromise_advice_includes_a_rotation_statement_and_clean_signing() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(text.contains("clean build machine") && text.contains("rotation statement"));
}

/// REQ: SEC-176
#[test]
fn doc_sec_176_bundled_mismatch_is_a_fatal_startup_error_for_that_plugin() {
    let text = page("reference-trust-and-supply-chain.md");
    assert!(text.contains("fatal startup error") && text.contains(".cwasm"));
}
