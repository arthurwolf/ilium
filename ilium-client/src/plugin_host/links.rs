//! Link detectors of the plugins (contract G5, manual `primitives-terminal.md`, "Manifest
//! contributions").
//!
//! A plugin declares `contributes.link_detectors`: `{ id, regex, command }` entries. When the
//! user activates a terminal link, the kernel asks this module first. The first detector whose
//! match covers the clicked cell wins: manifest order inside a plugin, then the order the
//! plugins are given in (the plugin load order). The result names the owner, the detector id,
//! the matched text and the command, so the caller can send `link-activated` to the owner and
//! run the command with the detector id and the text as arguments. When no detector matches,
//! the caller falls back to the kernel's own link handling.
//!
//! The module is pure: manifest text and a line of text in, a match out. A detector whose regex
//! does not compile, exceeds the limits of the manual (512 bytes of pattern, 1 MiB compiled
//! program) or has an empty id is skipped, and so is every entry past the 32nd of a plugin.

use regex::{Regex, RegexBuilder};
use serde_json::Value;

use crate::terminal_links::cell_column_to_byte_offset;

/// Most detectors one plugin may contribute.
pub(crate) const DETECTORS_PER_PLUGIN: usize = 32;
/// Longest accepted pattern, in bytes.
const PATTERN_LIMIT_BYTES: usize = 512;
/// Largest compiled program, in bytes.
const PROGRAM_LIMIT_BYTES: usize = 1024 * 1024;

/// One compiled detector of one plugin.
#[derive(Debug, Clone)]
pub(crate) struct LinkDetector {
    /// The plugin that contributed it.
    pub plugin: String,
    /// The detector id, unique inside the plugin.
    pub id: String,
    /// The fully qualified command to run (`<plugin>/<command>`).
    pub command: String,
    regex: Regex,
}

/// A detector that matched the clicked text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkMatch {
    /// The plugin that owns the detector.
    pub plugin: String,
    /// The id of the detector that matched.
    pub detector_id: String,
    /// The fully qualified command to run.
    pub command: String,
    /// The matched text.
    pub text: String,
}

impl LinkMatch {
    /// The JSON arguments of the command: the detector id and the matched text.
    pub(crate) fn arguments_json(&self) -> String {
        serde_json::json!({ "detector_id": self.detector_id, "text": self.text }).to_string()
    }
}

/// Compiles one pattern under the limits of the manual.
fn compile(pattern: &str) -> Option<Regex> {
    if pattern.is_empty() || pattern.len() > PATTERN_LIMIT_BYTES {
        return None;
    }
    RegexBuilder::new(pattern)
        .size_limit(PROGRAM_LIMIT_BYTES)
        .build()
        .ok()
}

/// The detectors of one plugin manifest, in manifest order. Invalid entries are skipped.
pub(crate) fn detectors_of(plugin: &str, manifest: &str) -> Vec<LinkDetector> {
    let Ok(manifest) = serde_json::from_str::<Value>(manifest) else {
        return Vec::new();
    };
    let Some(entries) = manifest
        .pointer("/contributes/link_detectors")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.trim();
            let command = entry.get("command")?.as_str()?.trim();
            let regex = compile(entry.get("regex")?.as_str()?)?;
            if id.is_empty() || command.is_empty() {
                return None;
            }
            let command = if command.contains('/') {
                command.to_string()
            } else {
                format!("{plugin}/{command}")
            };
            Some(LinkDetector {
                plugin: plugin.to_string(),
                id: id.to_string(),
                command,
                regex,
            })
        })
        .take(DETECTORS_PER_PLUGIN)
        .collect()
}

/// The detectors of every given plugin, in the order the plugins are given.
pub(crate) fn detectors_for<'a>(
    manifests: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<LinkDetector> {
    manifests
        .into_iter()
        .flat_map(|(plugin, manifest)| detectors_of(plugin, manifest))
        .collect()
}

/// The first detector whose match covers the cell `column` of `line`, or `None` so the caller
/// falls back to the kernel's link handling. `column` is a terminal cell column, so wide
/// characters before it are accounted for.
pub(crate) fn plugin_link_at(
    detectors: &[LinkDetector],
    line: &str,
    column: usize,
) -> Option<LinkMatch> {
    let offset = cell_column_to_byte_offset(line, column);
    if offset >= line.len() {
        return None;
    }
    detectors.iter().find_map(|detector| {
        detector
            .regex
            .find_iter(line)
            .find(|found| (found.start()..found.end()).contains(&offset))
            .map(|found| LinkMatch {
                plugin: detector.plugin.clone(),
                detector_id: detector.id.clone(),
                command: detector.command.clone(),
                text: found.as_str().to_string(),
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(detectors: &str) -> String {
        format!(r#"{{"id":"x","contributes":{{"link_detectors":{detectors}}}}}"#)
    }

    fn terminal_manifest() -> String {
        manifest(
            r#"[
                {"id":"path-line","regex":"[./A-Za-z0-9_-]+\\.[a-z]+:[0-9]+","command":"link-open"},
                {"id":"url","regex":"https?://[^\\s]+","command":"link-open"}
            ]"#,
        )
    }

    #[test]
    fn path_line_matches_inside_its_span_and_not_outside() {
        let detectors = detectors_of("core", &terminal_manifest());
        let line = "error at src/a.rs:12 here";
        let found = plugin_link_at(&detectors, line, 12).expect("inside the span");
        assert_eq!(found.detector_id, "path-line");
        assert_eq!(found.text, "src/a.rs:12");
        assert_eq!(found.command, "core/link-open");
        assert_eq!(plugin_link_at(&detectors, line, 2), None);
        assert_eq!(plugin_link_at(&detectors, line, 22), None);
        assert_eq!(plugin_link_at(&detectors, line, 500), None);
    }

    #[test]
    fn url_matches_and_arguments_carry_the_detector_and_the_text() {
        let detectors = detectors_of("core", &terminal_manifest());
        let found = plugin_link_at(&detectors, "see https://example.test/a now", 8).unwrap();
        assert_eq!(found.detector_id, "url");
        assert_eq!(found.text, "https://example.test/a");
        let arguments: Value = serde_json::from_str(&found.arguments_json()).unwrap();
        assert_eq!(arguments["detector_id"], "url");
        assert_eq!(arguments["text"], "https://example.test/a");
    }

    #[test]
    fn first_detector_wins_in_manifest_order_then_plugin_order() {
        let both = manifest(
            r#"[{"id":"first","regex":"abc","command":"a"},{"id":"second","regex":"abc","command":"b"}]"#,
        );
        let other = manifest(r#"[{"id":"third","regex":"abc","command":"c"}]"#);
        let detectors = detectors_for([("one", both.as_str()), ("two", other.as_str())]);
        assert_eq!(detectors.len(), 3);
        let found = plugin_link_at(&detectors, "abc", 1).unwrap();
        assert_eq!(
            (found.plugin.as_str(), found.detector_id.as_str()),
            ("one", "first")
        );
        let reversed = detectors_for([("two", other.as_str()), ("one", both.as_str())]);
        let found = plugin_link_at(&reversed, "abc", 1).unwrap();
        assert_eq!(
            (found.plugin.as_str(), found.detector_id.as_str()),
            ("two", "third")
        );
    }

    #[test]
    fn invalid_oversized_and_incomplete_entries_are_skipped() {
        let long = "a".repeat(PATTERN_LIMIT_BYTES + 1);
        let text = manifest(&format!(
            r#"[
                {{"id":"bad","regex":"(unclosed","command":"c"}},
                {{"id":"look","regex":"(?=a)a","command":"c"}},
                {{"id":"long","regex":"{long}","command":"c"}},
                {{"id":"","regex":"a","command":"c"}},
                {{"id":"nocommand","regex":"a"}},
                {{"id":"empty","regex":"","command":"c"}},
                {{"id":"good","regex":"zz+","command":"c"}}
            ]"#
        ));
        let detectors = detectors_of("p", &text);
        assert_eq!(detectors.len(), 1);
        assert_eq!(detectors[0].id, "good");
    }

    #[test]
    fn a_qualified_command_is_kept_and_the_count_is_capped() {
        let qualified = manifest(r#"[{"id":"a","regex":"a","command":"other.plugin/open"}]"#);
        assert_eq!(
            detectors_of("p", &qualified)[0].command,
            "other.plugin/open"
        );
        let many: Vec<String> = (0..40)
            .map(|index| format!(r#"{{"id":"d{index}","regex":"a","command":"c"}}"#))
            .collect();
        let text = manifest(&format!("[{}]", many.join(",")));
        assert_eq!(detectors_of("p", &text).len(), DETECTORS_PER_PLUGIN);
    }

    #[test]
    fn no_detector_or_a_broken_manifest_falls_back_to_the_kernel() {
        assert!(detectors_of("p", "{}").is_empty());
        assert!(detectors_of("p", "not json").is_empty());
        assert_eq!(plugin_link_at(&[], "src/a.rs:1", 2), None);
    }

    #[test]
    fn the_column_counts_terminal_cells_not_bytes() {
        let detectors = detectors_of("core", &terminal_manifest());
        // Two wide characters take four cells, so the path starts at cell 5.
        let line = "世界 a.rs:3";
        let found = plugin_link_at(&detectors, line, 6).unwrap();
        assert_eq!(found.text, "a.rs:3");
        assert_eq!(plugin_link_at(&detectors, line, 1), None);
    }
}
