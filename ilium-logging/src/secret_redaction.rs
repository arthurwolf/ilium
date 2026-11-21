//! Shared credential-pattern redaction for bounded outbound evidence.
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
