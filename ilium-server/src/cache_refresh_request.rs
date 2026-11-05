//! Provider-specific preparation of zero-output cache refresh requests.
//!
//! This module neither captures credentials nor sends requests. A caller must
//! preserve the original account, endpoint, headers and exact cached prefix,
//! and must separately establish that the original request created a cache.

use std::collections::HashSet;
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{value::RawValue, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeRefreshError {
    NotObject,
    MissingModelOrMessages,
    ExtendedThinking,
    StructuredOutput,
    ForcedTool,
    Compaction,
    InvalidJson,
}

/// Preserve every original top-level value, including nested object ordering,
/// string escapes and whitespace, rather than reserializing prompt content.
struct RawFields(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for RawFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FieldsVisitor;
        impl<'de> Visitor<'de> for FieldsVisitor {
            type Value = RawFields;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object without duplicate keys")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawFields, A::Error> {
                let mut keys = HashSet::new();
                let mut fields = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    if !keys.insert(key.clone()) {
                        return Err(serde::de::Error::custom("duplicate request field"));
                    }
                    fields.push((key, value));
                }
                Ok(RawFields(fields))
            }
        }
        deserializer.deserialize_map(FieldsVisitor)
    }
}

/// Prepare a Claude Messages request for a zero-output refresh.
///
/// Anthropic documents these exclusions and requires the original headers:
/// https://platform.claude.com/docs/en/about-claude/models/optimizing-for-cost-and-intelligence
/// https://platform.claude.com/docs/en/build-with-claude/prompt-caching
///
/// Returned bytes retain the raw original values in their original order;
/// the caller must still preserve the account, endpoint, and headers.
pub(crate) fn claude_refresh_body(original: &str) -> Result<String, ClaudeRefreshError> {
    let parsed: Value = serde_json::from_str(original).map_err(|_| ClaudeRefreshError::InvalidJson)?;
    let source = parsed.as_object().ok_or(ClaudeRefreshError::NotObject)?;
    let fields: RawFields = serde_json::from_str(original).map_err(|_| ClaudeRefreshError::InvalidJson)?;
    if source.get("model").and_then(Value::as_str).filter(|s| !s.is_empty()).is_none()
        || !source.get("messages").is_some_and(Value::is_array)
    {
        return Err(ClaudeRefreshError::MissingModelOrMessages);
    }
    if source.get("thinking").and_then(|v| v.get("type")).and_then(Value::as_str) == Some("enabled") {
        return Err(ClaudeRefreshError::ExtendedThinking);
    }
    if source.get("output_config").and_then(|v| v.get("format")).is_some_and(|v| !v.is_null()) {
        return Err(ClaudeRefreshError::StructuredOutput);
    }
    if matches!(source.get("tool_choice").and_then(|v| v.get("type")).and_then(Value::as_str), Some("tool" | "any")) {
        return Err(ClaudeRefreshError::ForcedTool);
    }
    if source.get("compaction").is_some_and(|v| !v.is_null()) {
        return Err(ClaudeRefreshError::Compaction);
    }
    let mut body = String::from("{");
    for (key, value) in fields.0 {
        if key == "stream" || key == "max_tokens" {
            continue;
        }
        if body.len() > 1 {
            body.push(',');
        }
        // JSON string serialization for a String cannot fail.
        body.push_str(&serde_json::to_string(&key).map_err(|_| ClaudeRefreshError::InvalidJson)?);
        body.push(':');
        body.push_str(value.get());
    }
    if body.len() > 1 {
        body.push(',');
    }
    body.push_str("\"max_tokens\":0}");
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_request_content_and_only_changes_output_budget_and_streaming() {
        let original = json!({"model":"claude-example","max_tokens":900,"stream":true,
            "thinking":{"type":"adaptive"},"system":[{"type":"text","text":"prefix","cache_control":{"type":"ephemeral"}}],
            "messages":[{"role":"user","content":"hello"}],"metadata":{"user_id":"example"}});
        let mut expected = original.clone();
        expected["max_tokens"] = json!(0);
        expected.as_object_mut().unwrap().remove("stream");
        let prepared = claude_refresh_body(&original.to_string()).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&prepared).unwrap(), expected);
        assert_eq!(original["max_tokens"], 900);
    }

    #[test]
    fn rejects_documented_incompatible_output_modes() {
        let base = json!({"model":"claude-example","messages":[]});
        for (field, value, expected) in [
            ("thinking", json!({"type":"enabled"}), ClaudeRefreshError::ExtendedThinking),
            ("output_config", json!({"format":{"type":"json_schema"}}), ClaudeRefreshError::StructuredOutput),
            ("tool_choice", json!({"type":"tool","name":"x"}), ClaudeRefreshError::ForcedTool),
            ("tool_choice", json!({"type":"any"}), ClaudeRefreshError::ForcedTool),
            ("compaction", json!({}), ClaudeRefreshError::Compaction),
        ] {
            let mut request = base.clone();
            request[field] = value;
            assert_eq!(claude_refresh_body(&request.to_string()), Err(expected));
        }
        assert_eq!(claude_refresh_body("[]"), Err(ClaudeRefreshError::NotObject));
        assert_eq!(claude_refresh_body("{}"), Err(ClaudeRefreshError::MissingModelOrMessages));
    }

    #[test]
    fn retains_raw_nested_values_and_rejects_ambiguous_duplicate_fields() {
        let request = r#"{ "model":"claude-example", "messages" : [{"role":"user", "content":"a\\u0062"}], "tools":[{"z":1,"a":2}], "max_tokens":200,"stream":true }"#;
        let prepared = claude_refresh_body(request).unwrap();
        assert!(prepared.contains(r#"[{"role":"user", "content":"a\\u0062"}]"#));
        assert!(prepared.contains(r#"[{"z":1,"a":2}]"#));
        assert_eq!(claude_refresh_body(r#"{"model":"a","model":"b","messages":[]}"#), Err(ClaudeRefreshError::InvalidJson));
    }
}
