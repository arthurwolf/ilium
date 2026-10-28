//! Error codes, namespaced codes and the code registry (reference-errors-and-limits.md).

use std::borrow::Cow;

pub use crate::generated::types::{ErrorCode, PluginError};

/// Severity of a diagnostic (reference-check-and-lint-rules.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Refuses the check or install (reference-check-and-lint-rules.md).
    Error,
    /// Advisory, written `warn` (reference-check-and-lint-rules.md).
    Warning,
    /// Informational line (reference-check-and-lint-rules.md).
    Info,
}

impl Severity {
    /// Every severity, most severe first (reference-check-and-lint-rules.md).
    pub const ALL: [Severity; 3] = [Severity::Error, Severity::Warning, Severity::Info];

    /// `error`, `warn` or `info` (reference-check-and-lint-rules.md).
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warn",
            Severity::Info => "info",
        }
    }
}

impl std::fmt::Display for Severity {
    /// `error`, `warn` or `info` (reference-check-and-lint-rules.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Severity {
    type Err = UnknownName;

    /// Parses `error`, `warn` or `info` (reference-check-and-lint-rules.md).
    fn from_str(text: &str) -> Result<Severity, UnknownName> {
        Severity::ALL
            .into_iter()
            .find(|severity| severity.as_str() == text)
            .ok_or_else(|| UnknownName::new("severity", text))
    }
}

/// A namespaced code such as "manifest.grant-side" (reference-errors-and-limits.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Code(pub Cow<'static, str>);

impl Code {
    /// A code from one of the constants of [`crate::codes`] (reference-errors-and-limits.md).
    pub const fn from_static(code: &'static str) -> Code {
        Code(Cow::Borrowed(code))
    }

    /// The code text (reference-errors-and-limits.md).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before the first `.`, for example `manifest` (reference-errors-and-limits.md).
    pub fn namespace(&self) -> &str {
        self.as_str()
            .split_once('.')
            .map_or(self.as_str(), |(namespace, _)| namespace)
    }

    /// The `error-code` a plugin sees for this code (reference-errors-and-limits.md).
    pub fn base(&self) -> ErrorCode {
        registry()
            .iter()
            .find(|entry| entry.code == self.as_str())
            .map_or(ErrorCode::Internal, |entry| entry.base)
    }
}

impl std::fmt::Display for Code {
    /// The code text (reference-errors-and-limits.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Longest `plugin-error.message` the API produces, in characters (test plan 1.3, "API").
pub const MESSAGE_MAX_CHARS: usize = 256;

/// Renders as `<code>: <message>`, the line a user sees in a log or a terminal.
impl std::fmt::Display for PluginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl PluginError {
    /// API: message capped at 256 characters (reference-errors-and-limits.md "The plugin error record").
    pub fn new(code: ErrorCode, message: impl Into<String>) -> PluginError {
        PluginError {
            code,
            message: message.into().chars().take(MESSAGE_MAX_CHARS).collect(),
            retry_after_ms: None,
        }
    }

    /// An error whose message starts with `<extension code>: ` (reference-errors-and-limits.md
    /// "Extension point codes").
    pub fn with_prefix(code: ErrorCode, extension_code: &Code, message: &str) -> PluginError {
        Self::new(code, format!("{extension_code}: {message}"))
    }

    /// The HTTP status that a registry answer with this error carries (PKG-810: `registry.size` is 413).
    pub fn http_status(&self) -> u16 {
        let code = self
            .message
            .split_once(": ")
            .map(|(prefix, _)| prefix)
            .filter(|prefix| prefix.contains('.') && !prefix.contains(' '));
        match code {
            Some("registry.size") => 413,
            Some("registry.upload-timeout") => 408,
            Some("registry.not-owner" | "registry.auth") => 403,
            Some("registry.not-found" | "plugin.not-found") => 404,
            Some("registry.version-exists" | "registry.version-order" | "registry.last-owner") => {
                409
            }
            Some("registry.taken-down") => 410,
            _ => match self.code {
                ErrorCode::NotPermitted | ErrorCode::Revoked => 403,
                ErrorCode::NotFound => 404,
                ErrorCode::InvalidArgument => 400,
                ErrorCode::Conflict => 409,
                ErrorCode::QuotaExceeded => 413,
                ErrorCode::RateLimited => 429,
                ErrorCode::Busy | ErrorCode::PaneNotReady => 503,
                ErrorCode::TimedOut => 408,
                ErrorCode::Cancelled => 499,
                ErrorCode::Unsupported => 501,
                ErrorCode::Network => 502,
                ErrorCode::Io | ErrorCode::Internal => 500,
            },
        }
    }

    /// An error whose message starts with the namespaced `code` (for example `registry.size`),
    /// followed by `: ` and `message`; `base` is the `error-code` a plugin sees.
    pub fn coded(code: &str, base: ErrorCode, message: impl std::fmt::Display) -> PluginError {
        PluginError::new(base, format!("{code}: {message}"))
    }

    /// The namespaced code at the start of the message, if any (reference-errors-and-limits.md).
    pub fn extension_code(&self) -> Option<Code> {
        let (prefix, _) = self.message.split_once(": ")?;
        registry()
            .iter()
            .find(|entry| entry.code == prefix)
            .map(|entry| Code::from_static(entry.code))
    }
}

/// Every registered namespaced code with its base code (reference-errors-and-limits.md).
pub fn registry() -> &'static [CodeInfo] {
    static REGISTRY: std::sync::OnceLock<Vec<CodeInfo>> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| {
        crate::codes::ALL
            .iter()
            .map(|&code| CodeInfo {
                code,
                base: runtime_base(code),
                retryable: matches!(code, "extension.disabled" | "snapshot.write-failed"),
                page: match code.split_once('.').map(|(namespace, _)| namespace) {
                    Some("extension") => "api-extension-points.md",
                    Some("trap" | "settings" | "host" | "config" | "snapshot") => {
                        "reference-errors-and-limits.md"
                    }
                    Some("component") => "reference-host-runtime.md",
                    _ => "reference-manifest-schema.md",
                },
            })
            .collect()
    })
}

// Install, trap and log-only diagnostics have no plugin-error representation. The
// existing non-optional API uses Internal for those diagnostics and unknown codes;
// this does not turn a diagnostic into a caller-visible failure or a success.
fn runtime_base(code: &str) -> ErrorCode {
    match code {
        "extension.unknown-point" | "settings.unknown-key" => ErrorCode::NotFound,
        "extension.not-granted" | "settings.key-not-owned" | "settings.locked" => {
            ErrorCode::NotPermitted
        }
        // A full point instead uses QuotaExceeded. with_prefix keeps the explicit
        // caller-selected base because a code alone cannot distinguish these cases.
        "extension.schema-mismatch"
        | "extension.cap-exceeded"
        | "extension.entry-rejected"
        | "settings.invalid-value" => ErrorCode::InvalidArgument,
        "extension.reentrant" | "extension.vetoed" | "extension.depth-exceeded" => {
            ErrorCode::Conflict
        }
        "extension.not-dynamic" => ErrorCode::Unsupported,
        _ => ErrorCode::Internal,
    }
}

/// One entry of the code registry (reference-errors-and-limits.md).
#[derive(Debug, Clone)]
pub struct CodeInfo {
    /// The namespaced code, for example `trap.deadline`.
    pub code: &'static str,
    /// The `error-code` a plugin sees.
    pub base: ErrorCode,
    /// Whether a retry can succeed (reference-errors-and-limits.md, "Retry" column).
    pub retryable: bool,
    /// The manual page that defines the code.
    pub page: &'static str,
}

/// The closed list of refusal reason words, in table order (reference-errors-and-limits.md
/// "Refusal reasons"). For `not-permitted`, `quota-exceeded` and `network` the message starts
/// with `<reason>: `.
pub const REFUSAL_REASONS: [&str; 15] = [
    "declined",
    "revoked",
    "not-granted",
    "scope",
    "policy",
    "offline",
    "private-address",
    "reserved",
    "downgrade",
    "not-user-initiated",
    "file-type",
    "unavailable",
    "response-size",
    "stream-size",
    "tls",
];

/// The `error-code` of a refusal reason word (reference-errors-and-limits.md "Refusal reasons").
pub fn refusal_reason_code(reason: &str) -> Option<ErrorCode> {
    match reason {
        "response-size" | "stream-size" => Some(ErrorCode::QuotaExceeded),
        "tls" => Some(ErrorCode::Network),
        reason if REFUSAL_REASONS.contains(&reason) => Some(ErrorCode::NotPermitted),
        _ => None,
    }
}

/// A name that is not one of the closed set of a `FromStr` enum of this crate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {kind} name {name:?}")]
pub struct UnknownName {
    /// What was parsed, for example `tier`.
    pub kind: &'static str,
    /// The text that matched no name.
    pub name: String,
}

impl UnknownName {
    /// A new error for `kind` and the offending `name`.
    pub fn new(kind: &'static str, name: &str) -> UnknownName {
        UnknownName {
            kind,
            name: name.to_owned(),
        }
    }
}

impl PartialEq<str> for Code {
    /// Compares the code text with a literal (reference-errors-and-limits.md).
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Code {
    /// Compares the code text with a literal (reference-errors-and-limits.md).
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: DEV-023
    #[test]
    fn severity_all_lists_every_severity_most_severe_first() {
        assert_eq!(
            Severity::ALL,
            [Severity::Error, Severity::Warning, Severity::Info]
        );
    }

    /// REQ: DEV-023
    #[test]
    fn severity_as_str_uses_the_documented_names() {
        assert_eq!(Severity::Error.as_str(), "error");
        assert_eq!(Severity::Warning.as_str(), "warn");
        assert_eq!(Severity::Info.as_str(), "info");
    }

    /// REQ: DEV-023
    #[test]
    fn severity_display_matches_as_str_for_every_severity() {
        for severity in Severity::ALL {
            assert_eq!(format!("{severity}"), severity.as_str());
        }
    }

    /// REQ: DEV-023
    #[test]
    fn severity_from_str_parses_every_documented_name() {
        assert_eq!("error".parse::<Severity>(), Ok(Severity::Error));
        assert_eq!("warn".parse::<Severity>(), Ok(Severity::Warning));
        assert_eq!("info".parse::<Severity>(), Ok(Severity::Info));
    }

    /// REQ: DEV-023
    #[test]
    fn severity_from_str_rejects_the_long_warning_name() {
        assert_eq!(
            "warning".parse::<Severity>(),
            Err(UnknownName::new("severity", "warning"))
        );
    }

    /// REQ: DEV-023
    #[test]
    fn severity_from_str_rejects_empty_and_wrong_case_text() {
        assert_eq!(
            "".parse::<Severity>(),
            Err(UnknownName::new("severity", ""))
        );
        assert_eq!(
            "Error".parse::<Severity>(),
            Err(UnknownName::new("severity", "Error"))
        );
    }

    /// REQ: DEV-023
    #[test]
    fn severity_text_round_trips_through_from_str() {
        for severity in Severity::ALL {
            assert_eq!(severity.as_str().parse::<Severity>(), Ok(severity));
        }
    }

    /// REQ: API-081
    #[test]
    fn code_from_static_keeps_the_text_in_as_str_and_display() {
        let code = Code::from_static("manifest.grant-side");
        assert_eq!(code.as_str(), "manifest.grant-side");
        assert_eq!(format!("{code}"), "manifest.grant-side");
    }

    /// REQ: API-081
    #[test]
    fn code_namespace_is_the_part_before_the_first_dot() {
        assert_eq!(
            Code::from_static("manifest.grant-side").namespace(),
            "manifest"
        );
        assert_eq!(Code::from_static("trap.deadline").namespace(), "trap");
        assert_eq!(
            Code::from_static("extension.vetoed.sub").namespace(),
            "extension"
        );
    }

    /// REQ: API-081
    #[test]
    fn code_namespace_of_empty_or_leading_dot_text_is_empty() {
        assert_eq!(Code::from_static("").namespace(), "");
        assert_eq!(Code::from_static(".deadline").namespace(), "");
    }

    /// REQ: EXT-003
    #[test]
    fn code_base_maps_extension_codes_to_their_documented_base_codes() {
        assert_eq!(
            Code::from_static("extension.vetoed").base(),
            ErrorCode::Conflict
        );
        assert_eq!(
            Code::from_static("extension.not-granted").base(),
            ErrorCode::NotPermitted
        );
        assert_eq!(
            Code::from_static("extension.unknown-point").base(),
            ErrorCode::NotFound
        );
        assert_eq!(
            Code::from_static("extension.schema-mismatch").base(),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            Code::from_static("extension.not-dynamic").base(),
            ErrorCode::Unsupported
        );
        assert_eq!(
            Code::from_static("extension.reentrant").base(),
            ErrorCode::Conflict
        );
        assert_eq!(
            Code::from_static("extension.depth-exceeded").base(),
            ErrorCode::Conflict
        );
        assert_eq!(
            Code::from_static("extension.entry-rejected").base(),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: API-081
    #[test]
    fn code_compares_equal_to_its_text_as_str_and_str_literals() {
        let code = Code::from_static("trap.deadline");
        assert!(code == *"trap.deadline");
        assert!(code == "trap.deadline");
        assert!(code != "trap.memory");
    }

    /// REQ: API-001
    #[test]
    fn message_max_chars_is_256() {
        assert_eq!(MESSAGE_MAX_CHARS, 256);
    }

    /// REQ: API-001
    #[test]
    fn plugin_error_new_keeps_a_short_message_and_the_code() {
        let error = PluginError::new(ErrorCode::NotFound, "pane 4 is gone");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(error.message, "pane 4 is gone");
    }

    /// REQ: API-003
    #[test]
    fn plugin_error_new_leaves_retry_after_ms_unset() {
        let error = PluginError::new(ErrorCode::Busy, "queue full");
        assert_eq!(error.retry_after_ms, None);
    }

    /// REQ: API-001
    #[test]
    fn plugin_error_new_accepts_an_empty_message() {
        let error = PluginError::new(ErrorCode::Internal, "");
        assert_eq!(error.message, "");
    }

    /// REQ: API-001
    #[test]
    fn plugin_error_new_keeps_a_message_of_exactly_256_characters() {
        let message = "a".repeat(256);
        let error = PluginError::new(ErrorCode::Io, message.as_str());
        assert_eq!(error.message, message);
    }

    /// REQ: API-001
    #[test]
    fn plugin_error_new_cuts_a_message_over_256_characters() {
        let message = "a".repeat(300);
        let error = PluginError::new(ErrorCode::Io, message.as_str());
        assert_eq!(error.message.chars().count(), 256);
        assert_eq!(error.message, "a".repeat(256));
    }

    /// REQ: API-001
    #[test]
    fn plugin_error_new_counts_characters_not_bytes_when_cutting() {
        let message = "é".repeat(300);
        let error = PluginError::new(ErrorCode::Io, message.as_str());
        assert_eq!(error.message.chars().count(), 256);
        assert_eq!(error.message, "é".repeat(256));
    }

    /// REQ: EXT-003
    #[test]
    fn with_prefix_starts_the_message_with_the_extension_code_colon_and_space() {
        let error = PluginError::with_prefix(
            ErrorCode::Conflict,
            &Code::from_static("extension.vetoed"),
            "quiet hours are on",
        );
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(error.message, "extension.vetoed: quiet hours are on");
    }

    /// REQ: EXT-003
    #[test]
    fn with_prefix_with_an_empty_message_keeps_only_the_prefix() {
        let error = PluginError::with_prefix(
            ErrorCode::Conflict,
            &Code::from_static("extension.vetoed"),
            "",
        );
        assert_eq!(error.message, "extension.vetoed: ");
    }

    /// REQ: API-001
    #[test]
    fn with_prefix_caps_the_whole_message_at_256_characters() {
        let long_text = "b".repeat(300);
        let error = PluginError::with_prefix(
            ErrorCode::Conflict,
            &Code::from_static("extension.vetoed"),
            long_text.as_str(),
        );
        assert_eq!(error.message.chars().count(), 256);
        assert!(error.message.starts_with("extension.vetoed: "));
    }

    /// REQ: EXT-003
    #[test]
    fn extension_code_reads_the_prefix_written_by_with_prefix() {
        let error = PluginError::with_prefix(
            ErrorCode::Conflict,
            &Code::from_static("extension.vetoed"),
            "quiet hours are on",
        );
        assert_eq!(
            error.extension_code(),
            Some(Code::from_static("extension.vetoed"))
        );
    }

    /// REQ: EXT-003
    #[test]
    fn extension_code_reads_only_the_leading_code_before_the_plugin_id() {
        let error = PluginError::new(
            ErrorCode::Conflict,
            "extension.vetoed: com.example.quiet-hours: quiet hours are on",
        );
        assert_eq!(
            error.extension_code(),
            Some(Code::from_static("extension.vetoed"))
        );
    }

    /// REQ: EXT-003
    #[test]
    fn extension_code_is_none_when_the_message_has_no_prefix() {
        let error = PluginError::new(ErrorCode::NotFound, "pane 4 is gone");
        assert_eq!(error.extension_code(), None);
    }

    /// REQ: EXT-003
    #[test]
    fn extension_code_is_none_for_an_empty_message() {
        let error = PluginError::new(ErrorCode::Internal, "");
        assert_eq!(error.extension_code(), None);
    }

    /// REQ: EXT-003
    #[test]
    fn extension_code_is_none_when_the_colon_is_not_followed_by_a_space() {
        let error = PluginError::new(ErrorCode::Conflict, "extension.vetoed:quiet");
        assert_eq!(error.extension_code(), None);
    }

    /// REQ: EXT-003
    #[test]
    fn extension_code_is_none_when_the_code_is_not_at_the_start() {
        let error = PluginError::new(ErrorCode::Conflict, "failed on extension.vetoed: quiet");
        assert_eq!(error.extension_code(), None);
    }

    /// REQ: API-081
    #[test]
    fn extension_code_is_none_for_a_prefix_without_a_namespace_dot() {
        let error = PluginError::new(ErrorCode::Internal, "internal: failure");
        assert_eq!(error.extension_code(), None);
    }

    /// REQ: EXT-003
    #[test]
    fn registry_maps_extension_codes_to_their_documented_base_codes() {
        let base_of = |wanted: &str| {
            registry()
                .iter()
                .find(|info| info.code == wanted)
                .map(|info| info.base)
        };
        assert_eq!(base_of("extension.vetoed"), Some(ErrorCode::Conflict));
        assert_eq!(
            base_of("extension.schema-mismatch"),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            base_of("extension.unknown-point"),
            Some(ErrorCode::NotFound)
        );
    }

    /// REQ: API-002
    #[test]
    fn registry_codes_are_unique_namespaced_and_name_a_page() {
        let mut seen = std::collections::HashSet::new();
        for info in registry() {
            assert!(
                seen.insert(info.code),
                "duplicate registry code {}",
                info.code
            );
            assert!(
                info.code.contains('.'),
                "code {} is not namespaced",
                info.code
            );
            assert!(!info.page.is_empty(), "code {} names no page", info.code);
        }
        assert!(!seen.is_empty());
    }

    /// REQ: API-002
    #[test]
    fn refusal_reasons_are_the_closed_list_in_table_order() {
        assert_eq!(
            REFUSAL_REASONS,
            [
                "declined",
                "revoked",
                "not-granted",
                "scope",
                "policy",
                "offline",
                "private-address",
                "reserved",
                "downgrade",
                "not-user-initiated",
                "file-type",
                "unavailable",
                "response-size",
                "stream-size",
                "tls",
            ]
        );
    }

    /// REQ: API-002
    #[test]
    fn refusal_reason_code_maps_the_not_permitted_reasons() {
        for reason in [
            "declined",
            "revoked",
            "not-granted",
            "scope",
            "policy",
            "offline",
            "private-address",
            "reserved",
            "downgrade",
            "not-user-initiated",
            "file-type",
            "unavailable",
        ] {
            assert_eq!(
                refusal_reason_code(reason),
                Some(ErrorCode::NotPermitted),
                "reason {reason}"
            );
        }
    }

    /// REQ: API-002
    #[test]
    fn refusal_reason_code_maps_the_quota_and_network_reasons() {
        assert_eq!(
            refusal_reason_code("response-size"),
            Some(ErrorCode::QuotaExceeded)
        );
        assert_eq!(
            refusal_reason_code("stream-size"),
            Some(ErrorCode::QuotaExceeded)
        );
        assert_eq!(refusal_reason_code("tls"), Some(ErrorCode::Network));
    }

    /// REQ: API-002
    #[test]
    fn refusal_reason_code_is_none_for_unknown_empty_or_wrong_case_words() {
        assert_eq!(refusal_reason_code("bogus"), None);
        assert_eq!(refusal_reason_code(""), None);
        assert_eq!(refusal_reason_code("Declined"), None);
    }

    /// REQ: API-002
    #[test]
    fn unknown_name_stores_the_kind_and_the_name() {
        let error = UnknownName::new("tier", "gold");
        assert_eq!(error.kind, "tier");
        assert_eq!(error.name, "gold");
    }

    /// REQ: API-002
    #[test]
    fn unknown_name_display_quotes_the_offending_name() {
        let error = UnknownName::new("severity", "warning");
        assert_eq!(format!("{error}"), r#"unknown severity name "warning""#);
    }

    /// REQ: API-002
    #[test]
    fn unknown_name_display_handles_an_empty_name() {
        let error = UnknownName::new("tier", "");
        assert_eq!(format!("{error}"), r#"unknown tier name """#);
    }
}
