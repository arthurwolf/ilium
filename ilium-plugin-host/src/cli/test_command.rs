//! `ilium plugin test` (reference-plugin-cli.md DEV-080..089).
//!
//! Runs the built components against the fixtures under `tests/fixtures/`. Each fixture is one
//! `test-case` record preceded by its `submission` and `outcome` records. Expectations live in
//! `<name>.expect.json` next to the fixture; `--update` rewrites those that differ.
use super::{offline, CliEnv, CliOutput, RunResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const SIDES: [&str; 3] = ["server", "client", "scene"];

struct Options {
    project: PathBuf,
    side: Option<String>,
    fixture: Option<String>,
    record: Option<PathBuf>,
    update: bool,
    coverage: bool,
}

fn usage(code: &str, message: &str) -> CliOutput {
    offline::failure(code, message, 2)
}

fn parse(args: &[String], env: &CliEnv) -> Result<Options, CliOutput> {
    let mut project = None;
    let (mut side, mut fixture, mut record) = (None, None, None);
    let (mut update, mut coverage) = (false, false);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--update" => update = true,
            "--coverage" => coverage = true,
            "--side" | "--fixture" | "--record" => {
                let Some(value) = iter.next() else {
                    return Err(usage("usage.missing-argument", "The flag needs a value"));
                };
                match arg.as_str() {
                    "--side" => side = Some(value.clone()),
                    "--fixture" => fixture = Some(value.clone()),
                    _ => record = Some(env.cwd.join(value)),
                }
            }
            flag if flag.starts_with('-') => {
                return Err(usage("usage.unknown-flag", "Unsupported flag"));
            }
            _ if project.is_none() => project = Some(arg.clone()),
            _ => return Err(usage("usage.argument-invalid", "Unexpected argument")),
        }
    }
    if side.as_deref().is_some_and(|side| !SIDES.contains(&side)) {
        return Err(usage("usage.invalid-value", "Unknown side"));
    }
    let update = update
        || env
            .env
            .get("ILIUM_PLUGIN_UPDATE_GOLDEN")
            .is_some_and(|value| !value.is_empty() && value != "0");
    Ok(Options {
        project: project.map_or_else(|| env.cwd.clone(), |path| env.cwd.join(path)),
        side,
        fixture,
        record,
        update,
        coverage,
    })
}

fn declared_sides(project: &Path) -> Vec<String> {
    let manifest: Value = std::fs::read_to_string(project.join("manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    SIDES
        .iter()
        .filter(|side| manifest.get(**side).is_some_and(|value| !value.is_null()))
        .map(|side| (*side).to_owned())
        .collect()
}

/// Whether every field of `expected` is present and equal in `actual` (objects recurse).
fn subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(want), Value::Object(have)) => want
            .iter()
            .all(|(key, value)| have.get(key).is_some_and(|other| subset(value, other))),
        _ => expected == actual,
    }
}

/// Whether one expected item holds for one actual item. Only the given fields are compared
/// (DEV-515); `contains` is a substring test on the entry's `message`.
fn item_matches(want: &Value, have: &Value) -> bool {
    match want {
        Value::Object(fields) => fields.iter().all(|(key, value)| {
            if key == "contains" {
                let needle = value.as_str().unwrap_or_default();
                have.get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|message| message.contains(needle))
            } else {
                have.get(key).is_some_and(|other| subset(value, other))
            }
        }),
        _ => want == have,
    }
}

/// One expectation section (`submissions`, `logs` or `outcomes`) against the actual items.
fn section_matches(section: &Value, actual: &[Value]) -> bool {
    let Some(items) = section["items"].as_array() else {
        return true;
    };
    match section["match"].as_str().unwrap_or("subset") {
        "exact" => {
            items.len() == actual.len()
                && items.iter().zip(actual).all(|(want, have)| item_matches(want, have))
        }
        "ordered" => {
            let mut rest = actual.iter();
            items
                .iter()
                .all(|want| rest.by_ref().any(|have| item_matches(want, have)))
        }
        _ => items
            .iter()
            .all(|want| actual.iter().any(|have| item_matches(want, have))),
    }
}

fn matches(expect: &Value, result: &RunResult) -> bool {
    section_matches(&expect["submissions"], &result.submissions)
        && section_matches(&expect["logs"], &result.logs)
        && section_matches(&expect["outcomes"], &result.outcomes)
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    std::fs::write(path, text)
}

fn fixture_files(options: &Options) -> Result<Vec<PathBuf>, CliOutput> {
    if let Some(one) = &options.fixture {
        return Ok(vec![options.project.join(one)]);
    }
    let folder = options.project.join("tests/fixtures");
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".json") && !name.ends_with(".expect.json"))
        })
        .collect();
    files.sort();
    Ok(files)
}

/// Runs `ilium plugin test`.
pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let options = match parse(args, env) {
        Ok(options) => options,
        Err(output) => return output,
    };
    let runner = env.runner.as_ref();
    let declared = declared_sides(&options.project);
    let side = match (&options.side, declared.as_slice()) {
        (Some(side), _) => side.clone(),
        (None, [only]) => only.clone(),
        (None, []) => "server".to_owned(),
        (None, _) => {
            return usage("usage.missing-argument", "The plugin has several sides; pass --side");
        }
    };
    let component = match std::fs::read(options.project.join("dist").join(format!("{side}.wasm"))) {
        Ok(bytes) => bytes,
        Err(_) => {
            return offline::failure(
                "test.component-missing",
                format!("dist/{side}.wasm is missing; run ilium plugin build"),
                1,
            )
        }
    };
    let files = match fixture_files(&options) {
        Ok(files) => files,
        Err(output) => return output,
    };
    let mut records: Vec<Value> = Vec::new();
    let mut failed = false;
    let mut replays: Vec<Value> = Vec::new();
    // With no fixture on disk a single smoke case still exercises the component.
    let cases: Vec<(String, Option<PathBuf>)> = if files.is_empty() {
        vec![("smoke".to_owned(), None)]
    } else {
        files
            .iter()
            .map(|path| {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("fixture.json");
                (format!("fixture:{name}"), Some(path.clone()))
            })
            .collect()
    };
    for (name, path) in cases {
        let fixture = match &path {
            Some(path) => match std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            {
                Some(fixture) => fixture,
                None => {
                    return offline::failure(
                        "test.fixture-invalid",
                        format!("{} is not readable JSON", path.display()),
                        1,
                    )
                }
            },
            None => json!({"schema": 1, "side": side}),
        };
        if fixture["side"].as_str().is_some_and(|fixture_side| fixture_side != side) {
            continue;
        }
        let file_name = path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        let Some(runner) = runner else {
            // Without a connected component runner nothing can be executed: say so per case.
            records.push(json!({"type":"test-case","name":name,"fixture":file_name,"side":side,
                "status":"skipped","duration_ms":0,"reason":"no component runner is connected"}));
            continue;
        };
        let result: RunResult = match runner.run_fixture(&side, &component, &fixture) {
            Ok(result) => result,
            Err(message) => {
                return offline::failure("test.run-failed", message, 1);
            }
        };
        for (index, submission) in result.submissions.iter().enumerate() {
            let mut record = json!({"type":"submission","index":index});
            for key in ["correlation", "intent", "dedup_key"] {
                if let Some(value) = submission.get(key) {
                    record[key] = value.clone();
                }
            }
            records.push(record);
        }
        for outcome in &result.outcomes {
            let mut record = json!({"type":"outcome","outcome":outcome});
            if let Some(code) = outcome.get("code") {
                record["code"] = code.clone();
            }
            records.push(record);
        }
        let expect_path = path
            .as_ref()
            .map(|path| path.with_extension("expect.json"));
        let expect: Option<Value> = match &expect_path {
            Some(path) if path.is_file() => {
                match std::fs::read_to_string(path)
                    .ok()
                    .and_then(|text| serde_json::from_str(&text).ok())
                {
                    Some(value) => Some(value),
                    None => {
                        return offline::failure(
                            "test.fixture-invalid",
                            format!("{} is not readable JSON", path.display()),
                            1,
                        )
                    }
                }
            }
            _ => None,
        };
        let mut case = json!({"type":"test-case","name":name,"fixture":file_name,"side":side,
            "status":"pass","duration_ms":0});
        if let Some(reason) = &result.trap {
            case["status"] = json!("trap");
            case["reason"] = json!(reason);
            failed = true;
        } else if let Some(expect) = &expect {
            if !matches(expect, &result) {
                if options.update {
                    if let Some(path) = &expect_path {
                        let rewritten = json!({"schema":1,"submissions":{"match":"exact",
                            "items":result.submissions}});
                        if let Err(error) = write_json(path, &rewritten) {
                            return offline::failure("test.update-failed", error.to_string(), 1);
                        }
                        records.push(json!({"type":"warning","code":"test.updated",
                            "message":format!("rewrote the expectation of {name}")}));
                    }
                } else {
                    records.push(json!({"type":"diff","subject":name,
                        "expected":expect["submissions"],"actual":result.submissions}));
                    case["status"] = json!("fail");
                    failed = true;
                }
            }
        } else {
            records.push(json!({"type":"warning","code":"test.no-expectation",
                "message":format!("{name} has no expectation file")}));
        }
        replays.push(json!({"fixture":fixture,"submissions":result.submissions,
            "outcomes":result.outcomes,"logs":result.logs}));
        records.push(case);
    }
    if let Some(record) = &options.record {
        if let Err(error) = write_json(record, &json!({"side":side,"runs":replays})) {
            return offline::failure("test.record-failed", error.to_string(), 1);
        }
        let written = std::fs::read(record).unwrap_or_default();
        let sha256: String = Sha256::digest(&written)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        records.push(json!({"type":"artifact","kind":"replay","path":record.display().to_string(),
            "bytes":written.len(),"sha256":sha256}));
    }
    let mut details = serde_json::Map::new();
    if options.coverage {
        if let Some(runner) = runner {
            let status = runner.run_project_tests(&options.project);
            details.insert("project_tests_exit".into(), json!(status));
            if status != 0 {
                failed = true;
            }
        }
    }
    let warnings = records.iter().filter(|r| r["type"] == "warning").count();
    records.push(json!({"type":"summary","duration_ms":0,"warnings":warnings,"errors":0,
        "details":details}));
    let exit = if failed { 3 } else { 0 };
    records.push(json!({"type":"result","ok":exit == 0,"exit_code":exit}));
    offline::render(records, exit)
}
