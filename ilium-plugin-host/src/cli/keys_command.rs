//! `ilium plugin keygen|sign|trust` (reference-plugin-cli.md DEV-110..128,
//! reference-signing-and-trust.md).

use super::{offline, CliEnv, CliOutput, SessionRequest, SessionResponse};
use crate::signing::{sign_for_publisher, SignedAt, SigningKey};
use crate::trust::{PublisherKey, RevocationReason, TrustDocument, TrustRepository};
use base64::{engine::general_purpose::STANDARD, engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// PKCS#8 wrapper in front of a 32-byte Ed25519 seed (RFC 8410).
const PKCS8_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct Parsed {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Parsed {
    fn value(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(flag, _)| flag == name)
            .and_then(|(_, value)| value.as_deref())
    }
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(flag, _)| flag == name)
    }
}

fn parse(args: &[String], valued: &[&str], boolean: &[&str]) -> Result<Parsed, CliOutput> {
    let mut parsed = Parsed { positional: Vec::new(), flags: Vec::new() };
    let mut iterator = args.iter();
    while let Some(argument) = iterator.next() {
        let name = argument.as_str();
        if matches!(name, "--json" | "--quiet" | "--no-color") || boolean.contains(&name) {
            parsed.flags.push((name.to_owned(), None));
        } else if valued.contains(&name) {
            match iterator.next() {
                Some(value) => parsed.flags.push((name.to_owned(), Some(value.clone()))),
                None => {
                    return Err(offline::failure(
                        "usage.missing-argument",
                        format!("{name} needs a value"),
                        2,
                    ))
                }
            }
        } else if name.starts_with("--") {
            return Err(offline::failure(
                "usage.unknown-flag",
                format!("Unknown flag: {name}"),
                2,
            ));
        } else {
            parsed.positional.push(argument.clone());
        }
    }
    Ok(parsed)
}

fn resolve(env: &CliEnv, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() { path.to_path_buf() } else { env.cwd.join(path) }
}

fn stamp(env: &CliEnv) -> Result<String, CliOutput> {
    crate::trust::utc_timestamp(env.now)
        .map_err(|error| offline::failure("trust.store-invalid", error.message, 1))
}

pub fn run(command: &str, args: &[String], env: &CliEnv) -> CliOutput {
    match command {
        "keygen" => keygen(args, env),
        "sign" => sign(args, env),
        "publish" => publish(args),
        _ => trust(args, env),
    }
}

fn artifact(kind: &str, path: &Path, bytes: &[u8]) -> Value {
    json!({"type":"artifact","kind":kind,"path":path.to_string_lossy(),
           "sha256":hex(&Sha256::digest(bytes)),"bytes":bytes.len()})
}

fn keygen(args: &[String], env: &CliEnv) -> CliOutput {
    let parsed = match parse(args, &["--publisher-id", "--publisher", "--out"], &[]) {
        Ok(parsed) => parsed,
        Err(output) => return output,
    };
    let (Some(publisher_id), Some(publisher)) =
        (parsed.value("--publisher-id"), parsed.value("--publisher"))
    else {
        return offline::failure(
            "usage.missing-argument",
            "keygen needs --publisher-id and --publisher",
            2,
        );
    };
    let out = resolve(env, parsed.value("--out").unwrap_or("."));
    let key_path = out.join("publisher.key");
    let public_path = out.join("publisher.pub");
    if key_path.exists() || public_path.exists() {
        return offline::failure(
            "trust.key-exists",
            "publisher.key or publisher.pub already exists; keygen never overwrites",
            1,
        );
    }
    let created = match stamp(env) {
        Ok(created) => created,
        Err(output) => return output,
    };
    let seed = match ilium_platform::random::random_seed() {
        Ok(seed) => seed,
        Err(error) => return offline::failure("trust.key-unreadable", error.to_string(), 1),
    };
    let secret = ed25519_dalek::SigningKey::from_bytes(&seed);
    let public = secret.verifying_key().to_bytes();
    let mut der = PKCS8_PREFIX.to_vec();
    der.extend_from_slice(&seed);
    let encoded = STANDARD.encode(&der);
    let pem = format!("-----BEGIN PRIVATE KEY-----\n{encoded}\n-----END PRIVATE KEY-----\n");
    let document = json!({
        "format":"ilium-pub/1","publisher_id":publisher_id,"publisher":publisher,
        "algorithm":"ed25519","public_key":URL_SAFE_NO_PAD.encode(public),
        "key_id":hex(&Sha256::digest(public)),"created":created
    });
    let mut public_bytes = serde_json::to_vec_pretty(&document).unwrap_or_default();
    public_bytes.push(b'\n');
    let write = || -> std::io::Result<()> {
        ilium_platform::secure_fs::create_private_directory(&out)?;
        write_new(&key_path, pem.as_bytes(), true)?;
        write_new(&public_path, &public_bytes, false)
    };
    if let Err(error) = write() {
        return offline::failure("trust.key-write-failed", error.to_string(), 1);
    }
    offline::finish(
        vec![
            artifact("key", &key_path, pem.as_bytes()),
            artifact("public-key", &public_path, &public_bytes),
        ],
        0,
    )
}

fn write_new(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = if private {
        ilium_platform::secure_fs::private_open_options()
    } else {
        std::fs::OpenOptions::new()
    };
    let mut file = options.write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn load_signing_key(path: &Path) -> Result<SigningKey, CliOutput> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|error| offline::failure("sign.key-unreadable", error.to_string(), 1))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(offline::failure(
                "sign.key-permissions",
                "the key file must not be readable by group or others (mode 0600)",
                1,
            ));
        }
    }
    let unreadable = |message: &str| offline::failure("sign.key-unreadable", message, 1);
    let text = std::fs::read_to_string(path).map_err(|error| unreadable(&error.to_string()))?;
    let body: String = text
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let der = STANDARD
        .decode(body.trim())
        .map_err(|_| unreadable("the key is not PKCS#8 PEM"))?;
    if der.len() != 48 || der[..16] != PKCS8_PREFIX {
        return Err(unreadable("the key is not an Ed25519 PKCS#8 key"));
    }
    let seed: [u8; 32] = der[16..].try_into().map_err(|_| unreadable("bad key length"))?;
    Ok(SigningKey::from_seed(seed))
}

fn sign(args: &[String], env: &CliEnv) -> CliOutput {
    let parsed = match parse(args, &["--key"], &["--replace"]) {
        Ok(parsed) => parsed,
        Err(output) => return output,
    };
    let Some(package_arg) = parsed.positional.first() else {
        return offline::failure("usage.missing-argument", "sign needs a package file", 2);
    };
    let key_arg = parsed
        .value("--key")
        .map(str::to_owned)
        .or_else(|| env.env.get("ILIUM_PLUGIN_SIGNING_KEY").cloned())
        .or_else(|| env.env.get("ILIUM_PUBLISHER_KEY").cloned());
    let Some(key_arg) = key_arg else {
        return offline::failure(
            "usage.missing-argument",
            "sign needs --key, ILIUM_PLUGIN_SIGNING_KEY or ILIUM_PUBLISHER_KEY",
            2,
        );
    };
    let key_path = resolve(env, &key_arg);
    let key = match load_signing_key(&key_path) {
        Ok(key) => key,
        Err(output) => return output,
    };
    let package_path = resolve(env, package_arg);
    let bytes = match std::fs::read(&package_path) {
        Ok(bytes) => bytes,
        Err(error) => return offline::failure("plugin.not-found", error.to_string(), 5),
    };
    let archive = match crate::package::read_package(&bytes) {
        Ok(archive) => archive,
        Err(diagnostics) => {
            let first = diagnostics.first();
            return offline::failure(
                first.map_or("package.invalid", |d| d.code.as_str()),
                first.map_or("invalid package", |d| d.message.as_str()),
                1,
            );
        }
    };
    if archive.signature.is_some() && !parsed.has("--replace") {
        return offline::failure(
            "sign.already-signed",
            "the package is signed; pass --replace to sign again",
            1,
        );
    }
    let publisher_id = std::fs::read(key_path.with_extension("pub"))
        .ok()
        .and_then(|raw| PublisherKey::parse(&raw).ok())
        .map(|public| public.publisher_id)
        .unwrap_or_else(|| {
            archive.manifest.id.split('.').next().unwrap_or_default().to_owned()
        });
    let id = &archive.manifest.id;
    if !id.strip_prefix(publisher_id.as_str()).is_some_and(|rest| rest.starts_with('.')) {
        return offline::failure(
            "sign.publisher-mismatch",
            format!("the plugin id {id} does not belong to publisher {publisher_id}"),
            1,
        );
    }
    let signed_at = match stamp(env) {
        Ok(signed_at) => signed_at,
        Err(output) => return output,
    };
    let signed = match sign_for_publisher(&bytes, &key, &publisher_id, SignedAt(signed_at)) {
        Ok(signed) => signed,
        Err(diagnostic) => {
            return offline::failure(diagnostic.code.as_str(), diagnostic.message, 1)
        }
    };
    let temporary = package_path.with_extension("iliumpkg.tmp");
    if let Err(error) = std::fs::write(&temporary, &signed)
        .and_then(|()| std::fs::rename(&temporary, &package_path))
    {
        return offline::failure("sign.write-failed", error.to_string(), 1);
    }
    offline::finish(vec![artifact("signature", &package_path, &signed)], 0)
}

pub(super) fn trust_store_path(env: &CliEnv) -> PathBuf {
    env.env
        .get("ILIUM_TRUST_STORE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| env.state_dir.join("ilium/trust.json"))
}

fn display_form(key_id: &str) -> String {
    key_id
        .as_bytes()
        .chunks(4)
        .filter_map(|chunk| std::str::from_utf8(chunk).ok())
        .collect::<Vec<_>>()
        .join(" ")
}

fn trust_failure(code: &str, message: String) -> CliOutput {
    let exit = match code {
        "trust.key-unknown" | "trust.not-found" => 5,
        _ => 1,
    };
    let code = if code == "trust.key-unknown" { "trust.not-found" } else { code };
    offline::failure(code, message, exit)
}

fn trust(args: &[String], env: &CliEnv) -> CliOutput {
    let parsed = match parse(args, &["--reason"], &["--force"]) {
        Ok(parsed) => parsed,
        Err(output) => return output,
    };
    let Some(action) = parsed.positional.first().map(String::as_str) else {
        return offline::failure(
            "usage.missing-argument",
            "trust needs add, list, remove, export or revoke",
            2,
        );
    };
    let path = trust_store_path(env);
    let repository = match TrustRepository::new(path.clone(), TrustDocument::default()) {
        Ok(repository) => repository,
        Err(error) => return trust_failure(error.code.as_str(), error.message),
    };
    let now = match stamp(env) {
        Ok(now) => now,
        Err(output) => return output,
    };
    let operand = parsed.positional.get(1);
    match action {
        "list" => {
            let document = std::fs::read(&path)
                .ok()
                .and_then(|raw| TrustDocument::parse(&raw).ok())
                .unwrap_or_default();
            let mut text = String::new();
            let mut records = Vec::new();
            for key in &document.keys {
                text.push_str(&format!(
                    "{} {}… {}\n",
                    key.publisher,
                    &key.key_id[..8],
                    &key.added[..10]
                ));
                records.push(json!({"type":"trust","key_id":key.key_id,
                    "publisher_id":key.publisher_id,"publisher":key.publisher,"added":key.added}));
            }
            let mut output = offline::finish(records, 0);
            output.stdout_text = text;
            output
        }
        "add" => {
            let Some(file) = operand else {
                return offline::failure("usage.missing-argument", "trust add needs a key file", 2);
            };
            let key = match std::fs::read(resolve(env, file))
                .map_err(|error| error.to_string())
                .and_then(|raw| PublisherKey::parse(&raw).map_err(|e| e.message))
            {
                Ok(key) => key,
                Err(message) => return offline::failure("trust.key-malformed", message, 1),
            };
            let known = std::fs::read(&path)
                .ok()
                .and_then(|raw| TrustDocument::parse(&raw).ok())
                .is_some_and(|document| document.keys.iter().any(|k| k.key_id == key.key_id));
            if known {
                return offline::failure(
                    "trust.already-trusted",
                    format!("{} is already trusted", display_form(&key.key_id)),
                    1,
                );
            }
            if let Err(error) = repository.add(&key, &now, true) {
                return trust_failure(error.code.as_str(), error.message);
            }
            let mut output = offline::finish(
                vec![json!({"type":"progress","step":"install",
                    "message":format!("trusted {} {}", key.publisher, display_form(&key.key_id))})],
                0,
            );
            output.stdout_text = format!("{}\n", display_form(&key.key_id));
            output
        }
        "remove" | "revoke" => {
            let Some(key_id) = operand.map(|id| id.replace(' ', "")) else {
                return offline::failure("usage.missing-argument", "trust needs a key id", 2);
            };
            let signed: Vec<String> = match &env.session {
                Some(session) => match session.request(SessionRequest::List) {
                    Ok(SessionResponse::Plugins(plugins)) => plugins
                        .into_iter()
                        .filter(|plugin| plugin.signed_by.as_deref() == Some(key_id.as_str()))
                        .map(|plugin| plugin.id)
                        .collect(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            let change = if action == "remove" {
                repository.remove(&key_id, &signed, parsed.has("--force"))
            } else {
                let reason = match parsed.value("--reason").unwrap_or("retired") {
                    "compromised" => RevocationReason::Compromised,
                    "superseded" => RevocationReason::Superseded,
                    "retired" => RevocationReason::Retired,
                    other => {
                        return offline::failure(
                            "usage.invalid-value",
                            format!("unknown revocation reason {other}"),
                            2,
                        )
                    }
                };
                repository.revoke(&key_id, reason, &now, &signed)
            };
            match change {
                Ok(change) => {
                    if let Some(session) = &env.session {
                        for id in change.disabled_untrusted.iter().chain(&change.disabled_blocked) {
                            let _ = session.request(SessionRequest::MarkUntrusted {
                                id: id.clone(),
                                reason: format!("trust-{action}"),
                            });
                        }
                    }
                    offline::finish(vec![], 0)
                }
                Err(error) => trust_failure(error.code.as_str(), error.message),
            }
        }
        "export" => {
            let Some(key_id) = operand else {
                return offline::failure("usage.missing-argument", "trust export needs a key id", 2);
            };
            match repository.export_with_created(key_id, &now) {
                Ok(bytes) => {
                    let mut output = offline::finish(vec![], 0);
                    output.stdout_text = String::from_utf8_lossy(&bytes).into_owned();
                    output
                }
                Err(error) => trust_failure(error.code.as_str(), error.message),
            }
        }
        other => offline::failure(
            "usage.invalid-value",
            format!("unknown trust action {other}"),
            2,
        ),
    }
}

/// `publish <package> --registry <url>`: the registry upload channel is not connected to
/// this adapter, so a well-formed request reports the registry as unreachable.
fn publish(args: &[String]) -> CliOutput {
    let parsed = match parse(args, &["--registry"], &[]) {
        Ok(parsed) => parsed,
        Err(output) => return output,
    };
    let Some(registry) = parsed.value("--registry") else {
        return offline::failure("usage.missing-argument", "publish needs --registry <url>", 2);
    };
    if parsed.positional.is_empty() {
        return offline::failure("usage.missing-argument", "publish needs a package file", 2);
    }
    offline::failure(
        "registry.unreachable",
        format!("no upload channel to {registry} is connected"),
        6,
    )
}
