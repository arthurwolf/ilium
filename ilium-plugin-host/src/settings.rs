//! Pure settings schema checks and frozen kernel-tab identities.
//! See reference-settings-schema.md and api-settings.md. Persistence, merge-patch
//! application, scope filtering and optional guest validation belong to the caller.

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::manifest::Diagnostic;
use ilium_plugin_api::Code;
use serde_json::Value;

pub const KERNEL_TAB_COUNT: usize = 25;

/// Frozen rail order (reference-settings-schema.md, Built-in tab ids).
pub fn kernel_tab_ids() -> &'static [&'static str] {
    &[
        "user-interface",
        "icons",
        "agent-monitoring",
        "agent-cost",
        "optimization",
        "remote-compaction",
        "keyboard",
        "terminal",
        "editor",
        "session",
        "git",
        "kanban-board",
        "animations",
        "sound",
        "voice-control",
        "reset-planning",
        "inference",
        "llm-instructions",
        "titles",
        "triggers",
        "text-triggers",
        "debug",
        "api",
        "about",
        "setup",
    ]
}

/// Checks a complete candidate object against the compiled row-schema subset.
/// Does not apply defaults or merge a patch: null-reset, side ownership and atomic
/// persistence require the caller's stored state and section ownership metadata.
/// Unknown properties are refused unless an explicit map-value schema permits them.
pub fn validate_settings(schema_json: &str, value_json: &str) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let schema = match serde_json::from_str::<Value>(schema_json) {
        Ok(schema) => schema,
        Err(_) => {
            return Err(vec![finding(
                "manifest.schema",
                "",
                "Settings schema is not valid JSON",
            )]);
        }
    };
    if value_json.len() > 256 * 1024 {
        return Err(vec![finding(
            "invalid-argument",
            "",
            "Settings values exceed 256 KiB",
        )]);
    }
    let value = match serde_json::from_str::<Value>(value_json) {
        Ok(value) => value,
        Err(_) => {
            return Err(vec![finding(
                "invalid-argument",
                "",
                "Settings values are not valid JSON",
            )]);
        }
    };
    validate_schema(&schema, "", 0, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    if schema.get("type").and_then(Value::as_str) != Some("object") || !value.is_object() {
        return Err(vec![finding(
            "invalid-argument",
            "",
            "Settings schema and values must be objects",
        )]);
    }
    validate_value(&schema, &value, "", &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Tests the effective grant list only. The caller must have validated package
/// tier and exclusive tab ownership, and must pass current, unrevoked grants.
pub fn may_write_kernel_tab(granted: &[Grant], tab: &str) -> bool {
    kernel_tab_ids().contains(&tab)
        && granted
            .iter()
            .any(|grant| grant.family() == "settings.write-kernel" && grant.argument() == Some(tab))
}

fn finding(code: &'static str, pointer: &str, message: &str) -> Diagnostic {
    Diagnostic::error(Code::from_static(code), pointer, message)
}

fn child_pointer(pointer: &str, key: &str) -> String {
    format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn validate_schema(schema: &Value, pointer: &str, depth: usize, errors: &mut Vec<Diagnostic>) {
    let Some(object) = schema.as_object() else {
        errors.push(finding(
            "manifest.schema",
            pointer,
            "A settings schema must be an object",
        ));
        return;
    };
    if depth > 32 {
        errors.push(finding(
            "manifest.schema",
            pointer,
            "Settings schema nesting exceeds 32",
        ));
        return;
    }
    const KEYWORDS: &[&str] = &[
        "type",
        "properties",
        "additionalProperties",
        "enum",
        "enum_titles",
        "items",
        "maxItems",
        "title",
        "help",
        "description",
        "default",
        "minimum",
        "maximum",
        "step",
        "unit",
        "pattern",
        "maxLength",
        "required",
        "secret",
        "multiline",
        "picker",
        "extensions",
        "x-visible-when",
        "x-scope",
        "x-applies",
        "x-steps",
        "x-special-values",
        "x-widget",
        "propertyNames",
        "maxProperties",
        "uniqueItems",
        "minItems",
    ];
    for key in object.keys() {
        if !KEYWORDS.contains(&key.as_str()) {
            errors.push(finding(
                "manifest.schema-keyword",
                &child_pointer(pointer, key),
                "Unsupported settings schema keyword",
            ));
        }
    }
    let kind = schema_kind(schema);
    if !matches!(
        kind,
        Some("object" | "boolean" | "integer" | "number" | "string" | "array")
    ) {
        errors.push(finding(
            "manifest.schema",
            pointer,
            "A supported settings schema type is required",
        ));
    }
    if let (Some(minimum), Some(maximum)) = (
        schema.get("minimum").and_then(Value::as_f64),
        schema.get("maximum").and_then(Value::as_f64),
    ) {
        if minimum > maximum {
            errors.push(finding(
                "manifest.schema",
                pointer,
                "Minimum exceeds maximum",
            ));
        }
    }
    for key in ["minimum", "maximum", "step"] {
        if let Some(bound) = schema.get(key) {
            if !bound.is_number() || !matches!(kind, Some("integer" | "number")) {
                errors.push(finding(
                    "manifest.schema",
                    &child_pointer(pointer, key),
                    "Numeric keyword requires a numeric value and type",
                ));
            }
        }
    }
    for (key, expected_kind) in [
        ("maxLength", "string"),
        ("maxItems", "array"),
        ("minItems", "array"),
        ("maxProperties", "object"),
    ] {
        if let Some(bound) = schema.get(key) {
            if bound.as_u64().is_none() || kind != Some(expected_kind) {
                errors.push(finding(
                    "manifest.schema",
                    &child_pointer(pointer, key),
                    "Length keyword requires a nonnegative integer and matching type",
                ));
            }
        }
    }
    if let Some(pattern) = schema.get("pattern") {
        if kind != Some("string")
            || pattern
                .as_str()
                .is_none_or(|pattern| regex::Regex::new(pattern).is_err())
        {
            errors.push(finding(
                "manifest.schema",
                &child_pointer(pointer, "pattern"),
                "Pattern must be a supported regular expression on a string",
            ));
        }
    }
    if let Some(values) = schema.get("enum") {
        if !matches!(kind, Some("string" | "integer"))
            || values.as_array().is_none_or(Vec::is_empty)
        {
            errors.push(finding(
                "manifest.schema",
                &child_pointer(pointer, "enum"),
                "Enum requires a nonempty array on a string or integer",
            ));
        } else if let Some(values) = values.as_array() {
            for value in values {
                if !has_type(value, kind.unwrap_or_default()) {
                    errors.push(finding(
                        "manifest.schema",
                        &child_pointer(pointer, "enum"),
                        "Enum values must match the declared type",
                    ));
                    break;
                }
            }
            if schema.get("enum_titles").is_some_and(|titles| {
                titles.as_array().is_none_or(|titles| {
                    titles.len() != values.len() || titles.iter().any(|title| !title.is_string())
                })
            }) {
                errors.push(finding(
                    "manifest.schema",
                    &child_pointer(pointer, "enum_titles"),
                    "Enum titles must match the enum length and contain strings",
                ));
            }
        }
    }
    if let Some(properties) = schema.get("properties") {
        if kind != Some("object") || !properties.is_object() {
            errors.push(finding(
                "manifest.schema",
                &child_pointer(pointer, "properties"),
                "Properties require an object schema",
            ));
        } else if let Some(properties) = properties.as_object() {
            for (key, property) in properties {
                validate_schema(
                    property,
                    &child_pointer(&child_pointer(pointer, "properties"), key),
                    depth + 1,
                    errors,
                );
            }
        }
    }
    if let Some(required) = schema.get("required") {
        if kind != Some("object")
            || required.as_array().is_none_or(|keys| {
                keys.iter().any(|key| {
                    !key.is_string()
                        || schema
                            .get("properties")
                            .and_then(|properties| properties.get(key.as_str().unwrap_or_default()))
                            .is_none()
                })
            })
        {
            errors.push(finding(
                "manifest.schema",
                &child_pointer(pointer, "required"),
                "Required must name declared object properties",
            ));
        }
    }
    if let Some(additional) = schema.get("additionalProperties") {
        if kind != Some("object") || (!additional.is_object() && additional != &Value::Bool(false))
        {
            errors.push(finding(
                "manifest.schema",
                &child_pointer(pointer, "additionalProperties"),
                "Additional properties require a value schema or false",
            ));
        } else if additional.is_object() {
            validate_schema(
                additional,
                &child_pointer(pointer, "additionalProperties"),
                depth + 1,
                errors,
            );
        }
    }
    if let Some(names) = schema.get("propertyNames") {
        if kind != Some("object") {
            errors.push(finding(
                "manifest.schema",
                pointer,
                "Property names require an object schema",
            ));
        } else {
            let mut names = names.clone();
            if let Some(names) = names.as_object_mut() {
                names
                    .entry("type")
                    .or_insert(Value::String("string".into()));
            }
            validate_schema(
                &names,
                &child_pointer(pointer, "propertyNames"),
                depth + 1,
                errors,
            );
        }
    }
    if let Some(unique) = schema.get("uniqueItems") {
        if kind != Some("array") || !unique.is_boolean() {
            errors.push(finding(
                "manifest.schema",
                pointer,
                "Unique items requires an array schema and boolean",
            ));
        }
    }
    if let Some(items) = schema.get("items") {
        if kind != Some("array") {
            errors.push(finding(
                "manifest.schema",
                &child_pointer(pointer, "items"),
                "Items require an array schema",
            ));
        } else {
            validate_schema(items, &child_pointer(pointer, "items"), depth + 1, errors);
        }
    } else if kind == Some("array") {
        errors.push(finding(
            "manifest.schema",
            pointer,
            "Array settings require an items schema",
        ));
    }
}

fn schema_kind(schema: &Value) -> Option<&str> {
    schema.get("type").and_then(Value::as_str).or_else(|| {
        schema
            .get("enum")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(|value| {
                if value.is_string() {
                    Some("string")
                } else if has_type(value, "integer") {
                    Some("integer")
                } else {
                    None
                }
            })
    })
}

fn has_type(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => {
            value.is_i64()
                || value.is_u64()
                || value.as_f64().is_some_and(|number| number.fract() == 0.0)
        }
        _ => false,
    }
}

fn validate_value(schema: &Value, value: &Value, pointer: &str, errors: &mut Vec<Diagnostic>) {
    if !has_type(value, schema_kind(schema).unwrap_or_default()) {
        errors.push(finding(
            "invalid-argument",
            pointer,
            "Settings value has the wrong type",
        ));
        return;
    }
    let mut invalid = false;
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        invalid |= !values.iter().any(|allowed| {
            allowed == value
                || (allowed.is_number()
                    && value.is_number()
                    && allowed.as_f64() == value.as_f64()
                    && allowed
                        .as_f64()
                        .is_some_and(|number| number.abs() <= 9_007_199_254_740_991.0))
        });
    }
    if let Some(number) = value.as_f64() {
        invalid |= schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|minimum| number < minimum);
        invalid |= schema
            .get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|maximum| number > maximum);
    }
    if let Some(text) = value.as_str() {
        invalid |= schema
            .get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|limit| text.chars().count() as u64 > limit);
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            invalid |= !regex::Regex::new(pattern).is_ok_and(|pattern| pattern.is_match(text));
        }
    }
    if let Some(items) = value.as_array() {
        invalid |= schema
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|limit| (items.len() as u64) < limit);
        if schema.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
            invalid |= items
                .iter()
                .enumerate()
                .any(|(index, item)| items[..index].iter().any(|previous| previous == item));
        }
        invalid |= schema
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|limit| items.len() as u64 > limit);
        for (index, item) in items.iter().enumerate() {
            validate_value(
                &schema["items"],
                item,
                &child_pointer(pointer, &index.to_string()),
                errors,
            );
        }
    }
    if let Some(values) = value.as_object() {
        invalid |= schema
            .get("maxProperties")
            .and_then(Value::as_u64)
            .is_some_and(|limit| values.len() as u64 > limit);
        for (key, value) in values {
            let path = child_pointer(pointer, key);
            if let Some(property) = schema
                .get("properties")
                .and_then(|properties| properties.get(key))
            {
                validate_value(property, value, &path, errors);
            } else if let Some(additional) = schema
                .get("additionalProperties")
                .filter(|additional| additional.is_object())
            {
                validate_value(additional, value, &path, errors);
            } else {
                errors.push(finding("invalid-argument", &path, "Unknown settings key"));
            }
            if let Some(names) = schema.get("propertyNames") {
                let mut names = names.clone();
                if let Some(names) = names.as_object_mut() {
                    names
                        .entry("type")
                        .or_insert(Value::String("string".into()));
                }
                validate_value(&names, &Value::String(key.clone()), &path, errors);
            }
        }
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !values.contains_key(key) {
                    errors.push(finding(
                        "invalid-argument",
                        &child_pointer(pointer, key),
                        "Required settings value is missing",
                    ));
                }
            }
        }
    }
    if invalid {
        errors.push(finding(
            "invalid-argument",
            pointer,
            "Settings value violates its schema",
        ));
    }
}
