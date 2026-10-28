//! Personal-data redaction and the span file `<state>/plugin-trace.jsonl`
//! (reference-host-telemetry.md, HST-129, HST-979, HST-980).

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

/// Spans older than this are deleted at server start (HST-979).
pub const SPAN_RETENTION_DAYS: u64 = 7;

/// Replaces every e-mail address and every absolute path outside `home` with
/// `[redacted-email]` and `[redacted-path]` (HST-129). Returns the text and the replacement count.
pub fn redact_personal(text: &str, home: &str) -> (String, usize) {
    let mut count = 0;
    let mut words = Vec::new();
    for word in text.split(' ') {
        let core = word.trim_matches(|c: char| matches!(c, ',' | ';' | '"' | '\'' | '(' | ')'));
        let is_email = core.contains('@')
            && core
                .split_once('@')
                .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
        let in_home = !home.is_empty() && (core == home || core.starts_with(&format!("{home}/")));
        if is_email {
            count += 1;
            words.push(word.replace(core, "[redacted-email]"));
        } else if core.starts_with('/') && core.len() > 1 && core[1..].contains(|c: char| c != '/') && !in_home {
            count += 1;
            words.push(word.replace(core, "[redacted-path]"));
        } else {
            words.push(word.to_owned());
        }
    }
    (words.join(" "), count)
}

fn rewrite(path: &Path, keep: impl Fn(&Value) -> bool) -> io::Result<usize> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut kept = String::new();
    let mut removed = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(span) if !keep(&span) => removed += 1,
            _ => {
                kept.push_str(line);
                kept.push('\n');
            }
        }
    }
    if removed > 0 {
        let partial = path.with_extension("jsonl.partial");
        fs::write(&partial, kept)?;
        fs::rename(&partial, path)?;
    }
    Ok(removed)
}

/// Deletes spans whose `time_ms` is more than seven days before `now_ms` (HST-979). Returns the
/// number removed.
pub fn prune_at_server_start(path: &Path, now_ms: u64) -> io::Result<usize> {
    let cutoff = now_ms.saturating_sub(SPAN_RETENTION_DAYS * 86_400_000);
    rewrite(path, |span| span.get("time_ms").and_then(Value::as_u64).is_none_or(|time| time >= cutoff))
}

/// Removes every span of `plugin` by writing a new file and renaming it over the old one
/// (HST-980). Returns the removed count, reported as the kind `trace`.
pub fn erase_plugin_spans(path: &Path, plugin: &str) -> io::Result<usize> {
    rewrite(path, |span| span.get("plugin").and_then(Value::as_str) != Some(plugin))
}
