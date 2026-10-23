//! Manifest parsing and validation entry points (reference-manifest-schema.md,
//! packages.md "Install checks").

use super::{Diagnostic, Manifest, Stage};
use crate::platform::Platform;
use crate::tier::Tier;
use crate::{Code, Side};
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::collections::BTreeSet;

/// Which command runs the checks (reference-check-and-lint-rules.md DEV-200).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckMode {
    /// `ilium plugin check` (reference-check-and-lint-rules.md).
    Check,
    /// The install path (reference-manifest-schema.md).
    Install,
    /// `ilium plugin dev` (packages.md).
    Dev,
}

/// What the validator knows about the machine (reference-manifest-schema.md).
#[derive(Debug, Clone)]
pub struct ValidationContext {
    /// Tier of the package under check (security.md).
    pub tier: Tier,
    /// Which command runs the checks.
    pub mode: CheckMode,
    /// Host operating system.
    pub platform: Platform,
    /// Installed plugins, for dependency and conflict rules.
    pub installed: Vec<InstalledSummary>,
    /// Core plugins, for `replaces` (reference-manifest.md PKG-028).
    pub core_plugins: Vec<CorePluginSummary>,
    /// Unstable features this host accepts (reference-abi-evolution.md, ABI-022).
    pub unstable_features: Vec<String>,
}

/// One installed plugin as the validator sees it (reference-dependency-resolution.md).
#[derive(Debug, Clone)]
pub struct InstalledSummary {
    /// Plugin id.
    pub id: String,
    /// Installed version.
    pub version: String,
    /// Services it provides: (name, version).
    pub services: Vec<(String, String)>,
    /// Settings tabs it owns.
    pub settings_owner_tabs: Vec<String>,
}

/// One core plugin as the validator sees it (reference-manifest.md PKG-028, PKG-029).
#[derive(Debug, Clone)]
pub struct CorePluginSummary {
    /// Core plugin id.
    pub id: String,
    /// Whether a third party may replace it.
    pub replaceable: bool,
    /// Its replaceable surface.
    pub surface: Vec<String>,
}

/// Parses a strict source document and validates its schema before constructing typed values.
pub fn parse_manifest(json: &str) -> Result<Manifest, Vec<Diagnostic>> {
    let (value, _) = document(json).map_err(|error| vec![error])?;
    let errors = schema_errors(&value);
    if !errors.is_empty() {
        return Err(errors);
    }
    serde_json::from_value(value)
        .map_err(|error| vec![finding("manifest.schema", "", error.to_string(), 3)])
}

/// Validates a typed manifest. Raw JSON validation additionally preserves lexical
/// integer spelling, member presence and source ordering.
pub fn validate(manifest: &Manifest, context: &ValidationContext) -> Vec<Diagnostic> {
    let value = match serde_json::to_value(manifest) {
        Ok(value) => value,
        Err(error) => return vec![finding("manifest.schema", "", error.to_string(), 3)],
    };
    let mut errors = schema_errors(&value);
    if !errors
        .iter()
        .any(|error| error.stage.is_none_or(|stage| stage.0 <= 3))
    {
        errors.extend(semantics(manifest, context));
    }
    errors.retain(|error| {
        context.mode != CheckMode::Check || error.stage.is_none_or(|stage| stage.0 <= 4)
    });
    first_stage(errors)
}

/// Runs pure manifest stages in install order, stopping after the first failing stage.
pub fn validate_json(json: &str, context: &ValidationContext) -> Vec<Diagnostic> {
    let (value, _order) = match document(json) {
        Ok(value) => value,
        Err(error) => return vec![error],
    };
    let mut errors = schema_errors(&value);
    if context.mode == CheckMode::Install && value.get("files").is_none() {
        errors.push(finding(
            "manifest.key-missing",
            "/files",
            "/files: the required key \"files\" is missing.",
            3,
        ));
    }
    if !errors
        .iter()
        .any(|error| error.stage.is_none_or(|stage| stage.0 <= 3))
    {
        match serde_json::from_value::<Manifest>(value) {
            Ok(manifest) => errors.extend(semantics(&manifest, context)),
            Err(error) => errors.push(finding("manifest.schema", "", error.to_string(), 3)),
        }
    }
    errors.retain(|error| {
        context.mode != CheckMode::Check || error.stage.is_none_or(|stage| stage.0 <= 4)
    });
    first_stage(errors)
}

pub(super) fn finding(
    code: &'static str,
    pointer: impl Into<String>,
    message: impl Into<String>,
    stage: u8,
) -> Diagnostic {
    let mut error = Diagnostic::error(Code::from_static(code), pointer, message);
    error.stage = Some(Stage(stage));
    error
}

fn first_stage(mut errors: Vec<Diagnostic>) -> Vec<Diagnostic> {
    if let Some(stage) = errors.iter().filter_map(|error| error.stage).min() {
        errors.retain(|error| error.stage == Some(stage));
    }
    let mut seen = BTreeSet::new();
    errors.retain(|error| seen.insert((error.pointer.clone(), error.code.as_str().to_owned())));
    errors.sort_by(|left, right| left.pointer.as_bytes().cmp(right.pointer.as_bytes()));
    if errors.len() > 100 {
        let stage = errors[0].stage.map_or(3, |stage| stage.0);
        let omitted = errors.len() - 100;
        errors.truncate(100);
        let mut warning = finding(
            "manifest.schema",
            "",
            format!("{omitted} more errors omitted."),
            stage,
        );
        warning.severity = crate::error::Severity::Warning;
        errors.push(warning);
    }
    errors
}

fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

struct JsonSeed<'a> {
    path: String,
    depth: usize,
    order: &'a mut Vec<String>,
    error: &'a mut Option<Diagnostic>,
}
impl<'de> DeserializeSeed<'de> for JsonSeed<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for JsonSeed<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }
    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.into()))
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        if self.depth >= 32 {
            *self.error = Some(finding(
                "manifest.schema",
                &self.path,
                "Nesting exceeds 32 container levels.",
                3,
            ));
            return Err(serde::de::Error::custom("manifest nesting"));
        }
        let mut result = Vec::new();
        while let Some(value) = sequence.next_element_seed(JsonSeed {
            path: pointer(&self.path, &result.len().to_string()),
            depth: self.depth + 1,
            order: self.order,
            error: self.error,
        })? {
            result.push(value);
        }
        Ok(Value::Array(result))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        if self.depth >= 32 {
            *self.error = Some(finding(
                "manifest.schema",
                &self.path,
                "Nesting exceeds 32 container levels.",
                3,
            ));
            return Err(serde::de::Error::custom("manifest nesting"));
        }
        let mut result = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if result.contains_key(&key) {
                *self.error = Some(finding(
                    "manifest.duplicate-key",
                    &self.path,
                    format!(
                        "The key {key:?} appears twice in the object at {}.",
                        self.path
                    ),
                    2,
                ));
                return Err(serde::de::Error::custom("duplicate manifest key"));
            }
            let path = pointer(&self.path, &key);
            self.order.push(path.clone());
            let value = map.next_value_seed(JsonSeed {
                path,
                depth: self.depth + 1,
                order: self.order,
                error: self.error,
            })?;
            result.insert(key, value);
        }
        Ok(Value::Object(result))
    }
}

fn document(text: &str) -> Result<(Value, Vec<String>), Diagnostic> {
    if text.len() > 1_048_576 {
        return Err(finding(
            "package.manifest-size",
            "",
            format!(
                "manifest.json is {} bytes. The limit is 1048576.",
                text.len()
            ),
            1,
        ));
    }
    let mut order = Vec::new();
    let mut error = None;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let result = JsonSeed {
        path: String::new(),
        depth: 0,
        order: &mut order,
        error: &mut error,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| {
        deserializer.end()?;
        Ok(value)
    });
    result.map(|value| (value, order)).map_err(|json_error| {
        error.unwrap_or_else(|| {
            finding(
                "manifest.json-invalid",
                "",
                format!("manifest.json is not valid JSON or not UTF-8 ({json_error})."),
                2,
            )
        })
    })
}

fn schema_errors(value: &Value) -> Vec<Diagnostic> {
    // The literal is the published schema, not a filesystem dependency.
    let schema: Value =
        serde_json::from_str(json_schema()).expect("embedded manifest schema is valid JSON");
    let mut errors = Vec::new();
    check_schema(value, &schema, &schema, "", &mut errors);
    if let Some(settings) = value
        .pointer("/contributes/settings")
        .and_then(Value::as_array)
    {
        let mut keys = BTreeSet::new();
        for (index, entry) in settings.iter().enumerate() {
            check_settings_rows(
                entry,
                &format!("/contributes/settings/{index}"),
                false,
                &mut keys,
                &mut errors,
            );
        }
    }
    errors
}

fn schema_code(path: &str) -> &'static str {
    match path {
        "/id" => "manifest.id-invalid",
        "/version" => "manifest.version-invalid",
        "/name" | "/publisher" | "/description" => "manifest.display-name",
        "/data_version" => "manifest.data-version",
        _ => "manifest.schema",
    }
}

fn check_schema(
    value: &Value,
    schema: &Value,
    root: &Value,
    path: &str,
    errors: &mut Vec<Diagnostic>,
) {
    if schema == &Value::Bool(true) {
        return;
    }
    if schema == &Value::Bool(false) {
        errors.push(finding("manifest.schema", path, "Value is not allowed.", 3));
        return;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if let Some(target) = reference
            .strip_prefix('#')
            .and_then(|path| root.pointer(path))
        {
            check_schema(value, target, root, path, errors);
        } else {
            errors.push(finding(
                "manifest.schema",
                path,
                "Invalid schema reference.",
                3,
            ));
        }
        return;
    }
    let wrong = |errors: &mut Vec<Diagnostic>, reason: &str| {
        errors.push(finding(
            if path == "/data_version" {
                schema_code(path)
            } else {
                "manifest.schema"
            },
            path,
            if path == "/data_version" {
                format!("data_version must be an integer from 1 to 4294967295. Found {value}.")
            } else {
                format!("{path}: {reason}.")
            },
            3,
        ))
    };
    let matches_type = |name: &str| match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_u64().is_some() || value.as_i64().is_some(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        _ => false,
    };
    if value.is_null() {
        wrong(errors, "null is not a manifest value");
        return;
    }
    if path == "/limits" && value.is_object() {
        match serde_json::from_value::<crate::limits::Limits>(value.clone()) {
            Ok(limits) => {
                if let Err(mut error) = crate::limits::clamp(&limits, Tier::T3) {
                    error.stage = Some(Stage(3));
                    errors.push(error);
                }
            }
            Err(_) => wrong(errors, "limits must be unsigned integer values"),
        }
        return;
    }
    if path.starts_with("/capabilities/") && path.split('/').count() == 4 {
        if let Some(text) = value.as_str() {
            if let Err(mut error) = crate::grants::Grant::parse(text) {
                error.pointer = path.into();
                error.stage = Some(Stage(4));
                errors.push(error);
            }
            return;
        }
    }
    if let Some(name) = schema.get("type").and_then(Value::as_str) {
        if !matches_type(name) {
            wrong(errors, &format!("expected {name}"));
            return;
        }
    }
    if let Some(expected) = schema.get("const") {
        if value != expected {
            // A syntactically valid API version is checked by the host window below.
            if path != "/api" || value.as_str().and_then(api_version).is_none() {
                if ["/server/entry", "/client/entry", "/scene/entry"].contains(&path) {
                    errors.push(finding(
                        "manifest.entry-invalid",
                        path,
                        "Component entry must match its side.",
                        3,
                    ));
                } else {
                    wrong(errors, "value differs from the required constant");
                }
            }
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) && !path.starts_with("/unstable/") {
            wrong(errors, "value is outside the allowed enumeration");
        }
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if schema
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|min| length < min)
            || schema
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|max| length > max)
        {
            errors.push(finding(
                "manifest.schema",
                path,
                format!("{path}: string length is outside its bounds."),
                3,
            ));
        }
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            if !pattern_matches(text, pattern) {
                let code = if path.starts_with("/requires/services/") {
                    "manifest.service-version"
                } else {
                    schema_code(path)
                };
                errors.push(finding(
                    code,
                    path,
                    if code == "manifest.version-invalid" {
                        format!("The version {text:?} is not a valid semantic version.")
                    } else {
                        format!("{path}: {text:?} does not match the required grammar.")
                    },
                    3,
                ));
            }
        }
    }
    if let Some(number) = value.as_f64() {
        if schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|min| number < min)
            || schema
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|max| number > max)
        {
            wrong(errors, "number is outside its bounds");
        }
    }
    if let Some(values) = value.as_array() {
        let length = values.len() as u64;
        if schema
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|min| length < min)
            || schema
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|max| length > max)
        {
            let code = if path.starts_with("/contributes/") {
                "manifest.contribution-limit"
            } else {
                "manifest.schema"
            };
            errors.push(finding(
                code,
                path,
                "Array length is outside its bounds.",
                3,
            ));
        }
        if schema.get("uniqueItems") == Some(&Value::Bool(true))
            && values
                .iter()
                .enumerate()
                .any(|(index, value)| values[..index].contains(value))
        {
            wrong(errors, "array has duplicate entries");
        }
        if let Some(items) = schema.get("items") {
            for (index, value) in values.iter().enumerate() {
                check_schema(
                    value,
                    items,
                    root,
                    &pointer(path, &index.to_string()),
                    errors,
                );
            }
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (key, value) in object {
                if let Some(property) = properties.get(key) {
                    check_schema(value, property, root, &pointer(path, key), errors);
                } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                    errors.push(finding(
                        "manifest.unknown-key",
                        pointer(path, key),
                        format!(
                            "{}: {key:?} is not a key of this API version.",
                            pointer(path, key)
                        ),
                        3,
                    ));
                }
            }
        }
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    errors.push(finding(
                        if schema.get("properties").is_some() {
                            "manifest.key-missing"
                        } else {
                            "manifest.schema"
                        },
                        pointer(path, key),
                        format!("{path}: the required key {key:?} is missing."),
                        3,
                    ));
                }
            }
        }
        if let Some(dependencies) = schema.get("dependentRequired").and_then(Value::as_object) {
            for (key, required) in dependencies {
                if object.contains_key(key) {
                    for name in required
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        if !object.contains_key(name) {
                            errors.push(finding(
                                "manifest.schema",
                                pointer(path, name),
                                format!("{name} is required with {key}."),
                                3,
                            ));
                        }
                    }
                }
            }
        }
    }
    if let Some(options) = schema.get("anyOf").and_then(Value::as_array) {
        if !options.iter().any(|option| {
            let mut findings = Vec::new();
            check_schema(value, option, root, path, &mut findings);
            findings.is_empty()
        }) {
            wrong(errors, "none of the allowed object forms matches");
        }
    }
    if let Some(rules) = schema.get("allOf").and_then(Value::as_array) {
        for rule in rules {
            check_schema(value, rule, root, path, errors);
        }
    }
    if let Some(condition) = schema.get("if") {
        let mut findings = Vec::new();
        check_schema(value, condition, root, path, &mut findings);
        if findings.is_empty() {
            if let Some(rule) = schema.get("then") {
                check_schema(value, rule, root, path, errors);
            }
        }
    }
}

fn valid_id(text: &str) -> bool {
    text.len() <= 128 && id_pattern_matches(text)
}

// maxLength is an independent schema constraint, not part of the id regex.
fn id_pattern_matches(text: &str) -> bool {
    let parts: Vec<_> = text.split('.').collect();
    (2..=8).contains(&parts.len())
        && parts
            .iter()
            .all(|part| (1..=32).contains(&part.len()) && local_id(part))
}
fn local_id(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && !text.ends_with('-')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
fn display(text: &str) -> bool {
    !text.chars().any(|c| {
        c.is_control()
            || matches!(c,'\u{2028}'|'\u{2029}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
    })
}
fn api_version(text: &str) -> Option<semver::Version> {
    let version = semver::Version::parse(text.strip_prefix("ilium:plugin@")?).ok()?;
    if version.pre.is_empty()
        && version.build.is_empty()
        && version.major <= u16::MAX.into()
        && version.minor <= u16::MAX.into()
        && version.patch <= u16::MAX.into()
    {
        Some(version)
    } else {
        None
    }
}

// This is a finite interpreter for the patterns present in the embedded schema,
// not a general regular-expression evaluator. Unrecognized schema patterns fail closed.
fn pattern_matches(text: &str, pattern: &str) -> bool {
    if pattern.starts_with("^(0|[1-9]") {
        return semver::Version::parse(text).is_ok_and(|version| {
            [version.major, version.minor, version.patch]
                .into_iter()
                .all(|part| part <= i32::MAX as u64)
        });
    }
    if pattern.contains("{1,7}") {
        return id_pattern_matches(text);
    }
    if pattern.starts_with("^[^\\u0000") {
        return if pattern.contains("\\u0009\\u000b") {
            display(&text.replace('\n', ""))
        } else {
            display(text)
        };
    }
    if pattern == "^[^\\s]+$" {
        return !text.is_empty() && !text.chars().any(char::is_whitespace);
    }
    if pattern.starts_with("^https://") {
        return text
            .strip_prefix("https://")
            .is_some_and(|host| !host.is_empty() && !host.chars().any(char::is_whitespace));
    }
    if pattern == "^/" {
        return text.starts_with('/');
    }
    if pattern == "^[0-9a-f]{64}$" {
        return text.len() == 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    }
    if pattern.starts_with("^[a-z0-9]") {
        return local_id(text);
    }
    if pattern.starts_with("^(startup|") {
        return super::parse_activation(text).is_ok();
    }
    if pattern.starts_with("^(services|") {
        return text.split_once(':').is_some_and(|(kind, name)| {
            [
                "services",
                "points",
                "commands",
                "pane-kinds",
                "topics",
                "cli",
            ]
            .contains(&kind)
                && !name.is_empty()
                && !name.chars().any(char::is_whitespace)
        });
    }
    if pattern.starts_with("^[^@") {
        return super::ServiceRequirement::parse(text).is_ok();
    }
    if pattern.starts_with("^[A-Za-z0-9._-") {
        return safe_path(text);
    }
    if pattern.starts_with("^help/") {
        return text
            .strip_prefix("help/")
            .and_then(|name| name.strip_suffix(".md"))
            .is_some_and(file_name);
    }
    if pattern.starts_with("^sounds/") {
        return text
            .strip_prefix("sounds/")
            .and_then(|name| {
                name.strip_suffix(".wav")
                    .or_else(|| name.strip_suffix(".ogg"))
            })
            .is_some_and(file_name);
    }
    if pattern.starts_with("^[a-z][") {
        let (family, argument) = text
            .split_once(':')
            .map_or((text, None), |(family, argument)| (family, Some(argument)));
        return family.starts_with(|c: char| c.is_ascii_lowercase())
            && family.split('.').all(local_id)
            && argument
                .is_none_or(|value| !value.is_empty() && !value.chars().any(char::is_control));
    }
    false
}
fn file_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
fn safe_path(text: &str) -> bool {
    !text.is_empty()
        && text
            .split('/')
            .all(|part| part != "." && part != ".." && file_name(part))
}

/// Published JSON Schema, embedded so this pure crate never reads files.
pub fn json_schema() -> &'static str {
    MANIFEST_SCHEMA
}

const MANIFEST_SCHEMA: &str = r###"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:ilium:schema:plugin-manifest:0.1.0",
  "title": "Ilium plugin manifest, contract ilium:plugin@0.1.0",
  "type": "object",
  "additionalProperties": false,
  "required": ["api", "id", "name", "version", "publisher"],
  "anyOf": [{"required": ["server"]}, {"required": ["client"]}, {"required": ["scene"]}],
  "properties": {
    "api": {"const": "ilium:plugin@0.1.0"},
    "id": {
      "type": "string",
      "maxLength": 128,
      "pattern": "^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?){1,7}$"
    },
    "name": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[^\\u0000-\\u001f\\u007f-\\u009f\u2028\u2029\u202a-\u202e\u2066-\u2069]+$"},
    "version": {
      "type": "string",
      "maxLength": 64,
      "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(\\.(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?(\\+[0-9A-Za-z-]+(\\.[0-9A-Za-z-]+)*)?$"
    },
    "publisher": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[^\\u0000-\\u001f\\u007f-\\u009f\u2028\u2029\u202a-\u202e\u2066-\u2069]+$"},
    "description": {"type": "string", "maxLength": 500, "pattern": "^[^\\u0000-\\u0009\\u000b-\\u001f\\u007f-\\u009f\u2028\u2029\u202a-\u202e\u2066-\u2069]*$"},
    "license": {"type": "string", "minLength": 1, "maxLength": 128},
    "icon": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
    "categories": {
      "type": "array",
      "minItems": 1,
      "maxItems": 3,
      "uniqueItems": true,
      "items": {
        "enum": [
          "agents",
          "appearance",
          "scenes",
          "editors",
          "git",
          "inference",
          "notifications",
          "search",
          "terminal",
          "tools",
          "voice"
        ]
      }
    },
    "keywords": {
      "type": "array",
      "maxItems": 8,
      "uniqueItems": true,
      "items": {"type": "string", "minLength": 1, "maxLength": 24, "pattern": "^[^\\s]+$"}
    },
    "homepage": {"type": "string", "maxLength": 256, "pattern": "^https://[^\\s]+$"},
    "repository": {"type": "string", "maxLength": 256, "pattern": "^https://[^\\s]+$"},
    "data_use": {
      "type": "array",
      "maxItems": 8,
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["what", "where", "purpose", "opt_in"],
        "properties": {
          "what": {"type": "string", "minLength": 1, "maxLength": 200},
          "where": {"type": "string", "minLength": 1, "maxLength": 200},
          "purpose": {"type": "string", "minLength": 1, "maxLength": 200},
          "opt_in": {"type": "boolean"}
        }
      }
    },
    "security_contact": {"type": "string", "minLength": 1, "maxLength": 200},
    "server": {
      "type": "object",
      "additionalProperties": false,
      "required": ["entry", "world"],
      "properties": {
        "entry": {"const": "server.wasm"},
        "world": {"const": "ilium:server/base@0.1.0"},
        "exports": {
          "type": "array",
          "uniqueItems": true,
          "maxItems": 9,
          "items": {
            "enum": [
              "detector",
              "job-runner",
              "service-provider",
              "inference-provider",
              "settings-validate",
              "catalog-provider",
              "virtual-rows-provider",
              "hook-handler",
              "migrator"
            ]
          }
        }
      }
    },
    "client": {
      "type": "object",
      "additionalProperties": false,
      "required": ["entry", "world"],
      "properties": {
        "entry": {"const": "client.wasm"},
        "world": {"const": "ilium:client/base@0.1.0"},
        "exports": {
          "type": "array",
          "uniqueItems": true,
          "maxItems": 12,
          "items": {
            "enum": [
              "views",
              "annotator",
              "job-runner",
              "service-provider",
              "inference-provider",
              "settings-validate",
              "catalog-provider",
              "menu-provider",
              "virtual-rows-provider",
              "token-provider",
              "hook-handler",
              "migrator"
            ]
          }
        }
      }
    },
    "scene": {
      "type": "object",
      "additionalProperties": false,
      "required": ["entry", "world"],
      "properties": {
        "entry": {"const": "scene.wasm"},
        "world": {"const": "ilium:anim/base@0.1.0"},
        "exports": {"type": "array", "uniqueItems": true, "maxItems": 1, "items": {"enum": ["job-runner"]}}
      }
    },
    "capabilities": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "server": {"type": "array", "uniqueItems": true, "maxItems": 256, "items": {"$ref": "#/$defs/grant"}},
        "client": {"type": "array", "uniqueItems": true, "maxItems": 256, "items": {"$ref": "#/$defs/grant"}},
        "scene": {"type": "array", "uniqueItems": true, "maxItems": 256, "items": {"$ref": "#/$defs/grant"}}
      }
    },
    "activation": {
      "type": "array",
      "uniqueItems": true,
      "maxItems": 32,
      "items": {
        "type": "string",
        "maxLength": 256,
        "pattern": "^(startup|durable-timer|(event|command|view|pane-kind|scene|cli|http|service|topic|settings|catalog|menu|virtual-rows|language|client-request):[^\\s]+)$"
      }
    },
    "requires": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "host": {"type": "string", "minLength": 1, "maxLength": 128},
        "services": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 300, "pattern": "^[^@\\s]+@(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$"}, "maxItems": 32},
        "plugins": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {
                "type": "string",
                "maxLength": 128,
                "pattern": "^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?){1,7}$"
              },
              "version": {"type": "string", "minLength": 1, "maxLength": 128},
              "optional": {"type": "boolean"}
            },
            "required": ["id"]
          },
          "maxItems": 32
        },
        "wait_ms": {"type": "integer", "minimum": 0, "maximum": 5000}
      }
    },
    "accepts_messages_from": {
      "type": "array",
      "maxItems": 32,
      "uniqueItems": true,
      "items": {"type": "string", "maxLength": 128}
    },
    "replaces": {
      "type": "array",
      "maxItems": 4,
      "uniqueItems": true,
      "items": {
        "type": "string",
        "maxLength": 128,
        "pattern": "^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?){1,7}$"
      }
    },
    "replaceable": {"type": "boolean"},
    "surface": {
      "type": "array",
      "minItems": 1,
      "maxItems": 64,
      "uniqueItems": true,
      "items": {"type": "string", "pattern": "^(services|points|commands|pane-kinds|topics|cli):[^\\s]+$"}
    },
    "contributes": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "commands": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 64},
              "palette": {"type": "boolean"},
              "voice": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 64}, "maxItems": 8},
              "arguments_schema": {"type": "object"},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "when": {"type": "string", "minLength": 1, "maxLength": 256},
              "side": {"enum": ["server", "client"]},
              "category": {"type": "string", "minLength": 1, "maxLength": 32},
              "keywords": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 24}, "maxItems": 8}
            },
            "required": ["id", "title"]
          },
          "maxItems": 256
        },
        "keybindings": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "chord": {"type": "string", "minLength": 1, "maxLength": 64},
              "mode": {"type": "string", "minLength": 1, "maxLength": 64}
            },
            "required": ["command", "chord"]
          },
          "maxItems": 256
        },
        "settings": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "kind": {"enum": ["tab", "section"]},
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "short_title": {"type": "string", "minLength": 1, "maxLength": 16},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "glyph": {"type": "string", "minLength": 1, "maxLength": 4},
              "after": {"type": "string", "minLength": 1, "maxLength": 64},
              "order": {"type": "integer", "minimum": 0, "maximum": 1000},
              "render": {"enum": ["rows", "view"]},
              "keywords": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 24}, "maxItems": 16},
              "sections": {"type": "array", "items": {"type": "object"}, "maxItems": 64},
              "tab": {"type": "string", "minLength": 1, "maxLength": 64},
              "placement": {"type": "string", "minLength": 1, "maxLength": 64},
              "scope": {"enum": ["server", "client"]},
              "rows": {"type": "array", "items": {"type": "object"}, "maxItems": 1024},
              "collapsed": {"type": "boolean"}
            },
            "required": ["kind", "id"]
          },
          "maxItems": 64
        },
        "help": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "file": {"type": "string", "pattern": "^help/[A-Za-z0-9._-]+\\.md$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 64}
            },
            "required": ["id", "file", "title"]
          },
          "maxItems": 64
        },
        "decoration_slots": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "placement": {"enum": ["badge", "icon", "subtitle", "footer"]},
              "priority": {"type": "integer", "minimum": 0, "maximum": 100},
              "visibility": {"enum": ["always", "on-hover"]},
              "trailing": {"type": "boolean"}
            },
            "required": ["id", "placement"]
          },
          "maxItems": 16
        },
        "virtual_rows": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "parent": {"enum": ["project", "session"]},
              "expandable": {"type": "boolean"},
              "children": {"enum": ["static", "provider"]},
              "accepts_drop": {"type": "array", "items": {"enum": ["pane", "file", "text"]}, "maxItems": 3},
              "drop_command": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "required": ["id", "title", "parent"]
          },
          "maxItems": 16
        },
        "toolbar_buttons": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "label": {"type": "string", "minLength": 1, "maxLength": 24},
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "applies_to": {},
              "requires_action": {"type": "string", "minLength": 1, "maxLength": 64},
              "submenu": {},
              "right_click_command": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "required": ["id", "icon", "command"]
          },
          "maxItems": 16
        },
        "context_menu": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "target": {"type": "string", "minLength": 1, "maxLength": 64},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "when": {"type": "string", "minLength": 1, "maxLength": 256},
              "dynamic": {"type": "boolean"},
              "items": {"type": "array", "items": {"type": "object"}, "maxItems": 64}
            },
            "required": ["id", "title", "target"]
          },
          "maxItems": 16
        },
        "panels": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "default_size": {},
              "modal": {"type": "boolean"},
              "placement": {"enum": ["overlay", "left", "right", "bottom"]},
              "min_size": {},
              "mode_label": {"type": "string", "minLength": 1, "maxLength": 16},
              "wants_keys": {"type": "boolean"}
            },
            "required": ["id", "title"]
          },
          "maxItems": 16
        },
        "pane_kinds": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "primitive": {"enum": ["basic"]},
              "wants_keys": {"type": "boolean"},
              "persist": {"enum": ["metadata", "none"]}
            },
            "required": ["id", "title"]
          },
          "maxItems": 16
        },
        "scenes": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "family": {"type": "string", "minLength": 1, "maxLength": 32},
              "modes": {},
              "settings_schema": {"type": "object"},
              "frame_rate_cap": {"type": "integer", "minimum": 1, "maximum": 60},
              "prerender": {"type": "object"},
              "glyph_plane": {"type": "boolean"}
            },
            "required": ["id", "title"]
          },
          "maxItems": 16
        },
        "signatures": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "process_names": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 255}, "maxItems": 32},
              "aliases": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 255}, "maxItems": 32},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "resume": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 512}, "maxItems": 32},
              "actions": {"type": "array", "items": {"type": "object"}, "maxItems": 32},
              "submit_keys": {"type": "string", "minLength": 1, "maxLength": 64},
              "session_scope": {"type": "string", "minLength": 1, "maxLength": 32}
            },
            "required": ["id", "process_names"]
          },
          "maxItems": 16
        },
        "output_filters": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "regex": {"type": "string", "minLength": 1, "maxLength": 512},
              "panes": {"enum": ["all", "agents", "terminals"]},
              "rate_per_second": {"type": "integer", "minimum": 1, "maximum": 100}
            },
            "required": ["id", "regex"]
          },
          "maxItems": 32
        },
        "link_detectors": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "regex": {"type": "string", "minLength": 1, "maxLength": 512},
              "command": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "required": ["id", "regex", "command"]
          },
          "maxItems": 16
        },
        "editor_line_actions": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "regex": {"type": "string", "minLength": 1, "maxLength": 512},
              "command": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "required": ["id", "title", "command"]
          },
          "maxItems": 16
        },
        "sounds": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "file": {"type": "string", "pattern": "^sounds/[A-Za-z0-9._-]+\\.(wav|ogg)$"}
            },
            "required": ["id", "file"]
          },
          "maxItems": 32
        },
        "cli": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "name": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "summary": {"type": "string", "minLength": 1, "maxLength": 120},
              "arguments_schema": {"type": "object"},
              "output": {"enum": ["jsonl", "text"]},
              "requires_server": {"type": "boolean"},
              "stdin": {"enum": ["none", "optional", "required"]},
              "argument_groups": {"type": "array", "items": {"enum": ["pane-selector"]}, "maxItems": 4}
            },
            "required": ["name", "summary"]
          },
          "maxItems": 16
        },
        "http_routes": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "method": {"enum": ["GET", "POST", "PUT", "PATCH", "DELETE"]},
              "path": {"type": "string", "pattern": "^/", "maxLength": 256}
            },
            "required": ["method", "path"]
          },
          "maxItems": 32
        },
        "services": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "provides": {
              "type": "array",
              "maxItems": 16,
              "items": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                  "name": {"type": "string", "minLength": 1, "maxLength": 256},
                  "version": {"type": "string", "minLength": 5, "maxLength": 64},
                  "schema": {"type": "string", "minLength": 1, "maxLength": 256}
                },
                "required": ["name", "version", "schema"]
              }
            }
          }
        },
        "topics": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "publishes": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 256}, "maxItems": 64},
            "subscribes": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 256}, "maxItems": 64}
          }
        },
        "jobs": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "kind": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "deadline_ms": {"type": "integer", "minimum": 1},
              "memory_mib": {"type": "integer", "minimum": 1}
            },
            "required": ["kind"]
          },
          "maxItems": 32
        },
        "onboarding_steps": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 64},
              "order": {"type": "integer", "minimum": 0, "maximum": 1000}
            },
            "required": ["id", "title"]
          },
          "maxItems": 8
        },
        "extension_points": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "kind": {"enum": ["collect", "hook"]},
              "mode": {"enum": ["observe", "transform", "veto", "first"]},
              "title": {"type": "string", "minLength": 1, "maxLength": 64},
              "entry_schema": {"type": "object"},
              "payload_schema": {"type": "object"},
              "reply_schema": {"type": "object"},
              "risk": {"enum": ["standard", "high"]},
              "deadline_ms": {"type": "integer", "minimum": 5, "maximum": 50},
              "max_handlers": {"type": "integer", "minimum": 1, "maximum": 8},
              "max_entries": {"type": "integer", "minimum": 1, "maximum": 256},
              "dynamic": {"type": "boolean"},
              "on_failure": {"enum": ["pass", "refuse"]},
              "final_check": {"type": "boolean"},
              "version": {
                "type": "string",
                "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(\\.(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?(\\+[0-9A-Za-z-]+(\\.[0-9A-Za-z-]+)*)?$"
              }
            },
            "required": ["id", "kind"]
          },
          "maxItems": 16
        },
        "extensions": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "point": {"type": "string", "minLength": 1, "maxLength": 160},
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "priority": {"type": "integer", "minimum": 0, "maximum": 100},
              "entry": {"type": "object"},
              "handler": {"type": "string", "minLength": 1, "maxLength": 64},
              "guard": {"type": "boolean"}
            },
            "required": ["point", "id"]
          },
          "maxItems": 64
        },
        "tree_toolbar": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "label": {"type": "string", "minLength": 1, "maxLength": 24},
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "caption": {"type": "string", "minLength": 1, "maxLength": 12},
              "popup_view": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "when": {"type": "string", "minLength": 1, "maxLength": 256}
            },
            "required": ["id", "icon", "label"]
          },
          "maxItems": 4
        },
        "row_actions": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "label": {"type": "string", "minLength": 1, "maxLength": 24},
              "command": {"type": "string", "minLength": 1, "maxLength": 128},
              "applies_to": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 64}, "maxItems": 16},
              "when": {"type": "string", "minLength": 1, "maxLength": 256},
              "order": {"type": "integer", "minimum": 0, "maximum": 1000}
            },
            "required": ["id", "icon", "command"]
          },
          "maxItems": 4
        },
        "tree_orderings": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 32},
              "slot": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"}
            },
            "required": ["id", "title", "slot"]
          },
          "maxItems": 4
        },
        "status_segments": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "priority": {"type": "integer", "minimum": 0, "maximum": 100},
              "max_width": {"type": "integer", "minimum": 4, "maximum": 40}
            },
            "required": ["id"]
          },
          "maxItems": 2
        },
        "pane_frame_slots": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "placement": {"enum": ["header-icon", "header-text", "footer-row", "banner"]},
              "applies_to": {}
            },
            "required": ["id", "placement"]
          },
          "maxItems": 4
        },
        "display_modes": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 32},
              "icon": {"type": "string", "minLength": 1, "maxLength": 64},
              "slot": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "hide_unmarked": {"type": "boolean"}
            },
            "required": ["id", "title", "slot"]
          },
          "maxItems": 2
        },
        "catalogs": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "item_kind": {"enum": ["text", "glyph", "swatch", "pattern"]}
            },
            "required": ["id", "title", "item_kind"]
          },
          "maxItems": 8
        },
        "client_requests": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "timeout_ms": {"type": "integer", "minimum": 1, "maximum": 30000}
            },
            "required": ["id"]
          },
          "maxItems": 16
        },
        "languages": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "id": {"type": "string", "minLength": 1, "maxLength": 64, "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"},
              "title": {"type": "string", "minLength": 1, "maxLength": 48},
              "extensions": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 16}, "maxItems": 32},
              "file_names": {"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 64}, "maxItems": 32}
            },
            "required": ["id", "title"]
          },
          "maxItems": 8
        }
      }
    },
    "limits": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "memory_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "table_elements": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "event_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "command_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "render_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "annotate_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "frame_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "detector_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 50, "description": "per detector call; 0 = default"},
        "activate_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "deactivate_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "migrate_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 60000, "description": "hard deadline of one migrate call; 0 = default"},
        "job_deadline_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "job_wall_ms": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "job_memory_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "job_io_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "concurrent_jobs": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "job_queue": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "event_queue": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "pending_requests": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "timers": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "output_filters": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "watches": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "key_grabs": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "ui_tree_nodes": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "ui_tree_kib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "decorations": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "node_metadata_kib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "storage_bucket_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "disk_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "fs_read_all_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "fs_write_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "http_response_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "process_output_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "log_lines_per_second": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "open_videos": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "video_buffer_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "audio_captures": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "http_streams": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "children": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "durable_timers": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "vector_indexes": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "vector_entries": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "shared_cache_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "journal_entries": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "toasts_pending": {"type": "integer", "minimum": 0, "maximum": 4294967295},
        "package_assets_mib": {"type": "integer", "minimum": 0, "maximum": 4294967295}
      }
    },
    "data_version": {"type": "integer", "minimum": 1, "maximum": 4294967295},
    "limits_tier": {"enum": ["standard", "first-party"]},
    "unstable": {
      "type": "array",
      "uniqueItems": true,
      "maxItems": 8,
      "items": {"enum": ["ui-calendar", "plugin-services", "gpu-kernels", "websocket"]}
    },
    "files": {
      "type": "array",
      "maxItems": 4096,
      "items": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": {"type": "string", "minLength": 1, "maxLength": 240, "pattern": "^[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*$"},
          "length": {"type": "integer", "minimum": 0, "maximum": 67108864},
          "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"}
        },
        "required": ["path", "length", "sha256"]
      }
    }
  },
  "dependentRequired": {"surface": ["replaceable"]},
  "allOf": [
    {
      "if": {"required": ["replaceable"], "properties": {"replaceable": {"const": true}}},
      "then": {"required": ["surface"]}
    }
  ],
  "$defs": {
    "grant": {
      "type": "string",
      "minLength": 3,
      "maxLength": 256,
      "pattern": "^[a-z][a-z0-9-]*(\\.[a-z0-9-]+)*(:[^\\u0000-\\u001f\\u007f]+)?$"
    }
  }
}"###;

fn semantics(manifest: &Manifest, context: &ValidationContext) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    let Some(api) = api_version(&manifest.api) else {
        return vec![finding(
            "manifest.schema",
            "/api",
            "Malformed plugin contract identity.",
            3,
        )];
    };
    let current = crate::version::CURRENT;
    if api.major != u64::from(current.major)
        || api.minor > u64::from(current.minor)
        || api.minor < u64::from(current.minor.saturating_sub(1))
    {
        return vec![finding(
            "package.api-unsupported",
            "/api",
            format!(
                "The api {} is outside the window of this Ilium.",
                manifest.api
            ),
            3,
        )];
    }
    let first = manifest.id.split('.').next().unwrap_or("");
    if ["core", "local", "test"].contains(&first) || first == "ilium" && context.tier != Tier::T1 {
        errors.push(finding(
            "manifest.id-reserved",
            "/id",
            format!("The id {:?} uses a reserved prefix.", manifest.id),
            3,
        ));
    }
    for (index, id) in manifest.accepts_messages_from.iter().enumerate() {
        if !valid_id(id) {
            errors.push(finding(
                "manifest.id-invalid",
                format!("/accepts_messages_from/{index}"),
                "Invalid peer plugin id.",
                3,
            ));
        }
    }
    if let Some(contact) = &manifest.security_contact {
        if !(contact.starts_with("https://") && contact.len() > 8
            || contact.split_once('@').is_some_and(|(local, domain)| {
                !local.is_empty() && domain.contains('.') && !domain.starts_with('.')
            }))
            || contact.chars().any(char::is_whitespace)
        {
            errors.push(finding(
                "manifest.schema",
                "/security_contact",
                "Expected an email or HTTPS security contact.",
                3,
            ));
        }
    }
    if let Some(requires) = &manifest.requires {
        if requires
            .host
            .as_deref()
            .is_some_and(|range| parse_range(range).is_none())
        {
            errors.push(finding(
                "manifest.requires-invalid",
                "/requires/host",
                "Invalid host version range.",
                3,
            ));
        }
        let mut names = BTreeSet::new();
        for (index, dependency) in requires.plugins.iter().enumerate() {
            let path = format!("/requires/plugins/{index}");
            if dependency.id == manifest.id {
                errors.push(finding(
                    "manifest.requires-self",
                    format!("{path}/id"),
                    format!(
                        "The plugin {} lists itself in requires.plugins.",
                        manifest.id
                    ),
                    3,
                ));
            }
            if !names.insert(&dependency.id) || parse_range(&dependency.version).is_none() {
                errors.push(finding(
                    "manifest.requires-invalid",
                    path,
                    "Duplicate plugin or invalid dependency range.",
                    3,
                ));
            }
        }
        let mut services = BTreeSet::new();
        for (index, service) in requires.services.iter().enumerate() {
            match super::ServiceRequirement::parse(service) {
                Ok(service) if services.insert(service.name.clone()) => {}
                Ok(_) => errors.push(finding(
                    "manifest.requires-invalid",
                    format!("/requires/services/{index}"),
                    "Duplicate service requirement.",
                    3,
                )),
                Err(mut error) => {
                    error.pointer = format!("/requires/services/{index}");
                    errors.push(error);
                }
            }
        }
    }
    if let Err(mut error) = crate::limits::clamp(&manifest.limits, context.tier) {
        error.stage = Some(Stage(3));
        errors.push(error);
    }
    for (index, feature) in manifest.unstable.iter().enumerate() {
        let path = format!("/unstable/{index}");
        if !context.unstable_features.contains(feature) {
            errors.push(finding(
                "manifest.unstable-unknown",
                path,
                format!("Unknown unstable feature {feature:?}."),
                4,
            ));
        } else if context.tier != Tier::T1 && context.mode != CheckMode::Dev {
            errors.push(finding(
                "manifest.unstable-tier",
                path,
                "Third-party unstable features require development mode.",
                9,
            ));
        }
    }
    if manifest.limits_tier == super::LimitsTier::FirstParty
        && (context.tier != Tier::T1 || !manifest.id.starts_with("ilium."))
        || manifest.replaceable && (context.tier != Tier::T1 || !manifest.id.starts_with("ilium."))
    {
        errors.push(finding(
            "manifest.limits-tier",
            "/limits_tier",
            "First-party limits and replaceability require a bundled ilium plugin.",
            9,
        ));
    }
    for (side, component, grants) in [
        (
            Side::Server,
            &manifest.server,
            &manifest.capabilities.server,
        ),
        (
            Side::Client,
            &manifest.client,
            &manifest.capabilities.client,
        ),
        (Side::Scene, &manifest.scene, &manifest.capabilities.scene),
    ] {
        let side_name = side.as_str();
        if component.is_none() && !grants.is_empty() {
            errors.push(finding(
                "manifest.grant-side",
                format!("/capabilities/{side_name}"),
                "Grant list has no component on this side.",
                4,
            ));
        }
        for (index, grant) in grants.iter().enumerate() {
            let path = format!("/capabilities/{side_name}/{index}");
            // Side validity is stage 4, independent of the later tier admission.
            if crate::grants::info(grant.family()).is_some_and(|info| !info.sides.contains(&side)) {
                errors.push(finding(
                    "manifest.grant-side",
                    &path,
                    "Grant is not available on this component side.",
                    4,
                ));
            }
            let checked = crate::grants::Grant::parse(grant.as_str())
                .and_then(|_| crate::grants::check_grant(grant, context.tier, side));
            if let Err(mut error) = checked {
                error.pointer = path.clone();
                error.stage = Some(Stage(if error.code.as_str() == "manifest.grant-tier" {
                    9
                } else {
                    4
                }));
                errors.push(error);
            }
            let has = |text: &str| grants.iter().any(|grant| grant.as_str() == text);
            if grant.as_str() == "fs.pick" && !has("fs.read:picked") && !has("fs.write:picked")
                || grant.as_str() == "settings.write-all" && !has("settings.read-all")
                || grant.family() == "panes.create-command" && !has("panes.create")
                || grant.family() == "process.long-lived"
                    && !grants
                        .iter()
                        .any(|candidate| candidate.family() == "process.run")
            {
                errors.push(finding(
                    "manifest.grant-dependency",
                    path,
                    "Required grant is missing on this side.",
                    4,
                ));
            }
        }
        if let Some(component) = component {
            let handler = entries(manifest, "extensions")
                .any(|entry| entry.get("handler").and_then(Value::as_str) == Some(side_name));
            if component
                .exports
                .iter()
                .any(|export| export == "hook-handler")
                != handler
            {
                errors.push(finding(
                    "manifest.exports-mismatch",
                    format!("/{side_name}/exports"),
                    "hook-handler must match handler extensions on this side.",
                    6,
                ));
            }
            if manifest.data_version > 1
                && side != Side::Scene
                && !component.exports.iter().any(|export| export == "migrator")
            {
                errors.push(finding(
                    "manifest.exports-mismatch",
                    format!("/{side_name}/exports"),
                    "A data version above 1 requires migrator on every server/client side.",
                    6,
                ));
            }
        }
    }
    contribution_checks(manifest, &mut errors);
    for (index, target) in manifest.replaces.iter().enumerate() {
        let path = format!("/replaces/{index}");
        if context.tier == Tier::T3 {
            errors.push(finding(
                "manifest.replaces-tier",
                &path,
                "Only bundled or trusted plugins can replace core plugins.",
                9,
            ));
        }
        match context
            .core_plugins
            .iter()
            .find(|plugin| plugin.id == *target)
        {
            None => errors.push(finding(
                "manifest.replaces-unknown",
                path,
                "Replacement target is not a known core plugin.",
                10,
            )),
            Some(plugin) if !plugin.replaceable => errors.push(finding(
                "manifest.replaces-not-replaceable",
                path,
                "Core plugin is not replaceable.",
                10,
            )),
            Some(plugin) => {
                let provided = provided_surface(manifest);
                let missing: Vec<_> = plugin
                    .surface
                    .iter()
                    .filter(|surface| !provided.contains(*surface))
                    .collect();
                if !missing.is_empty() {
                    errors.push(finding(
                        "manifest.replaces-surface-incomplete",
                        path,
                        format!("Missing replacement surface: {missing:?}."),
                        10,
                    ));
                }
            }
        }
    }
    let mut last = None;
    for (index, file) in manifest.files.iter().enumerate() {
        let path = format!("/files/{index}/path");
        if !safe_path(&file.path) || ["manifest.json", "signature"].contains(&file.path.as_str()) {
            errors.push(finding(
                if ["manifest.json", "signature"].contains(&file.path.as_str()) {
                    "manifest.file-self"
                } else {
                    "manifest.schema"
                },
                &path,
                "Inventory path is not allowed.",
                5,
            ));
        }
        if last.is_some_and(|previous: &str| previous >= file.path.as_str()) {
            errors.push(finding(
                "manifest.files-order",
                &path,
                "Inventory paths must be unique and in bytewise archive order.",
                5,
            ));
        }
        last = Some(file.path.as_str());
    }
    if !manifest.files.is_empty() || context.mode == CheckMode::Install {
        for component in [&manifest.server, &manifest.client, &manifest.scene]
            .into_iter()
            .flatten()
        {
            if !manifest
                .files
                .iter()
                .any(|file| file.path == component.component)
            {
                errors.push(finding(
                    "manifest.file-missing",
                    "/files",
                    format!("{} is missing from the inventory.", component.component),
                    5,
                ));
            }
        }
        for kind in ["help", "sounds"] {
            for (index, entry) in entries(manifest, kind).enumerate() {
                if let Some(file) = entry.get("file").and_then(Value::as_str) {
                    if !manifest.files.iter().any(|entry| entry.path == file) {
                        errors.push(finding(
                            "manifest.file-missing",
                            format!("/contributes/{kind}/{index}/file"),
                            format!("{file} is missing from the inventory."),
                            5,
                        ));
                    }
                }
            }
        }
    }
    // The supplied context has no host release, available sources or dependency
    // graphs. Resolution, signature/trust and archive-byte checks belong to the
    // installer; absence here is not evidence that those stages passed.
    errors
}

fn entries<'a>(manifest: &'a Manifest, key: &str) -> impl Iterator<Item = &'a Value> {
    manifest
        .contributes
        .0
        .get(key)
        .into_iter()
        .flat_map(|value| match value {
            Value::Array(values) => values.iter().collect::<Vec<_>>(),
            Value::Object(_) => vec![value],
            _ => Vec::new(),
        })
}

pub(super) fn parse_range(text: &str) -> Option<semver::VersionReq> {
    if text.is_empty()
        || text.len() > 128
        || text.contains("||")
        || text.contains(',')
        || text.trim() != text
        || text.contains("  ")
    {
        return None;
    }
    let normalized = text.split(' ').collect::<Vec<_>>().join(", ");
    semver::VersionReq::parse(&normalized).ok()
}

fn provided_surface(manifest: &Manifest) -> BTreeSet<String> {
    let mut surface = BTreeSet::new();
    for (key, kind, field) in [
        ("commands", "commands", "id"),
        ("pane_kinds", "pane-kinds", "id"),
        ("extension_points", "points", "id"),
        ("cli", "cli", "name"),
    ] {
        for entry in entries(manifest, key) {
            if let Some(id) = entry.get(field).and_then(Value::as_str) {
                surface.insert(format!("{kind}:{id}"));
            }
        }
    }
    for (key, kind, field) in [
        ("services", "services", "provides"),
        ("topics", "topics", "publishes"),
    ] {
        for entry in entries(manifest, key) {
            for item in entry
                .get(field)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(name) = item
                    .as_str()
                    .or_else(|| item.get("name").and_then(Value::as_str))
                {
                    surface.insert(format!("{kind}:{name}"));
                }
            }
        }
    }
    surface
}

fn contribution_checks(manifest: &Manifest, errors: &mut Vec<Diagnostic>) {
    if let Some(publishes) = manifest
        .contributes
        .0
        .get("topics")
        .and_then(|topics| topics.get("publishes"))
        .and_then(Value::as_array)
    {
        let prefix = format!("{}/", manifest.id);
        for (index, topic) in publishes.iter().enumerate() {
            if topic
                .as_str()
                .is_some_and(|topic| !topic.starts_with(&prefix))
            {
                errors.push(finding(
                    "manifest.contribution-target",
                    format!("/contributes/topics/publishes/{index}"),
                    "Published topics must use the plugin's own id prefix.",
                    4,
                ));
            }
        }
    }
    let owns = |key: &str, target: &str| {
        let local = target
            .strip_prefix(&format!("{}/", manifest.id))
            .unwrap_or(target);
        entries(manifest, key).any(|entry| {
            entry
                .get("id")
                .or_else(|| entry.get("name"))
                .and_then(Value::as_str)
                == Some(local)
        })
    };
    for (kind, value) in &manifest.contributes.0 {
        let mut ids = BTreeSet::new();
        for (index, entry) in entries(manifest, kind).enumerate() {
            let path = format!("/contributes/{kind}/{index}");
            if let Some(id) = entry
                .get(if kind == "cli" { "name" } else { "id" })
                .and_then(Value::as_str)
            {
                if !ids.insert(id) {
                    let code = match kind.as_str() {
                        "cli" => "manifest.cli-name-taken",
                        "sounds" => "manifest.sound-id-taken",
                        _ => "manifest.schema",
                    };
                    errors.push(finding(
                        code,
                        &path,
                        "Contribution identifier occurs twice in its list.",
                        4,
                    ));
                }
                if kind == "sounds" && id == "chirping" {
                    errors.push(finding(
                        "manifest.sound-id-taken",
                        format!("{path}/id"),
                        "The sound id chirping is reserved.",
                        4,
                    ));
                }
            }
            if let Some(chord) = entry.get("chord").and_then(Value::as_str) {
                if let Err(mut error) = super::parse_chord(chord) {
                    error.pointer = format!("{path}/chord");
                    errors.push(error);
                }
            }
            if let Some(mode) = entry
                .get("mode")
                .and_then(Value::as_str)
                .filter(|_| kind == "keybindings")
            {
                if !["global", "prefix"].contains(&mode)
                    && mode
                        .strip_prefix("pane:")
                        .is_none_or(|kind| kind.is_empty())
                {
                    errors.push(finding(
                        "manifest.schema",
                        format!("{path}/mode"),
                        "Invalid keybinding mode.",
                        3,
                    ));
                }
            }
            for (field, target_kind) in [
                ("command", "commands"),
                ("drop_command", "commands"),
                ("popup_view", "panels"),
                ("slot", "decoration_slots"),
                ("help", "help"),
            ] {
                if let Some(target) = entry.get(field).and_then(Value::as_str) {
                    if !owns(target_kind, target) {
                        errors.push(finding(
                            "manifest.contribution-target",
                            format!("{path}/{field}"),
                            format!("No own {target_kind} contribution named {target:?}."),
                            4,
                        ));
                    }
                }
            }
            if kind == "commands" {
                if let Some(side) = entry.get("side").and_then(Value::as_str) {
                    if side == "server" && manifest.server.is_none()
                        || side == "client" && manifest.client.is_none()
                    {
                        errors.push(finding(
                            "manifest.contribution-target",
                            format!("{path}/side"),
                            "Command side has no component.",
                            4,
                        ));
                    }
                }
            }
        }
        check_embedded_fields(value, &format!("/contributes/{kind}"), errors);
    }
    if manifest.contributes.0.contains_key("scenes") && manifest.scene.is_none() {
        errors.push(finding(
            "manifest.contribution-target",
            "/contributes/scenes",
            "Scenes require a scene component.",
            4,
        ));
    }
    for (index, trigger) in manifest.activation.iter().enumerate() {
        let path = format!("/activation/{index}");
        match super::parse_activation(trigger) {
            Err(mut error) => {
                error.pointer = path;
                errors.push(error);
            }
            Ok(_) => {
                if let Some((kind, target)) = trigger.split_once(':') {
                    let key = match kind {
                        "command" => Some("commands"),
                        "view" => Some("panels"),
                        "pane-kind" => Some("pane_kinds"),
                        "scene" => Some("scenes"),
                        "cli" => Some("cli"),
                        "settings" => Some("settings"),
                        "catalog" => Some("catalogs"),
                        "menu" => Some("context_menu"),
                        "virtual-rows" => Some("virtual_rows"),
                        "language" => Some("languages"),
                        _ => None,
                    };
                    if let Some(key) = key {
                        if !owns(key, target) {
                            errors.push(finding(
                                "manifest.contribution-target",
                                path,
                                "Activation target is not an own contribution.",
                                4,
                            ));
                        }
                    }
                }
            }
        }
    }
    // Provider declarations and their exports must agree on at least one side.
    for (kind, export) in [
        ("catalogs", "catalog-provider"),
        ("languages", "token-provider"),
        ("services", "service-provider"),
    ] {
        let present = entries(manifest, kind).next().is_some();
        let exported = [&manifest.server, &manifest.client, &manifest.scene]
            .into_iter()
            .flatten()
            .any(|side| side.exports.iter().any(|name| name == export));
        if present && !exported {
            errors.push(finding(
                "manifest.exports-mismatch",
                format!("/contributes/{kind}"),
                format!("{kind} requires the {export} export."),
                6,
            ));
        }
        if kind == "catalogs" && exported && !present {
            errors.push(finding(
                "manifest.catalog-unknown",
                "/contributes/catalogs",
                "catalog-provider requires a catalogs entry.",
                4,
            ));
        }
    }
    if entries(manifest, "languages").next().is_some()
        && !manifest
            .capabilities
            .client
            .iter()
            .any(|grant| grant.as_str() == "editor.read")
    {
        errors.push(finding(
            "manifest.grant-dependency",
            "/contributes/languages",
            "Languages require client editor.read.",
            4,
        ));
    }
    let all_grants: Vec<_> = manifest
        .capabilities
        .server
        .iter()
        .chain(&manifest.capabilities.client)
        .chain(&manifest.capabilities.scene)
        .map(|grant| grant.as_str())
        .collect();
    for (kind, family) in [
        ("panels", "ui.panels"),
        ("tree_toolbar", "ui.chrome"),
        ("row_actions", "ui.chrome"),
        ("tree_footer", "ui.chrome"),
        ("status_segments", "ui.chrome"),
        ("header_widgets", "ui.chrome"),
    ] {
        if entries(manifest, kind).next().is_some() && !all_grants.contains(&family) {
            errors.push(finding(
                "manifest.grant-dependency",
                format!("/contributes/{kind}"),
                format!("{kind} requires {family}."),
                4,
            ));
        }
    }
    for (index, entry) in entries(manifest, "cli").enumerate() {
        if let Some(name) = entry.get("name").and_then(Value::as_str) {
            if ["plugin", "session", "kill-session", "help", "version"].contains(&name) {
                errors.push(finding(
                    "manifest.cli-name-reserved",
                    format!("/contributes/cli/{index}/name"),
                    format!("The command line name {name:?} is a kernel subcommand."),
                    4,
                ));
            }
        }
    }
    for (index, point) in entries(manifest, "extension_points").enumerate() {
        let path = format!("/contributes/extension_points/{index}");
        let kind = point.get("kind").and_then(Value::as_str);
        let mode = point.get("mode").and_then(Value::as_str);
        let high = point.get("risk").and_then(Value::as_str) == Some("high");
        let invalid = if kind == Some("collect") {
            point.get("entry_schema").is_none()
                || [
                    "mode",
                    "payload_schema",
                    "reply_schema",
                    "on_failure",
                    "final_check",
                    "deadline_ms",
                    "max_handlers",
                ]
                .iter()
                .any(|key| point.get(*key).is_some())
        } else {
            mode.is_none()
                || point.get("payload_schema").is_none()
                || (matches!(mode, Some("transform" | "first"))
                    != point.get("reply_schema").is_some())
                || point.get("entry_schema").is_some()
                || point.get("max_entries").is_some()
                || point.get("dynamic").is_some()
                || (point.get("on_failure").is_some()
                    && !(high && matches!(mode, Some("veto" | "transform"))))
                || (point.get("final_check").is_some() && mode != Some("transform"))
        };
        if invalid {
            errors.push(finding(
                "manifest.extension-point-invalid",
                path,
                "The extension point fields do not match its kind, mode and risk.",
                4,
            ));
        }
    }
    let mut service_names = BTreeSet::new();
    for provider in entries(manifest, "services").flat_map(|value| {
        value
            .get("provides")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
    }) {
        let Some(name) = provider.get("name").and_then(Value::as_str) else {
            continue;
        };
        let own = name
            .strip_prefix(&format!("{}/", manifest.id))
            .is_some_and(|local| local.len() <= 64 && local_id(local));
        let replacement = manifest
            .replaces
            .iter()
            .any(|plugin| name.starts_with(&format!("{plugin}/")));
        if !own && !replacement || !service_names.insert(name) {
            errors.push(finding(
                "manifest.service-name",
                "/contributes/services/provides",
                format!("Service {name:?} has a foreign prefix or duplicate name."),
                4,
            ));
        }
        if let Some(path) = provider.get("schema").and_then(Value::as_str) {
            if !manifest.files.is_empty() && !manifest.files.iter().any(|file| file.path == path) {
                errors.push(finding(
                    "manifest.file-missing",
                    "/contributes/services/provides",
                    format!("Service schema {path} is not in files."),
                    5,
                ));
            }
        }
    }
}

fn check_embedded_fields(value: &Value, path: &str, errors: &mut Vec<Diagnostic>) {
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                check_embedded_fields(value, &pointer(path, &index.to_string()), errors);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                let path = pointer(path, key);
                if [
                    "arguments_schema",
                    "entry_schema",
                    "payload_schema",
                    "reply_schema",
                    "settings_schema",
                ]
                .contains(&key.as_str())
                {
                    check_data_schema(value, &path, errors);
                    continue;
                }
                if key == "entry" {
                    continue;
                }
                if [
                    "title",
                    "label",
                    "summary",
                    "description",
                    "short_title",
                    "caption",
                ]
                .contains(&key.as_str())
                    && value.as_str().is_some_and(|text| !display(text))
                {
                    errors.push(finding(
                        "manifest.display-name",
                        &path,
                        "The text contains a control, line or bidirectional character.",
                        3,
                    ));
                }
                if key == "when" {
                    if let Some(expression) = value.as_str() {
                        if let Err(mut error) = super::parse_when(expression) {
                            error.pointer = path.clone();
                            errors.push(error);
                        }
                    }
                }
                if value.is_null() {
                    errors.push(finding(
                        "manifest.schema",
                        &path,
                        "null is not a manifest value.",
                        3,
                    ));
                }
                check_embedded_fields(value, &path, errors);
            }
        }
        _ => {}
    }
}

fn check_data_schema(value: &Value, path: &str, errors: &mut Vec<Diagnostic>) {
    const KEYS: &[&str] = &[
        "type",
        "properties",
        "required",
        "enum",
        "title",
        "description",
        "default",
        "minimum",
        "maximum",
        "pattern",
        "maxLength",
        "help",
        "step",
        "unit",
        "secret",
        "multiline",
        "enum_titles",
        "picker",
        "extensions",
        "items",
        "maxItems",
    ];
    let Some(object) = value.as_object() else {
        errors.push(finding(
            "manifest.schema",
            path,
            "A data schema must be an object.",
            3,
        ));
        return;
    };
    for (key, value) in object {
        let path = pointer(path, key);
        if !KEYS.contains(&key.as_str()) {
            errors.push(finding(
                "manifest.schema-keyword",
                &path,
                format!("The schema keyword {key:?} is not supported."),
                3,
            ));
            continue;
        }
        if key == "properties" {
            if let Some(properties) = value.as_object() {
                for (name, schema) in properties {
                    check_data_schema(schema, &pointer(&path, name), errors);
                }
            } else {
                errors.push(finding(
                    "manifest.schema",
                    &path,
                    "properties must be an object.",
                    3,
                ));
            }
        }
        if key == "items" {
            check_data_schema(value, &path, errors);
        }
        if key == "type"
            && !value.as_str().is_some_and(|name| {
                ["object", "array", "boolean", "integer", "number", "string"].contains(&name)
            })
        {
            errors.push(finding(
                "manifest.schema",
                &path,
                "Unsupported data schema type.",
                3,
            ));
        }
        if key == "maxItems" && value.as_u64().is_none_or(|max| max > 64) {
            errors.push(finding(
                "manifest.schema",
                &path,
                "maxItems must be an integer at most 64.",
                3,
            ));
        }
    }
}

// Nested settings records intentionally have their own contract, beyond the
// published outer schema's object placeholders.
fn check_settings_rows(
    value: &Value,
    path: &str,
    inside_group: bool,
    keys: &mut BTreeSet<String>,
    errors: &mut Vec<Diagnostic>,
) {
    if let Some(rows) = value.get("rows").and_then(Value::as_array) {
        for (index, row) in rows.iter().enumerate() {
            let path = format!("{path}/rows/{index}");
            let Some(object) = row.as_object() else {
                errors.push(finding(
                    "manifest.schema",
                    &path,
                    "A settings row must be an object.",
                    3,
                ));
                continue;
            };
            let kind = row.get("type").and_then(Value::as_str).unwrap_or("");
            let fields = match kind {
                "toggle" => "on_label off_label style",
                "choice" => {
                    "options source style dynamic filter allow_custom custom_pattern view_modes"
                }
                "number" => {
                    "minimum maximum step unit integer style steps special_values debounce_ms"
                }
                "text" => "max_length pattern placeholder secret reveal_last debounce_ms",
                "text-area" => "max_length language debounce_ms",
                "path" => "picker extensions",
                "color" => "swatches",
                "keybinding" => "command",
                "button" => "label command confirm",
                "info" => "text live rich",
                "group" => "rows collapsed",
                "list" => {
                    "item max_items item_format style separator reorderable remote item_status"
                }
                "location" => "search allow_current precision",
                "preview" => "view height live",
                "notice" => "text tone dismissible",
                "map" => "value_type key_pattern max_entries key_title value_title",
                "multi-choice" => "options source min_items max_items style",
                "meter" => "segments max show_values",
                _ => {
                    errors.push(finding(
                        "manifest.settings-row-type",
                        format!("{path}/type"),
                        format!("Unknown row type {kind:?}."),
                        3,
                    ));
                    continue;
                }
            };
            let has_value =
                !["button", "info", "group", "preview", "notice", "meter"].contains(&kind);
            let common = "type title description help depends_on keywords";
            for key in object.keys() {
                let allowed = common
                    .split_whitespace()
                    .chain(fields.split_whitespace())
                    .any(|field| field == key)
                    || (has_value
                        && [
                            "key",
                            "default",
                            "reset",
                            "visible_when",
                            "scope",
                            "applies",
                        ]
                        .contains(&key.as_str()))
                    || (!has_value && kind != "preview" && key == "id");
                if !allowed {
                    errors.push(finding(
                        "manifest.settings-row-field",
                        pointer(&path, key),
                        format!("Field {key:?} is not valid for a {kind} row."),
                        3,
                    ));
                }
            }
            if kind == "group" && inside_group {
                errors.push(finding(
                    "manifest.settings-row-field",
                    &path,
                    "Groups cannot nest.",
                    3,
                ));
            }
            if has_value {
                if let Some(key) = row.get("key").and_then(Value::as_str) {
                    let mut characters = key.bytes();
                    let valid = key.len() <= 64
                        && characters
                            .next()
                            .is_some_and(|byte| byte.is_ascii_lowercase())
                        && characters.all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                        });
                    if !valid {
                        errors.push(finding(
                            "manifest.settings-key",
                            format!("{path}/key"),
                            "Invalid settings key.",
                            3,
                        ));
                    } else if !keys.insert(key.into()) {
                        errors.push(finding(
                            "manifest.settings-key-clash",
                            format!("{path}/key"),
                            format!("The settings key {key:?} is used twice."),
                            4,
                        ));
                    }
                }
                if row.get("key").and_then(Value::as_str).is_none() {
                    errors.push(finding(
                        "manifest.key-missing",
                        format!("{path}/key"),
                        "A value row requires a key.",
                        3,
                    ));
                }
                let default = row.get("default");
                let valid = default.is_some_and(|value| match kind {
                    "toggle" => value.is_boolean(),
                    "number" => {
                        value.is_number()
                            && (row.get("integer") == Some(&Value::Bool(false))
                                || value.as_i64().is_some()
                                || value.as_u64().is_some())
                            && value.as_f64().is_some_and(|number| {
                                row.get("minimum")
                                    .and_then(Value::as_f64)
                                    .is_none_or(|min| number >= min)
                                    && row
                                        .get("maximum")
                                        .and_then(Value::as_f64)
                                        .is_none_or(|max| number <= max)
                            })
                    }
                    "text" | "text-area" | "path" | "color" | "keybinding" => value.is_string(),
                    "list" | "multi-choice" => value.is_array(),
                    "map" | "location" => value.is_object(),
                    "choice" => {
                        row.get("source").is_some()
                            || row
                                .get("options")
                                .and_then(Value::as_array)
                                .is_some_and(|options| {
                                    options
                                        .iter()
                                        .any(|option| option.get("value") == Some(value))
                                })
                    }
                    _ => false,
                });
                if !valid {
                    errors.push(finding(
                        "manifest.settings-default",
                        format!("{path}/default"),
                        "The default does not match the row.",
                        3,
                    ));
                }
            }
            if ["choice", "multi-choice"].contains(&kind)
                && row.get("options").is_some() == row.get("source").is_some()
            {
                errors.push(finding(
                    "manifest.settings-source",
                    &path,
                    "A row needs exactly one of options and source.",
                    3,
                ));
            }
            check_settings_rows(row, &path, kind == "group", keys, errors);
        }
    }
    if let Some(sections) = value.get("sections").and_then(Value::as_array) {
        for (index, section) in sections.iter().enumerate() {
            let path = format!("{path}/sections/{index}");
            if let Some(object) = section.as_object() {
                for key in object.keys().filter(|key| {
                    !["id", "title", "scope", "rows", "collapsed"].contains(&key.as_str())
                }) {
                    errors.push(finding(
                        "manifest.schema",
                        pointer(&path, key),
                        "Unknown inline section field.",
                        3,
                    ));
                }
            }
            check_settings_rows(section, &path, false, keys, errors);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        json_schema, parse_manifest, validate, validate_json, CheckMode, ValidationContext,
    };
    use crate::error::Severity;
    use crate::manifest::{Diagnostic, Manifest, Stage};
    use crate::platform::Platform;
    use crate::tier::Tier;
    use serde_json::{json, Value};

    /// The four unstable features of version 0.1.0 (reference-abi-evolution.md, ABI-022).
    const UNSTABLE_FEATURES: [&str; 4] =
        ["ui-calendar", "plugin-services", "gpu-kernels", "websocket"];

    fn context(tier: Tier, mode: CheckMode) -> ValidationContext {
        ValidationContext {
            tier,
            mode,
            platform: Platform::Linux,
            installed: Vec::new(),
            core_plugins: Vec::new(),
            unstable_features: UNSTABLE_FEATURES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        }
    }

    /// `ilium plugin check` runs the rules of a T3 source package.
    fn check_context() -> ValidationContext {
        context(Tier::T3, CheckMode::Check)
    }

    /// A minimal valid source manifest with a server component (reference-manifest.md).
    fn valid_server_manifest() -> Value {
        json!({
            "api": "ilium:plugin@0.1.0",
            "id": "example.demo",
            "name": "Example demo",
            "version": "0.1.0",
            "publisher": "Example Publisher",
            "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }
        })
    }

    fn with_key(mut value: Value, key: &str, new_value: Value) -> Value {
        value[key] = new_value;
        value
    }

    fn check_text(text: &str) -> Vec<Diagnostic> {
        validate_json(text, &check_context())
    }

    fn check_value(value: &Value) -> Vec<Diagnostic> {
        check_text(&value.to_string())
    }

    fn codes(found: &[Diagnostic]) -> Vec<String> {
        found
            .iter()
            .map(|diagnostic| diagnostic.code.as_str().to_owned())
            .collect()
    }

    fn assert_has_code(found: &[Diagnostic], expected_code: &str) {
        let found_codes = codes(found);
        assert!(
            found_codes.iter().any(|code| code == expected_code),
            "expected code {expected_code}, got {found_codes:?}"
        );
    }

    fn assert_lacks_code(found: &[Diagnostic], unexpected_code: &str) {
        let found_codes = codes(found);
        assert!(
            !found_codes.iter().any(|code| code == unexpected_code),
            "code {unexpected_code} must not be reported, got {found_codes:?}"
        );
    }

    fn assert_no_errors(found: &[Diagnostic]) {
        assert!(
            found.is_empty(),
            "a valid manifest must have no diagnostics, got {:?}",
            codes(found)
        );
    }

    fn decode(value: &Value) -> Manifest {
        serde_json::from_value(value.clone())
            .expect("a manifest with all required keys must deserialize")
    }

    /// REQ: PKG-561
    #[test]
    fn valid_server_manifest_has_no_diagnostics() {
        assert_no_errors(&check_value(&valid_server_manifest()));
    }

    /// REQ: PKG-561
    #[test]
    fn valid_manifest_passes_validate_on_the_decoded_struct() {
        let manifest = decode(&valid_server_manifest());
        assert_no_errors(&validate(&manifest, &check_context()));
    }

    /// REQ: PKG-561
    #[test]
    fn parse_manifest_accepts_well_formed_json() {
        let manifest = parse_manifest(&valid_server_manifest().to_string())
            .expect("well-formed JSON must parse at stage 2");
        assert_eq!(manifest.id, "example.demo");
        assert_eq!(manifest.version, "0.1.0");
    }

    /// REQ: PKG-576
    #[test]
    fn parse_manifest_empty_input_is_json_invalid_at_stage_two() {
        let found = parse_manifest("").expect_err("empty input is not JSON");
        assert_eq!(found.len(), 1, "stage 2 stops at the first error");
        assert_has_code(&found, "manifest.json-invalid");
        assert_eq!(found[0].stage, Some(Stage(2)));
    }

    /// REQ: PKG-576
    #[test]
    fn validate_json_empty_input_is_json_invalid() {
        let found = check_text("");
        assert_has_code(&found, "manifest.json-invalid");
    }

    /// REQ: PKG-576
    #[test]
    fn parse_manifest_rejects_byte_order_mark_as_json_invalid() {
        let text = format!("\u{feff}{}", valid_server_manifest());
        let found = parse_manifest(&text).expect_err("a BOM at the start of the file is refused");
        assert_has_code(&found, "manifest.json-invalid");
    }

    /// REQ: PKG-576
    #[test]
    fn parse_manifest_repeated_key_is_duplicate_key_at_stage_two() {
        let text = r#"{"api": "ilium:plugin@0.1.0", "id": "example.demo", "id": "example.other"}"#;
        let found = parse_manifest(text).expect_err("a repeated key is refused at stage 2");
        assert_eq!(found.len(), 1, "no later stage runs after a duplicate key");
        assert_has_code(&found, "manifest.duplicate-key");
        assert_eq!(found[0].stage, Some(Stage(2)));
    }

    /// REQ: PKG-562
    #[test]
    fn empty_object_reports_each_missing_required_key() {
        let found = check_text("{}");
        for key in ["api", "id", "name", "version", "publisher"] {
            let expected_message_part = format!("\"{key}\"");
            assert!(
                found.iter().any(
                    |diagnostic| diagnostic.code.as_str() == "manifest.key-missing"
                        && diagnostic.message.contains(&expected_message_part)
                ),
                "expected manifest.key-missing for {key}, got {:?}",
                codes(&found)
            );
        }
    }

    /// REQ: PKG-562
    #[test]
    fn empty_object_also_reports_missing_component_as_schema_error() {
        let found = check_text("{}");
        assert_has_code(&found, "manifest.schema");
    }

    /// REQ: PKG-562
    #[test]
    fn json_array_root_is_schema_error_at_the_root_pointer() {
        let found = check_text("[]");
        assert!(
            found
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == "manifest.schema"
                    && diagnostic.pointer.is_empty()),
            "a JSON array must be refused by manifest.schema at pointer \"\", got {:?}",
            codes(&found)
        );
    }

    /// REQ: PKG-562
    #[test]
    fn unknown_top_level_key_is_unknown_key_at_its_pointer() {
        let manifest = with_key(valid_server_manifest(), "bogus", json!(true));
        let found = check_value(&manifest);
        assert!(
            found.iter().any(
                |diagnostic| diagnostic.code.as_str() == "manifest.unknown-key"
                    && diagnostic.pointer == "/bogus"
            ),
            "expected manifest.unknown-key at /bogus, got {:?}",
            codes(&found)
        );
    }

    /// REQ: PKG-561
    #[test]
    fn manifest_without_any_component_is_refused_as_schema_error() {
        let mut manifest = valid_server_manifest();
        manifest.as_object_mut().expect("object").remove("server");
        let found = check_value(&manifest);
        assert_has_code(&found, "manifest.schema");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_one_segment_is_invalid_with_schema_stage() {
        let found = check_value(&with_key(valid_server_manifest(), "id", json!("demo")));
        assert_has_code(&found, "manifest.id-invalid");
        let invalid = found
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == "manifest.id-invalid")
            .expect("id-invalid is present");
        assert_eq!(invalid.stage, Some(Stage(3)));
        assert_eq!(invalid.pointer, "/id");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_uppercase_letters_is_invalid() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "id",
            json!("Example.Demo"),
        ));
        assert_has_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_empty_segment_is_invalid() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "id",
            json!("example..demo"),
        ));
        assert_has_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_segment_starting_with_hyphen_is_invalid() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "id",
            json!("example.-demo"),
        ));
        assert_has_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_ilium_prefix_is_reserved() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "id",
            json!("ilium.demo"),
        ));
        assert_has_code(&found, "manifest.id-reserved");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_eight_segments_of_fifteen_characters_is_the_accepted_maximum() {
        let id = ["abcdefghijklmno"; 8].join(".");
        assert_eq!(
            id.len(),
            127,
            "fixture must stay within the 128 character limit"
        );
        let found = check_value(&with_key(valid_server_manifest(), "id", json!(id)));
        assert_lacks_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_with_nine_segments_is_invalid() {
        let id = ["a"; 9].join(".");
        let found = check_value(&with_key(valid_server_manifest(), "id", json!(id)));
        assert_has_code(&found, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn id_segment_boundary_is_thirty_two_characters() {
        let accepted = format!("example.{}", "a".repeat(32));
        let refused = format!("example.{}", "a".repeat(33));
        assert_lacks_code(
            &check_value(&with_key(valid_server_manifest(), "id", json!(accepted))),
            "manifest.id-invalid",
        );
        assert_has_code(
            &check_value(&with_key(valid_server_manifest(), "id", json!(refused))),
            "manifest.id-invalid",
        );
    }

    /// REQ: PKG-016
    #[test]
    fn version_with_leading_v_is_invalid() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "version",
            json!("v1.0.0"),
        ));
        assert_has_code(&found, "manifest.version-invalid");
    }

    /// REQ: PKG-016
    #[test]
    fn version_with_two_numeric_parts_is_invalid() {
        let found = check_value(&with_key(valid_server_manifest(), "version", json!("1.0")));
        assert_has_code(&found, "manifest.version-invalid");
    }

    /// REQ: PKG-016
    #[test]
    fn version_with_prerelease_and_build_metadata_is_valid() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "version",
            json!("1.0.0-rc.1+build.5"),
        ));
        assert_lacks_code(&found, "manifest.version-invalid");
    }

    /// REQ: PKG-013
    #[test]
    fn api_outside_the_window_is_api_unsupported() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "api",
            json!("ilium:plugin@9.0.0"),
        ));
        assert_has_code(&found, "package.api-unsupported");
    }

    /// REQ: PKG-013
    #[test]
    fn api_that_is_not_well_formed_is_schema_error() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "api",
            json!("ilium:plugin"),
        ));
        assert_has_code(&found, "manifest.schema");
        assert_lacks_code(&found, "package.api-unsupported");
    }

    /// REQ: PKG-009
    #[test]
    fn name_with_control_character_is_display_name_error_at_its_pointer() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "name",
            json!("Demo\u{7}"),
        ));
        let display = found
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == "manifest.display-name")
            .expect("a control character in name is refused");
        assert_eq!(display.pointer, "/name");
        assert_eq!(display.severity, Severity::Error);
    }

    /// REQ: PKG-009
    #[test]
    fn publisher_with_bidirectional_override_is_display_name_error() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "publisher",
            json!("Ex\u{202E}ample"),
        ));
        assert_has_code(&found, "manifest.display-name");
    }

    /// REQ: PKG-009
    #[test]
    fn description_with_line_separator_is_display_name_error() {
        let mut manifest = valid_server_manifest();
        manifest["description"] = json!("first\u{2028}second");
        let found = check_value(&manifest);
        assert_has_code(&found, "manifest.display-name");
    }

    /// REQ: LIF-100
    #[test]
    fn absent_data_version_defaults_to_one() {
        let manifest = decode(&valid_server_manifest());
        assert_eq!(manifest.data_version, 1);
        assert_lacks_code(
            &check_value(&valid_server_manifest()),
            "manifest.data-version",
        );
    }

    /// REQ: LIF-100
    #[test]
    fn data_version_zero_is_refused() {
        let found = check_value(&with_key(valid_server_manifest(), "data_version", json!(0)));
        assert_has_code(&found, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn data_version_as_string_is_refused() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "data_version",
            json!("2"),
        ));
        assert_has_code(&found, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn data_version_at_the_u32_maximum_is_accepted() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "data_version",
            json!(4_294_967_295_u64),
        ));
        assert_lacks_code(&found, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn data_version_above_the_u32_maximum_is_refused() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "data_version",
            json!(4_294_967_296_u64),
        ));
        assert_has_code(&found, "manifest.data-version");
    }

    /// REQ: PKG-323
    #[test]
    fn plugin_requiring_itself_is_requires_self_at_semantic_stage() {
        let manifest = with_key(
            valid_server_manifest(),
            "requires",
            json!({ "plugins": [{ "id": "example.demo", "version": "*" }] }),
        );
        let found = check_value(&manifest);
        let requires_self = found
            .iter()
            .find(|diagnostic| diagnostic.code.as_str() == "manifest.requires-self")
            .expect("a plugin that lists its own id is refused");
        assert_eq!(requires_self.stage, Some(Stage(4)));
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_name_not_known_to_host_is_unstable_unknown() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "unstable",
            json!(["nope"]),
        ));
        assert_has_code(&found, "manifest.unstable-unknown");
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_feature_outside_development_mode_is_unstable_tier() {
        let context = context(Tier::T3, CheckMode::Check);
        let manifest = with_key(valid_server_manifest(), "unstable", json!(["websocket"]));
        let found = validate_json(&manifest.to_string(), &context);
        assert_has_code(&found, "manifest.unstable-tier");
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_feature_in_development_mode_is_allowed_for_t3() {
        let context = context(Tier::T3, CheckMode::Dev);
        let manifest = with_key(valid_server_manifest(), "unstable", json!(["websocket"]));
        let found = validate_json(&manifest.to_string(), &context);
        assert_lacks_code(&found, "manifest.unstable-tier");
    }

    /// REQ: PKG-034
    #[test]
    fn unstable_feature_in_bundled_t1_package_is_allowed_outside_development_mode() {
        let context = context(Tier::T1, CheckMode::Check);
        let manifest = with_key(valid_server_manifest(), "unstable", json!(["websocket"]));
        let found = validate_json(&manifest.to_string(), &context);
        assert_lacks_code(&found, "manifest.unstable-tier");
    }

    /// REQ: PKG-562
    #[test]
    fn several_stage_three_failures_are_all_collected() {
        let mut manifest = valid_server_manifest();
        manifest["id"] = json!("demo");
        manifest["version"] = json!("1.0");
        manifest["data_version"] = json!(0);
        let found = check_value(&manifest);
        assert_has_code(&found, "manifest.id-invalid");
        assert_has_code(&found, "manifest.version-invalid");
        assert_has_code(&found, "manifest.data-version");
    }

    /// REQ: PKG-571
    #[test]
    fn every_diagnostic_is_an_error_with_a_stage_and_pointer_where_a_place_exists() {
        let found = check_value(&with_key(
            valid_server_manifest(),
            "name",
            json!("Demo\u{7}"),
        ));
        assert!(!found.is_empty());
        for diagnostic in &found {
            assert_eq!(diagnostic.severity, Severity::Error, "{diagnostic}");
            assert!(
                diagnostic.stage.is_some(),
                "every catalogue error has a stage: {diagnostic}"
            );
        }
    }

    /// REQ: PKG-560
    #[test]
    fn json_schema_declares_draft_2020_12_and_its_urn_id() {
        let schema: Value =
            serde_json::from_str(json_schema()).expect("the embedded schema is JSON");
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(schema["$id"], "urn:ilium:schema:plugin-manifest:0.1.0");
    }

    /// REQ: PKG-561
    #[test]
    fn json_schema_requires_the_five_identity_keys() {
        let schema: Value =
            serde_json::from_str(json_schema()).expect("the embedded schema is JSON");
        let required: Vec<&str> = schema["required"]
            .as_array()
            .expect("required is an array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for key in ["api", "id", "name", "version", "publisher"] {
            assert!(
                required.contains(&key),
                "schema must require {key}, got {required:?}"
            );
        }
    }

    /// REQ: PKG-563
    #[test]
    fn json_schema_is_the_same_embedded_text_on_every_call() {
        assert_eq!(json_schema(), json_schema());
    }
}
