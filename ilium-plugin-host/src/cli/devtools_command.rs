//! `ilium plugin trace` and `ilium plugin profile` (reference-debugging-tools.md DEV-670..693,
//! reference-plugin-cli.md DEV-820..831). Both read the running session through the backend.
use super::{offline, CliEnv, CliOutput, SessionBackend};
use crate::telemetry::{check_unredacted, Profiler, TraceKind};
use serde_json::{json, Value};
use std::time::Duration;

const TRACE_KINDS: [TraceKind; 7] = [
    TraceKind::Event,
    TraceKind::Submission,
    TraceKind::Outcome,
    TraceKind::HostCall,
    TraceKind::Render,
    TraceKind::Trap,
    TraceKind::Lifecycle,
];
const DEFAULT_LAST: usize = 50;
const DEFAULT_DURATION: u32 = 10;

fn usage(code: &str, message: &str) -> CliOutput {
    offline::failure(code, message, 2)
}

/// Splits the arguments into the plugin id and the value of every flag the command accepts.
fn parse<'a>(
    args: &'a [String],
    valued: &[&str],
    switches: &[&str],
) -> Result<(Option<&'a str>, Vec<(&'a str, &'a str)>, Vec<&'a str>), CliOutput> {
    let mut id = None;
    let mut values = Vec::new();
    let mut seen = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let arg = arg.as_str();
        if valued.contains(&arg) {
            let Some(value) = iter.next() else {
                return Err(usage("usage.missing-argument", "A flag value is required"));
            };
            values.push((arg, value.as_str()));
        } else if switches.contains(&arg) {
            seen.push(arg);
        } else if matches!(arg, "--json" | "--quiet" | "--no-color") {
        } else if arg.starts_with('-') {
            return Err(usage("usage.unknown-flag", "Unsupported flag"));
        } else if id.is_some() {
            return Err(usage("usage.argument-invalid", "Unexpected positional argument"));
        } else {
            id = Some(arg);
        }
    }
    Ok((id, values, seen))
}

fn value_of<'a>(values: &[(&'a str, &'a str)], flag: &str) -> Option<&'a str> {
    values.iter().rev().find(|(name, _)| *name == flag).map(|(_, v)| *v)
}

fn session_of(env: &CliEnv) -> Result<&dyn SessionBackend, CliOutput> {
    env.session.as_deref().ok_or_else(|| {
        offline::failure("session.unreachable", "No reachable session was supplied", 6)
    })
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// `ilium plugin trace <id> [--last N] [--kinds a,b] [--record FILE] [--unredacted]`.
pub(super) fn trace(args: &[String], env: &CliEnv) -> CliOutput {
    let (id, values, switches) =
        match parse(args, &["--last", "--kinds", "--record"], &["--unredacted"]) {
            Ok(parsed) => parsed,
            Err(output) => return output,
        };
    let Some(id) = id else {
        return usage("usage.missing-argument", "A plugin id is required");
    };
    let last = match value_of(&values, "--last") {
        None => DEFAULT_LAST,
        Some(text) => match text.parse::<usize>() {
            Ok(count) if count > 0 => count,
            _ => return usage("usage.invalid-value", "--last must be a positive integer"),
        },
    };
    let mut kinds = Vec::new();
    if let Some(list) = value_of(&values, "--kinds") {
        for word in list.split(',') {
            match TRACE_KINDS.iter().find(|kind| kind.as_str() == word) {
                Some(kind) => kinds.push(*kind),
                None => return usage("usage.invalid-value", "Unknown trace kind"),
            }
        }
    }
    let session = match session_of(env) {
        Ok(session) => session,
        Err(output) => return output,
    };
    let Some(plugin) = session.plugin(id) else {
        return offline::failure("plugin.not-found", "The plugin is not installed", 5);
    };
    if switches.contains(&"--unredacted") {
        if let Err(refusal) = check_unredacted(plugin.dev) {
            return offline::failure(&refusal.code, refusal.message, 1);
        }
    }
    let mut records = session.trace_records(id);
    if !kinds.is_empty() {
        records.retain(|record| kinds.contains(&record.kind));
    }
    let skip = records.len().saturating_sub(last);
    let records = records.split_off(skip);
    let mut output: Vec<Value> = records
        .iter()
        .map(|record| {
            json!({
                "type": "trace", "seq": record.seq, "time_ms": record.time_ms,
                "kind": record.kind.as_str(), "instance": record.instance,
                "detail": record.detail,
            })
        })
        .collect();
    if let Some(path) = value_of(&values, "--record") {
        let replay = json!({
            "plugin": id,
            "records": records.iter().map(|record| json!({
                "seq": record.seq, "time_ms": record.time_ms, "kind": record.kind.as_str(),
                "instance": record.instance, "detail": record.detail,
            })).collect::<Vec<_>>(),
        });
        let bytes = replay.to_string().into_bytes();
        if let Err(error) = std::fs::write(env.cwd.join(path), &bytes) {
            return offline::failure("trace.record-failed", format!("Cannot write {path}: {error}"), 1);
        }
        output.push(json!({"type": "artifact", "kind": "replay", "path": path, "bytes": bytes.len()}));
    }
    offline::finish(output, 0)
}

/// `ilium plugin profile <id> [--duration SECONDS]`.
pub(super) fn profile(args: &[String], env: &CliEnv) -> CliOutput {
    let (id, values, _) = match parse(args, &["--duration"], &[]) {
        Ok(parsed) => parsed,
        Err(output) => return output,
    };
    let Some(id) = id else {
        return usage("usage.missing-argument", "A plugin id is required");
    };
    let duration = match value_of(&values, "--duration") {
        None => DEFAULT_DURATION,
        Some(text) => match text.parse::<u32>() {
            Ok(seconds) if (1..=600).contains(&seconds) => seconds,
            _ => return usage("usage.invalid-value", "--duration must be 1 to 600 seconds"),
        },
    };
    let session = match session_of(env) {
        Ok(session) => session,
        Err(output) => return output,
    };
    let Some(plugin) = session.plugin(id) else {
        return offline::failure("plugin.not-found", "The plugin is not installed", 5);
    };
    if !plugin.running {
        return offline::failure("plugin.not-running", "The plugin has no running instance", 1);
    }
    let calls = session.profile_calls(id, duration);
    let mut profiler = Profiler::new();
    let mut exports: Vec<String> = Vec::new();
    for call in &calls {
        if !exports.contains(&call.export) {
            exports.push(call.export.clone());
        }
        profiler.record(&call.export, Duration::from_secs_f64(call.duration_ms.max(0.0) / 1000.0));
    }
    if exports.is_empty() {
        exports = session.profile_exports(id);
    }
    let mut output = Vec::new();
    for export in &exports {
        let timing = profiler.timing(export);
        output.push(json!({
            "type": "timing", "export": export, "calls": timing.calls,
            "total_ms": round3(timing.total_ms),
            "p50_ms": timing.p50_ms.map(round3), "p95_ms": timing.p95_ms.map(round3),
            "max_ms": timing.max_ms.map(round3), "over_budget": timing.over_budget,
        }));
    }
    profiler.clear();
    offline::finish(output, 0)
}

fn tree_stats(node: &crate::telemetry::InspectNode, level: usize) -> (usize, usize) {
    node.children.iter().fold((1, level), |(count, depth), child| {
        let (inner_count, inner_depth) = tree_stats(child, level + 1);
        (count + inner_count, depth.max(inner_depth))
    })
}

/// `ilium plugin inspect <id> --view <target>` and `--memory` (DEV-680..702, DEV-730).
pub(super) fn inspect_live(args: &[String], env: &CliEnv) -> CliOutput {
    let (id, values, switches) = match parse(args, &["--view"], &["--memory", "--verify"]) {
        Ok(parsed) => parsed,
        Err(output) => return output,
    };
    let view = value_of(&values, "--view");
    let memory = switches.contains(&"--memory");
    let Some(id) = id else {
        return usage("usage.argument-missing", "A plugin id is required");
    };
    if view.is_some() == memory {
        return usage("usage.argument-invalid", "Give exactly one of --view and --memory");
    }
    let session = match session_of(env) {
        Ok(session) => session,
        Err(output) => return output,
    };
    let Some(plugin) = session.plugin(id) else {
        return offline::failure("inspect.id-unknown", "The plugin is not installed", 5);
    };
    let json_mode = args.iter().any(|arg| arg == "--json");
    if let Some(target) = view {
        let tree = plugin
            .running
            .then(|| session.last_tree(id, target))
            .flatten();
        let Some(tree) = tree else {
            return offline::failure(
                "inspect.no-render",
                "The plugin has not rendered that target",
                1,
            );
        };
        let (nodes, depth) = tree_stats(&tree, 1);
        let record = json!({
            "type": "tree", "target": target, "nodes": nodes, "depth": depth,
            "root": serde_json::to_value(&tree).unwrap_or(Value::Null),
        });
        let mut output = offline::finish(vec![record], 0);
        if !json_mode {
            output.stdout_text = format!(
                "{}\n",
                crate::telemetry::render_inspector(target, &tree, 4_096, 16)
            );
        }
        return output;
    }
    let views = session.memory_views(id);
    let records: Vec<Value> = views
        .iter()
        .map(|view| {
            json!({
                "type": "memory", "side": view.side.as_str(),
                "linear_bytes": view.linear_bytes, "peak_bytes": view.peak_bytes,
                "limit_bytes": view.limit_bytes, "storage_bytes": view.storage_bytes,
                "instances": view.instances, "restarts": view.restarts,
            })
        })
        .collect();
    let mut output = offline::finish(records, 0);
    if !json_mode {
        let text: Vec<String> = views
            .iter()
            .map(|view| crate::telemetry::render_memory_view(id, view))
            .collect();
        output.stdout_text = format!("{}\n", text.join("\n"));
    }
    output
}
