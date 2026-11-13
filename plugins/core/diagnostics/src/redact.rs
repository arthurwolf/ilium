//! Credential redaction for diagnostic text: URLs, HTTP headers and JSON payloads.
//!
//! Pure string and JSON transforms moved from `ilium-logging` (the credential registry itself lives in
//! `server_debug` and is shared by every surface so a name is never redacted on one and logged on another).

use crate::server_debug::is_credential_name;
pub use crate::server_debug::redacted_diagnostic_field_value;

/// Removes URL user-info, every query value, and any fragment before an
/// endpoint enters a diagnostic event. Provider and proxy URLs are
/// user-configurable and may embed tokens in all three places even when the
/// normal built-in endpoints do not.
pub fn redacted_url(url: &str) -> String {
    // A fragment follows the query in a well-formed URL, so it has to come off
    // first -- and it has to come off at all, because a redirect URL carrying
    // `#access_token=...` has no `?` for the query branch below to catch.
    let (before_fragment, had_fragment) = match url.split_once('#') {
        Some((base, _fragment)) => (base, true),
        None => (url, false),
    };
    let (without_query, had_query) = match before_fragment.split_once('?') {
        Some((base, _query)) => (base, true),
        None => (before_fragment, false),
    };
    let mut redacted = redacted_user_info(without_query);
    if had_query {
        redacted.push_str("?<redacted>");
    }
    if had_fragment {
        redacted.push_str("#<redacted>");
    }
    redacted
}

/// Replaces a `user:password@` prefix on the authority of a URL whose query
/// and fragment have already been removed.
///
/// The authority is redacted whether or not a scheme is present. A
/// user-configured proxy is the one endpoint that routinely carries
/// credentials there, it is commonly written scheme-less
/// (`user:password@proxy.example:8080`), and a scheme-less proxy string is
/// also the shape most likely to be rejected and end up quoted verbatim in a
/// `GatewayError::InvalidProxy` message.
fn redacted_user_info(url_without_query: &str) -> String {
    let (scheme_prefix, authority_and_path) = match url_without_query.split_once("://") {
        Some((scheme, remainder)) => (format!("{scheme}://"), remainder),
        None => (String::new(), url_without_query),
    };
    // `/` is ASCII, so this byte index is always a character boundary and the
    // split below cannot land inside a multi-byte character.
    let authority_end = authority_and_path
        .find('/')
        .unwrap_or(authority_and_path.len());
    let (authority, path) = authority_and_path.split_at(authority_end);
    match authority.rsplit_once('@') {
        Some((_user_info, host)) => format!("{scheme_prefix}<redacted>@{host}{path}"),
        None => url_without_query.to_owned(),
    }
}

/// Redacts credential-bearing HTTP header values while leaving request IDs,
/// rate-limit state, content metadata, and other debugging headers intact.
pub fn redacted_header_value(name: &str, value: &str) -> String {
    if is_credential_name(name) {
        "<redacted>".to_owned()
    } else {
        value.to_owned()
    }
}

/// Keeps only response headers that help diagnose payload shape, request
/// correlation, throttling, or retry behavior. Browser-security policy
/// headers can be many kilobytes long and add no value to an API-call log.
pub fn is_diagnostic_response_header(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "content-type"
            | "content-length"
            | "retry-after"
            | "request-id"
            | "x-request-id"
            | "openai-request-id"
            | "cf-ray"
    ) || normalized.starts_with("x-ratelimit-")
}

/// Reads only the simple `[debug] file_logging_enabled = <bool>` contract from configuration text, so an
/// enabled diagnostic file can capture errors in unrelated malformed sections. The host reads the file and
/// passes its text in.
pub fn file_logging_enabled_hint(contents: &str) -> Option<bool> {
    let mut is_debug_section = false;
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            is_debug_section = line == "[debug]";
            continue;
        }
        if !is_debug_section || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "file_logging_enabled" {
            continue;
        }
        let value = raw_value.split('#').next().unwrap_or_default().trim();
        return match value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
    }
    None
}

/// ordinary fields, path-addressed settings writes, and stringified tool
/// arguments/results. Prompts and non-secret tool data remain complete.
pub fn redacted_json_credentials(value: &serde_json::Value) -> serde_json::Value {
    let mut redacted = value.clone();
    redact_json_credentials_in_place(&mut redacted);
    redacted
}

/// Redacts one structured diagnostic field only when its label is itself a
/// credential name. Agent lifecycle fields use a label/value representation,
/// so passing the enclosing JSON through [`redacted_json_credentials`] cannot
/// infer that a generic `value` belongs to an `api_key` label.

/// JSON has no reliable field boundary for selective redaction, so diagnostics
/// retain its size without risking disclosure of an unstructured credential.
pub fn redacted_json_string_credentials(value: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) else {
        return format!("<invalid JSON omitted: {} bytes>", value.len());
    };
    redacted_json_credentials(&parsed).to_string()
}

fn redact_json_credentials_in_place(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            // Settings tools address fields through a separate `path`, so the
            // generic `value` key becomes secret only in this object shape.
            let targets_credential_path = object
                .get("path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_credential_path);
            if targets_credential_path && object.contains_key("value") {
                object.insert(
                    "value".to_owned(),
                    serde_json::Value::String("<redacted>".to_owned()),
                );
            }

            for (field, nested_value) in object {
                if is_credential_name(field) {
                    *nested_value = serde_json::Value::String("<redacted>".to_owned());
                    continue;
                }
                // Realtime tool arguments and outputs are JSON encoded inside
                // protocol strings, so recurse through that second envelope.
                if matches!(field.as_str(), "arguments" | "output" | "body" | "result") {
                    if let Some(serialized) = nested_value.as_str() {
                        *nested_value =
                            serde_json::Value::String(redacted_json_string_credentials(serialized));
                        continue;
                    }
                }
                redact_json_credentials_in_place(nested_value);
            }
        }
        serde_json::Value::Array(values) => {
            for nested_value in values {
                redact_json_credentials_in_place(nested_value);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

fn is_credential_path(path: &str) -> bool {
    path.rsplit(['.', '/'])
        .next()
        .is_some_and(is_credential_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: PORT:ilium-logging/src/lib.rs::diagnostic_urls_remove_user_info_and_query_values
    #[test]
    fn diagnostic_urls_remove_user_info_and_query_values() {
        assert_eq!(
            redacted_url("https://user:secret@example.test/v1/chat?api_key=hidden&model=x"),
            "https://<redacted>@example.test/v1/chat?<redacted>"
        );
        assert_eq!(
            redacted_url("http://127.0.0.1:11434/api/tags"),
            "http://127.0.0.1:11434/api/tags"
        );
        // A rejected proxy string is quoted into the gateway's InvalidProxy error, and the shape most likely
        // to be rejected is the scheme-less one, which is also where a proxy password lives.
        assert_eq!(
            redacted_url("user:secret@proxy.example:8080"),
            "<redacted>@proxy.example:8080"
        );
        // A fragment carries no diagnostic value in an API endpoint and can carry an implicit-flow token.
        assert_eq!(
            redacted_url("https://example.test/callback#access_token=hidden"),
            "https://example.test/callback#<redacted>"
        );
    }

    /// REQ: PORT:ilium-logging/src/lib.rs::credential_headers_are_redacted_without_hiding_request_metadata
    #[test]
    fn credential_headers_are_redacted_without_hiding_request_metadata() {
        assert_eq!(
            redacted_header_value("Authorization", "Bearer secret"),
            "<redacted>"
        );
        assert_eq!(
            redacted_header_value("Set-Cookie", "session=secret"),
            "<redacted>"
        );
        assert_eq!(
            redacted_header_value("X-Goog-Api-Key", "secret"),
            "<redacted>"
        );
        assert_eq!(
            redacted_header_value("x-request-id", "request-42"),
            "request-42"
        );
        assert_eq!(redacted_header_value("Password", "secret"), "<redacted>");
        assert_eq!(
            redacted_header_value("x-ratelimit-remaining-tokens", "17"),
            "17"
        );
    }

    /// REQ: PORT:ilium-logging/src/lib.rs::diagnostic_response_headers_keep_correlation_and_rate_limit_data_only
    #[test]
    fn diagnostic_response_headers_keep_correlation_and_rate_limit_data_only() {
        assert!(is_diagnostic_response_header("Content-Type"));
        assert!(is_diagnostic_response_header("X-Request-ID"));
        assert!(is_diagnostic_response_header(
            "x-ratelimit-remaining-requests"
        ));
        assert!(!is_diagnostic_response_header("Content-Security-Policy"));
        assert!(!is_diagnostic_response_header("Permissions-Policy"));
    }

    /// REQ: PORT:ilium-logging/src/lib.rs::debug_setting_hint_survives_an_unrelated_malformed_section
    #[test]
    fn debug_setting_hint_survives_an_unrelated_malformed_section() {
        // The host reads the configuration file; the plugin interprets its text.
        let contents = "[debug]\nfile_logging_enabled = true # keep diagnostics\n\n[broken\n";
        assert_eq!(file_logging_enabled_hint(contents), Some(true));
        assert_eq!(file_logging_enabled_hint(""), None);
    }

    /// REQ: PORT:ilium-logging/src/lib.rs::diagnostic_json_redacts_direct_path_addressed_and_stringified_credentials
    #[test]
    fn diagnostic_json_redacts_direct_path_addressed_and_stringified_credentials() {
        let payload = serde_json::json!({
            "api_key": "direct-secret",
            "tool": {
                "arguments": "{\"path\":\"voice.api_key\",\"value\":\"path-secret\",\"action\":\"kept\"}"
            },
            "nested": [{"access-token": "nested-secret", "message": "complete text"}],
            "accessToken": "camel-secret",
            "clientSecret": "camel-client-secret",
            "max_tokens": 512,
        });

        let redacted = redacted_json_credentials(&payload);
        let diagnostic = redacted.to_string();

        assert!(!diagnostic.contains("direct-secret"));
        assert!(!diagnostic.contains("path-secret"));
        assert!(!diagnostic.contains("nested-secret"));
        assert!(!diagnostic.contains("camel-secret"));
        assert!(!diagnostic.contains("camel-client-secret"));
        assert!(diagnostic.contains("<redacted>"));
        assert!(diagnostic.contains("kept"));
        assert!(diagnostic.contains("complete text"));
        // A name that merely contains a credential word is not one: token budgets stay readable.
        assert!(diagnostic.contains("512"));
    }

    /// REQ: PORT:ilium-logging/src/lib.rs::labelled_diagnostic_fields_redact_credentials_but_keep_lifecycle_evidence
    #[test]
    fn labelled_diagnostic_fields_redact_credentials_but_keep_lifecycle_evidence() {
        assert_eq!(
            redacted_diagnostic_field_value("API key", "field-secret"),
            "<redacted>"
        );
        assert_eq!(
            redacted_diagnostic_field_value("submitted input", "/clear"),
            "/clear"
        );
        assert_eq!(
            redacted_diagnostic_field_value("session ID before invalidation", "session-old"),
            "session-old"
        );
    }

    /// REQ: PORT:ilium-logging/src/lib.rs::malformed_json_is_omitted_instead_of_returned_verbatim
    #[test]
    fn malformed_json_is_omitted_instead_of_returned_verbatim() {
        let input = "{\"api_key\":\"unclosed-secret";
        let diagnostic = redacted_json_string_credentials(input);

        assert_eq!(
            diagnostic,
            format!("<invalid JSON omitted: {} bytes>", input.len())
        );
        assert!(!diagnostic.contains("unclosed-secret"));
    }
}
