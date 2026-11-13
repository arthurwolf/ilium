//! Credential redaction of diagnostic JSON, a copy of the pure helpers in the diagnostics plugin (a plugin
//! cannot depend on another plugin, and the credential registry is the same single list).

const CREDENTIAL_NAMES: &[&str] = &[
    "apikey",
    "authorization",
    "proxyauthorization",
    "password",
    "secret",
    "clientsecret",
    "accesstoken",
    "refreshtoken",
    "idtoken",
    "authtoken",
    "token",
    "cookie",
    "setcookie",
    "xapikey",
    "xgoogapikey",
    "xauthtoken",
    "xaccesstoken",
];

/// True when `name` itself names a credential in any spelling (`api_key`, `api-key`, `apiKey`, `API key`).
pub fn is_credential_name(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect();
    CREDENTIAL_NAMES.contains(&normalized.as_str())
}

/// Removes URL user-info, every query value, and any fragment before an

/// Redacts credential-named fields, path-addressed settings writes and stringified tool arguments/results.
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
