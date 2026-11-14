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

/// One value row of a plugin's `contributes.settings`, as the client lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsRow {
    /// The value key (`[a-z][a-z0-9_]{0,63}`).
    pub key: String,
    /// The row type of the manifest (`toggle`, `choice`, `number`, `text`, ...).
    pub kind: String,
    /// The label of the row.
    pub title: String,
    /// The description line under the label, when the manifest gives one.
    pub description: Option<String>,
    /// The `default` of the manifest.
    pub default: Value,
    /// The tab the row belongs to: a built-in tab id, or `self:<id>` for the plugin's own tab.
    pub tab: String,
    /// The title of the section the row sits in, when it has one.
    pub section: Option<String>,
    /// The side that owns the value: `client` or `server`.
    pub scope: String,
    /// The compiled schema of the row (its property in the plugin's schema).
    pub schema: Value,
}

/// Every value row a manifest contributes, in declaration order.
///
/// A tab entry contributes the rows of its `sections` under `self:<tab id>`; a section entry
/// contributes its rows to its `tab`. A `group` row folds its children, which are listed as
/// ordinary rows. Rows without a valid `key` are skipped: the install check refuses them.
pub fn manifest_rows(manifest: &Value) -> Vec<SettingsRow> {
    let mut rows = Vec::new();
    let Some(entries) = manifest
        .pointer("/contributes/settings")
        .and_then(Value::as_array)
    else {
        return rows;
    };
    for entry in entries {
        match entry.get("kind").and_then(Value::as_str) {
            Some("tab") => {
                let tab = format!("self:{}", text_of(entry, "id").unwrap_or_default());
                for section in entry
                    .get("sections")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    collect_section_rows(section, &tab, &mut rows);
                }
            }
            Some("section") => {
                let tab = text_of(entry, "tab").unwrap_or_default();
                collect_section_rows(entry, &tab, &mut rows);
            }
            _ => {}
        }
    }
    rows
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn collect_section_rows(section: &Value, tab: &str, rows: &mut Vec<SettingsRow>) {
    let scope = text_of(section, "scope").unwrap_or_else(|| "client".to_owned());
    let title = text_of(section, "title");
    collect_rows(section.get("rows"), tab, title.as_deref(), &scope, rows);
}

fn collect_rows(
    list: Option<&Value>,
    tab: &str,
    section: Option<&str>,
    scope: &str,
    rows: &mut Vec<SettingsRow>,
) {
    for row in list.and_then(Value::as_array).into_iter().flatten() {
        let kind = text_of(row, "type").unwrap_or_default();
        if kind == "group" {
            collect_rows(row.get("rows"), tab, section, scope, rows);
            continue;
        }
        let Some(key) = text_of(row, "key") else {
            continue;
        };
        let Some(schema) = compile_row(&kind, row) else {
            continue;
        };
        rows.push(SettingsRow {
            title: text_of(row, "title").unwrap_or_else(|| key.clone()),
            description: text_of(row, "description"),
            default: row.get("default").cloned().unwrap_or(Value::Null),
            tab: tab.to_owned(),
            section: section.map(str::to_owned),
            scope: scope.to_owned(),
            kind,
            key,
            schema,
        });
    }
}

/// The schema of one value row (reference-settings-schema.md, "Row types"); `None` for a type
/// that holds no value.
fn compile_row(kind: &str, row: &Value) -> Option<Value> {
    let mut schema = serde_json::Map::new();
    let mut set = |key: &str, value: Value| {
        schema.insert(key.to_owned(), value);
    };
    match kind {
        "toggle" => set("type", Value::from("boolean")),
        "number" => {
            let integer = row.get("integer").and_then(Value::as_bool) != Some(false);
            set(
                "type",
                Value::from(if integer { "integer" } else { "number" }),
            );
            let steps: Vec<f64> = row
                .get("steps")
                .and_then(Value::as_array)
                .map(|steps| steps.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default();
            let ladder_bound = |pick: fn(f64, f64) -> f64| steps.iter().copied().reduce(pick);
            if let Some(minimum) = row
                .get("minimum")
                .and_then(Value::as_f64)
                .or_else(|| ladder_bound(f64::min))
            {
                set("minimum", Value::from(minimum));
            }
            if let Some(maximum) = row
                .get("maximum")
                .and_then(Value::as_f64)
                .or_else(|| ladder_bound(f64::max))
            {
                set("maximum", Value::from(maximum));
            }
            for key in ["step", "unit"] {
                if let Some(value) = row.get(key).filter(|value| !value.is_null()) {
                    set(key, value.clone());
                }
            }
            if !steps.is_empty() {
                set("x-steps", Value::from(steps));
            }
            if let Some(special) = row.get("special_values") {
                set("x-special-values", special.clone());
            }
        }
        "choice" => {
            let options = choice_values(row.get("options"));
            let integer =
                options.iter().all(|value| value.is_i64() || value.is_u64()) && !options.is_empty();
            set(
                "type",
                Value::from(if integer { "integer" } else { "string" }),
            );
            let fixed = row.get("allow_custom").and_then(Value::as_bool) != Some(true)
                && row.get("source").is_none();
            if fixed && !options.is_empty() && (integer || options.iter().all(Value::is_string)) {
                let titles = choice_titles(row.get("options"));
                let count = options.len();
                set("enum", Value::Array(options));
                if titles.len() == count {
                    set("enum_titles", Value::from(titles));
                }
            }
        }
        "text" | "text-area" | "path" | "color" | "keybinding" => {
            set("type", Value::from("string"));
            if let Some(limit) = row.get("max_length").and_then(Value::as_u64) {
                set("maxLength", Value::from(limit));
            }
            if let Some(pattern) = row.get("pattern").and_then(Value::as_str) {
                set("pattern", Value::from(pattern));
            }
            match kind {
                "color" => set("pattern", Value::from("^#[0-9a-fA-F]{6}$")),
                "text-area" => set("multiline", Value::Bool(true)),
                "text" if row.get("secret").and_then(Value::as_bool) == Some(true) => {
                    set("secret", Value::Bool(true));
                }
                "path" => {
                    if let Some(picker) = row.get("picker").and_then(Value::as_str) {
                        set("picker", Value::from(picker));
                    }
                }
                _ => {}
            }
        }
        "list" => {
            set("type", Value::from("array"));
            set("items", serde_json::json!({"type": "string"}));
            let limit = row
                .get("max_items")
                .and_then(Value::as_u64)
                .map_or(64, |limit| limit.min(64));
            set("maxItems", Value::from(limit));
        }
        "multi-choice" => {
            set("type", Value::from("array"));
            let values = choice_values(row.get("options"));
            if row.get("source").is_none()
                && values.iter().all(Value::is_string)
                && !values.is_empty()
            {
                set(
                    "items",
                    serde_json::json!({"type": "string", "enum": values}),
                );
            } else {
                set("items", serde_json::json!({"type": "string"}));
            }
            set("uniqueItems", Value::Bool(true));
            if let Some(minimum) = row.get("min_items").and_then(Value::as_u64) {
                set("minItems", Value::from(minimum));
            }
            if let Some(maximum) = row.get("max_items").and_then(Value::as_u64) {
                set("maxItems", Value::from(maximum));
            }
        }
        "map" => {
            set("type", Value::from("object"));
            set(
                "additionalProperties",
                serde_json::json!({"type": "string"}),
            );
            if let Some(limit) = row.get("max_entries").and_then(Value::as_u64) {
                set("maxProperties", Value::from(limit));
            }
        }
        "location" => {
            set("type", Value::from("object"));
            set(
                "properties",
                serde_json::json!({
                    "lat": {"type": "number", "minimum": -90, "maximum": 90},
                    "lon": {"type": "number", "minimum": -180, "maximum": 180},
                    "label": {"type": "string"}
                }),
            );
            set("required", serde_json::json!(["lat", "lon"]));
            set("x-widget", Value::from("location"));
        }
        _ => return None,
    }
    if let Some(default) = row.get("default") {
        set("default", default.clone());
    }
    for key in ["title", "help", "description"] {
        if let Some(text) = row.get(key).and_then(Value::as_str) {
            set(key, Value::from(text));
        }
    }
    if row.get("scope").and_then(Value::as_str) == Some("project") {
        set("x-scope", Value::from("project"));
    }
    if let Some(applies) = row.get("applies").and_then(Value::as_str) {
        set("x-applies", Value::from(applies));
    }
    Some(Value::Object(schema))
}

/// The values of a choice's `options`: plain strings or `{ value, title }` records.
fn choice_values(options: Option<&Value>) -> Vec<Value> {
    options
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| match option {
            Value::Object(fields) => fields.get("value").cloned(),
            other => Some(other.clone()),
        })
        .collect()
}

/// The display titles of a choice's `options`, falling back to the value.
fn choice_titles(options: Option<&Value>) -> Vec<String> {
    options
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|option| match option {
            Value::Object(fields) => fields
                .get("title")
                .or_else(|| fields.get("value"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .collect()
}

/// The names the plugin may address with the dynamic row calls (`set-info`, `set-row-note`,
/// `set-meter`, ...): the `key` of every value row and the `id` of every row without a value
/// (info, meter, notice, button, preview), group children included.
pub fn manifest_row_names(manifest: &Value) -> std::collections::BTreeSet<String> {
    fn walk(list: Option<&Value>, names: &mut std::collections::BTreeSet<String>) {
        for row in list.and_then(Value::as_array).into_iter().flatten() {
            if text_of(row, "type").as_deref() == Some("group") {
                walk(row.get("rows"), names);
                continue;
            }
            if let Some(name) = text_of(row, "key").or_else(|| text_of(row, "id")) {
                names.insert(name);
            }
        }
    }
    let mut names = std::collections::BTreeSet::new();
    for entry in manifest
        .pointer("/contributes/settings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match entry.get("kind").and_then(Value::as_str) {
            Some("tab") => {
                for section in entry
                    .get("sections")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    walk(section.get("rows"), &mut names);
                }
            }
            Some("section") => walk(entry.get("rows"), &mut names),
            _ => {}
        }
    }
    names
}

/// The schema a plugin side checks writes against: one object with a property per value row
/// that side owns (a section's `scope` is the side that hosts its keys).
pub fn schema_for_side(manifest: &Value, side: &str) -> Value {
    let mut properties = serde_json::Map::new();
    for row in manifest_rows(manifest) {
        if row.scope == side {
            properties.insert(row.key, row.schema);
        }
    }
    serde_json::json!({"type": "object", "properties": properties})
}

#[cfg(test)]
mod tests {
    use super::{kernel_tab_ids, may_write_kernel_tab, validate_settings, KERNEL_TAB_COUNT};
    use ilium_plugin_api::codes::settings as settings_codes;
    use ilium_plugin_api::grants::Grant;
    use ilium_plugin_api::manifest::Diagnostic;

    /// Object schema with one property per row type the validator must check: a bounded
    /// integer, a length-limited string, an enum and a boolean (reference-settings-schema.md).
    const SETTINGS_SCHEMA: &str = r#"{"type":"object","properties":{"max_entries":{"type":"integer","minimum":10,"maximum":5000},"label":{"type":"string","maxLength":40},"mode":{"type":"string","enum":["text","rich"]},"enabled":{"type":"boolean"}}}"#;

    fn grant(text: &str) -> Grant {
        Grant::parse(text).expect("test grant text must parse")
    }

    fn expect_rejected(value_json: &str) -> Vec<Diagnostic> {
        match validate_settings(SETTINGS_SCHEMA, value_json) {
            Ok(()) => panic!("value {value_json} must be rejected"),
            Err(diagnostics) => diagnostics,
        }
    }

    #[test]
    fn kernel_tab_ids_count_matches_frozen_constant() {
        assert_eq!(kernel_tab_ids().len(), KERNEL_TAB_COUNT);
        assert_eq!(KERNEL_TAB_COUNT, 25);
    }

    #[test]
    fn kernel_tab_ids_follow_rail_order() {
        let expected = [
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
        ];
        assert_eq!(kernel_tab_ids(), &expected[..]);
    }

    #[test]
    fn kernel_tab_ids_are_unique() {
        let ids = kernel_tab_ids();
        for (index, id) in ids.iter().enumerate() {
            assert!(
                !ids[index + 1..].contains(id),
                "kernel tab id {id} appears twice"
            );
        }
    }

    #[test]
    fn kernel_tab_ids_match_tab_id_grammar() {
        for id in kernel_tab_ids() {
            let mut characters = id.chars();
            let first_is_letter = characters.next().is_some_and(|c| c.is_ascii_lowercase());
            let rest_is_valid =
                characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            assert!(first_is_letter && rest_is_valid, "bad id {id}");
            assert!(id.len() <= 32, "id {id} is longer than 32 characters");
        }
    }

    /// REQ: SET-021
    #[test]
    fn validate_accepts_a_value_that_passes_every_row() {
        let value = r#"{"max_entries":500,"label":"work","mode":"text","enabled":true}"#;
        assert!(validate_settings(SETTINGS_SCHEMA, value).is_ok());
    }

    /// REQ: SET-011
    #[test]
    fn validate_accepts_an_empty_object_so_defaults_apply() {
        assert!(validate_settings(SETTINGS_SCHEMA, "{}").is_ok());
    }

    /// REQ: SET-020
    #[test]
    fn validate_accepts_number_at_minimum_boundary() {
        assert!(validate_settings(SETTINGS_SCHEMA, r#"{"max_entries":10}"#).is_ok());
    }

    /// REQ: SET-020
    #[test]
    fn validate_accepts_number_at_maximum_boundary() {
        assert!(validate_settings(SETTINGS_SCHEMA, r#"{"max_entries":5000}"#).is_ok());
    }

    /// REQ: SET-020
    #[test]
    fn validate_rejects_number_one_below_minimum_with_invalid_value_code() {
        let diagnostics = expect_rejected(r#"{"max_entries":9}"#);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_str(), settings_codes::INVALID_VALUE);
    }

    /// REQ: SET-020
    #[test]
    fn validate_rejects_number_one_above_maximum_with_invalid_value_code() {
        let diagnostics = expect_rejected(r#"{"max_entries":5001}"#);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_str(), settings_codes::INVALID_VALUE);
    }

    /// REQ: SET-021
    #[test]
    fn validate_accepts_string_at_max_length_boundary() {
        let label = "a".repeat(40);
        let value = format!(r#"{{"label":"{label}"}}"#);
        assert!(validate_settings(SETTINGS_SCHEMA, &value).is_ok());
    }

    /// REQ: SET-021
    #[test]
    fn validate_rejects_string_one_past_max_length() {
        let label = "a".repeat(41);
        let value = format!(r#"{{"label":"{label}"}}"#);
        let diagnostics = expect_rejected(&value);
        assert!(!diagnostics.is_empty());
    }

    /// REQ: SET-021
    #[test]
    fn validate_rejects_enum_value_outside_options() {
        let diagnostics = expect_rejected(r#"{"mode":"other"}"#);
        assert!(!diagnostics.is_empty());
    }

    /// REQ: SET-021
    #[test]
    fn validate_rejects_number_sent_as_string() {
        let diagnostics = expect_rejected(r#"{"max_entries":"500"}"#);
        assert!(!diagnostics.is_empty());
    }

    /// REQ: SET-021
    #[test]
    fn validate_rejects_boolean_sent_as_string() {
        let diagnostics = expect_rejected(r#"{"enabled":"yes"}"#);
        assert!(!diagnostics.is_empty());
    }

    /// REQ: COR-031
    #[test]
    fn write_kernel_grant_permits_its_own_tab() {
        let granted = vec![grant("settings.write-kernel:api")];
        assert!(may_write_kernel_tab(&granted, "api"));
    }

    /// REQ: COR-031
    #[test]
    fn write_kernel_grant_does_not_permit_another_tab() {
        let granted = vec![grant("settings.write-kernel:api")];
        assert!(!may_write_kernel_tab(&granted, "terminal"));
    }

    /// REQ: COR-031
    #[test]
    fn write_kernel_grant_matches_tab_id_exactly_not_by_prefix() {
        let granted = vec![grant("settings.write-kernel:api")];
        assert!(!may_write_kernel_tab(&granted, "ap"));
        assert!(!may_write_kernel_tab(&granted, "api-keys"));
    }

    /// REQ: COR-031
    #[test]
    fn empty_tab_id_is_never_writable() {
        let granted = vec![grant("settings.write-kernel:api")];
        assert!(!may_write_kernel_tab(&granted, ""));
    }

    /// REQ: COR-031
    #[test]
    fn no_grants_permit_no_tab() {
        assert!(!may_write_kernel_tab(&[], "api"));
    }

    /// REQ: COR-031
    #[test]
    fn read_kernel_grant_does_not_permit_writes() {
        let granted = vec![grant("settings.read-kernel:api")];
        assert!(!may_write_kernel_tab(&granted, "api"));
    }

    /// REQ: COR-031
    #[test]
    fn write_all_grant_does_not_permit_kernel_tab_writes() {
        let granted = vec![grant("settings.write-all")];
        assert!(!may_write_kernel_tab(&granted, "api"));
    }

    /// REQ: COR-031
    #[test]
    fn matching_grant_is_found_among_unrelated_grants() {
        let granted = vec![
            grant("settings.read-kernel:terminal"),
            grant("settings.write-all"),
            grant("settings.write-kernel:terminal"),
        ];
        assert!(may_write_kernel_tab(&granted, "terminal"));
        assert!(!may_write_kernel_tab(&granted, "editor"));
    }

    fn demo_manifest() -> serde_json::Value {
        serde_json::json!({"contributes": {"settings": [
            {"kind": "section", "id": "notes", "tab": "editor", "title": "Notes", "scope": "client",
             "rows": [
                {"type": "toggle", "key": "sound_enabled", "title": "Play a sound", "default": true},
                {"type": "number", "key": "indent_width", "title": "Indent", "minimum": 1,
                 "maximum": 8, "step": 1, "unit": "spaces", "default": 4},
                {"type": "number", "key": "zoom", "title": "Zoom", "integer": false,
                 "minimum": 0.5, "maximum": 2, "default": 1},
                {"type": "text", "key": "token", "title": "Token", "secret": true, "default": ""},
                {"type": "choice", "key": "density", "title": "Density", "default": "compact",
                 "options": [{"value": "compact", "title": "Compact"}, {"value": "roomy", "title": "Roomy"}]},
                {"type": "group", "title": "More", "rows": [
                    {"type": "color", "key": "tint", "title": "Tint", "default": "#112233"},
                    {"type": "list", "key": "names", "title": "Names", "default": []},
                    {"type": "multi-choice", "key": "kinds", "title": "Kinds", "default": ["a"],
                     "options": ["a", "b"]},
                    {"type": "map", "key": "aliases", "title": "Aliases", "default": {}},
                    {"type": "location", "key": "home", "title": "Home",
                     "default": {"lat": 1.0, "lon": 2.0}}
                ]},
                {"type": "button", "title": "Run", "label": "Run", "command": "demo.run"},
                {"type": "info", "id": "status", "title": "Status", "text": "ok"}
             ]},
            {"kind": "section", "id": "remote", "tab": "self:demo", "scope": "server",
             "rows": [{"type": "text", "key": "endpoint", "title": "Endpoint", "default": "x"}]},
            {"kind": "tab", "id": "demo", "title": "Demo", "sections": [
                {"id": "main", "title": "Main", "scope": "client",
                 "rows": [{"type": "toggle", "key": "own_tab", "title": "Own", "default": false}]}
            ]}
        ]}})
    }

    #[test]
    fn the_rows_of_a_manifest_are_listed_in_order_with_their_tab_and_scope() {
        let rows = super::manifest_rows(&demo_manifest());
        let keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "sound_enabled",
                "indent_width",
                "zoom",
                "token",
                "density",
                "tint",
                "names",
                "kinds",
                "aliases",
                "home",
                "endpoint",
                "own_tab"
            ]
        );
        assert_eq!(rows[0].tab, "editor");
        assert_eq!(rows[0].section.as_deref(), Some("Notes"));
        assert_eq!(rows[10].scope, "server");
        assert_eq!(rows[10].tab, "self:demo");
        assert_eq!(rows[11].tab, "self:demo");
        assert_eq!(rows[1].default, 4);
    }

    #[test]
    fn the_compiled_schema_of_every_row_type_is_a_valid_schema_of_the_validator() {
        let schema = super::schema_for_side(&demo_manifest(), "client");
        let properties = schema["properties"].as_object().expect("properties");
        assert!(!properties.contains_key("endpoint"));
        assert_eq!(properties.len(), 11);
        let defaults = serde_json::json!({
            "sound_enabled": true, "indent_width": 4, "zoom": 1, "token": "",
            "density": "compact", "tint": "#112233", "names": [], "kinds": ["a"],
            "aliases": {}, "home": {"lat": 1.0, "lon": 2.0}
        });
        validate_settings(&schema.to_string(), &defaults.to_string())
            .expect("the defaults pass their own schema");
        let server = super::schema_for_side(&demo_manifest(), "server");
        assert_eq!(
            server["properties"].as_object().map(|map| map.len()),
            Some(1)
        );
    }

    #[test]
    fn the_compiled_schema_refuses_a_value_outside_its_row() {
        let schema = super::schema_for_side(&demo_manifest(), "client").to_string();
        for bad in [
            r#"{"indent_width": 9}"#,
            r#"{"indent_width": 2.5}"#,
            r#"{"density": "huge"}"#,
            r#"{"tint": "red"}"#,
            r#"{"kinds": ["c"]}"#,
            r#"{"home": {"lat": 100, "lon": 0}}"#,
            r#"{"sound_enabled": 1}"#,
            r#"{"unknown": 1}"#,
        ] {
            assert!(validate_settings(&schema, bad).is_err(), "{bad}");
        }
        validate_settings(&schema, r#"{"zoom": 1.5, "density": "roomy"}"#)
            .expect("a value inside its rows");
    }
}
