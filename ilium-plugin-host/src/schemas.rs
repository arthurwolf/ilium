//! Service schemas at run time (gap G13).
//!
//! A provider names a schema file per service in its manifest
//! (`contributes.services.provides[].schema`). The file maps each method to the schema of its
//! request (`params`, or `request`) and of its reply (`result`, or `response`):
//!
//! ```json
//! { "usage": { "params": { "type": "object" }, "result": { "type": "object" } } }
//! ```
//!
//! [`ServiceSchemas`] holds the files of one runtime; the service pump checks every request
//! before the provider sees it and every reply before the caller does. The validator takes a
//! subset of JSON Schema: `type`, `required`, `properties`, `additionalProperties`, `enum`,
//! `const`, `items`, `minimum`, `maximum`, `minLength` and `maxLength`. Other keywords are
//! ignored, so a schema written for a full validator never refuses more than it says.

use std::collections::BTreeMap;

use serde_json::Value;

/// Why a value does not satisfy a schema: the JSON pointer of the failing field and what is
/// wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaViolation {
    /// The JSON pointer (RFC 6901) of the failing value; empty for the root.
    pub pointer: String,
    /// What the value breaks.
    pub message: String,
}

impl std::fmt::Display for SchemaViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pointer = if self.pointer.is_empty() {
            "/"
        } else {
            &self.pointer
        };
        write!(formatter, "{pointer}: {}", self.message)
    }
}

/// One step of a JSON pointer, escaped (`~` as `~0`, `/` as `~1`).
fn pointer_step(pointer: &str, step: &str) -> String {
    format!("{pointer}/{}", step.replace('~', "~0").replace('/', "~1"))
}

fn violation(pointer: &str, message: impl Into<String>) -> SchemaViolation {
    SchemaViolation {
        pointer: pointer.to_owned(),
        message: message.into(),
    }
}

/// True when `value` has the JSON Schema type `name`.
fn has_type(value: &Value, name: &str) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => {
            value.is_i64()
                || value.is_u64()
                || value
                    .as_f64()
                    .is_some_and(|number| number.fract() == 0.0 && number.is_finite())
        }
        // An unknown type name constrains nothing.
        _ => true,
    }
}

/// Checks `value` against `schema`; the first violation found, depth first, in key order.
pub fn validate(schema: &Value, value: &Value) -> Result<(), SchemaViolation> {
    check(schema, value, "")
}

fn check(schema: &Value, value: &Value, pointer: &str) -> Result<(), SchemaViolation> {
    // `true`, `{}` and anything that is not an object accept every value.
    let Some(schema) = schema.as_object() else {
        return match schema {
            Value::Bool(false) => Err(violation(pointer, "no value is allowed here")),
            _ => Ok(()),
        };
    };
    match schema.get("type") {
        Some(Value::String(name)) if !has_type(value, name) => {
            return Err(violation(pointer, format!("expected {name}")));
        }
        Some(Value::Array(names)) => {
            let names: Vec<&str> = names.iter().filter_map(Value::as_str).collect();
            if !names.is_empty() && !names.iter().any(|name| has_type(value, name)) {
                return Err(violation(
                    pointer,
                    format!("expected one of {}", names.join(", ")),
                ));
            }
        }
        _ => {}
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            return Err(violation(pointer, "not one of the allowed values"));
        }
    }
    if let Some(expected) = schema.get("const") {
        if expected != value {
            return Err(violation(pointer, format!("expected {expected}")));
        }
    }
    if let Some(number) = value.as_f64() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
            if number < minimum {
                return Err(violation(pointer, format!("below the minimum {minimum}")));
            }
        }
        if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
            if number > maximum {
                return Err(violation(pointer, format!("above the maximum {maximum}")));
            }
        }
    }
    if let Some(text) = value.as_str() {
        let length = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
        if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64) {
            if length > maximum {
                return Err(violation(
                    pointer,
                    format!("longer than {maximum} characters"),
                ));
            }
        }
        if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64) {
            if length < minimum {
                return Err(violation(
                    pointer,
                    format!("shorter than {minimum} characters"),
                ));
            }
        }
    }
    if let Some(items) = value.as_array() {
        if let Some(item_schema) = schema.get("items") {
            for (index, item) in items.iter().enumerate() {
                check(
                    item_schema,
                    item,
                    &pointer_step(pointer, &index.to_string()),
                )?;
            }
        }
    }
    if let Some(object) = value.as_object() {
        for name in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !object.contains_key(name) {
                return Err(violation(
                    &pointer_step(pointer, name),
                    "required field missing",
                ));
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        let additional = schema.get("additionalProperties");
        for (name, field) in object {
            let at = pointer_step(pointer, name);
            match properties.and_then(|properties| properties.get(name)) {
                Some(field_schema) => check(field_schema, field, &at)?,
                None => match additional {
                    Some(Value::Bool(false)) => {
                        return Err(violation(&at, "field not allowed"));
                    }
                    Some(extra_schema @ Value::Object(_)) => check(extra_schema, field, &at)?,
                    _ => {}
                },
            }
        }
    }
    Ok(())
}

/// The request and reply schemas of one method.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MethodSchema {
    /// The schema of the call payload.
    pub request: Option<Value>,
    /// The schema of the reply.
    pub response: Option<Value>,
}

/// Which half of a call a schema checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The payload the caller sends.
    Request,
    /// The reply the provider gives.
    Response,
}

/// The service schemas one runtime knows, by service name.
#[derive(Debug, Default)]
pub struct ServiceSchemas {
    services: BTreeMap<String, ServiceSchema>,
}

/// One service's schema file: its text and the methods it describes.
#[derive(Debug)]
struct ServiceSchema {
    text: String,
    methods: BTreeMap<String, MethodSchema>,
}

/// Parses one schema file. The root must be an object of method entries.
pub fn parse_schema_file(text: &str) -> Result<BTreeMap<String, MethodSchema>, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|error| format!("not valid JSON: {error}"))?;
    let Some(methods) = root.as_object() else {
        return Err("the root is not an object of methods".to_owned());
    };
    let mut parsed = BTreeMap::new();
    for (method, entry) in methods {
        let Some(entry) = entry.as_object() else {
            return Err(format!("method {method} is not an object"));
        };
        let request = entry
            .get("params")
            .or_else(|| entry.get("request"))
            .cloned();
        let response = entry
            .get("result")
            .or_else(|| entry.get("response"))
            .cloned();
        parsed.insert(method.clone(), MethodSchema { request, response });
    }
    Ok(parsed)
}

impl ServiceSchemas {
    /// Installs the schema file of `service`, replacing an earlier one.
    pub fn install(&mut self, service: &str, text: &str) -> Result<(), String> {
        let methods = parse_schema_file(text)?;
        self.services.insert(
            service.to_owned(),
            ServiceSchema {
                text: text.to_owned(),
                methods,
            },
        );
        Ok(())
    }

    /// Whether `service` has a schema.
    pub fn has(&self, service: &str) -> bool {
        self.services.contains_key(service)
    }

    /// The schema text of `service`, for `describe`.
    pub fn text(&self, service: &str) -> Option<&str> {
        self.services
            .get(service)
            .map(|schema| schema.text.as_str())
    }

    /// Checks one half of a call. A service without a schema, a method the schema does not
    /// list and a half the method does not describe all pass: the schema only refuses what
    /// it says. A payload that is not JSON is refused when a schema applies.
    pub fn check(
        &self,
        service: &str,
        method: &str,
        direction: Direction,
        payload_json: &str,
    ) -> Result<(), SchemaViolation> {
        let Some(method_schema) = self
            .services
            .get(service)
            .and_then(|schema| schema.methods.get(method))
        else {
            return Ok(());
        };
        let schema = match direction {
            Direction::Request => method_schema.request.as_ref(),
            Direction::Response => method_schema.response.as_ref(),
        };
        let Some(schema) = schema else {
            return Ok(());
        };
        // An empty payload is a call without arguments and reads as `{}`.
        let value = if payload_json.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str::<Value>(payload_json)
                .map_err(|_| violation("", "the payload is not JSON"))?
        };
        // Providers answer a failure with `{"error": "<text>"}`; that reply is not the
        // method's result, so the result schema does not apply to it.
        if direction == Direction::Response && is_error_reply(&value) {
            return Ok(());
        }
        validate(schema, &value)
    }
}

/// True for the error reply convention of the services: an object with an `error` string.
fn is_error_reply(value: &Value) -> bool {
    value.get("error").is_some_and(Value::is_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pointer_of(schema: Value, value: Value) -> Option<String> {
        validate(&schema, &value).err().map(|error| error.pointer)
    }

    #[test]
    fn type_checks_every_json_type() {
        assert!(validate(&json!({"type": "integer"}), &json!(3)).is_ok());
        assert!(validate(&json!({"type": "integer"}), &json!(3.5)).is_err());
        assert!(validate(&json!({"type": "number"}), &json!(3.5)).is_ok());
        assert!(validate(&json!({"type": "string"}), &json!(1)).is_err());
        assert!(validate(&json!({"type": ["string", "null"]}), &json!(null)).is_ok());
        assert!(validate(&json!({"type": ["string", "null"]}), &json!(true)).is_err());
        assert!(validate(&json!(false), &json!(1)).is_err());
        assert!(validate(&json!(true), &json!(1)).is_ok());
    }

    #[test]
    fn required_and_properties_name_the_failing_field() {
        let schema = json!({
            "type": "object",
            "required": ["pane"],
            "properties": {
                "pane": {"type": "integer", "minimum": 1},
                "lines": {"type": "array", "items": {"type": "string", "maxLength": 3}}
            }
        });
        assert_eq!(
            pointer_of(schema.clone(), json!({})),
            Some("/pane".to_owned())
        );
        assert_eq!(
            pointer_of(schema.clone(), json!({"pane": 0})),
            Some("/pane".to_owned())
        );
        assert_eq!(
            pointer_of(schema.clone(), json!({"pane": 1, "lines": ["ok", "long"]})),
            Some("/lines/1".to_owned())
        );
        assert_eq!(
            pointer_of(schema, json!({"pane": 2, "lines": ["ok"]})),
            None
        );
    }

    #[test]
    fn additional_properties_enum_const_and_bounds() {
        let closed =
            json!({"type": "object", "properties": {"a": {}}, "additionalProperties": false});
        assert_eq!(
            pointer_of(closed, json!({"a": 1, "b/c": 2})),
            Some("/b~1c".to_owned())
        );
        let typed_extra = json!({"type": "object", "additionalProperties": {"type": "string"}});
        assert_eq!(
            pointer_of(typed_extra, json!({"x": 1})),
            Some("/x".to_owned())
        );
        assert!(validate(&json!({"enum": ["a", "b"]}), &json!("c")).is_err());
        assert!(validate(&json!({"enum": ["a", "b"]}), &json!("b")).is_ok());
        assert!(validate(&json!({"const": 1}), &json!(2)).is_err());
        assert!(validate(&json!({"maximum": 10}), &json!(11)).is_err());
        assert!(validate(&json!({"minLength": 2}), &json!("a")).is_err());
        assert_eq!(
            validate(&json!({"maxLength": 1}), &json!("ab"))
                .err()
                .map(|error| error.to_string()),
            Some("/: longer than 1 characters".to_owned())
        );
    }

    #[test]
    fn a_service_schema_checks_requests_and_replies() {
        let mut schemas = ServiceSchemas::default();
        let text = r#"{"usage": {
            "params": {"type": "object", "properties": {"window": {"type": "string"}}},
            "result": {"type": "object", "required": ["panes"]}
        }}"#;
        schemas.install("ilium.core.cost/usage", text).unwrap();
        assert!(schemas.has("ilium.core.cost/usage"));
        assert_eq!(schemas.text("ilium.core.cost/usage"), Some(text));
        let check = |direction, payload| {
            schemas.check("ilium.core.cost/usage", "usage", direction, payload)
        };
        assert!(check(Direction::Request, r#"{"window": "day"}"#).is_ok());
        assert_eq!(
            check(Direction::Request, r#"{"window": 5}"#)
                .err()
                .map(|e| e.pointer),
            Some("/window".to_owned())
        );
        assert!(check(Direction::Response, r#"{}"#).is_err());
        // An error reply passes; an empty request reads as `{}`.
        assert!(check(Direction::Response, r#"{"error":"no such window"}"#).is_ok());
        assert!(check(Direction::Request, "").is_ok());
        assert!(check(Direction::Response, "not json").is_err());
        // Methods and services without a schema pass.
        assert!(schemas
            .check("ilium.core.cost/usage", "other", Direction::Request, "1")
            .is_ok());
        assert!(schemas.check("x/y", "m", Direction::Request, "1").is_ok());
        assert!(schemas.install("x/y", "[]").is_err());
        assert!(schemas.install("x/y", "{").is_err());
    }
}
