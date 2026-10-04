//! Bounded primitive settings schema validation before module evaluation.
use crate::error::{AnimationError, Result};
use serde_json::{Map, Value};

const MAX_SETTINGS: usize = 128;
const MAX_STRING: usize = 4096;
const MAX_SERIALIZED: usize = 64 * 1024;

pub fn validate_settings(schema: &Value, authored: &Value) -> Result<Value> {
    validate(schema, authored, true)
}

/// Catalogue inspection validates declarations without requiring user values.
pub fn validate_schema(schema: &Value) -> Result<()> {
    validate(schema, &Value::Object(Map::new()), false).map(|_| ())
}

fn validate(schema: &Value, authored: &Value, require_values: bool) -> Result<Value> {
    let fail = |message: &str| AnimationError::InvalidPackage(format!("settings: {message}"));
    let specification = schema
        .as_object()
        .ok_or_else(|| fail("schema must be an object"))?;
    if specification.get("type").and_then(Value::as_str) != Some("object") {
        return Err(fail("root schema must be object"));
    }
    let properties = specification
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| fail("properties are required"))?;
    let values = authored
        .as_object()
        .ok_or_else(|| fail("values must be an object"))?;
    if properties.len() > MAX_SETTINGS
        || values.len() > MAX_SETTINGS
        || serde_json::to_vec(schema)?.len() > MAX_SERIALIZED
        || serde_json::to_vec(authored)?.len() > MAX_SERIALIZED
    {
        return Err(fail("schema or values exceed budget"));
    }
    // Dynamic settings remain closed even if a manifest omits additionalProperties.
    // Accepting arbitrary keys would give the UI and planner different contracts.
    if values.keys().any(|name| !properties.contains_key(name)) {
        return Err(fail("unknown setting"));
    }
    let mut output = Map::new();
    for (name, property) in properties {
        if name.is_empty()
            || name.len() > 80
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(fail("invalid setting name"));
        }
        let definition = property
            .as_object()
            .ok_or_else(|| fail("property schema must be an object"))?;
        let kind = definition
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("property type missing"))?;
        if !matches!(kind, "number" | "integer" | "boolean" | "string") {
            return Err(fail("unsupported property type"));
        }
        for bound in ["minimum", "maximum"] {
            if definition
                .get(bound)
                .is_some_and(|value| value.as_f64().is_none())
            {
                return Err(fail("invalid numeric bound"));
            }
        }
        if let (Some(minimum), Some(maximum)) = (
            definition.get("minimum").and_then(Value::as_f64),
            definition.get("maximum").and_then(Value::as_f64),
        ) {
            if minimum > maximum {
                return Err(fail("reversed numeric bounds"));
            }
        }
        if let Some(options) = definition.get("enum") {
            let options = options
                .as_array()
                .filter(|options| !options.is_empty() && options.len() <= 128)
                .ok_or_else(|| fail("invalid enum"))?;
            for option in options {
                validate_value(kind, definition, option, false)?;
            }
        }
        // Even an unused invalid default is an invalid package, never silently clamped.
        if let Some(default) = definition.get("default") {
            validate_value(kind, definition, default, true)?;
        }
        if let Some(value) = values.get(name).or_else(|| definition.get("default")) {
            validate_value(kind, definition, value, true)?;
            output.insert(name.clone(), value.clone());
        }
    }
    if let Some(required) = specification.get("required") {
        let required = required
            .as_array()
            .filter(|items| items.len() <= MAX_SETTINGS)
            .ok_or_else(|| fail("invalid required list"))?;
        for name in required {
            let name = name
                .as_str()
                .ok_or_else(|| fail("required name must be string"))?;
            if !properties.contains_key(name) || (require_values && !output.contains_key(name)) {
                return Err(fail("required setting missing"));
            }
        }
    }
    Ok(Value::Object(output))
}

fn validate_value(
    kind: &str,
    definition: &Map<String, Value>,
    value: &Value,
    check_enum: bool,
) -> Result<()> {
    let fail = |message: &str| AnimationError::InvalidPackage(format!("settings: {message}"));
    match kind {
        "number" | "integer" => {
            let number = value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| fail("number expected"))?;
            if kind == "integer" && number.fract() != 0.0 {
                return Err(fail("integer expected"));
            }
            if definition
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|minimum| number < minimum)
                || definition
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .is_some_and(|maximum| number > maximum)
            {
                return Err(fail("number outside declared bounds"));
            }
        }
        "boolean" => {
            if !value.is_boolean() {
                return Err(fail("boolean expected"));
            }
        }
        "string" => {
            let text = value.as_str().ok_or_else(|| fail("string expected"))?;
            let limit = definition
                .get("maxLength")
                .and_then(Value::as_u64)
                .unwrap_or(MAX_STRING as u64)
                .min(MAX_STRING as u64);
            if text.len() > MAX_STRING || text.chars().count() as u64 > limit {
                return Err(fail("string exceeds limit"));
            }
            if definition
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|minimum| (text.chars().count() as u64) < minimum)
            {
                return Err(fail("string below minimum length"));
            }
        }
        _ => return Err(fail("unsupported property type")),
    }
    if check_enum
        && definition
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|options| !options.contains(value))
    {
        return Err(fail("value outside enum"));
    }
    Ok(())
}
