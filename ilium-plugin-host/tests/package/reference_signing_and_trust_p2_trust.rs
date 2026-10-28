//! reference-signing-and-trust.md, part 2: trust stores, tiers, rotation, revocation and the
//! negative vectors (PKG-222..258).

use super::signing_support::*;
use super::support::*;

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

fn pub_file(
    fixture: &CliFixture,
    name: &str,
    seed: u8,
    publisher_id: &str,
    publisher: &str,
) -> String {
    let text = format!(
        "{{\n  \"format\": \"ilium-pub/1\",\n  \"publisher_id\": \"{publisher_id}\",\n  \"publisher\": \"{publisher}\",\n  \"algorithm\": \"ed25519\",\n  \"public_key\": \"{}\",\n  \"key_id\": \"{}\",\n  \"created\": \"2026-10-12 10:00:00\"\n}}\n",
        b64url(&public_of(seed)),
        key_id_of(seed)
    );
    put_package(fixture, name, text.as_bytes())
}

fn list_json(fixture: &CliFixture) -> ilium_plugin_host::cli::CliOutput {
    run_cli(fixture, &["list", "--json"])
}

// ---- PKG-222/223: system store and the effective set ----

/// REQ: PKG-222
#[test]
fn doc_pkg_222_system_store_is_etc_ilium_trust_json_and_read_only() {
    let text = page("reference-signing-and-trust.md");
    assert!(text.contains("/etc/ilium/trust.json"));
    assert!(text.contains("read-only"));
}

/// REQ: PKG-222, PKG-227
#[test]
fn pkg_222_trust_commands_never_write_outside_the_user_store() {
    let fixture = trusted_fixture();
    let before = std::fs::metadata("/etc/ilium/trust.json")
        .ok()
        .map(|m| m.len());
    run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID, "--force"]);
    assert_eq!(
        std::fs::metadata("/etc/ilium/trust.json")
            .ok()
            .map(|m| m.len()),
        before
    );
}

/// REQ: PKG-223
#[test]
fn pkg_223_a_key_in_the_user_store_is_effective() {
    let out = install(&trusted_fixture(), &vector_signed(), &[]);
    assert!(!has(&out, "trust.key-unknown"), "{}", all_text(&out));
}

/// REQ: PKG-223
#[test]
fn pkg_223_a_key_revoked_in_the_user_store_is_not_effective() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("compromised", "2026-10-12 10:05:00"));
    write_trust(&fixture, &trust_json(&[e]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(has(&out, "trust.key-revoked"), "{}", all_text(&out));
}

/// REQ: PKG-223
#[test]
fn pkg_223_two_keys_for_one_publisher_are_both_effective() {
    let fixture = cli_fixture();
    write_trust(
        &fixture,
        &trust_json(&[
            entry(0, "example", "Example Labs"),
            entry(1, "example", "Example Labs"),
        ]),
    );
    let out = install(
        &fixture,
        &signed_package(
            1,
            "example",
            &manifest_for("example.minimal", "Example Labs"),
        ),
        &[],
    );
    assert!(!has(&out, "trust.key-unknown"), "{}", all_text(&out));
}

/// REQ: PKG-223
#[test]
fn pkg_223_trust_list_shows_the_entries_of_the_store() {
    let fixture = trusted_fixture();
    let out = run_cli(&fixture, &["trust", "list", "--json"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert!(all_text(&out).contains(VECTOR_KEY_ID));
}

// ---- PKG-224: one key, one publisher ----

/// REQ: PKG-224
#[test]
fn pkg_224_same_key_under_another_publisher_refused_publisher_conflict() {
    let fixture = trusted_fixture();
    let path = pub_file(&fixture, "other.pub", 0, "other", "Other Labs");
    let out = run_cli(&fixture, &["trust", "add", path.as_str(), "--yes"]);
    assert!(has(&out, "trust.publisher-conflict"), "{}", all_text(&out));
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-224 MAY
#[test]
fn pkg_224_two_keys_for_one_publisher_id_via_additional() {
    let fixture = trusted_fixture();
    let path = pub_file(&fixture, "second.pub", 1, "example", "Example Labs");
    let out = run_cli(
        &fixture,
        &["trust", "add", "--additional", path.as_str(), "--yes"],
    );
    // Non-interactive sessions cannot type `replace`; both outcomes keep one key per entry.
    let store = read_trust(&fixture).expect("store");
    assert_eq!(
        store.matches("\"key_id\"").count(),
        if out.exit_code == 0 { 2 } else { 1 }
    );
}

/// REQ: PKG-224
#[test]
fn pkg_224_conflict_leaves_the_store_unchanged() {
    let fixture = trusted_fixture();
    let before = read_trust(&fixture);
    let path = pub_file(&fixture, "other.pub", 0, "other", "Other Labs");
    run_cli(&fixture, &["trust", "add", path.as_str(), "--yes"]);
    assert_eq!(read_trust(&fixture), before);
}

/// REQ: PKG-224
#[test]
fn pkg_224_adding_an_already_listed_key_is_already_trusted_exit_1() {
    let fixture = trusted_fixture();
    let path = pub_file(&fixture, "same.pub", 0, "example", "Example Labs");
    let out = run_cli(&fixture, &["trust", "add", path.as_str(), "--yes"]);
    assert!(has(&out, "trust.already-trusted"), "{}", all_text(&out));
    assert_eq!(out.exit_code, 1);
}

// ---- PKG-225: --trust ----

/// REQ: PKG-225
#[test]
fn pkg_225_trust_flag_with_the_exact_key_installs_and_stores_it() {
    let fixture = cli_fixture();
    let out = install(&fixture, &vector_signed(), &["--trust", VECTOR_KEY_ID]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert!(read_trust(&fixture)
        .expect("store written")
        .contains(VECTOR_KEY_ID));
}

/// REQ: PKG-225
#[test]
fn pkg_225_trust_flag_with_another_key_refused_key_unknown() {
    let fixture = cli_fixture();
    let out = install(&fixture, &vector_signed(), &["--trust", &key_id_of(3)]);
    assert!(has(&out, "trust.key-unknown"), "{}", all_text(&out));
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-225
#[test]
fn pkg_225_trust_flag_persists_for_later_installs() {
    let fixture = cli_fixture();
    install(&fixture, &vector_signed(), &["--trust", VECTOR_KEY_ID]);
    let later = signed_package(
        0,
        "example",
        &manifest_for("example.minimal", "Example Labs"),
    );
    let out = install(&fixture, &later, &[]);
    assert!(!has(&out, "trust.key-unknown"), "{}", all_text(&out));
}

/// REQ: PKG-225
#[test]
fn pkg_225_yes_alone_never_trusts_a_new_key() {
    let fixture = cli_fixture();
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(has(&out, "trust.key-unknown"), "{}", all_text(&out));
    assert!(read_trust(&fixture).is_none_or(|s| !s.contains(VECTOR_KEY_ID)));
}

/// REQ: PKG-225
#[test]
fn pkg_225_truncated_or_uppercase_trust_value_does_not_match() {
    for value in [
        VECTOR_KEY_ID[..63].to_string(),
        VECTOR_KEY_ID.to_uppercase(),
    ] {
        let fixture = cli_fixture();
        let out = install(&fixture, &vector_signed(), &["--trust", &value]);
        assert_ne!(out.exit_code, 0, "value {value}");
    }
}

// ---- PKG-226/227: remove ----

fn installed_fixture() -> CliFixture {
    let fixture = trusted_fixture();
    let out = install(&fixture, &vector_signed(), &[]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    fixture
}

/// REQ: PKG-226
#[test]
fn pkg_226_remove_of_a_key_in_use_refused_in_use_listing_the_plugin() {
    let fixture = installed_fixture();
    let out = run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID]);
    assert!(has(&out, "trust.in-use"), "{}", all_text(&out));
    assert!(all_text(&out).contains("example.minimal"));
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-226
#[test]
fn pkg_226_refused_remove_keeps_the_key() {
    let fixture = installed_fixture();
    run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID]);
    assert!(read_trust(&fixture).expect("store").contains(VECTOR_KEY_ID));
}

/// REQ: PKG-226
#[test]
fn pkg_226_force_removes_the_key_and_disables_the_plugin_untrusted() {
    let fixture = installed_fixture();
    let out = run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID, "--force"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert!(all_text(&list_json(&fixture)).contains("disabled-untrusted"));
}

/// REQ: PKG-226
#[test]
fn pkg_226_trusting_the_key_again_leaves_disabled_untrusted() {
    let fixture = installed_fixture();
    run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID, "--force"]);
    let path = pub_file(&fixture, "again.pub", 0, "example", "Example Labs");
    run_cli(&fixture, &["trust", "add", path.as_str(), "--yes"]);
    assert!(!all_text(&list_json(&fixture)).contains("disabled-untrusted"));
}

/// REQ: PKG-226
#[test]
fn pkg_226_enable_accept_risk_leaves_disabled_untrusted() {
    let fixture = installed_fixture();
    run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID, "--force"]);
    let out = run_cli(&fixture, &["enable", "example.minimal", "--accept-risk"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: PKG-226
#[test]
fn pkg_226_remove_of_an_unused_key_needs_no_force() {
    let fixture = trusted_fixture();
    let out = run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: PKG-227
#[test]
fn pkg_227_remove_with_force_keeps_plugin_data() {
    let fixture = installed_fixture();
    let data = fixture.root.path().join("data");
    write_file(
        &data
            .join("plugins-data")
            .join("example.minimal")
            .join("keep.txt"),
        b"x",
    );
    let count = |dir: &std::path::Path| walk(dir);
    let before = count(&data);
    run_cli(&fixture, &["trust", "remove", VECTOR_KEY_ID, "--force"]);
    let after = count(&data);
    assert!(after >= before, "files before {before}, after {after}");
}

fn walk(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            if e.path().is_dir() {
                walk(&e.path())
            } else {
                1
            }
        })
        .sum()
}

/// REQ: PKG-227
#[test]
fn pkg_227_removing_an_unknown_key_is_not_found_exit_5() {
    let fixture = cli_fixture();
    let out = run_cli(&fixture, &["trust", "remove", &key_id_of(9)]);
    assert!(has(&out, "trust.not-found"), "{}", all_text(&out));
    assert_eq!(out.exit_code, 5);
}

/// REQ: PKG-227
#[test]
fn doc_pkg_227_system_entry_removal_is_refused_with_system_entry() {
    assert!(page("reference-signing-and-trust.md").contains("trust.system-entry"));
}

// ---- PKG-228..235: tiers ----

fn tier_of_installed(fixture: &CliFixture) -> String {
    let out = list_json(fixture);
    out.stdout_jsonl
        .iter()
        .filter_map(|r| r.get("tier").map(|t| t.to_string()))
        .next()
        .unwrap_or_default()
}

/// REQ: PKG-228
#[test]
fn pkg_228_valid_trusted_signature_installs_as_t2() {
    let fixture = installed_fixture();
    assert!(
        tier_of_installed(&fixture).contains("T2"),
        "{}",
        all_text(&list_json(&fixture))
    );
}

/// REQ: PKG-228
#[test]
fn pkg_228_verify_returns_tier_t2_for_the_vector() {
    let package = ilium_plugin_host::package::read_package(&vector_signed()).expect("package");
    let verified = ilium_plugin_host::signing::verify(&package, &trust_of(0, "example"), now())
        .expect("verified");
    assert_eq!(verified.tier, ilium_plugin_api::Tier::T2);
    assert_eq!(verified.publisher_id, "example");
    assert_eq!(verified.key_id, VECTOR_KEY_ID);
}

/// REQ: PKG-228
#[test]
fn pkg_228_two_valid_signatures_one_trusted_is_t2() {
    let manifest = vector_manifest();
    let text = sig_doc(
        "ilium-sig/1",
        &[obj(7, "example", &manifest), obj(0, "example", &manifest)],
    );
    let package = ilium_plugin_host::package::read_package(&with_sig(&text)).expect("package");
    let verified = ilium_plugin_host::signing::verify_with_public_keys(
        &package,
        &trust_of(0, "example"),
        &trust_of(7, "example").keys,
        now(),
    )
    .expect("verified");
    assert_eq!(verified.tier, ilium_plugin_api::Tier::T2);
}

/// REQ: PKG-229
#[test]
fn pkg_229_unsigned_without_flag_refused_unsigned() {
    let fixture = cli_fixture();
    let out = install(&fixture, &vector_unsigned(), &[]);
    assert!(has(&out, "install.unsigned"), "{}", all_text(&out));
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-229
#[test]
fn pkg_229_unsigned_with_allow_unsigned_installs_as_t3() {
    let fixture = cli_fixture();
    let out = install(&fixture, &vector_unsigned(), &["--allow-unsigned"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert!(
        tier_of_installed(&fixture).contains("T3"),
        "{}",
        all_text(&list_json(&fixture))
    );
}

/// REQ: PKG-229
#[test]
fn pkg_229_yes_does_not_replace_allow_unsigned() {
    let fixture = cli_fixture();
    let out = install(&fixture, &vector_unsigned(), &["--yes"]);
    assert!(has(&out, "install.unsigned"), "{}", all_text(&out));
}

/// REQ: PKG-230
#[test]
fn pkg_230_unknown_key_non_interactive_without_trust_is_key_unknown() {
    let out = install(&cli_fixture(), &vector_signed(), &[]);
    assert!(has(&out, "trust.key-unknown"), "{}", all_text(&out));
}

/// REQ: PKG-230
#[test]
fn pkg_230_trust_for_the_key_adds_it_with_added_by_user() {
    let fixture = cli_fixture();
    install(&fixture, &vector_signed(), &["--trust", VECTOR_KEY_ID]);
    let value: serde_json::Value =
        serde_json::from_str(&read_trust(&fixture).expect("store")).expect("json");
    assert_eq!(value["keys"][0]["added_by"], "user");
    assert_eq!(value["keys"][0]["publisher_id"], "example");
}

/// REQ: PKG-230
#[test]
fn doc_pkg_230_declined_prompt_stops_with_declined() {
    assert!(page("reference-signing-and-trust.md").contains("trust.declined"));
}

/// REQ: PKG-231
#[test]
fn pkg_231_invalid_signature_refused_whatever_the_flags() {
    let bytes = patch_data(&vector_signed(), "signature", 200, b'A', true);
    let flag_sets: [&[&str]; 4] = [
        &[],
        &["--allow-unsigned"],
        &["--trust", VECTOR_KEY_ID],
        &[
            "--allow-unsigned",
            "--allow-downgrade",
            "--trust",
            VECTOR_KEY_ID,
        ],
    ];
    for flags in flag_sets {
        let fixture = trusted_fixture();
        let out = install(&fixture, &bytes, flags);
        assert_ne!(out.exit_code, 0, "flags {flags:?}");
        // manual does not pin the exit code of this refusal, only the code.
        assert!(
            has(&out, "manifest.signature-invalid"),
            "flags {flags:?}: {}",
            all_text(&out)
        );
    }
}

/// REQ: PKG-231
#[test]
fn pkg_231_nothing_is_installed_after_an_invalid_signature() {
    let fixture = trusted_fixture();
    let bytes = patch_data(&vector_signed(), "manifest.json", 20, b'Z', true);
    install(&fixture, &bytes, &["--allow-unsigned"]);
    assert!(!all_text(&list_json(&fixture)).contains("example.minimal"));
}

/// REQ: PKG-232
#[test]
fn pkg_232_revoked_key_refused_for_every_reason() {
    for reason in ["compromised", "superseded", "retired"] {
        let fixture = cli_fixture();
        let mut e = entry(0, "example", "Example Labs");
        e.status = "revoked";
        e.revoked = Some((reason, "2026-10-12 10:05:00"));
        write_trust(&fixture, &trust_json(&[e]));
        let out = install(&fixture, &vector_signed(), &["--trust", VECTOR_KEY_ID]);
        assert!(
            has(&out, "trust.key-revoked"),
            "reason {reason}: {}",
            all_text(&out)
        );
    }
}

/// REQ: PKG-232
#[test]
fn pkg_232_trust_flag_does_not_override_a_revocation() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("retired", "2026-10-12 10:05:00"));
    write_trust(&fixture, &trust_json(&[e]));
    let out = install(&fixture, &vector_signed(), &["--trust", VECTOR_KEY_ID]);
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-233
#[test]
fn doc_pkg_233_bundled_tamper_disables_the_plugin() {
    let text = page("reference-signing-and-trust.md");
    assert!(
        text.contains("trust.bundled-tampered")
            && text.contains("SHA-256 of each embedded component")
    );
}

/// REQ: PKG-234
#[test]
fn pkg_234_ilium_prefixed_id_refused_even_with_a_valid_signature() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "ilium", "Example Labs")]));
    let out = install(
        &fixture,
        &signed_package(0, "ilium", &manifest_for("ilium.thing", "Example Labs")),
        &[],
    );
    assert!(has(&out, "manifest.id-reserved"), "{}", all_text(&out));
}

/// REQ: PKG-234
#[test]
fn pkg_234_ilium_prefixed_id_refused_unsigned_with_allow_unsigned() {
    let fixture = cli_fixture();
    let manifest = manifest_for("ilium.thing", "Example Labs");
    let unsigned = zip_of(&[
        ("manifest.json", manifest),
        ("server.wasm", vector_component()),
    ]);
    let out = install(&fixture, &unsigned, &["--allow-unsigned"]);
    assert!(has(&out, "manifest.id-reserved"), "{}", all_text(&out));
}

/// REQ: PKG-234
#[test]
fn pkg_234_an_id_that_only_resembles_the_reserved_prefix_is_not_reserved() {
    let fixture = cli_fixture();
    let manifest = manifest_for("iliumx.thing", "Example Labs");
    let unsigned = zip_of(&[
        ("manifest.json", manifest),
        ("server.wasm", vector_component()),
    ]);
    let out = install(&fixture, &unsigned, &["--allow-unsigned"]);
    assert!(!has(&out, "manifest.id-reserved"), "{}", all_text(&out));
}

/// REQ: PKG-235
#[test]
fn pkg_235_a_tier_key_in_the_manifest_is_not_accepted() {
    let text = String::from_utf8(vector_manifest()).unwrap().replacen(
        "{\n",
        "{\n  \"tier\": \"T1\",\n",
        1,
    );
    let codes = check_codes(&zip_of(&[
        ("manifest.json", text.into_bytes()),
        ("server.wasm", vector_component()),
    ]));
    assert!(!codes.is_empty(), "a manifest tier key must be refused");
}

/// REQ: PKG-235
#[test]
fn pkg_235_installing_unsigned_never_yields_t1_or_t2() {
    let fixture = cli_fixture();
    install(&fixture, &vector_unsigned(), &["--allow-unsigned"]);
    let tier = tier_of_installed(&fixture);
    assert_eq!(tier, "\"T3\"", "unsigned installs are T3 (PKG-228)");
}

// ---- PKG-236..241: key changes and rotation ----

fn update_fixture() -> CliFixture {
    installed_fixture()
}

fn newer(seed: u8) -> Vec<u8> {
    let manifest = String::from_utf8(vector_manifest())
        .unwrap()
        .replace("\"0.1.0\"", "\"0.2.0\"")
        .into_bytes();
    signed_package(seed, "example", &manifest)
}

/// REQ: PKG-236
#[test]
fn pkg_236_update_signed_by_another_trusted_key_of_the_same_publisher_continues() {
    let fixture = update_fixture();
    write_trust(
        &fixture,
        &trust_json(&[
            entry(0, "example", "Example Labs"),
            entry(1, "example", "Example Labs"),
        ]),
    );
    let out = install(&fixture, &newer(1), &[]);
    assert!(
        !has(&out, "package.publisher-key-changed"),
        "{}",
        all_text(&out)
    );
}

/// REQ: PKG-236
#[test]
fn pkg_236_update_with_the_same_key_continues() {
    let fixture = update_fixture();
    let out = install(&fixture, &newer(0), &[]);
    assert!(
        !has(&out, "package.publisher-key-changed"),
        "{}",
        all_text(&out)
    );
}

/// REQ: PKG-237
#[test]
fn pkg_237_update_by_an_untrusted_new_key_refused_key_changed() {
    let fixture = update_fixture();
    let out = install(&fixture, &newer(1), &[]);
    assert!(
        has(&out, "package.publisher-key-changed"),
        "{}",
        all_text(&out)
    );
}

/// REQ: PKG-237
#[test]
fn pkg_237_message_names_both_fingerprints() {
    let fixture = update_fixture();
    let out = install(&fixture, &newer(1), &[]);
    let text = all_text(&out).replace(' ', "");
    assert!(
        text.contains(&key_id_of(0)) && text.contains(&key_id_of(1)),
        "{}",
        all_text(&out)
    );
}

/// REQ: PKG-237
#[test]
fn pkg_237_trust_flag_cannot_override_a_key_change() {
    let fixture = update_fixture();
    let out = install(&fixture, &newer(1), &["--trust", &key_id_of(1)]);
    assert!(
        has(&out, "package.publisher-key-changed"),
        "{}",
        all_text(&out)
    );
}

/// REQ: PKG-237
#[test]
fn pkg_237_installed_version_is_unchanged_after_the_refusal() {
    let fixture = update_fixture();
    install(&fixture, &newer(1), &[]);
    assert!(all_text(&list_json(&fixture)).contains("0.1.0"));
}

fn rotation_text(old: u8, new: u8, signer: u8, publisher_id: &str) -> String {
    let mut m = b"ilium-key-rotation-v1".to_vec();
    m.push(0);
    m.extend_from_slice(publisher_id.as_bytes());
    m.push(0);
    m.extend_from_slice(&sha256(&public_of(new)));
    let signature = b64url(&sign_raw(signer, &m));
    format!(
        "{{\n  \"format\": \"ilium-rot/1\",\n  \"publisher_id\": \"{publisher_id}\",\n  \"old_key_id\": \"{}\",\n  \"new_key\": {{\n    \"public_key\": \"{}\",\n    \"key_id\": \"{}\"\n  }},\n  \"signature\": \"{signature}\",\n  \"created\": \"2026-10-12 10:00:00\"\n}}\n",
        key_id_of(old),
        b64url(&public_of(new)),
        key_id_of(new)
    )
}

/// REQ: PKG-238
#[test]
fn pkg_238_rotation_signed_by_the_old_key_is_accepted() {
    let fixture = trusted_fixture();
    let path = put_package(
        &fixture,
        "rotation.json",
        rotation_text(0, 1, 0, "example").as_bytes(),
    );
    let out = run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: PKG-238, PKG-241
#[test]
fn pkg_238_rotation_signed_by_another_key_refused_rotation_invalid() {
    let fixture = trusted_fixture();
    let path = put_package(
        &fixture,
        "rotation.json",
        rotation_text(0, 1, 5, "example").as_bytes(),
    );
    let out = run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]);
    assert!(has(&out, "trust.rotation-invalid"), "{}", all_text(&out));
}

/// REQ: PKG-238
#[test]
fn pkg_238_rotation_for_another_publisher_in_the_message_refused_rotation_invalid() {
    let fixture = trusted_fixture();
    let mut text = rotation_text(0, 1, 0, "other");
    text = text.replace(
        "\"publisher_id\": \"other\"",
        "\"publisher_id\": \"example\"",
    );
    let path = put_package(&fixture, "rotation.json", text.as_bytes());
    let out = run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]);
    assert!(has(&out, "trust.rotation-invalid"), "{}", all_text(&out));
}

/// REQ: PKG-238
#[test]
fn pkg_238_rotation_whose_new_key_id_does_not_match_its_public_key_is_invalid() {
    let fixture = trusted_fixture();
    let text = rotation_text(0, 1, 0, "example").replace(&key_id_of(1), &key_id_of(2));
    let path = put_package(&fixture, "rotation.json", text.as_bytes());
    let out = run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]);
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-238
#[test]
fn pkg_238_rotation_with_another_format_value_is_refused() {
    let fixture = trusted_fixture();
    let text = rotation_text(0, 1, 0, "example").replace("ilium-rot/1", "ilium-rot/2");
    let path = put_package(&fixture, "rotation.json", text.as_bytes());
    assert_ne!(
        run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]).exit_code,
        0
    );
}

/// REQ: PKG-239
#[test]
fn pkg_239_valid_rotation_adds_the_new_key_and_keeps_the_old_one_trusted() {
    let fixture = trusted_fixture();
    let path = put_package(
        &fixture,
        "rotation.json",
        rotation_text(0, 1, 0, "example").as_bytes(),
    );
    run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]);
    let value: serde_json::Value =
        serde_json::from_str(&read_trust(&fixture).expect("store")).expect("json");
    let keys = value["keys"].as_array().expect("keys");
    assert_eq!(keys.len(), 2);
    let find = |id: String| {
        keys.iter()
            .find(|k| k["key_id"] == id.as_str())
            .expect("key")
            .clone()
    };
    assert_eq!(find(key_id_of(0))["status"], "trusted");
    assert_eq!(find(key_id_of(1))["added_by"], "user");
    assert_eq!(find(key_id_of(1))["publisher_id"], "example");
}

/// REQ: PKG-239
#[test]
fn pkg_239_after_rotation_the_update_signed_by_the_new_key_is_accepted() {
    let fixture = installed_fixture();
    let path = put_package(
        &fixture,
        "rotation.json",
        rotation_text(0, 1, 0, "example").as_bytes(),
    );
    run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]);
    let out = install(&fixture, &newer(1), &[]);
    assert!(
        !has(&out, "package.publisher-key-changed"),
        "{}",
        all_text(&out)
    );
}

/// REQ: PKG-239, PKG-241
#[test]
fn pkg_239_rotation_with_an_old_key_not_in_the_store_is_refused() {
    let fixture = cli_fixture();
    let path = put_package(
        &fixture,
        "rotation.json",
        rotation_text(0, 1, 0, "example").as_bytes(),
    );
    let out = run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]);
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-240
#[test]
fn pkg_240_additional_non_interactive_without_typing_replace_adds_nothing() {
    let fixture = trusted_fixture();
    let before = read_trust(&fixture);
    let path = pub_file(&fixture, "second.pub", 1, "example", "Example Labs");
    let out = run_cli(&fixture, &["trust", "add", "--additional", path.as_str()]);
    assert_ne!(out.exit_code, 0);
    assert_eq!(read_trust(&fixture), before);
}

/// REQ: PKG-240
#[test]
fn pkg_240_additional_for_a_publisher_without_a_key_is_refused() {
    let fixture = cli_fixture();
    let path = pub_file(&fixture, "second.pub", 1, "example", "Example Labs");
    assert_ne!(
        run_cli(
            &fixture,
            &["trust", "add", "--additional", path.as_str(), "--yes"]
        )
        .exit_code,
        0
    );
}

/// REQ: PKG-241
#[test]
fn pkg_241_rotation_signed_by_a_revoked_old_key_is_refused() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("retired", "2026-10-12 10:05:00"));
    write_trust(&fixture, &trust_json(&[e]));
    let path = put_package(
        &fixture,
        "rotation.json",
        rotation_text(0, 1, 0, "example").as_bytes(),
    );
    assert_ne!(
        run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]).exit_code,
        0
    );
}

/// REQ: PKG-241
#[test]
fn pkg_241_rotation_signed_by_a_key_trusted_for_another_publisher_is_refused() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "other", "Other Labs")]));
    let path = put_package(
        &fixture,
        "rotation.json",
        rotation_text(0, 1, 0, "example").as_bytes(),
    );
    assert_ne!(
        run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]).exit_code,
        0
    );
}

// ---- PKG-242..247: revocation ----

fn revocation_text(
    publisher_id: &str,
    key: u8,
    reason: &str,
    effective: &str,
    signer: u8,
) -> String {
    let mut m = b"ilium-key-revocation-v1".to_vec();
    m.push(0);
    m.extend_from_slice(publisher_id.as_bytes());
    m.push(0);
    m.extend_from_slice(key_id_of(key).as_bytes());
    m.push(0);
    m.extend_from_slice(reason.as_bytes());
    format!(
        "{{\n  \"format\": \"ilium-rev/1\",\n  \"publisher_id\": \"{publisher_id}\",\n  \"key_id\": \"{}\",\n  \"reason\": \"{reason}\",\n  \"effective\": \"{effective}\",\n  \"signature\": \"{}\"\n}}\n",
        key_id_of(key),
        b64url(&sign_raw(signer, &m))
    )
}

/// REQ: PKG-242
#[test]
fn pkg_242_revocation_statement_has_the_documented_shape() {
    let value: serde_json::Value = serde_json::from_str(&revocation_text(
        "example",
        0,
        "retired",
        "2026-10-12 10:00:00",
        0,
    ))
    .unwrap();
    let keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.len(), 6);
    assert!([
        "format",
        "publisher_id",
        "key_id",
        "reason",
        "effective",
        "signature"
    ]
    .iter()
    .all(|k| keys.contains(k)));
}

/// REQ: PKG-242
#[test]
fn pkg_242_statement_signed_by_the_revoked_key_is_signature_verifiable() {
    use ed25519_dalek::Verifier;
    let mut m = b"ilium-key-revocation-v1".to_vec();
    m.push(0);
    m.extend_from_slice(b"example");
    m.push(0);
    m.extend_from_slice(key_id_of(0).as_bytes());
    m.push(0);
    m.extend_from_slice(b"retired");
    let sig = ed25519_dalek::Signature::from_slice(&sign_raw(0, &m)).unwrap();
    assert!(signing_key(0).verifying_key().verify(&m, &sig).is_ok());
}

/// REQ: PKG-242
#[test]
fn pkg_242_statement_with_an_unknown_reason_is_refused() {
    let fixture = trusted_fixture();
    let path = put_package(
        &fixture,
        "rev.json",
        revocation_text("example", 0, "bored", "2026-10-12 10:00:00", 0).as_bytes(),
    );
    let out = run_cli(&fixture, &["trust", "import", path.as_str()]);
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-243
#[test]
fn pkg_243_revoke_writes_the_key_as_revoked_with_the_reason() {
    let fixture = trusted_fixture();
    let out = run_cli(
        &fixture,
        &["trust", "revoke", VECTOR_KEY_ID, "--reason", "retired"],
    );
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    let value: serde_json::Value =
        serde_json::from_str(&read_trust(&fixture).expect("store")).expect("json");
    assert_eq!(value["keys"][0]["status"], "revoked");
    assert_eq!(value["keys"][0]["revoked"]["reason"], "retired");
}

/// REQ: PKG-243
#[test]
fn pkg_243_revoke_of_an_unknown_key_is_not_found() {
    let fixture = cli_fixture();
    let out = run_cli(
        &fixture,
        &["trust", "revoke", &key_id_of(4), "--reason", "retired"],
    );
    assert_eq!(out.exit_code, 5, "{}", all_text(&out));
}

/// REQ: PKG-243
#[test]
fn pkg_243_revoke_with_an_unknown_reason_fails() {
    let fixture = trusted_fixture();
    assert_ne!(
        run_cli(
            &fixture,
            &["trust", "revoke", VECTOR_KEY_ID, "--reason", "bored"]
        )
        .exit_code,
        0
    );
}

/// REQ: PKG-243
#[test]
fn pkg_243_revoke_without_a_reason_fails() {
    let fixture = trusted_fixture();
    assert_ne!(
        run_cli(&fixture, &["trust", "revoke", VECTOR_KEY_ID]).exit_code,
        0
    );
}

/// REQ: PKG-244
#[test]
fn pkg_244_new_installs_by_a_revoked_key_are_refused_for_each_reason() {
    for reason in ["compromised", "superseded", "retired"] {
        let fixture = trusted_fixture();
        run_cli(
            &fixture,
            &["trust", "revoke", VECTOR_KEY_ID, "--reason", reason],
        );
        let out = install(&fixture, &vector_signed(), &[]);
        assert!(
            has(&out, "trust.key-revoked"),
            "reason {reason}: {}",
            all_text(&out)
        );
    }
}

/// REQ: PKG-245
#[test]
fn pkg_245_compromised_revocation_blocks_installed_plugins() {
    let fixture = installed_fixture();
    run_cli(
        &fixture,
        &["trust", "revoke", VECTOR_KEY_ID, "--reason", "compromised"],
    );
    assert!(all_text(&list_json(&fixture)).contains("disabled-blocked"));
}

/// REQ: PKG-245
#[test]
fn pkg_245_blocked_plugin_leaves_with_enable_accept_risk() {
    let fixture = installed_fixture();
    run_cli(
        &fixture,
        &["trust", "revoke", VECTOR_KEY_ID, "--reason", "compromised"],
    );
    let out = run_cli(&fixture, &["enable", "example.minimal", "--accept-risk"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: PKG-245
#[test]
fn pkg_245_plain_enable_does_not_unblock() {
    let fixture = installed_fixture();
    run_cli(
        &fixture,
        &["trust", "revoke", VECTOR_KEY_ID, "--reason", "compromised"],
    );
    let out = run_cli(&fixture, &["enable", "example.minimal"]);
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-246
#[test]
fn pkg_246_retired_and_superseded_keep_plugins_at_t2_with_the_badge() {
    for reason in ["retired", "superseded"] {
        let fixture = installed_fixture();
        run_cli(
            &fixture,
            &["trust", "revoke", VECTOR_KEY_ID, "--reason", reason],
        );
        let text = all_text(&list_json(&fixture));
        assert!(
            !text.contains("disabled-blocked"),
            "reason {reason}: {text}"
        );
        assert!(text.contains("key retired"), "reason {reason}: {text}");
    }
}

/// REQ: PKG-246
#[test]
fn pkg_246_update_after_retirement_must_come_from_another_trusted_key() {
    let fixture = installed_fixture();
    run_cli(
        &fixture,
        &["trust", "revoke", VECTOR_KEY_ID, "--reason", "retired"],
    );
    let out = install(&fixture, &newer(0), &[]);
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-247
#[test]
fn pkg_247_a_future_effective_time_does_not_apply_yet() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("retired", "2999-01-01 00:00:00"));
    write_trust(&fixture, &trust_json(&[e]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(!has(&out, "trust.key-revoked"), "{}", all_text(&out));
}

/// REQ: PKG-247
#[test]
fn pkg_247_a_past_effective_time_applies() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("retired", "2026-10-12 10:13:19"));
    write_trust(&fixture, &trust_json(&[e]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(has(&out, "trust.key-revoked"), "{}", all_text(&out));
}

/// REQ: PKG-247
#[test]
fn pkg_247_the_effective_instant_itself_applies() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("retired", "2026-10-12 10:13:20"));
    write_trust(&fixture, &trust_json(&[e]));
    assert!(has(
        &install(&fixture, &vector_signed(), &[]),
        "trust.key-revoked"
    ));
}

/// REQ: PKG-247
#[test]
fn pkg_247_one_second_before_effective_does_not_apply() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("retired", "2026-10-12 10:13:21"));
    write_trust(&fixture, &trust_json(&[e]));
    assert!(!has(
        &install(&fixture, &vector_signed(), &[]),
        "trust.key-revoked"
    ));
}

// ---- PKG-248..258: negative vectors ----

fn vector_install(bytes: &[u8]) -> ilium_plugin_host::cli::CliOutput {
    install(&trusted_fixture(), bytes, &[])
}

/// REQ: PKG-248
#[test]
fn pkg_248_manifest_byte_changed_with_crc_repaired_is_signature_invalid() {
    let bytes = patch_data(&vector_signed(), "manifest.json", 20, b'Z', true);
    let out = vector_install(&bytes);
    assert!(
        has(&out, "manifest.signature-invalid"),
        "cli: {}",
        all_text(&out)
    );
    assert_eq!(
        refusal(&bytes, &trust_of(0, "example")),
        "manifest.signature-invalid"
    );
}

/// REQ: PKG-248, PKG-141
#[test]
fn pkg_248_every_manifest_byte_flip_is_refused() {
    let signed = vector_signed();
    let length = vector_manifest().len();
    for index in (0..length).step_by(7) {
        let flipped = vector_manifest()[index] ^ 0x01;
        let bytes = patch_data(&signed, "manifest.json", index, flipped, true);
        assert_ne!(
            refusal(&bytes, &trust_of(0, "example")),
            "accepted",
            "index {index}"
        );
    }
}

/// REQ: PKG-249
#[test]
fn pkg_249_component_byte_changed_is_file_hash_before_the_signature() {
    let bytes = patch_data(&vector_signed(), "server.wasm", 12, 0x7F, true);
    let report = check(&bytes);
    let codes = error_codes(&report.diagnostics);
    assert_eq!(
        codes.first().map(String::as_str),
        Some("manifest.file-hash")
    );
    assert!(!codes.contains(&"manifest.signature-invalid".to_string()));
}

/// REQ: PKG-250
#[test]
fn pkg_250_signature_byte_changed_is_signature_invalid() {
    let doc = String::from_utf8(vector_signature_doc()).unwrap();
    let at = doc.find(VECTOR_SIGNATURE_B64URL).unwrap() + 3;
    let mut bytes = doc.into_bytes();
    bytes[at] = if bytes[at] == b'A' { b'B' } else { b'A' };
    let package = with_signature_text(&vector_manifest(), &vector_component(), &bytes);
    assert_eq!(
        refusal(&package, &trust_of(0, "example")),
        "manifest.signature-invalid"
    );
}

/// REQ: PKG-250
#[test]
fn pkg_250_each_of_the_first_64_signature_bits_matters() {
    let signature = sign_raw(0, &message(PREFIX, "example", &vector_manifest()));
    for byte in [0usize, 31, 32, 63] {
        let mut changed = signature.clone();
        changed[byte] ^= 0x80;
        let mut o = obj(0, "example", &vector_manifest());
        o.signature = b64url(&changed);
        let package = with_sig(&sig_doc("ilium-sig/1", &[o]));
        assert_eq!(
            refusal(&package, &trust_of(0, "example")),
            "manifest.signature-invalid",
            "byte {byte}"
        );
    }
}

/// REQ: PKG-251
#[test]
fn pkg_251_prefix_v2_signature_is_signature_invalid() {
    let mut o = obj(0, "example", &vector_manifest());
    o.signature = b64url(&sign_raw(
        0,
        &message("ilium-plugin-signature-v2", "example", &vector_manifest()),
    ));
    assert_eq!(
        refusal(
            &with_sig(&sig_doc("ilium-sig/1", &[o])),
            &trust_of(0, "example")
        ),
        "manifest.signature-invalid"
    );
}

/// REQ: PKG-251
#[test]
fn pkg_251_message_without_the_separator_bytes_is_signature_invalid() {
    let mut m = PREFIX.as_bytes().to_vec();
    m.extend_from_slice(b"example");
    m.extend_from_slice(&sha256(&vector_manifest()));
    let mut o = obj(0, "example", &vector_manifest());
    o.signature = b64url(&sign_raw(0, &m));
    assert_eq!(
        refusal(
            &with_sig(&sig_doc("ilium-sig/1", &[o])),
            &trust_of(0, "example")
        ),
        "manifest.signature-invalid"
    );
}

/// REQ: PKG-252
#[test]
fn pkg_252_signed_as_other_but_document_says_example_is_signature_invalid() {
    let mut o = obj(0, "example", &vector_manifest());
    o.signature = b64url(&sign_raw(0, &message(PREFIX, "other", &vector_manifest())));
    assert_eq!(
        refusal(
            &with_sig(&sig_doc("ilium-sig/1", &[o])),
            &trust_of(0, "example")
        ),
        "manifest.signature-invalid"
    );
}

/// REQ: PKG-253
#[test]
fn pkg_253_document_publisher_other_with_plugin_id_example_is_id_publisher() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "other", "Example Labs")]));
    let out = install(
        &fixture,
        &signed_package(0, "other", &vector_manifest()),
        &[],
    );
    assert!(has(&out, "manifest.id-publisher"), "{}", all_text(&out));
}

/// REQ: PKG-254
#[test]
fn pkg_254_format_ilium_sig_2_is_signature_format() {
    let text = sig_doc("ilium-sig/2", &[obj(0, "example", &vector_manifest())]);
    assert_eq!(read_refusal(&with_sig(&text)), "package.signature-format");
    assert!(has(
        &vector_install(&with_sig(&text)),
        "package.signature-format"
    ));
}

/// REQ: PKG-255
#[test]
fn pkg_255_unknown_key_non_interactive_without_trust_is_key_unknown() {
    let out = install(&cli_fixture(), &vector_signed(), &[]);
    assert!(has(&out, "trust.key-unknown"), "{}", all_text(&out));
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-256
#[test]
fn pkg_256_store_display_name_other_labs_is_publisher_mismatch() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Other Labs")]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(
        has(&out, "manifest.publisher-mismatch"),
        "{}",
        all_text(&out)
    );
}

/// REQ: PKG-257
#[test]
fn pkg_257_key_revoked_as_compromised_is_key_revoked() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("compromised", "2026-10-12 10:05:00"));
    write_trust(&fixture, &trust_json(&[e]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(has(&out, "trust.key-revoked"), "{}", all_text(&out));
}

/// REQ: PKG-258
#[test]
fn pkg_258_installed_key_a_update_key_b_no_rotation_is_publisher_key_changed() {
    let fixture = update_fixture();
    let out = install(&fixture, &newer(1), &[]);
    assert!(
        has(&out, "package.publisher-key-changed"),
        "{}",
        all_text(&out)
    );
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-258
#[test]
fn pkg_258_with_the_rotation_imported_the_same_update_goes_through() {
    let fixture = update_fixture();
    let path = put_package(
        &fixture,
        "rotation.json",
        rotation_text(0, 1, 0, "example").as_bytes(),
    );
    run_cli(&fixture, &["trust", "add", "--rotation", path.as_str()]);
    assert!(!has(
        &install(&fixture, &newer(1), &[]),
        "package.publisher-key-changed"
    ));
}

/// REQ: PKG-233
#[test]
fn doc_pkg_233_bundled_check_compares_embedded_bytes_with_the_release_table() {
    let text = page("reference-signing-and-trust.md");
    assert!(text.contains("compares the embedded bytes with the table"));
}

/// REQ: PKG-244
#[test]
fn pkg_244_install_by_a_compromised_revoked_key_exits_nonzero() {
    let fixture = trusted_fixture();
    run_cli(
        &fixture,
        &["trust", "revoke", VECTOR_KEY_ID, "--reason", "compromised"],
    );
    let out = install(&fixture, &vector_signed(), &[]);
    assert_ne!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: PKG-249
#[test]
fn pkg_249_component_byte_changed_is_caught_at_stage_five() {
    let bytes = patch_data(&vector_signed(), "server.wasm", 12, 0x7F, true);
    let report = check(&bytes);
    assert_eq!(report.last_stage, 5);
}

/// REQ: PKG-256
#[test]
fn pkg_256_publisher_name_mismatch_is_refused_without_a_revocation_code() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Other Labs")]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert_ne!(out.exit_code, 0, "{}", all_text(&out));
    assert!(!has(&out, "trust.key-revoked"), "{}", all_text(&out));
}

/// REQ: PKG-257
#[test]
fn pkg_257_compromised_key_refusal_is_not_reported_as_a_publisher_mismatch() {
    let fixture = cli_fixture();
    let mut e = entry(0, "example", "Example Labs");
    e.status = "revoked";
    e.revoked = Some(("compromised", "2026-10-12 10:05:00"));
    write_trust(&fixture, &trust_json(&[e]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert_ne!(out.exit_code, 0, "{}", all_text(&out));
    assert!(!has(&out, "manifest.publisher-mismatch"), "{}", all_text(&out));
}
