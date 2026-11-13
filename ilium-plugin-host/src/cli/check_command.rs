//! Read-only author checks. No trust repository, fetcher or guest is opened.
//! The selected-rule path refuses analyzers not yet supplied here. This is not
//! the embedded view/fixture lint executor; unavailable execution is reported.
use super::{offline, CliEnv, CliOutput};
use ilium_plugin_api::{
    manifest::{self, CheckMode, ComponentSummary, ValidationContext},
    Severity, Side, Tier,
};
use serde_json::{json, Value};
use std::path::{Component, Path};

const RULES: &[&str] = &[
    "lint.grant-unused",
    "lint.widget-missing-key",
    "lint.render-trap",
    "lint.help-topic-missing",
    "lint.settings-help-missing",
    "lint.limit-above-ceiling",
    "lint.debug-info-in-release",
    "lint.package-size-near-cap",
    "lint.unstable-feature",
    "lint.dedup-key-charset",
    "lint.icon-name-unknown",
    "lint.extension-schema",
    "lint.description-missing",
    "lint.license-missing",
    "lint.grant-unexplained",
    "lint.help-title-mismatch",
    "lint.help-link-broken",
    "lint.help-topic-too-large",
    "lint.extension-point-undocumented",
    "lint.license-file-missing",
    "lint.activation-startup",
    "lint.high-risk-grants",
    "lint.src-in-package",
    "lint.contribution-near-cap",
    "lint.notice-missing",
    "lint.notice-incomplete",
    "lint.readme-missing",
    "lint.changelog-missing",
    "lint.suppression-reason-missing",
    "lint.suppression-unknown",
    "lint.suppression-unused",
    "lint.skipped",
];

/// Arguments exclude `ilium plugin` and the command name. Defaults to `env.cwd`.
pub fn run(command: &str, args: &[String], env: &CliEnv) -> CliOutput {
    if !matches!(command, "check" | "lint") {
        return offline::failure("usage.invalid-value", "Expected check or lint", 2);
    }
    let mut target = None;
    let mut selected = Vec::new();
    let mut deny = false;
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--rule" if command == "lint" => match arguments.next() {
                Some(rule) if RULES.contains(&rule.as_str()) => selected.push(rule.clone()),
                Some(_) => return offline::failure("usage.invalid-value", "Unknown lint rule", 2),
                None => {
                    return offline::failure(
                        "usage.argument-missing",
                        "--rule requires a rule id",
                        2,
                    )
                }
            },
            "--deny" if command == "lint" => match arguments.next().map(String::as_str) {
                Some("warnings") => deny = true,
                Some(_) => {
                    return offline::failure("usage.invalid-value", "--deny accepts warnings", 2)
                }
                None => {
                    return offline::failure(
                        "usage.argument-missing",
                        "--deny requires warnings",
                        2,
                    )
                }
            },
            flag if flag.starts_with('-') => {
                return offline::failure("usage.unknown-flag", format!("Unknown flag: {flag}"), 2)
            }
            _ if target.is_none() => target = Some(argument),
            _ => {
                return offline::failure(
                    "usage.argument-unexpected",
                    "Only one path is accepted",
                    2,
                )
            }
        }
    }
    let path = target.map_or_else(|| env.cwd.clone(), |target| env.cwd.join(target));
    if command == "check" {
        return check(&path, env);
    }
    lint(&path, &selected, deny)
}

fn finding(
    kind: &str,
    rule: &str,
    severity: &str,
    message: impl Into<String>,
    pointer: &str,
) -> Value {
    json!({"type":kind,"rule":rule,"severity":severity,"message":message.into(),"key_path":pointer,"suppressed":false})
}
fn diagnostic(error: manifest::Diagnostic) -> Value {
    finding(
        "check",
        error.code.as_str(),
        match error.severity {
            Severity::Error => "error",
            Severity::Warning => "warn",
            _ => "info",
        },
        error.message,
        &error.pointer,
    )
}
fn finish(mut records: Vec<Value>, deny: bool) -> CliOutput {
    let active = |record: &&Value| record["suppressed"] != true;
    let errors = records
        .iter()
        .filter(active)
        .filter(|r| r["severity"] == "error")
        .count();
    let warnings = records
        .iter()
        .filter(active)
        .filter(|r| r["severity"] == "warn")
        .count();
    let exit = if errors > 0 || (deny && warnings > 0) {
        3
    } else {
        0
    };
    // Finding counts and command errors are distinct (DEV-071).
    records.push(json!({"type":"summary","duration_ms":0,"errors":0,"warnings":warnings,"finding_errors":errors}));
    records.push(json!({"type":"result","ok":exit==0,"exit_code":exit}));
    offline::render(records, exit)
}
fn context(env: &CliEnv) -> ValidationContext {
    ValidationContext {
        tier: Tier::T3,
        mode: CheckMode::Check,
        platform: env.platform,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: ilium_plugin_api::abi::UNSTABLE_FEATURES
            .iter()
            .map(|name| (*name).into())
            .collect(),
    }
}
fn check(path: &Path, env: &CliEnv) -> CliOutput {
    if !path.is_dir() {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => return offline::failure("check.file-unreadable", error.to_string(), 1),
        };
        let installer = crate::install::Installer::new(crate::registry::PackageStore {
            root: env.data_dir.join("plugins"),
        })
        .with_check_context(context(env), Vec::new());
        return finish(
            installer
                .check(&bytes, CheckMode::Check)
                .diagnostics
                .into_iter()
                .map(diagnostic)
                .collect(),
            false,
        );
    }
    let text = match std::fs::read_to_string(path.join("manifest.json")) {
        Ok(text) => text,
        Err(error) => {
            return finish(
                vec![finding(
                    "check",
                    "manifest.file-unreadable",
                    "error",
                    error.to_string(),
                    "",
                )],
                false,
            )
        }
    };
    let context = context(env);
    let mut records: Vec<_> = manifest::validate_json(&text, &context)
        .into_iter()
        .map(diagnostic)
        .collect();
    let parsed = match manifest::parse_manifest(&text) {
        Ok(manifest) => manifest,
        Err(errors) => {
            if records.is_empty() {
                records.extend(errors.into_iter().map(diagnostic));
            }
            return finish(records, false);
        }
    };
    for (side, declaration) in [
        (Side::Server, &parsed.server),
        (Side::Client, &parsed.client),
        (Side::Scene, &parsed.scene),
    ] {
        let Some(declaration) = declaration else {
            continue;
        };
        let relative = Path::new(&declaration.component);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            records.push(finding(
                "check",
                "component.invalid",
                "error",
                "Unsafe component path",
                &format!("/{}", side.as_str()),
            ));
            continue;
        }
        let component = path.join("dist").join(relative);
        let bytes = match std::fs::read(&component) {
            Ok(bytes) => bytes,
            Err(error) => {
                records.push(finding(
                    "check",
                    "component.missing",
                    "error",
                    error.to_string(),
                    &format!("/{}", side.as_str()),
                ));
                continue;
            }
        };
        if !wasmparser::Parser::is_component(&bytes)
            || wasmparser::Validator::new().validate_all(&bytes).is_err()
        {
            records.push(finding(
                "check",
                "component.invalid",
                "error",
                "Invalid WebAssembly component",
                &format!("/{}", side.as_str()),
            ));
            continue;
        }
        let mut summary = ComponentSummary::default();
        let mut depth = 0usize;
        for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
            match payload {
                Ok(
                    wasmparser::Payload::ModuleSection { .. }
                    | wasmparser::Payload::ComponentSection { .. },
                ) => depth += 1,
                Ok(wasmparser::Payload::End(_)) if depth > 0 => depth -= 1,
                Ok(wasmparser::Payload::CustomSection(section))
                    if depth == 0 && section.name() == "ilium:api-version" =>
                {
                    summary.api_sections.push(section.data().to_vec())
                }
                Ok(wasmparser::Payload::ComponentImportSection(section)) if depth == 0 => {
                    for import in section.into_iter().flatten() {
                        summary.imports.push(import.name.name.into());
                    }
                }
                Ok(wasmparser::Payload::ComponentExportSection(section)) if depth == 0 => {
                    for export in section.into_iter().flatten() {
                        let name = export
                            .name
                            .name
                            .split('@')
                            .next()
                            .unwrap_or(export.name.name);
                        summary
                            .exports
                            .push(name.strip_prefix("ilium:exports/").unwrap_or(name).into());
                    }
                }
                _ => {}
            }
        }
        records.extend(
            manifest::check_component(&parsed, side, &summary, &context)
                .into_iter()
                .map(diagnostic),
        );
        if bytes.len() > 8 * 1024 * 1024 {
            records.push(finding(
                "check",
                "component.size-warning",
                if bytes.len() > 32 * 1024 * 1024 {
                    "error"
                } else {
                    "warn"
                },
                "Component exceeds 8 MiB",
                &format!("/{}", side.as_str()),
            ));
        }
    }
    if let Ok(bytes) = std::fs::read(path.join("dist/build.json")) {
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(build) => match super::project_input_hash(path) {
                Ok(hash) if build["input_hash"].as_str() == Some(&hash) => {}
                Ok(_) => records.push(finding(
                    "check",
                    "check.stale-build",
                    "warn",
                    "Build inputs differ from current sources",
                    "",
                )),
                Err(error) => records.push(finding(
                    "check",
                    "check.input-unreadable",
                    "error",
                    error,
                    "",
                )),
            },
            Err(error) => records.push(finding(
                "check",
                "check.build-record-invalid",
                "error",
                error.to_string(),
                "",
            )),
        }
    }
    finish(records, false)
}

fn lint(path: &Path, selected: &[String], deny: bool) -> CliOutput {
    const AVAILABLE: &[&str] = &[
        "lint.description-missing",
        "lint.license-missing",
        "lint.license-file-missing",
        "lint.readme-missing",
        "lint.changelog-missing",
        "lint.unstable-feature",
        "lint.limit-above-ceiling",
        "lint.package-size-near-cap",
        "lint.src-in-package",
        "lint.notice-missing",
        "lint.notice-incomplete",
        "lint.help-topic-missing",
        "lint.high-risk-grants",
        "lint.grant-unexplained",
        "lint.grant-unused",
        "lint.widget-missing-key",
        "lint.render-trap",
        "lint.dedup-key-charset",
        "lint.skipped",
        "lint.suppression-reason-missing",
        "lint.suppression-unknown",
        "lint.suppression-unused",
    ];
    if let Some(rule) = selected
        .iter()
        .find(|rule| !AVAILABLE.contains(&rule.as_str()))
    {
        return offline::failure(
            "lint.rule-context-required",
            format!("Known rule {rule} requires an analyzer not supplied to this command service"),
            1,
        );
    }
    let bytes = match std::fs::read(path.join("manifest.json")) {
        Ok(bytes) => bytes,
        Err(error) => return offline::failure("lint.file-unreadable", error.to_string(), 1),
    };
    let manifest: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => return offline::failure("lint.manifest-invalid", error.to_string(), 3),
    };
    let mut records = Vec::new();
    let mut add = |rule: &str, severity: &str, message: String, pointer: &str| {
        records.push(finding("lint", rule, severity, message, pointer));
    };
    let description = manifest["description"].as_str().unwrap_or_default();
    if description.trim().is_empty() || description.chars().count() > 280 {
        add(
            "lint.description-missing",
            "warn",
            "Description must contain 1–280 characters".into(),
            "/description",
        );
    }
    let license = manifest["license"].as_str().unwrap_or_default();
    if license.trim().is_empty() {
        add(
            "lint.license-missing",
            "info",
            "No license expression".into(),
            "/license",
        );
    }
    if !license.is_empty() && !path.join("LICENSE").is_file() {
        add(
            "lint.license-file-missing",
            "info",
            "No LICENSE file".into(),
            "/license",
        );
    }
    for (file, rule) in [
        ("README.md", "lint.readme-missing"),
        ("CHANGELOG.md", "lint.changelog-missing"),
    ] {
        if !path.join(file).is_file() {
            add(rule, "info", format!("Missing {file}"), "");
        }
    }
    if let Some(features) = manifest["unstable"].as_array() {
        for feature in features {
            add(
                "lint.unstable-feature",
                "warn",
                format!("Unstable feature {feature}"),
                "/unstable",
            );
        }
    }
    if let Some(limits) = manifest["limits"].as_object() {
        for (name, value) in limits {
            if let (Ok(key), Some(value)) = (
                name.parse::<ilium_plugin_api::limits::LimitKey>(),
                value.as_u64(),
            ) {
                if value > ilium_plugin_api::limits::ceiling(key, Tier::T3) {
                    add(
                        "lint.limit-above-ceiling",
                        "warn",
                        format!("{name} exceeds the T2/T3 ceiling"),
                        &format!("/limits/{name}"),
                    );
                }
            }
        }
    }
    let total = manifest["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|row| row["path"].as_str())
                .filter_map(|name| {
                    let relative = Path::new(name);
                    if relative.is_absolute()
                        || relative
                            .components()
                            .any(|part| !matches!(part, Component::Normal(_)))
                    {
                        return None;
                    }
                    [path.join(relative), path.join("dist").join(relative)]
                        .into_iter()
                        .find_map(|file| {
                            std::fs::symlink_metadata(file)
                                .ok()
                                .filter(|metadata| metadata.is_file())
                                .map(|metadata| metadata.len())
                        })
                })
                .fold(0u64, u64::saturating_add)
        })
        .unwrap_or_default();
    if total >= 100 * 1024 * 1024 {
        add(
            "lint.package-size-near-cap",
            "warn",
            "Actual inventoried files total at least 100 MiB".into(),
            "/files",
        );
    }
    if manifest["files"].as_array().is_some_and(|files| {
        files.iter().any(|row| {
            row["path"]
                .as_str()
                .is_some_and(|name| name.starts_with("src/"))
        })
    }) {
        add(
            "lint.src-in-package",
            "info",
            "Inventory contains source files".into(),
            "/files",
        );
    }
    if let Some(dependencies) = manifest["bundled-dependencies"].as_array() {
        let notices = std::fs::read_to_string(path.join("THIRD-PARTY-NOTICES"))
            .or_else(|_| std::fs::read_to_string(path.join("NOTICE")))
            .ok();
        if !dependencies.is_empty() && notices.is_none() {
            add(
                "lint.notice-missing",
                "warn",
                "Bundled dependencies have no notices file".into(),
                "/bundled-dependencies",
            );
        }
        if let Some(notices) = notices {
            for dependency in dependencies {
                if let Some(name) = dependency.as_str().or_else(|| dependency["name"].as_str()) {
                    if !notices.contains(name) {
                        add(
                            "lint.notice-incomplete",
                            "warn",
                            format!("Notices omit {name}"),
                            "/bundled-dependencies",
                        );
                    }
                }
            }
        }
    }
    let contributions = &manifest["contributes"];
    if ["commands", "pane_kinds", "panels", "settings"]
        .iter()
        .any(|key| {
            contributions[*key]
                .as_array()
                .is_some_and(|values| !values.is_empty())
        })
        && contributions["help"].as_array().is_none_or(Vec::is_empty)
    {
        add(
            "lint.help-topic-missing",
            "warn",
            "Contributions have no help topic".into(),
            "/contributes/help",
        );
    }
    let readme = std::fs::read_to_string(path.join("README.md")).ok();
    for side in ["server", "client", "scene"] {
        if let Some(grants) = manifest["capabilities"][side].as_array() {
            for (index, grant) in grants.iter().enumerate() {
                if let Some(grant) = grant.as_str() {
                    let pointer = format!("/capabilities/{side}/{index}");
                    if ilium_plugin_api::grants::is_high_risk(grant) {
                        add(
                            "lint.high-risk-grants",
                            "info",
                            format!("High-risk grant: {grant}"),
                            &pointer,
                        );
                    }
                    if readme.as_ref().is_some_and(|text| !text.contains(grant)) {
                        add(
                            "lint.grant-unexplained",
                            "warn",
                            format!("README does not explain {grant}"),
                            &pointer,
                        );
                    }
                }
            }
        }
    }
    // Runtime-dependent lint rules must never pretend that rendering/replay ran.
    for (rule, input) in [
        ("lint.grant-unused", "built component import analysis"),
        ("lint.widget-missing-key", "rendered views"),
        ("lint.render-trap", "rendered views"),
        ("lint.dedup-key-charset", "replayed fixtures"),
    ] {
        let relevant = if rule == "lint.grant-unused" {
            manifest["capabilities"].as_object().is_some_and(|sides| {
                sides
                    .values()
                    .any(|value| value.as_array().is_some_and(|values| !values.is_empty()))
            })
        } else {
            contributions["panels"]
                .as_array()
                .is_some_and(|values| !values.is_empty())
                || contributions["pane_kinds"]
                    .as_array()
                    .is_some_and(|values| !values.is_empty())
        };
        if relevant && (selected.is_empty() || selected.iter().any(|id| id == rule)) {
            let available = if rule == "lint.grant-unused" {
                path.join("dist").is_dir()
            } else if rule == "lint.dedup-key-charset" {
                path.join("tests/fixtures").is_dir()
            } else {
                path.join("dist").is_dir()
            };
            add(
                "lint.skipped",
                if available { "error" } else { "info" },
                format!(
                    "{rule}: {input} unavailable; this service has no embedded execution adapter"
                ),
                "",
            );
        }
    }
    suppress(path, &mut records);
    if !selected.is_empty() {
        records.retain(|record| {
            selected.iter().any(|rule| record["rule"] == *rule)
                || record["rule"] == "lint.skipped"
                || record["rule"]
                    .as_str()
                    .is_some_and(|rule| rule.starts_with("lint.suppression-"))
        });
    }
    finish(records, deny)
}

fn suppress(path: &Path, records: &mut Vec<Value>) {
    let text = match std::fs::read_to_string(path.join("lint.toml")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            records.push(finding(
                "lint",
                "lint.suppression-unknown",
                "error",
                error.to_string(),
                "",
            ));
            return;
        }
    };
    let config: toml::Value = match toml::from_str(&text) {
        Ok(config) => config,
        Err(error) => {
            records.push(finding(
                "lint",
                "lint.suppression-unknown",
                "error",
                error.to_string(),
                "",
            ));
            return;
        }
    };
    let Some(value) = config.get("suppress") else {
        return;
    };
    let Some(entries) = value.as_array() else {
        records.push(finding(
            "lint",
            "lint.suppression-unknown",
            "error",
            "suppress must be an array of tables",
            "",
        ));
        return;
    };
    let original_count = records.len();
    for entry in entries {
        if entry
            .get("key_path")
            .is_some_and(|value| value.as_str().is_none())
        {
            records.push(finding(
                "lint",
                "lint.suppression-unknown",
                "error",
                "Suppression key_path must be a string",
                "",
            ));
            continue;
        }
        let rule = entry
            .get("rule")
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        let reason = entry
            .get("reason")
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        if reason.trim().is_empty() {
            records.push(finding(
                "lint",
                "lint.suppression-reason-missing",
                "error",
                "Suppression requires a reason",
                "",
            ));
            continue;
        }
        if !RULES.contains(&rule) {
            records.push(finding(
                "lint",
                "lint.suppression-unknown",
                "error",
                format!("Unknown suppressible lint rule {rule}"),
                "",
            ));
            continue;
        }
        let pointer = entry.get("key_path").and_then(toml::Value::as_str);
        let mut matched = false;
        for record in &mut records[..original_count] {
            if record["rule"] == rule && pointer.is_none_or(|pointer| record["key_path"] == pointer)
            {
                record["suppressed"] = json!(true);
                matched = true;
            }
        }
        if !matched {
            records.push(finding(
                "lint",
                "lint.suppression-unused",
                "warn",
                format!("Suppression matched no finding: {rule}"),
                "",
            ));
        }
    }
}
