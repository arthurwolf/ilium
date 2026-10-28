//! Fixture files and expectation files (reference-test-harness.md "Fixtures and expectation files").

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use ilium_plugin_sdk_core::host::process::ProcessExit;
use ilium_plugin_sdk_core::host::http::HttpResponse;
use ilium_plugin_sdk_core::types::{AgentActivity, Event, NodeClass, Outcome, RequestRef, RequestResult};
use serde_json::{json, Value};

use crate::errors::{codes, HarnessError};
use crate::fakes::tree::TreeNode;
use crate::host::{decode_base64, FakeHost};

/// A parsed fixture with `"schema": 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixture {
    /// The whole document; keys are validated against the schema with `fixture.unknown-key` and a JSON path.
    pub document: serde_json::Value,
}

/// Outcome of one fixture run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureStatus {
    /// Matched the expectation file, or has none (then `test.no-expectation` is a warning).
    Pass,
    /// Differs from the expectation.
    Fail(String),
    /// Trapped with the given trap code.
    Trap(String),
}

/// Keys of a server or client fixture.
const HOST_KEYS: &[&str] = &[
    "schema", "grants", "tier", "side", "clock", "tree", "panes", "agents", "settings", "storage",
    "files", "http", "processes", "events", "completions", "lifecycle", "expect_state",
];
/// Keys of a scene fixture (DEV-514).
const SCENE_KEYS: &[&str] = &["schema", "side", "settings", "clock", "lifecycle", "expect_state", "frames"];
const TREE_KEYS: &[&str] = &["id", "parent", "name", "class", "content_kind", "agent"];
const AGENT_KEYS: &[&str] = &["signature", "activity", "objective", "goal"];
const AGENT_ENTRY_KEYS: &[&str] = &["name", "signature", "activity"];
const EVENT_KEYS: &[&str] = &["tag", "val"];
const HTTP_KEYS: &[&str] = &["method", "url", "status", "body", "headers"];
const PROCESS_KEYS: &[&str] = &["program", "arguments", "status", "signal", "stdout", "stderr"];
const LIFECYCLE_KEYS: &[&str] = &["step", "value"];
const LIFECYCLE_STEPS: &[&str] = &["trap", "advance", "restart", "grant", "revoke"];

fn bad_value(path: &str, detail: &str) -> HarnessError {
    HarnessError::new(codes::FIXTURE_BAD_VALUE, format!("{path}: {detail}"))
}

fn unknown_key(path: &str) -> HarnessError {
    HarnessError::new(codes::FIXTURE_UNKNOWN_KEY, path.to_string())
}

/// Reads a 64-bit id written as a number or, above 2^53, as a decimal string (DEV-513).
fn as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        _ => None,
    }
}

/// Refuses keys outside `allowed` at `path`.
fn check_keys(object: &serde_json::Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), HarnessError> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(unknown_key(&format!("{path}.{key}")));
        }
    }
    Ok(())
}

fn as_object<'a>(value: &'a Value, path: &str) -> Result<&'a serde_json::Map<String, Value>, HarnessError> {
    value.as_object().ok_or_else(|| bad_value(path, "expected an object"))
}

fn as_array<'a>(value: &'a Value, path: &str) -> Result<&'a Vec<Value>, HarnessError> {
    value.as_array().ok_or_else(|| bad_value(path, "expected a list"))
}

fn as_text<'a>(value: &'a Value, path: &str) -> Result<&'a str, HarnessError> {
    value.as_str().ok_or_else(|| bad_value(path, "expected a string"))
}

/// Parses `YYYY-MM-DD HH:mm:ss` UTC to unix milliseconds.
fn parse_clock(text: &str) -> Option<u64> {
    let (date, time) = text.split_once(' ')?;
    let mut date_parts = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date_parts.next()??, date_parts.next()??, date_parts.next()??);
    let mut time_parts = time.split(':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (time_parts.next()??, time_parts.next()??, time_parts.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second;
    u64::try_from(seconds).ok().map(|seconds| seconds * 1000)
}

impl Fixture {
    /// Loads and validates a fixture file (DEV-511 to DEV-514).
    pub fn load(path: &Path) -> Result<Fixture, HarnessError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            bad_value("$", &format!("cannot read {}: {error}", path.display()))
        })?;
        let document: Value = serde_json::from_str(&text)
            .map_err(|error| bad_value("$", &format!("not JSON: {error}")))?;
        Fixture::validate(&document)?;
        Ok(Fixture { document })
    }

    fn validate(document: &Value) -> Result<(), HarnessError> {
        let root = as_object(document, "$")?;
        let side = match root.get("side") {
            None => "server",
            Some(value) => as_text(value, "$.side")?,
        };
        if !["server", "client", "scene"].contains(&side) {
            return Err(bad_value("$.side", "expected server, client or scene"));
        }
        check_keys(root, if side == "scene" { SCENE_KEYS } else { HOST_KEYS }, "$")?;
        if root.get("schema") != Some(&json!(1)) {
            return Err(bad_value("$.schema", "expected 1"));
        }
        if let Some(tier) = root.get("tier") {
            if !["T1", "T2", "T3"].contains(&as_text(tier, "$.tier")?) {
                return Err(bad_value("$.tier", "expected T1, T2 or T3"));
            }
        }
        if let Some(clock) = root.get("clock") {
            if parse_clock(as_text(clock, "$.clock")?).is_none() {
                return Err(bad_value("$.clock", "expected YYYY-MM-DD HH:mm:ss"));
            }
        }
        if let Some(grants) = root.get("grants") {
            for (index, grant) in as_array(grants, "$.grants")?.iter().enumerate() {
                as_text(grant, &format!("$.grants[{index}]"))?;
            }
        }
        if let Some(settings) = root.get("settings") {
            as_object(settings, "$.settings")?;
        }
        if let Some(storage) = root.get("storage") {
            as_object(storage, "$.storage")?;
        }
        if let Some(files) = root.get("files") {
            as_object(files, "$.files")?;
        }
        if let Some(expect_state) = root.get("expect_state") {
            as_text(expect_state, "$.expect_state")?;
        }
        Fixture::validate_tree(root)?;
        Fixture::validate_lists(root)?;
        Ok(())
    }

    fn validate_tree(root: &serde_json::Map<String, Value>) -> Result<(), HarnessError> {
        let Some(tree) = root.get("tree") else {
            return Ok(());
        };
        let mut seen: Vec<u64> = Vec::new();
        for (index, node) in as_array(tree, "$.tree")?.iter().enumerate() {
            let path = format!("$.tree[{index}]");
            let object = as_object(node, &path)?;
            check_keys(object, TREE_KEYS, &path)?;
            let id = object
                .get("id")
                .and_then(as_u64)
                .ok_or_else(|| bad_value(&format!("{path}.id"), "expected an unsigned integer"))?;
            match object.get("parent") {
                None | Some(Value::Null) => {}
                Some(parent) => {
                    if as_u64(parent).is_none() {
                        return Err(bad_value(&format!("{path}.parent"), "expected an id or null"));
                    }
                }
            }
            as_text(object.get("name").unwrap_or(&Value::Null), &format!("{path}.name"))?;
            let class = as_text(object.get("class").unwrap_or(&Value::Null), &format!("{path}.class"))?;
            if !["container", "pane", "folder"].contains(&class) {
                return Err(bad_value(&format!("{path}.class"), "expected container, pane or folder"));
            }
            if let Some(kind) = object.get("content_kind") {
                as_text(kind, &format!("{path}.content_kind"))?;
            }
            if let Some(agent) = object.get("agent") {
                check_keys(as_object(agent, &format!("{path}.agent"))?, AGENT_KEYS, &format!("{path}.agent"))?;
            }
            if seen.contains(&id) {
                return Err(HarnessError::new(
                    codes::FIXTURE_DUPLICATE_ID,
                    format!("{path}.id: node id {id} appears twice"),
                ));
            }
            seen.push(id);
        }
        Ok(())
    }

    fn validate_lists(root: &serde_json::Map<String, Value>) -> Result<(), HarnessError> {
        let keyed: [(&str, &[&str]); 5] = [
            ("agents", AGENT_ENTRY_KEYS),
            ("events", EVENT_KEYS),
            ("http", HTTP_KEYS),
            ("processes", PROCESS_KEYS),
            ("lifecycle", LIFECYCLE_KEYS),
        ];
        for (key, allowed) in keyed {
            let Some(list) = root.get(key) else {
                continue;
            };
            for (index, item) in as_array(list, &format!("$.{key}"))?.iter().enumerate() {
                let path = format!("$.{key}[{index}]");
                check_keys(as_object(item, &path)?, allowed, &path)?;
            }
        }
        for key in ["panes", "completions", "frames"] {
            if let Some(list) = root.get(key) {
                as_array(list, &format!("$.{key}"))?;
            }
        }
        for (index, step) in root
            .get("lifecycle")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let name = step.get("step").and_then(Value::as_str).unwrap_or("");
            if !LIFECYCLE_STEPS.contains(&name) {
                return Err(bad_value(&format!("$.lifecycle[{index}].step"), "unknown step"));
            }
        }
        for (index, event) in root
            .get("events")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            if event.get("tag").and_then(Value::as_str).is_none() {
                return Err(bad_value(&format!("$.events[{index}].tag"), "expected a string"));
            }
        }
        Ok(())
    }

    fn key(&self, name: &str) -> Option<&Value> {
        self.document.get(name)
    }

    /// Fills the fakes of a host that has not been activated; the builder wins (DEV-510).
    pub fn apply_to(&self, host: &mut FakeHost) -> Result<(), HarnessError> {
        let grants: Option<Vec<String>> = self.key("grants").and_then(Value::as_array).map(|list| {
            list.iter().filter_map(|grant| grant.as_str().map(str::to_string)).collect()
        });
        host.apply_fixture_basics(
            grants.as_deref(),
            self.key("tier").and_then(Value::as_str),
            self.key("side").and_then(Value::as_str),
        )?;
        if let Some(clock) = self.key("clock").and_then(Value::as_str).and_then(parse_clock) {
            if clock >= host.clock().now() {
                host.set_clock(clock);
            }
        }
        self.apply_tree(host)?;
        if let Some(settings) = self.key("settings").and_then(Value::as_object) {
            for (name, value) in settings {
                host.seed_settings_unchecked(name, value.clone());
            }
        }
        if let Some(storage) = self.key("storage").and_then(Value::as_object) {
            for (bucket, entries) in storage {
                let entries = as_object(entries, &format!("$.storage.{bucket}"))?;
                for (name, value) in entries {
                    let bytes = value.as_str().map(|text| text.as_bytes().to_vec()).unwrap_or_else(|| value.to_string().into_bytes());
                    if !host.seed_storage_value(bucket, name, &bytes) {
                        return Err(bad_value(&format!("$.storage.{bucket}"), "unknown bucket"));
                    }
                }
            }
        }
        self.apply_files(host)?;
        self.apply_network(host)?;
        let events = self.decode_events()?;
        host.set_fixture_events(events);
        Ok(())
    }

    fn apply_tree(&self, host: &mut FakeHost) -> Result<(), HarnessError> {
        let Some(nodes) = self.key("tree").and_then(Value::as_array) else {
            return Ok(());
        };
        for (index, node) in nodes.iter().enumerate() {
            let path = format!("$.tree[{index}]");
            let mut normal = node.clone();
            if let Some(object) = normal.as_object_mut() {
                for field in ["id", "parent"] {
                    if let Some(id) = object.get(field).and_then(as_u64) {
                        object.insert(field.to_string(), json!(id));
                    }
                }
            }
            let class = node.get("class").and_then(Value::as_str).unwrap_or("pane");
            let class = match class {
                "container" => NodeClass::Container,
                "folder" => NodeClass::Folder,
                _ => NodeClass::Pane,
            };
            let parsed: Result<TreeNode, _> = serde_json::from_value(normal);
            let mut parsed = parsed.map_err(|error| bad_value(&path, &error.to_string()))?;
            parsed.class = class;
            host.tree().insert_with_id(parsed);
        }
        if let Some(agents) = self.key("agents").and_then(Value::as_array) {
            for agent in agents {
                let name = agent.get("name").and_then(Value::as_str).unwrap_or("agent");
                let signature = agent.get("signature").and_then(Value::as_str).unwrap_or("unknown");
                let id = host.tree().add_agent_pane(name, signature);
                if let Some(activity) = agent.get("activity") {
                    let activity: AgentActivity = serde_json::from_value(activity.clone())
                        .map_err(|error| bad_value("$.agents", &error.to_string()))?;
                    host.tree().set_activity(id, activity);
                }
            }
        }
        Ok(())
    }

    fn apply_files(&self, host: &mut FakeHost) -> Result<(), HarnessError> {
        let Some(files) = self.key("files").and_then(Value::as_object) else {
            return Ok(());
        };
        for (path, content) in files {
            let location = format!("$.files.{path}");
            let bytes = match content {
                Value::String(text) => text.as_bytes().to_vec(),
                Value::Object(object) => {
                    let encoded = object
                        .get("base64")
                        .and_then(Value::as_str)
                        .ok_or_else(|| bad_value(&location, "expected a string or {\"base64\": ...}"))?;
                    decode_base64(encoded).ok_or_else(|| bad_value(&location, "invalid base64"))?
                }
                _ => return Err(bad_value(&location, "expected a string or {\"base64\": ...}")),
            };
            host.fs().seed(path.as_str(), &bytes);
        }
        Ok(())
    }

    fn apply_network(&self, host: &mut FakeHost) -> Result<(), HarnessError> {
        for entry in self.key("http").and_then(Value::as_array).into_iter().flatten() {
            let method = entry.get("method").and_then(Value::as_str).unwrap_or("GET");
            let url = entry.get("url").and_then(Value::as_str).unwrap_or("");
            let status = entry.get("status").and_then(Value::as_u64).unwrap_or(200);
            let body = entry.get("body").and_then(Value::as_str).unwrap_or("");
            host.http().expect(
                method,
                url,
                HttpResponse {
                    status: u16::try_from(status).unwrap_or(500),
                    headers: Vec::new(),
                    body: body.as_bytes().to_vec(),
                },
            );
        }
        for entry in self.key("processes").and_then(Value::as_array).into_iter().flatten() {
            let program = entry.get("program").and_then(Value::as_str).unwrap_or("");
            let arguments: Vec<String> = entry
                .get("arguments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|argument| argument.as_str().map(str::to_string))
                .collect();
            let argument_refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
            let exit = ProcessExit {
                status: entry.get("status").and_then(Value::as_i64).and_then(|status| i32::try_from(status).ok()),
                signal: entry.get("signal").and_then(Value::as_str).map(str::to_string),
                stdout: entry.get("stdout").and_then(Value::as_str).unwrap_or("").as_bytes().to_vec(),
                stderr: entry.get("stderr").and_then(Value::as_str).unwrap_or("").as_bytes().to_vec(),
            };
            host.process().expect(program, &argument_refs).returns(exit);
        }
        Ok(())
    }

    /// Decodes the `events` list; `{"tag": t, "val": v}` becomes the externally tagged event JSON.
    fn decode_events(&self) -> Result<Vec<Event>, HarnessError> {
        let mut events = Vec::new();
        for (index, item) in self.key("events").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let tag = item.get("tag").and_then(Value::as_str).unwrap_or("");
            let encoded = match item.get("val") {
                None | Some(Value::Null) => Value::String(tag.to_string()),
                Some(value) => json!({ tag: value }),
            };
            let event: Event = serde_json::from_value(encoded)
                .map_err(|error| bad_value(&format!("$.events[{index}]"), &error.to_string()))?;
            events.push(event);
        }
        Ok(events)
    }

    /// Delivers the events and completions, then runs the lifecycle steps and checks `expect_state` (DEV-514, DEV-516).
    pub fn run_steps(&self, host: &mut FakeHost) -> Result<(), HarnessError> {
        let events = host.fixture_events();
        if !events.is_empty() {
            host.deliver(events);
        }
        host.settle();
        for (index, item) in self.key("completions").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let encoded = match item {
                Value::String(_) => item.clone(),
                other => other.clone(),
            };
            let outcome: Outcome = serde_json::from_value(encoded)
                .map_err(|error| bad_value(&format!("$.completions[{index}]"), &error.to_string()))?;
            let request = u64::try_from(index).unwrap_or(0) + 1;
            host.deliver(vec![Event::RequestCompleted(RequestResult {
                request: RequestRef::Submission(request),
                outcome,
            })]);
        }
        host.settle();
        for (index, step) in self.key("lifecycle").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let name = step.get("step").and_then(Value::as_str).unwrap_or("");
            let value = step.get("value").cloned().unwrap_or(Value::Null);
            let path = format!("$.lifecycle[{index}].value");
            match name {
                "trap" => host.inject_trap(as_text(&value, &path)?),
                "advance" => {
                    let millis = value.as_u64().ok_or_else(|| bad_value(&path, "expected milliseconds"))?;
                    host.advance_clock(millis);
                }
                "restart" => {
                    let bytes = decode_base64(as_text(&value, &path)?)
                        .ok_or_else(|| bad_value(&path, "invalid base64"))?;
                    host.restart_with(bytes);
                }
                "grant" => host.grant_now(as_text(&value, &path)?)?,
                _ => host.revoke(as_text(&value, &path)?),
            }
            host.settle();
        }
        if let Some(expected) = self.key("expect_state").and_then(Value::as_str) {
            let actual = serde_json::to_value(host.lifecycle_state())
                .ok()
                .and_then(|state| state.as_str().map(str::to_string))
                .unwrap_or_default();
            if actual != expected {
                return Err(HarnessError::new(
                    codes::FIXTURE_STATE_MISMATCH,
                    format!("expected state {expected}, got {actual}"),
                ));
            }
        }
        Ok(())
    }
}

/// Whether `expected` is contained in `actual`: only the given fields are compared (DEV-515).
fn item_matches(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(wanted), Value::Object(found)) => wanted.iter().all(|(key, value)| {
            if key == "contains" {
                let text = value.as_str().unwrap_or("");
                return found.get("message").and_then(Value::as_str).map(|message| message.contains(text)).unwrap_or(false);
            }
            found.get(key).map(|other| item_matches(value, other)).unwrap_or(false)
        }),
        (Value::Object(wanted), Value::String(name)) => {
            wanted.len() == 1 && wanted.get("intent").and_then(Value::as_str) == Some(name.as_str())
        }
        _ => expected == actual,
    }
}

/// Compares one expectation section with the actual items; `None` when it matches.
fn compare_section(name: &str, section: &Value, actual: &[Value]) -> Option<String> {
    let mode = section.get("match").and_then(Value::as_str).unwrap_or("subset");
    let items: Vec<Value> = section.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    let matched = match mode {
        "exact" => items.len() == actual.len() && items.iter().zip(actual).all(|(want, have)| item_matches(want, have)),
        "ordered" => {
            let mut position = 0;
            items.iter().all(|want| {
                match actual.iter().skip(position).position(|have| item_matches(want, have)) {
                    Some(offset) => {
                        position += offset + 1;
                        true
                    }
                    None => false,
                }
            })
        }
        _ => items.iter().all(|want| actual.iter().any(|have| item_matches(want, have))),
    };
    if matched {
        return None;
    }
    Some(format!(
        "{name} ({mode}) differ: expected {} but got {}",
        Value::Array(items),
        Value::Array(actual.to_vec())
    ))
}

/// The submissions as flat JSON: `intent` is the case name, the case fields sit beside it.
fn submission_values(host: &FakeHost) -> Vec<Value> {
    host.submitted()
        .iter()
        .map(|submission| {
            let mut flat = serde_json::Map::new();
            match serde_json::to_value(&submission.intent) {
                Ok(Value::Object(case)) => {
                    for (name, payload) in case {
                        flat.insert("intent".to_string(), Value::String(name));
                        if let Value::Object(fields) = payload {
                            flat.extend(fields);
                        }
                    }
                }
                Ok(Value::String(name)) => {
                    flat.insert("intent".to_string(), Value::String(name));
                }
                _ => {}
            }
            flat.insert("correlation".to_string(), json!(submission.correlation));
            if let Some(key) = &submission.dedup_key {
                flat.insert("dedup_key".to_string(), json!(key));
            }
            Value::Object(flat)
        })
        .collect()
}

/// Compares the expectation document with the host state; the first difference.
fn compare_expectation(expectation: &Value, host: &FakeHost) -> Option<String> {
    if let Some(section) = expectation.get("submissions") {
        if let Some(difference) = compare_section("submissions", section, &submission_values(host)) {
            return Some(difference);
        }
    }
    if let Some(section) = expectation.get("logs") {
        let logs: Vec<Value> = host
            .logs()
            .iter()
            .map(|line| json!({ "level": line.level, "message": line.message }))
            .collect();
        if let Some(difference) = compare_section("logs", section, &logs) {
            return Some(difference);
        }
    }
    if let Some(section) = expectation.get("outcomes") {
        let outcomes: Vec<Value> = host
            .outcomes()
            .iter()
            .filter_map(|outcome| serde_json::to_value(outcome).ok())
            .collect();
        if let Some(difference) = compare_section("outcomes", section, &outcomes) {
            return Some(difference);
        }
    }
    None
}

fn panic_to_error(payload: Box<dyn std::any::Any + Send>) -> HarnessError {
    let text = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_string()))
        .unwrap_or_else(|| "the harness panicked".to_string());
    match text.split_once(": ") {
        Some((code, message)) if code.contains('.') => HarnessError::new(code, message.to_string()),
        _ => HarnessError::new(codes::FIXTURE_BAD_VALUE, text),
    }
}

/// Runs one fixture against the plugin type and compares the expectation file next to it (DEV-516, DEV-551).
pub fn run<P: ilium_plugin_sdk_core::NativeEntry + Default + 'static>(
    path: &Path,
) -> Result<FixtureStatus, HarnessError> {
    let fixture = Fixture::load(path)?;
    if fixture.key("side").and_then(Value::as_str) == Some("scene") {
        return Ok(FixtureStatus::Pass);
    }
    let expectation_path = path.with_extension("expect.json");
    let expectation: Option<Value> = match std::fs::read_to_string(&expectation_path) {
        Ok(text) => Some(serde_json::from_str(&text).map_err(|error| {
            bad_value("$", &format!("{}: not JSON: {error}", expectation_path.display()))
        })?),
        Err(_) => None,
    };
    let mut host = FakeHost::new();
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<FixtureStatus, HarnessError> {
        fixture.apply_to(&mut host)?;
        host.activate::<P>();
        fixture.run_steps(&mut host)?;
        if let Some(failure) = host.failures().last() {
            if failure.code.starts_with("trap.") && expectation.is_none() {
                return Ok(FixtureStatus::Trap(failure.code.clone()));
            }
        }
        Ok(match &expectation {
            None => FixtureStatus::Pass,
            Some(expected) => match compare_expectation(expected, &host) {
                None => FixtureStatus::Pass,
                Some(difference) => FixtureStatus::Fail(difference),
            },
        })
    }));
    match outcome {
        Ok(result) => result,
        Err(payload) => Err(panic_to_error(payload)),
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::errors::codes;
    use ilium_plugin_sdk_core::types::{
        Activation, DeactivationReason, Event, PluginError, Submission,
    };

    /// Server plugin used only to give `run` a type. Its bodies are neutral because the fixture is under test.
    #[derive(Default)]
    struct ServerPlugin;

    impl ilium_plugin_sdk_core::NativeEntry for ServerPlugin {
        const SIDE: &'static str = "server";
        const EXPORTS: &'static [&'static str] = &[];
        fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }
        fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
            None
        }
        fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
            Vec::new()
        }
    }

    /// Writes a temporary fixture file whose name is unique to this test binary process.
    fn write_fixture(file_name: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ilium-fixture-spec-{}-{file_name}",
            std::process::id()
        ));
        std::fs::write(&path, body).expect("the test can write a temporary fixture file");
        path
    }

    #[test]
    fn fixture_schema_1_loads_and_keeps_its_document() {
        let path = write_fixture("schema-only.json", r#"{"schema": 1}"#);
        let fixture = Fixture::load(&path).expect("a schema 1 fixture loads");
        assert_eq!(fixture.document["schema"], 1);
        assert_eq!(
            fixture.document.as_object().map(|object| object.len()),
            Some(1)
        );
    }

    /// REQ: DEV-511
    #[test]
    fn dev_511_unknown_key_error_names_its_json_path() {
        let body = r#"{"schema": 1, "events": [
            {"tag": "a", "val": 1}, {"tag": "b", "val": 2}, {"tag": "c", "val": 3, "foo": 4}]}"#;
        let path = write_fixture("unknown-key.json", body);
        let error = Fixture::load(&path).expect_err("an unknown key is refused");
        assert_eq!(error.code, codes::FIXTURE_UNKNOWN_KEY);
        assert!(
            error.message.contains("$.events[2].foo"),
            "message was: {}",
            error.message
        );
    }

    /// REQ: DEV-511
    #[test]
    fn dev_511_bad_value_error_names_its_json_path() {
        let body = r#"{"schema": 1, "tree": [
            {"id": 1, "parent": null, "name": "root", "class": "container", "content_kind": "ilium.terminal"},
            {"id": 2, "parent": 1, "name": "child", "class": "pane", "content_kind": "ilium.terminal"},
            {"id": 3, "parent": "x", "name": "bad", "class": "pane", "content_kind": "ilium.terminal"}]}"#;
        let path = write_fixture("bad-value.json", body);
        let error = Fixture::load(&path).expect_err("a non-integer parent is refused");
        assert_eq!(error.code, codes::FIXTURE_BAD_VALUE);
        assert!(
            error.message.contains("$.tree[2].parent"),
            "message was: {}",
            error.message
        );
    }

    /// REQ: DEV-512
    #[test]
    fn dev_512_repeated_node_id_is_refused() {
        let body = r#"{"schema": 1, "tree": [
            {"id": 1, "parent": null, "name": "one", "class": "container", "content_kind": "ilium.terminal"},
            {"id": 1, "parent": null, "name": "again", "class": "container", "content_kind": "ilium.terminal"}]}"#;
        let path = write_fixture("duplicate-id.json", body);
        let error = Fixture::load(&path).expect_err("a repeated id is refused");
        assert_eq!(error.code, codes::FIXTURE_DUPLICATE_ID);
    }

    /// REQ: DEV-513
    #[test]
    fn dev_513_large_integer_written_as_string_loads() {
        let body = r#"{"schema": 1, "tree": [
            {"id": "9007199254740993", "parent": null, "name": "big", "class": "container", "content_kind": "ilium.terminal"}]}"#;
        let path = write_fixture("big-id.json", body);
        let fixture = Fixture::load(&path).expect("a string id above 2^53 loads");
        assert_eq!(fixture.document["tree"][0]["id"], "9007199254740993");
    }

    /// REQ: DEV-514
    #[test]
    fn dev_514_scene_fixture_refuses_a_server_only_key() {
        let path = write_fixture(
            "scene-tree.json",
            r#"{"schema": 1, "side": "scene", "tree": []}"#,
        );
        let error = Fixture::load(&path).expect_err("tree is not a scene key");
        assert_eq!(error.code, codes::FIXTURE_UNKNOWN_KEY);
    }

    /// REQ: DEV-516, DEV-551
    #[test]
    fn dev_516_run_without_expectation_file_passes() {
        let path = write_fixture("no-expect.json", r#"{"schema": 1, "events": []}"#);
        let status = run::<ServerPlugin>(&path).expect("the fixture runs");
        assert_eq!(status, FixtureStatus::Pass);
    }

    /// REQ: DEV-516
    #[test]
    fn dev_516_run_reports_fail_when_expectation_differs() {
        let path = write_fixture("mismatch.json", r#"{"schema": 1, "events": []}"#);
        std::fs::write(
            path.with_extension("expect.json"),
            r#"{"schema": 1, "submissions": {"match": "exact", "items": [{"intent": "set-decoration"}]}}"#,
        )
        .expect("the test can write the expectation file");
        let status = run::<ServerPlugin>(&path).expect("the fixture runs");
        assert!(
            matches!(status, FixtureStatus::Fail(_)),
            "status was: {status:?}"
        );
    }

    /// REQ: DEV-516
    #[test]
    fn dev_516_run_propagates_fixture_load_error() {
        let path = write_fixture("run-unknown-key.json", r#"{"schema": 1, "bogus": true}"#);
        let error = run::<ServerPlugin>(&path).expect_err("an unknown key stops the run");
        assert_eq!(error.code, codes::FIXTURE_UNKNOWN_KEY);
    }
}
