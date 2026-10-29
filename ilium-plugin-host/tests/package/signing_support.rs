//! Helpers of the signing and trust tests (part 2). Custom signatures need a real Ed25519
//! implementation in the test: the dev-dependency `ed25519-dalek` is listed in API_GAPS.md.

use super::support::*;
use ed25519_dalek::Signer;
use ilium_plugin_host::cli::CliOutput;
use ilium_plugin_host::package::read_package;
use ilium_plugin_host::signing::{PublicKey, TrustStore, TrustedKey};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub const PREFIX: &str = "ilium-plugin-signature-v1";

pub fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_800_000)
}

/// Seed number 0 is the manual's vector seed; other numbers give distinct fixed seeds.
pub fn seed_of(n: u8) -> Vec<u8> {
    if n == 0 {
        vector_seed()
    } else {
        vec![n; 32]
    }
}

pub fn signing_key(n: u8) -> ed25519_dalek::SigningKey {
    let seed: [u8; 32] = seed_of(n).try_into().expect("32-byte seed");
    ed25519_dalek::SigningKey::from_bytes(&seed)
}

pub fn public_of(n: u8) -> Vec<u8> {
    signing_key(n).verifying_key().to_bytes().to_vec()
}

pub fn key_id_of(n: u8) -> String {
    sha256_hex(&public_of(n))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn b64url(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        let digits = chunk.len() + 1;
        for i in 0..digits {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

pub fn unb64url(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = ALPHABET
            .iter()
            .position(|a| *a == c)
            .expect("base64url character") as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

pub fn message(prefix: &str, publisher_id: &str, manifest: &[u8]) -> Vec<u8> {
    let mut m = prefix.as_bytes().to_vec();
    m.push(0);
    m.extend_from_slice(publisher_id.as_bytes());
    m.push(0);
    m.extend_from_slice(&sha256(manifest));
    m
}

pub fn sign_raw(n: u8, message: &[u8]) -> Vec<u8> {
    signing_key(n).sign(message).to_bytes().to_vec()
}

#[derive(Clone)]
pub struct Obj {
    pub algorithm: String,
    pub key_id: String,
    pub publisher_id: String,
    pub signature: String,
    pub signed_at: String,
}

/// A valid signature object of key `n` over `manifest` for `publisher_id`.
pub fn obj(n: u8, publisher_id: &str, manifest: &[u8]) -> Obj {
    Obj {
        algorithm: "ed25519".into(),
        key_id: key_id_of(n),
        publisher_id: publisher_id.into(),
        signature: b64url(&sign_raw(n, &message(PREFIX, publisher_id, manifest))),
        signed_at: VECTOR_SIGNED_AT.into(),
    }
}

/// The signature document in the manual's serialisation (2-space indent, LF, final LF).
pub fn sig_doc(format: &str, objs: &[Obj]) -> String {
    let items: Vec<String> = objs
        .iter()
        .map(|o| {
            format!(
                "    {{\n      \"algorithm\": \"{}\",\n      \"key_id\": \"{}\",\n      \"publisher_id\": \"{}\",\n      \"signature\": \"{}\",\n      \"signed_at\": \"{}\"\n    }}",
                o.algorithm, o.key_id, o.publisher_id, o.signature, o.signed_at
            )
        })
        .collect();
    if items.is_empty() {
        return format!("{{\n  \"format\": \"{format}\",\n  \"signatures\": []\n}}\n");
    }
    format!(
        "{{\n  \"format\": \"{format}\",\n  \"signatures\": [\n{}\n  ]\n}}\n",
        items.join(",\n")
    )
}

/// The vector package with `signature` replaced by `text`, entries in bytewise path order.
pub fn with_signature_text(manifest: &[u8], component: &[u8], text: &[u8]) -> Vec<u8> {
    zip_of(&[
        ("manifest.json", manifest.to_vec()),
        ("server.wasm", component.to_vec()),
        ("signature", text.to_vec()),
    ])
}

pub fn with_sig(text: &str) -> Vec<u8> {
    with_signature_text(&vector_manifest(), &vector_component(), text.as_bytes())
}

/// The vector manifest with another plugin id and publisher display name. Component hash is
/// unchanged because the component is the same.
pub fn manifest_for(id: &str, publisher: &str) -> Vec<u8> {
    let text = String::from_utf8(vector_manifest()).expect("utf8");
    text.replace("example.minimal", id)
        .replace("Example Labs", publisher)
        .into_bytes()
}

/// A package whose manifest is `manifest`, signed by key `n` as `publisher_id`.
pub fn signed_package(n: u8, publisher_id: &str, manifest: &[u8]) -> Vec<u8> {
    with_signature_text(
        manifest,
        &vector_component(),
        sig_doc("ilium-sig/1", &[obj(n, publisher_id, manifest)]).as_bytes(),
    )
}

pub fn trust_of(n: u8, publisher_id: &str) -> TrustStore {
    TrustStore {
        keys: vec![TrustedKey {
            key_id: key_id_of(n),
            publisher_id: publisher_id.into(),
            public_key: PublicKey(public_of(n)),
        }],
    }
}

/// First error code from reading then verifying, or "accepted".
pub fn refusal(bytes: &[u8], trust: &TrustStore) -> String {
    refusal_with_keys(bytes, trust, &[])
}

/// Public verification keys do not grant trust. Countersignature tests must
/// supply these bytes explicitly; a fingerprint alone cannot verify Ed25519.
pub fn refusal_with_keys(bytes: &[u8], trust: &TrustStore, known: &[TrustedKey]) -> String {
    match read_package(bytes) {
        Err(diagnostics) => error_codes(&diagnostics)
            .first()
            .cloned()
            .unwrap_or_else(|| "no-error-code".into()),
        Ok(package) => {
            match ilium_plugin_host::signing::verify_with_public_keys(&package, trust, known, now())
            {
                Err(d) => d.code.as_str().to_string(),
                Ok(_) => "accepted".into(),
            }
        }
    }
}

pub fn read_refusal(bytes: &[u8]) -> String {
    match read_package(bytes) {
        Err(diagnostics) => error_codes(&diagnostics)
            .first()
            .cloned()
            .unwrap_or_else(|| "no-error-code".into()),
        Ok(_) => "accepted".into(),
    }
}

// ---- CLI side ----

pub struct Entry {
    pub seed: u8,
    pub publisher_id: &'static str,
    pub publisher: &'static str,
    pub status: &'static str,
    pub revoked: Option<(&'static str, &'static str)>,
    pub added_by: &'static str,
}

pub fn entry(seed: u8, publisher_id: &'static str, publisher: &'static str) -> Entry {
    Entry {
        seed,
        publisher_id,
        publisher,
        status: "trusted",
        revoked: None,
        added_by: "user",
    }
}

pub fn trust_json(entries: &[Entry]) -> String {
    let items: Vec<String> = entries
        .iter()
        .map(|e| {
            let revoked = match e.revoked {
                Some((reason, at)) => format!("{{\"reason\": \"{reason}\", \"at\": \"{at}\"}}"),
                None => "null".into(),
            };
            format!(
                "    {{\n      \"key_id\": \"{}\",\n      \"publisher_id\": \"{}\",\n      \"publisher\": \"{}\",\n      \"public_key\": \"{}\",\n      \"added\": \"2026-10-12 10:05:00\",\n      \"added_by\": \"{}\",\n      \"status\": \"{}\",\n      \"revoked\": {}\n    }}",
                key_id_of(e.seed), e.publisher_id, e.publisher, b64url(&public_of(e.seed)), e.added_by, e.status, revoked
            )
        })
        .collect();
    format!(
        "{{\n  \"format\": \"ilium-trust/1\",\n  \"keys\": [\n{}\n  ]\n}}\n",
        items.join(",\n")
    )
}

/// Writes the user trust store where either reading finds it (see API_GAPS.md).
pub fn write_trust(fixture: &CliFixture, text: &str) {
    write_file(&fixture.state().join("trust.json"), text.as_bytes());
    write_file(
        &fixture.state().join("ilium").join("trust.json"),
        text.as_bytes(),
    );
}

pub fn read_trust(fixture: &CliFixture) -> Option<String> {
    let a = fixture.state().join("ilium").join("trust.json");
    let b = fixture.state().join("trust.json");
    std::fs::read_to_string(&a)
        .or_else(|_| std::fs::read_to_string(&b))
        .ok()
}

pub fn put_package(fixture: &CliFixture, name: &str, bytes: &[u8]) -> String {
    let path = fixture.work().join(name);
    write_file(&path, bytes);
    path.to_string_lossy().into_owned()
}

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Runs the CLI with environment variables set, restoring them afterwards.
pub fn run_env(fixture: &CliFixture, args: &[&str], vars: &[(&str, &str)]) -> CliOutput {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let saved: Vec<(String, Option<String>)> = vars
        .iter()
        .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
        .collect();
    for (k, v) in vars {
        std::env::set_var(k, v);
    }
    let out = run_cli(fixture, args);
    for (k, old) in saved {
        match old {
            Some(v) => std::env::set_var(&k, v),
            None => std::env::remove_var(&k),
        }
    }
    out
}

/// Every `code` field of every output record, in order.
pub fn all_codes(out: &CliOutput) -> Vec<String> {
    out.stdout_jsonl
        .iter()
        .filter_map(|r| r.get("code").and_then(|c| c.as_str()).map(String::from))
        .collect()
}

pub fn all_text(out: &CliOutput) -> String {
    let mut text = out.stderr.clone();
    for r in &out.stdout_jsonl {
        text.push_str(&r.to_string());
        text.push('\n');
    }
    text
}

/// Installs `bytes` non-interactively (`--yes`) with extra arguments.
pub fn install(fixture: &CliFixture, bytes: &[u8], extra: &[&str]) -> CliOutput {
    let path = put_package(fixture, "pkg.ilpl", bytes);
    let mut args = vec!["install", path.as_str(), "--yes"];
    args.extend_from_slice(extra);
    run_cli(fixture, &args)
}

pub fn pkcs8_pem(n: u8) -> String {
    let mut der = unhex("302e020100300506032b657004220420");
    der.extend_from_slice(&seed_of(n));
    let std_b64 = to_std_b64(&der);
    format!("-----BEGIN PRIVATE KEY-----\n{std_b64}\n-----END PRIVATE KEY-----\n")
}

pub fn to_std_b64(bytes: &[u8]) -> String {
    let s = b64url(bytes).replace('-', "+").replace('_', "/");
    let pad = (4 - s.len() % 4) % 4;
    format!("{s}{}", "=".repeat(pad))
}

pub fn write_key(path: &std::path::Path, n: u8, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    write_file(path, pkcs8_pem(n).as_bytes());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

pub fn mode_of(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

/// Text of a manual page (page-text rows).
pub fn page(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}
