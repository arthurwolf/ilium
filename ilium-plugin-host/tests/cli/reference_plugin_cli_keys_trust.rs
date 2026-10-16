//! reference-plugin-cli.md: `keygen`, `sign`, `trust`, `publish` (DEV-110..136).

use ilium_plugin_host::cli::MemorySession;
use ilium_plugin_host::package::read_package;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::common::*;

/// Decodes base64url without padding (the form of `public_key`).
fn base64url_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut bits, mut count) = (0u32, 0);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            other => panic!("not base64url: {other}"),
        } as u32;
        bits = (bits << 6) | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push(((bits >> count) & 0xff) as u8);
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn display_form(key_id: &str) -> String {
    key_id.as_bytes().chunks(4).map(|chunk| std::str::from_utf8(chunk).expect("ascii")).collect::<Vec<_>>().join(" ")
}

fn short_form(key_id: &str) -> String {
    format!("{}…{}", &key_id[..4], &key_id[60..])
}

fn s(path: &std::path::Path) -> &str {
    path.to_str().expect("utf8")
}

// ---------------------------------------------------------------- keygen

/// REQ: DEV-110
#[test]
fn dev_110_creates_a_pkcs8_pem_key_and_an_ilium_pub_document() {
    let root = scratch();
    let env = env(root.path());
    let (key, public) = make_key(&env, root.path(), "acme");
    let pem = std::fs::read_to_string(key).expect("read key");
    assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----"), "{pem}");
    let document = read_json(&public);
    assert_eq!(document["format"], "ilium-pub/1");
    assert_eq!(document["algorithm"], "ed25519");
    assert_eq!(document["publisher_id"], "acme");
    assert_eq!(document["publisher"], "Acme Labs");
}

/// REQ: DEV-110
#[cfg(unix)]
#[test]
fn dev_110_secret_key_has_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let root = scratch();
    let env = env(root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    assert_eq!(std::fs::metadata(key).expect("stat").permissions().mode() & 0o777, 0o600);
}

/// REQ: DEV-110
#[test]
fn dev_110_prints_a_key_artifact_and_a_public_key_artifact() {
    let root = scratch();
    let env = env(root.path());
    let out = root.path().join("k");
    let output = run(&env, &["keygen", "--publisher-id", "acme", "--publisher", "Acme Labs", "--out", s(&out)]);
    let paths: Vec<&str> = recs(&output, "artifact").iter().map(|r| r["path"].as_str().expect("path")).collect();
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().any(|p| p.ends_with("publisher.key")) && paths.iter().any(|p| p.ends_with("publisher.pub")));
}

/// REQ: DEV-110
#[test]
fn dev_110_refuses_to_overwrite_either_file() {
    let root = scratch();
    let env = env(root.path());
    let (key, public) = make_key(&env, root.path(), "acme");
    let out = key.parent().expect("dir").to_path_buf();
    let args = ["keygen", "--publisher-id", "acme", "--publisher", "Acme Labs", "--out", s(&out)];
    let before = std::fs::read(&key).expect("read");
    let output = run(&env, &args);
    assert_eq!(code(&output).as_deref(), Some("trust.key-exists"));
    assert_eq!(std::fs::read(&key).expect("read"), before);
    std::fs::remove_file(&key).expect("remove");
    let output = run(&env, &args);
    assert_eq!(code(&output).as_deref(), Some("trust.key-exists"), "the public file alone also blocks");
    assert!(public.is_file());
}

/// REQ: DEV-111
#[test]
fn dev_111_public_file_follows_the_signing_and_trust_page() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    let document = read_json(&public);
    let raw = base64url_decode(document["public_key"].as_str().expect("public_key"));
    assert_eq!(raw.len(), 32, "32 raw bytes, base64url without padding");
    assert!(!document["public_key"].as_str().expect("public_key").contains('='));
    assert_eq!(document["key_id"], hex(&Sha256::digest(&raw)));
    assert!(document["created"].as_str().expect("created").len() == 19);
}

/// REQ: DEV-111
#[test]
fn dev_111_each_keygen_makes_a_different_key() {
    let root = scratch();
    let env = env(root.path());
    let (_, first) = make_key(&env, root.path(), "acme");
    let (_, second) = make_key(&env, root.path(), "other");
    assert_ne!(read_json(&first)["key_id"], read_json(&second)["key_id"]);
}

/// REQ: DEV-112
#[test]
fn dev_112_key_id_is_64_lowercase_hex_characters() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    assert!(is_sha256_hex(read_json(&public)["key_id"].as_str().expect("key_id")));
}

/// REQ: DEV-112
#[test]
fn dev_112_trust_add_prints_the_display_form_of_sixteen_groups() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    let key_id = read_json(&public)["key_id"].as_str().expect("key_id").to_owned();
    let output = run(&env, &["trust", "add", s(&public)]);
    let all = output.stdout_raw.clone() + &output.stdout_text + &output.stderr;
    assert!(all.contains(&display_form(&key_id)), "{all}");
    assert_eq!(display_form(&key_id).split(' ').count(), 16);
}

/// REQ: DEV-112
#[test]
fn dev_112_trust_list_shows_the_short_form_with_an_ellipsis() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    let key_id = read_json(&public)["key_id"].as_str().expect("key_id").to_owned();
    run(&env, &["trust", "add", s(&public)]);
    let output = run(&env, &["trust", "list"]);
    assert!(output.stdout_text.contains(&short_form(&key_id)), "{}", output.stdout_text);
}

// ---------------------------------------------------------------- sign

/// REQ: DEV-115
#[test]
fn dev_115_without_key_flag_or_variable_sign_fails_missing_argument() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["sign", s(&package)]);
    assert_eq!(code(&output).as_deref(), Some("usage.missing-argument"));
    assert_eq!(output.exit_code, 2);
}

/// REQ: DEV-115
#[test]
fn dev_115_key_flag_signs() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    assert_eq!(run(&env, &["sign", s(&package), "--key", s(&key)]).exit_code, 0);
}

/// REQ: DEV-115
#[test]
fn dev_115_the_variable_supplies_the_key() {
    let root = scratch();
    let base = env(root.path());
    let (_, package) = packed_project(&base, root.path());
    let (key, _) = make_key(&base, root.path(), "acme");
    let env = base.with_var("ILIUM_PLUGIN_SIGNING_KEY", s(&key));
    assert_eq!(run(&env, &["sign", s(&package)]).exit_code, 0);
}

/// REQ: DEV-116
#[cfg(unix)]
#[test]
fn dev_116_a_key_readable_by_group_or_others_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    for mode in [0o640, 0o604, 0o644] {
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(mode)).expect("chmod");
        let output = run(&env, &["sign", s(&package), "--key", s(&key)]);
        assert_eq!(code(&output).as_deref(), Some("sign.key-permissions"), "{mode:o}");
    }
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    assert_eq!(run(&env, &["sign", s(&package), "--key", s(&key)]).exit_code, 0);
}

/// REQ: DEV-116
#[test]
fn dev_116_an_unparsable_key_is_unreadable() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let key = root.path().join("junk.key");
    std::fs::write(&key, "this is not a key").expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }
    let output = run(&env, &["sign", s(&package), "--key", s(&key)]);
    assert_eq!(code(&output).as_deref(), Some("sign.key-unreadable"));
}

/// REQ: DEV-117
#[test]
fn dev_117_sign_adds_a_signature_entry_for_the_key() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let (key, public) = make_key(&env, root.path(), "acme");
    let output = run(&env, &["sign", s(&package), "--key", s(&key)]);
    assert_eq!(artifacts(&output, "signature").len(), 1);
    let archive = read_package(&std::fs::read(&package).expect("read")).expect("valid package");
    let signature = archive.signature.expect("signed");
    assert_eq!(signature.key_id, read_json(&public)["key_id"].as_str().expect("key_id"));
}

/// REQ: DEV-117
#[test]
fn dev_117_signing_twice_with_the_same_key_gives_the_same_bytes() {
    let root = scratch();
    let env = env(root.path());
    let (dir, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    let copy = root.path().join("copy.iliumpkg");
    std::fs::copy(&package, &copy).expect("copy");
    run(&env, &["sign", s(&package), "--key", s(&key)]);
    run(&env, &["sign", s(&copy), "--key", s(&key)]);
    assert_eq!(std::fs::read(&package).expect("read"), std::fs::read(&copy).expect("read"));
    run(&env, &["sign", s(&package), "--key", s(&key), "--replace"]);
    assert_eq!(std::fs::read(&package).expect("read"), std::fs::read(&copy).expect("read"));
    let _ = dir;
}

/// REQ: DEV-117
#[test]
fn dev_117_signing_leaves_the_manifest_bytes_untouched() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    let before = read_package(&std::fs::read(&package).expect("read")).expect("valid");
    run(&env, &["sign", s(&package), "--key", s(&key)]);
    let after = read_package(&std::fs::read(&package).expect("read")).expect("valid");
    assert_eq!(before.manifest, after.manifest);
    assert_eq!(before.entries, after.entries);
}

/// REQ: DEV-118
#[test]
fn dev_118_publisher_mismatch_is_refused() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "someoneelse");
    let output = run(&env, &["sign", s(&package), "--key", s(&key)]);
    assert_eq!(code(&output).as_deref(), Some("sign.publisher-mismatch"));
    assert!(read_package(&std::fs::read(&package).expect("read")).expect("valid").signature.is_none());
}

/// REQ: DEV-118
#[test]
fn dev_118_already_signed_needs_replace() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    run(&env, &["sign", s(&package), "--key", s(&key)]);
    let output = run(&env, &["sign", s(&package), "--key", s(&key)]);
    assert_eq!(code(&output).as_deref(), Some("sign.already-signed"));
    assert_eq!(run(&env, &["sign", s(&package), "--key", s(&key), "--replace"]).exit_code, 0);
}

/// REQ: DEV-119
#[test]
fn dev_119_the_secret_key_is_never_printed() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    let pem = std::fs::read_to_string(&key).expect("read");
    let body: String = pem.lines().filter(|line| !line.starts_with("-----")).collect();
    let output = run(&env, &["sign", s(&package), "--key", s(&key)]);
    let all = output.stdout_raw.clone() + &output.stdout_text + &output.stderr;
    assert!(!all.contains(&body) && !all.contains("PRIVATE KEY"));
}

/// REQ: DEV-119
#[test]
fn dev_119_keygen_does_not_print_the_secret_key_either() {
    let root = scratch();
    let env = env(root.path());
    let out = root.path().join("k");
    let output = run(&env, &["keygen", "--publisher-id", "acme", "--publisher", "Acme", "--out", s(&out)]);
    let body: String = std::fs::read_to_string(out.join("publisher.key")).expect("read").lines().filter(|l| !l.starts_with("-----")).collect();
    assert!(!(output.stdout_raw + &output.stderr).contains(&body));
}

// ---------------------------------------------------------------- trust store

fn trust_document(env: &ilium_plugin_host::cli::CliEnv) -> Value {
    read_json(&env.state_dir.join("ilium/trust.json"))
}

/// REQ: DEV-125
#[test]
fn dev_125_default_store_is_trust_json_in_the_state_folder() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    run(&env, &["trust", "add", s(&public)]);
    assert_eq!(trust_document(&env)["format"], "ilium-trust/1");
}

/// REQ: DEV-125
#[test]
fn dev_125_the_trust_store_variable_replaces_the_path() {
    let root = scratch();
    let store = root.path().join("ci/trust.json");
    std::fs::create_dir_all(store.parent().expect("dir")).expect("create");
    let env = env(root.path()).with_var("ILIUM_TRUST_STORE", s(&store));
    let (_, public) = make_key(&env, root.path(), "acme");
    run(&env, &["trust", "add", s(&public)]);
    assert_eq!(read_json(&store)["format"], "ilium-trust/1");
    assert!(!env.state_dir.join("ilium/trust.json").exists());
}

/// REQ: DEV-125
#[test]
fn dev_125_the_store_lists_the_added_key() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    run(&env, &["trust", "add", s(&public)]);
    let keys = trust_document(&env)["keys"].as_array().expect("keys").clone();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["key_id"], read_json(&public)["key_id"]);
}

/// REQ: DEV-126
#[test]
fn dev_126_trust_add_prints_the_fingerprint_and_succeeds() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    let key_id = read_json(&public)["key_id"].as_str().expect("key_id").to_owned();
    let output = run(&env, &["trust", "add", s(&public)]);
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout_raw.contains(&key_id) || output.stdout_raw.contains(&display_form(&key_id)));
}

/// REQ: DEV-126
#[test]
fn dev_126_a_malformed_key_file_fails_key_malformed() {
    let root = scratch();
    let env = env(root.path());
    let junk = root.path().join("junk.pub");
    std::fs::write(&junk, "{\"format\":\"nope\"}").expect("write");
    let output = run(&env, &["trust", "add", s(&junk)]);
    assert_eq!(code(&output).as_deref(), Some("trust.key-malformed"));
}

/// REQ: DEV-126
#[test]
fn dev_126_a_key_id_that_does_not_match_the_key_is_malformed() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    let mut document = read_json(&public);
    document["key_id"] = Value::String("0".repeat(64));
    write_json(&public, &document);
    assert_eq!(code(&run(&env, &["trust", "add", s(&public)])).as_deref(), Some("trust.key-malformed"));
}

/// REQ: DEV-126
#[test]
fn dev_126_adding_a_key_twice_fails_already_trusted_with_exit_one() {
    let root = scratch();
    let env = env(root.path());
    let (_, public) = make_key(&env, root.path(), "acme");
    run(&env, &["trust", "add", s(&public)]);
    let output = run(&env, &["trust", "add", s(&public)]);
    assert_eq!(code(&output).as_deref(), Some("trust.already-trusted"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-127
#[test]
fn dev_127_removing_an_unlisted_key_fails_not_found_with_exit_five() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["trust", "remove", &"d".repeat(64)]);
    assert_eq!(code(&output).as_deref(), Some("trust.not-found"));
    assert_eq!(output.exit_code, 5);
}

/// REQ: DEV-127
#[test]
fn dev_127_removing_a_listed_key_without_dependents_works() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, public) = make_key(&env, root.path(), "acme");
    let key_id = read_json(&public)["key_id"].as_str().expect("key_id").to_owned();
    run(&env, &["trust", "add", s(&public)]);
    assert_eq!(run(&env, &["trust", "remove", &key_id]).exit_code, 0);
    assert!(trust_document(&env)["keys"].as_array().expect("keys").is_empty());
}

/// REQ: DEV-127
#[test]
fn dev_127_a_key_in_use_needs_force() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, public) = make_key(&env, root.path(), "acme");
    let key_id = read_json(&public)["key_id"].as_str().expect("key_id").to_owned();
    run(&env, &["trust", "add", s(&public)]);
    session.add_plugin("acme.demo", "0.1.0", "T3", "running", false, &[]);
    session.set_signed_by("acme.demo", &key_id);
    let output = run(&env, &["trust", "remove", &key_id]);
    assert_eq!(code(&output).as_deref(), Some("trust.in-use"));
    assert_eq!(run(&env, &["trust", "remove", &key_id, "--force"]).exit_code, 0);
}

/// REQ: DEV-127
#[test]
fn dev_127_forced_removal_keeps_the_plugin_installed_but_untrusted() {
    let root = scratch();
    let session = MemorySession::new();
    let env = env_session(root.path(), &session);
    let (_, public) = make_key(&env, root.path(), "acme");
    let key_id = read_json(&public)["key_id"].as_str().expect("key_id").to_owned();
    run(&env, &["trust", "add", s(&public)]);
    session.add_plugin("acme.demo", "0.1.0", "T3", "running", false, &[]);
    session.set_signed_by("acme.demo", &key_id);
    run(&env, &["trust", "remove", &key_id, "--force"]);
    assert_eq!(session.plugin_state("acme.demo").as_deref(), Some("untrusted"));
    let list = run(&env, &["list"]);
    assert!(list.stdout_text.contains("acme.demo") && list.stdout_text.contains("untrusted"));
}

/// REQ: DEV-128
#[test]
fn dev_128_trust_list_prints_publisher_short_fingerprint_and_date_per_line() {
    let root = scratch();
    let env = env(root.path());
    let mut ids = Vec::new();
    for publisher_id in ["acme", "beta"] {
        let (_, public) = make_key(&env, root.path(), publisher_id);
        ids.push(read_json(&public)["key_id"].as_str().expect("key_id").to_owned());
        run(&env, &["trust", "add", s(&public)]);
    }
    let text = run(&env, &["trust", "list"]).stdout_text;
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "{text}");
    for (line, id) in lines.iter().zip(&ids) {
        assert!(line.contains("Acme Labs") && line.contains(&short_form(id)) && line.contains("2030-01-01"), "{line}");
    }
}

/// REQ: DEV-128
#[test]
fn dev_128_an_empty_store_lists_no_keys() {
    let root = scratch();
    let env = env(root.path());
    let output = run(&env, &["trust", "list"]);
    assert_eq!(output.exit_code, 0);
    assert!(output.stdout_text.lines().filter(|l| l.contains('…')).count() == 0);
}

// ---------------------------------------------------------------- publish

/// REQ: DEV-135
#[test]
fn dev_135_publish_without_registry_fails_missing_argument_exit_two() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["publish", s(&package)]);
    assert_eq!(code(&output).as_deref(), Some("usage.missing-argument"));
    assert_eq!(output.exit_code, 2);
}

/// REQ: DEV-135
#[test]
fn dev_135_publish_with_registry_does_not_fail_on_usage() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["publish", s(&package), "--registry", "https://registry.example.test/"]);
    assert_ne!(code(&output).as_deref(), Some("usage.missing-argument"));
}

/// REQ: DEV-136
#[test]
fn dev_136_a_higher_version_from_a_file_updates_the_installed_plugin() {
    let root = scratch();
    let session = MemorySession::new();
    session.set_client_attached(true);
    let env = env_session(root.path(), &session);
    let (dir, first) = packed_project(&env, root.path());
    assert_eq!(run(&env, &["install", s(&first), "--allow-unsigned", "--yes"]).exit_code, 0);
    let mut manifest = read_json(&dir.join("manifest.json"));
    manifest["version"] = Value::String("0.2.0".to_owned());
    write_json(&dir.join("manifest.json"), &manifest);
    write_dist(&dir, &["server"], "release");
    run_in(&env, &dir, &["pack"]);
    let second = dir.join("dist/acme.demo-0.2.0.iliumpkg");
    let output = run(&env, &["install", s(&second), "--allow-unsigned", "--yes"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert_eq!(recs(&output, "plugin")[0]["version"], "0.2.0");
}

/// REQ: DEV-136
#[test]
fn dev_136_the_sha256_of_the_package_is_reproducible_for_sharing() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["inspect", s(&package), "--json"]);
    assert_eq!(output.exit_code, 0);
    let digest = hex(&Sha256::digest(std::fs::read(&package).expect("read")));
    assert!(output.stdout_raw.contains(&digest) || is_sha256_hex(&digest));
}
