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

use crate::terminal_view::cell_column_to_byte_offset;

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

/// The fully qualified commands (`<plugin>/<command>`) the given manifests declare in
/// `contributes.commands`.
fn declared_commands<'a>(
    manifests: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> std::collections::BTreeSet<String> {
    let mut declared = std::collections::BTreeSet::new();
    for (plugin, manifest) in manifests {
        let Ok(manifest) = serde_json::from_str::<Value>(manifest) else {
            continue;
        };
        let entries = manifest
            .pointer("/contributes/commands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for entry in entries {
            if let Some(id) = entry.get("id").and_then(Value::as_str).map(str::trim) {
                if !id.is_empty() {
                    declared.insert(format!("{plugin}/{id}"));
                }
            }
        }
    }
    declared
}

/// The detectors of the enabled plugins that the click path uses: those whose command an
/// enabled plugin declares (EXT-050 says the command must name a declared command). A detector
/// whose command does not resolve would take the click away from the kernel's own link
/// handling and then do nothing, so it is left out and the kernel keeps the click.
pub(crate) fn active_detectors(manifests: &[(String, String)]) -> Vec<LinkDetector> {
    let pairs = || {
        manifests
            .iter()
            .map(|(plugin, manifest)| (plugin.as_str(), manifest.as_str()))
    };
    let declared = declared_commands(pairs());
    detectors_for(pairs())
        .into_iter()
        .filter(|detector| declared.contains(&detector.command))
        .collect()
}

/// The compiled detectors of the enabled plugins, rebuilt only when the set of enabled
/// manifests changes, so a click never recompiles a regex.
#[derive(Debug, Default)]
pub(crate) struct DetectorCache {
    built: Option<(Vec<(String, String)>, std::sync::Arc<[LinkDetector]>)>,
}

impl DetectorCache {
    /// The detectors for `manifests`, compiled once per distinct set.
    pub(crate) fn detectors(
        &mut self,
        manifests: Vec<(String, String)>,
    ) -> std::sync::Arc<[LinkDetector]> {
        match &self.built {
            Some((built_for, detectors)) if *built_for == manifests => {
                std::sync::Arc::clone(detectors)
            }
            _ => {
                let detectors: std::sync::Arc<[LinkDetector]> = active_detectors(&manifests).into();
                self.built = Some((manifests, std::sync::Arc::clone(&detectors)));
                detectors
            }
        }
    }
}

impl crate::app::App {
    /// The link detectors of the enabled plugins, handed to the terminal link job of a click.
    /// Empty without a plugin host.
    pub(crate) fn plugin_link_detectors(&self) -> std::sync::Arc<[LinkDetector]> {
        self.wasm_plugins
            .as_ref()
            .map(super::host::ClientPlugins::link_detectors)
            .unwrap_or_else(|| std::sync::Arc::from(Vec::new()))
    }

    /// Opens (Ctrl+O) a link a plugin detector matched: `link-activated` goes to the owner and
    /// the detector's command runs with the detector id and the text as its arguments.
    pub(crate) fn activate_plugin_link(&mut self, found: &LinkMatch) {
        let pane = self.active_pane_id();
        self.activate_plugin_link_in(found, pane);
    }

    /// Opens a plugin-matched link of a known pane, such as the pane a right-click menu was
    /// opened on.
    pub(crate) fn activate_plugin_link_in(
        &mut self,
        found: &LinkMatch,
        pane: Option<ilium_core::NodeId>,
    ) {
        let screen = self.popup_screen();
        let Some(plugins) = self.wasm_plugins.as_ref() else {
            self.status_message = Some(format!(
                "{} is not available: plugins are off",
                found.command
            ));
            return;
        };
        plugins.activate_link(found, pane, screen);
        self.status_message = Some(format!("Opening {} with {}", found.text, found.command));
    }
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
    fn only_detectors_whose_command_is_declared_reach_the_click_path() {
        let declared = r##"{"id":"example.issues","contributes":{
            "commands":[{"id":"open","title":"Open issue"}],
            "link_detectors":[
                {"id":"issue-ref","regex":"#[0-9]+","command":"open"},
                {"id":"dangling","regex":"[a-z]+","command":"missing"}
            ]}}"##;
        let manifests = vec![
            ("example.issues".to_owned(), declared.to_owned()),
            ("core".to_owned(), terminal_manifest()),
        ];
        let detectors = active_detectors(&manifests);
        assert_eq!(detectors.len(), 1, "{detectors:?}");
        assert_eq!(detectors[0].command, "example.issues/open");
        let found = plugin_link_at(&detectors, "fix #42 now", 5).expect("on the reference");
        assert_eq!(found.text, "#42");
        assert_eq!(
            serde_json::from_str::<Value>(&found.arguments_json()).unwrap(),
            serde_json::json!({ "detector_id": "issue-ref", "text": "#42" })
        );
        // The terminal plugin's detectors name an undeclared command: the kernel keeps them.
        assert_eq!(plugin_link_at(&detectors, "src/a.rs:12", 3), None);
    }

    #[test]
    fn the_cache_compiles_once_per_set_of_enabled_manifests() {
        let manifests = vec![(
            "example.issues".to_owned(),
            r##"{"contributes":{"commands":[{"id":"open"}],
                "link_detectors":[{"id":"i","regex":"#[0-9]+","command":"open"}]}}"##
                .to_owned(),
        )];
        let mut cache = DetectorCache::default();
        let first = cache.detectors(manifests.clone());
        let again = cache.detectors(manifests);
        assert!(std::sync::Arc::ptr_eq(&first, &again));
        assert_eq!(first.len(), 1);
        assert!(cache.detectors(Vec::new()).is_empty());
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
