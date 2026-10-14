//! Evidence custody for recaps: what may leave the evidence layer in a prompt,
//! a pointer, or a rendered output line.
//!
//! Secrets are replaced before any text is prompted or printed. Paths that
//! name credentials, key material, or agent transcript stores are never
//! offered as pointers. The rules here are deliberately blunt: a false
//! positive costs one pointer, a false negative leaks a credential.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

pub const REDACTION_MARKER: &str = "[redacted]";

/// Text with secret-looking values replaced, and how many replacements ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted {
    pub text: String,
    pub count: usize,
}

struct SecretRule {
    pattern: Regex,
    /// Replacement template. `$1`-style groups keep the key name visible.
    replacement: &'static str,
}

fn secret_rules() -> &'static [SecretRule] {
    static RULES: OnceLock<Vec<SecretRule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let rule = |pattern: &str, replacement: &'static str| SecretRule {
            pattern: Regex::new(pattern).expect("secret rule pattern is a valid regex literal"),
            replacement,
        };
        vec![
            rule(
                r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
                REDACTION_MARKER,
            ),
            rule(r"\b(?:sk|pk|rk)-[A-Za-z0-9_\-]{16,}", REDACTION_MARKER),
            rule(
                r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})",
                REDACTION_MARKER,
            ),
            rule(r"\bAKIA[0-9A-Z]{16}\b", REDACTION_MARKER),
            rule(r"\bxox[baprs]-[A-Za-z0-9\-]{10,}", REDACTION_MARKER),
            rule(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=\-]{12,}", "Bearer [redacted]"),
            rule(
                r#"(?i)\b([A-Z0-9_]*(?:API_?KEY|ACCESS_?TOKEN|AUTH_?TOKEN|SECRET|PASSWORD|PASSWD|TOKEN)(?:_[A-Z0-9]+)*)(\s*[=:]\s*["']?)[^\s"',;]{4,}"#,
                "$1$2[redacted]",
            ),
            rule(
                r#"(?i)\b((?:api[_-]?key|access[_-]?token|auth[_-]?token|client[_-]?secret|password|passwd|secret)\s*["']?\s*[:=]\s*["']?)[^\s"',;]{4,}"#,
                "$1[redacted]",
            ),
        ]
    })
}

/// Replaces credential-shaped text. Key names stay visible so the model can
/// still say that a key was configured; their values never leave.
pub fn redact_secrets(text: &str) -> Redacted {
    let mut current = text.to_string();
    let mut count = 0;
    for rule in secret_rules() {
        let matches = rule.pattern.find_iter(&current).count();
        if matches == 0 {
            continue;
        }
        count += matches;
        current = rule
            .pattern
            .replace_all(&current, rule.replacement)
            .into_owned();
    }
    Redacted {
        text: current,
        count,
    }
}

/// Directory names whose contents are never offered as evidence pointers.
/// Matching is per path component, case-insensitive.
const DENIED_DIRECTORY_NAMES: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".kube",
    ".docker",
    ".password-store",
    ".claude",
    ".codex",
    ".antigravity",
];

const DENIED_FILE_NAMES: &[&str] = &[
    ".netrc",
    ".git-credentials",
    ".npmrc",
    ".pypirc",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "credentials",
    "credentials.json",
    "secrets.json",
];

const DENIED_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "kdbx", "jks", "keystore"];

/// Why a path must not be offered as a pointer, or `None` when it may be.
pub fn denied_path_reason(path: &Path) -> Option<&'static str> {
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        if DENIED_DIRECTORY_NAMES.contains(&name.as_str()) {
            return Some("path is inside a protected or agent-store directory");
        }
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if file_name.starts_with(".env") {
        return Some("environment files may hold secrets");
    }
    if DENIED_FILE_NAMES.contains(&file_name.as_str()) {
        return Some("credential file name");
    }
    if file_name.contains("secret")
        || file_name.contains("token")
        || file_name.contains("credential")
    {
        return Some("credential-like file name");
    }
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if DENIED_EXTENSIONS.contains(&extension.as_str()) {
        return Some("key or keystore file");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_token_value_suffix_without_hiding_the_key_name() {
        let redacted = redact_secrets("export TOKEN_VALUE=supersecretvalue123");
        assert_eq!(redacted.text, "export TOKEN_VALUE=[redacted]");
        assert_eq!(redacted.count, 1);
    }

    #[test]
    fn redacts_key_values_but_keeps_key_names() {
        let redacted =
            redact_secrets("export ANTHROPIC_API_KEY=abcd1234efgh5678 and api_key: \"zzzz9999\"");
        assert!(!redacted.text.contains("abcd1234efgh5678"));
        assert!(!redacted.text.contains("zzzz9999"));
        assert!(redacted.text.contains("ANTHROPIC_API_KEY"));
        assert!(redacted.count >= 2);
    }

    #[test]
    fn redacts_token_shapes_and_private_key_blocks() {
        let text = "token sk-abcdefghijklmnopqrstu and ghp_abcdefghijklmnopqrstuvwxyz0123 \
                    -----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY----- \
                    Authorization: Bearer abcdefghijklmnop12345";
        let redacted = redact_secrets(text);
        assert!(!redacted.text.contains("abcdefghijklmnopqrstu"));
        assert!(!redacted.text.contains("ghp_abcdef"));
        assert!(!redacted.text.contains("AAAA"));
        assert!(!redacted.text.contains("abcdefghijklmnop12345"));
        assert!(redacted.count >= 4);
    }

    #[test]
    fn leaves_ordinary_prose_untouched() {
        let text = "Ran cargo test; 12 tests passed in src/lib.rs.";
        let redacted = redact_secrets(text);
        assert_eq!(redacted.text, text);
        assert_eq!(redacted.count, 0);
    }

    #[test]
    fn denies_credential_stores_and_key_files() {
        for path in [
            "/home/u/.ssh/config",
            "/home/u/project/.env.local",
            "/home/u/project/deploy/id_ed25519",
            "/home/u/project/certs/server.pem",
            "/home/u/project/config/api-secrets.yaml",
            "/home/u/.claude/projects/x/session.jsonl",
        ] {
            assert!(denied_path_reason(Path::new(path)).is_some(), "{path}");
        }
    }

    #[test]
    fn allows_ordinary_project_files() {
        for path in ["/home/u/project/src/lib.rs", "/home/u/project/docs/TODO.md"] {
            assert_eq!(denied_path_reason(Path::new(path)), None, "{path}");
        }
    }
}
