//! The global `[status_rules]` table of `config.toml`: client-side checks the
//! core engine cannot do (regular expressions, icon cell widths), tolerant
//! loading that keeps every readable rule, and a save that preserves the
//! entries it could not read.

use std::collections::HashSet;
use std::path::Path;

use ilium_core::status_rule_expression::{
    default_rules, error_summary, has_errors, validate, Condition, IssueKind, Rule, ScreenPattern,
    StatusRules, StatusVisual, ValidationIssue, DEFAULTS_VERSION, MAXIMUM_ENABLED_SCREEN_PATTERNS,
    MAXIMUM_NAME_CHARS, MAXIMUM_RULES, STATUS_RULES_SCHEMA_VERSION,
};

use super::{
    read_config_text, read_toml_document, write_toml_document, ClientError, ConfigLoadError,
    ConfigSaveError,
};

const TABLE: &str = "status_rules";
/// Readable rules beyond `MAXIMUM_RULES` that a hand edit added. They are not
/// loaded, but every save carries them here unchanged so none is lost.
const UNLOADED: &str = "unloaded_rules";

/// Rules read from disk, with everything the loader had to change or skip.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusRulesLoad {
    pub rules: StatusRules,
    /// Repairs made on load (rules disabled, identifiers renamed, entries or
    /// a fallback that could not be read) plus the remaining warnings.
    pub issues: Vec<ValidationIssue>,
}

fn compile_pattern(
    pattern: &ilium_core::status_rule_expression::ScreenPattern,
) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(&pattern.pattern)
        .case_insensitive(pattern.ignore_case)
        .multi_line(true)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
}

/// Core validation plus the client's visual and regular-expression checks.
/// Problems inside a disabled rule are warnings, like the core's own.
pub(crate) fn validate_status_rules(rules: &StatusRules) -> Vec<ValidationIssue> {
    let mut issues = validate(rules);
    for (index, rule) in rules.rules.iter().enumerate() {
        if let Err(error) = crate::status_rule_visual::validate_visual(&rule.then, 2) {
            issues.push(ValidationIssue::content(
                IssueKind::InvalidVisual,
                index,
                rule.enabled,
                format!("Rule {:?}: {error}", rule.name),
            ));
        }
        for (group_index, group) in rule.any_of.iter().enumerate() {
            for (term_index, term) in group.all_of.iter().enumerate() {
                let Condition::ScreenMatches(pattern) = &term.condition else {
                    continue;
                };
                if let Err(error) = compile_pattern(pattern) {
                    issues.push(
                        ValidationIssue::content(
                            IssueKind::ScreenPatternInvalid,
                            index,
                            rule.enabled,
                            format!(
                                "Rule {:?} has an invalid screen pattern: {error}",
                                rule.name
                            ),
                        )
                        .at(group_index, Some(term_index)),
                    );
                }
            }
        }
    }
    if let Some(fallback) = &rules.fallback {
        if let Err(error) = crate::status_rule_visual::validate_visual(fallback, 2) {
            issues.push(ValidationIssue::error(
                IssueKind::InvalidFallback,
                None,
                format!("Fallback: {error}"),
            ));
        }
    }
    issues
}

/// The one banner the rules editor and the Agent Monitoring page show about
/// the last load of `[status_rules]`, and the text of its log line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusRulesLoadBanner {
    /// The file could not be read at all: the defaults are shown and edits
    /// are refused so the saved rules are not overwritten.
    pub unreadable: bool,
    pub headline: String,
    /// One line per problem or repair, in the order the loader found them.
    pub details: Vec<String>,
}

/// Build the load banner from the loader's outcome (`App::
/// custom_status_rules_error` and `custom_status_rules_load_issues`).
/// `None` when the load was clean.
pub fn load_banner(
    error: Option<&str>,
    issues: &[ValidationIssue],
) -> Option<StatusRulesLoadBanner> {
    if let Some(error) = error {
        return Some(StatusRulesLoadBanner {
            unreadable: true,
            headline: "The custom status rules in config.toml could not be read; the default rules are shown and edits are refused until the file is fixed".to_owned(),
            details: vec![error.to_owned()],
        });
    }
    if issues.is_empty() {
        return None;
    }
    let count = issues.len();
    Some(StatusRulesLoadBanner {
        unreadable: false,
        headline: format!(
            "{count} note{} from loading the custom status rules in config.toml",
            if count == 1 { "" } else { "s" }
        ),
        details: issues.iter().map(|issue| issue.message.clone()).collect(),
    })
}

fn readable_rule(entry: &toml::Value) -> Result<Rule<Condition, StatusVisual>, toml::de::Error> {
    entry.clone().try_into()
}

/// Persist the rules in their own global table while preserving every other
/// configuration table and the rule entries this version cannot read. The
/// caller keeps edits in memory on failure.
pub fn save_status_rules(config_dir: &Path, rules: &StatusRules) -> Result<(), ClientError> {
    let path = config_dir.join("config.toml");
    let issues = validate_status_rules(rules);
    if has_errors(&issues) {
        return Err(ClientError::ConfigSave {
            path,
            source: Box::new(ConfigSaveError::InvalidStatusRules(error_summary(&issues))),
        });
    }
    let mut value = toml::Value::try_from(rules).map_err(|source| ClientError::ConfigSave {
        path: path.clone(),
        source: Box::new(ConfigSaveError::Serialize(source)),
    })?;
    let mut document = read_toml_document(&path)?;
    let stored = document.get(TABLE);
    let mut unreadable = Vec::new();
    // Readable entries past the rule limit were never loaded: keep them, with
    // those an earlier save already set aside, in their own list.
    let mut unloaded: Vec<toml::Value> = stored
        .and_then(|table| table.get(UNLOADED))
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut readable_position = 0;
    for entry in stored
        .and_then(|table| table.get("rules"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        if readable_rule(entry).is_err() {
            unreadable.push(entry.clone());
            continue;
        }
        if readable_position >= MAXIMUM_RULES {
            unloaded.push(entry.clone());
        }
        readable_position += 1;
    }
    if let Some(table) = value.as_table_mut() {
        if !unreadable.is_empty() {
            if let Some(entries) = table.get_mut("rules").and_then(toml::Value::as_array_mut) {
                entries.extend(unreadable);
            }
        }
        if !unloaded.is_empty() {
            table.insert(UNLOADED.to_owned(), toml::Value::Array(unloaded));
        }
    }
    document
        .as_table_mut()
        .expect("a TOML document's root is always a table")
        .insert(TABLE.to_string(), value);
    write_toml_document(&path, &document)
}

/// Read the global status rules without creating a config file on first use.
///
/// Every rule that can be read is kept: a rule whose content is invalid is
/// disabled, a duplicate identifier is renamed, an over-long name is cut,
/// an unreadable fallback is dropped and an unreadable entry is left in the
/// file for a later version (`save_status_rules` carries it along). Each
/// repair is reported in `issues`. Only an unreadable file or a table of the
/// wrong shape is an error, and then the saved data is never replaced.
pub fn load_status_rules(config_dir: &Path) -> Result<StatusRulesLoad, ClientError> {
    let path = config_dir.join("config.toml");
    let contents = match read_config_text(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StatusRulesLoad {
                rules: default_rules(),
                issues: Vec::new(),
            });
        }
        Err(source) => {
            return Err(ClientError::ConfigLoad {
                path,
                source: ConfigLoadError::Read(source),
            })
        }
    };
    let document: toml::Value = toml::from_str(&contents).map_err(|_| ClientError::ConfigLoad {
        path: path.clone(),
        source: ConfigLoadError::PrivateConfigParse,
    })?;
    let Some(table) = document.get(TABLE) else {
        return Ok(StatusRulesLoad {
            rules: default_rules(),
            issues: Vec::new(),
        });
    };
    let unsupported = |message: &str| ClientError::ConfigLoad {
        path: path.clone(),
        source: ConfigLoadError::InvalidStatusRules(message.to_owned()),
    };
    let table = table
        .as_table()
        .ok_or_else(|| unsupported("The status rules table has an unsupported format"))?;
    let entries = table
        .get("rules")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| unsupported("The status rules table has no rules list"))?;
    let defaults_version = table
        .get("defaults_version")
        .and_then(toml::Value::as_integer)
        .and_then(|version| u32::try_from(version).ok())
        .unwrap_or(DEFAULTS_VERSION);

    let mut issues = Vec::new();
    // A newer format is read as far as this version understands it; the next
    // save writes the format this version knows, which the warning says.
    if let Some(stored) = table
        .get("version")
        .and_then(toml::Value::as_integer)
        .filter(|stored| *stored > i64::from(STATUS_RULES_SCHEMA_VERSION))
    {
        issues.push(ValidationIssue::warning(
            IssueKind::NewerFormat,
            None,
            format!(
                "The status rules were written in format {stored}, newer than this Ilium's format {STATUS_RULES_SCHEMA_VERSION}; fields it does not know are ignored"
            ),
        ));
    }
    let mut rules = Vec::with_capacity(entries.len().min(MAXIMUM_RULES));
    let mut not_loaded = table
        .get(UNLOADED)
        .and_then(toml::Value::as_array)
        .map_or(0, Vec::len);
    for (position, entry) in entries.iter().enumerate() {
        match readable_rule(entry) {
            Ok(_) if rules.len() >= MAXIMUM_RULES => not_loaded += 1,
            Ok(rule) => rules.push(rule),
            Err(error) => issues.push(ValidationIssue::warning(
                IssueKind::UnreadableRule,
                None,
                format!(
                    "Rule entry {} could not be read and is kept in config.toml unchanged: {}",
                    position + 1,
                    error.message()
                ),
            )),
        }
    }
    if not_loaded > 0 {
        issues.push(ValidationIssue::warning(
            IssueKind::TooManyRules,
            None,
            format!(
                "{not_loaded} rules beyond the limit of {MAXIMUM_RULES} were not loaded and are kept in config.toml unchanged"
            ),
        ));
    }
    let fallback = match table.get("fallback") {
        None => None,
        Some(value) => match value.clone().try_into::<StatusVisual>() {
            Ok(fallback) => Some(fallback),
            Err(error) => {
                issues.push(ValidationIssue::warning(
                    IssueKind::InvalidFallback,
                    None,
                    format!(
                        "The fallback icon could not be read and is ignored: {}",
                        error.message()
                    ),
                ));
                None
            }
        },
    };
    let mut loaded = StatusRules {
        version: STATUS_RULES_SCHEMA_VERSION,
        defaults_version,
        rules,
        fallback,
    };
    repair(&mut loaded, &mut issues);
    let remaining = validate_status_rules(&loaded);
    if has_errors(&remaining) {
        return Err(ClientError::ConfigLoad {
            path,
            source: ConfigLoadError::InvalidStatusRules(error_summary(&remaining)),
        });
    }
    issues.extend(remaining);
    Ok(StatusRulesLoad {
        rules: loaded,
        issues,
    })
}

/// Make a readable ruleset saveable without losing a rule, recording each
/// change against the rule it touched.
fn repair(rules: &mut StatusRules, issues: &mut Vec<ValidationIssue>) {
    let mut seen: HashSet<String> = HashSet::new();
    for (index, rule) in rules.rules.iter_mut().enumerate() {
        if rule.id.is_empty() || seen.contains(&rule.id) {
            let base = if rule.id.is_empty() {
                "rule".to_owned()
            } else {
                rule.id.clone()
            };
            let mut suffix = 2;
            let mut candidate = format!("{base}-{suffix}");
            while seen.contains(&candidate) {
                suffix += 1;
                candidate = format!("{base}-{suffix}");
            }
            issues.push(ValidationIssue::warning(
                IssueKind::InvalidIdentifier,
                Some(index),
                format!(
                    "Rule {:?} had an empty or duplicate identifier and is now {candidate:?}",
                    rule.name
                ),
            ));
            rule.id = candidate;
        }
        seen.insert(rule.id.clone());
        if rule.name.chars().count() > MAXIMUM_NAME_CHARS {
            rule.name = rule.name.chars().take(MAXIMUM_NAME_CHARS).collect();
            issues.push(ValidationIssue::warning(
                IssueKind::NameTooLong,
                Some(index),
                format!("Rule {:?} had a name longer than {MAXIMUM_NAME_CHARS} characters and was shortened", rule.name),
            ));
        }
        let groups = rule.any_of.len();
        rule.any_of.retain(|group| !group.all_of.is_empty());
        if rule.any_of.len() != groups {
            issues.push(ValidationIssue::warning(
                IssueKind::EmptyGroup,
                Some(index),
                format!(
                    "Rule {:?} contained an empty AND group, which was removed",
                    rule.name
                ),
            ));
        }
    }
    if let Some(fallback) = &rules.fallback {
        if let Err(error) = crate::status_rule_visual::validate_visual(fallback, 2) {
            issues.push(ValidationIssue::warning(
                IssueKind::InvalidFallback,
                None,
                format!("The fallback icon is invalid and was dropped: {error}"),
            ));
            rules.fallback = None;
        }
    }
    disable_rules_beyond_the_screen_pattern_budget(rules, issues);
    for issue in validate_status_rules(rules)
        .into_iter()
        .filter(ValidationIssue::is_error)
    {
        if let Some(rule) = issue.rule.and_then(|index| rules.rules.get_mut(index)) {
            if rule.enabled {
                rule.enabled = false;
                // Keep the problem's kind and term path so the editor can
                // still point at the offending condition.
                issues.push(ValidationIssue {
                    path: issue.path,
                    ..ValidationIssue::warning(
                        issue.kind,
                        issue.rule,
                        format!(
                            "Rule {:?} was disabled on load: {}",
                            rule.name, issue.message
                        ),
                    )
                });
            }
        }
    }
}

/// The server watches at most `MAXIMUM_ENABLED_SCREEN_PATTERNS` distinct
/// patterns. Keep the earliest (highest-priority) enabled rules whose
/// patterns fit and disable each later rule that would need another one, so
/// a hand-edited file that asks for too many still loads and stays saveable.
fn disable_rules_beyond_the_screen_pattern_budget(
    rules: &mut StatusRules,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut watched: Vec<ScreenPattern> = Vec::new();
    for (index, rule) in rules.rules.iter_mut().enumerate() {
        if !rule.enabled {
            continue;
        }
        let mut needed: Vec<&ScreenPattern> = Vec::new();
        for term in rule.any_of.iter().flat_map(|group| &group.all_of) {
            if let Condition::ScreenMatches(pattern) = &term.condition {
                if !watched.contains(pattern) && !needed.contains(&pattern) {
                    needed.push(pattern);
                }
            }
        }
        if watched.len() + needed.len() <= MAXIMUM_ENABLED_SCREEN_PATTERNS {
            watched.extend(needed.into_iter().cloned());
            continue;
        }
        rule.enabled = false;
        issues.push(ValidationIssue::warning(
            IssueKind::TooManyScreenPatterns,
            Some(index),
            format!(
                "Rule {:?} was disabled on load: at most {MAXIMUM_ENABLED_SCREEN_PATTERNS} distinct screen patterns can be watched at once",
                rule.name
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::status_rule_expression::{
        default_rules, has_errors, Condition, IssueKind, ScreenPattern, ScreenRegion, StatusRules,
        TermPath, ValidationIssue, STATUS_RULES_SCHEMA_VERSION,
    };

    use super::{
        load_banner, load_status_rules, save_status_rules, validate_status_rules,
        StatusRulesLoadBanner,
    };

    #[test]
    fn load_banner_reports_an_unreadable_file_or_every_load_note() {
        assert_eq!(load_banner(None, &[]), None);
        let failure = load_banner(Some("config.toml is not valid TOML"), &[]).unwrap();
        assert_eq!(
            failure,
            StatusRulesLoadBanner {
                unreadable: true,
                headline: "The custom status rules in config.toml could not be read; the default rules are shown and edits are refused until the file is fixed".to_owned(),
                details: vec!["config.toml is not valid TOML".to_owned()],
            }
        );
        let notes = [
            ValidationIssue::warning(IssueKind::InvalidIdentifier, Some(1), "renamed"),
            ValidationIssue::warning(IssueKind::UnreadableRule, None, "kept as is"),
        ];
        let banner = load_banner(None, &notes).unwrap();
        assert!(!banner.unreadable);
        assert_eq!(
            banner.headline,
            "2 notes from loading the custom status rules in config.toml"
        );
        assert_eq!(banner.details, vec!["renamed", "kept as is"]);
        assert_eq!(
            load_banner(None, &notes[..1]).unwrap().headline,
            "1 note from loading the custom status rules in config.toml"
        );
        // An unreadable file wins over notes: nothing was loaded from it.
        assert!(load_banner(Some("broken"), &notes).unwrap().unreadable);
    }

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-client-status-rules-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn broken_pattern() -> Condition {
        Condition::ScreenMatches(ScreenPattern {
            pattern: "[".to_owned(),
            ignore_case: false,
            region: ScreenRegion::All,
        })
    }

    #[test]
    fn invalid_custom_screen_pattern_does_not_overwrite_saved_rules() {
        let directory = scratch_dir();
        let mut rules = default_rules();
        save_status_rules(&directory, &rules).unwrap();
        let original = std::fs::read(directory.join("config.toml")).unwrap();
        let example_index = rules
            .rules
            .iter()
            .position(|rule| rule.id == "screen-error-example")
            .unwrap();
        let example = &mut rules.rules[example_index];
        example.enabled = true;
        example.any_of[0].all_of[0].condition = broken_pattern();
        let issues = validate_status_rules(&rules);
        assert!(
            issues
                .iter()
                .any(|issue| issue.is_error() && issue.rule == Some(example_index)),
            "{issues:?}"
        );
        assert!(save_status_rules(&directory, &rules).is_err());
        assert_eq!(
            std::fs::read(directory.join("config.toml")).unwrap(),
            original
        );
    }

    #[test]
    fn custom_status_rules_round_trip_and_preserve_other_settings() {
        let directory = scratch_dir();
        let rules = default_rules();
        let loaded = load_status_rules(&directory).unwrap();
        assert_eq!((loaded.rules, loaded.issues.len()), (rules.clone(), 0));
        assert!(!directory.join("config.toml").exists());
        std::fs::write(
            directory.join("config.toml"),
            "[ui]\nshow_context_menu_icons = false\n",
        )
        .unwrap();
        save_status_rules(&directory, &rules).unwrap();
        let loaded = load_status_rules(&directory).unwrap();
        assert_eq!(loaded.rules, rules);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert!(
            !super::super::load(&directory)
                .unwrap()
                .ui
                .show_context_menu_icons
        );
    }

    #[test]
    fn malformed_custom_status_rules_are_kept_in_the_file_and_reported() {
        let directory = scratch_dir();
        let original = "[status_rules]\ndefaults_version = 1\nrules = [\"not a rule\"]\n";
        std::fs::write(directory.join("config.toml"), original).unwrap();
        let loaded = load_status_rules(&directory).unwrap();
        assert!(loaded.rules.rules.is_empty());
        assert_eq!(loaded.issues.len(), 1);
        assert_eq!(loaded.issues[0].rule, None);
        assert!(
            loaded.issues[0].message.contains("entry 1"),
            "{}",
            loaded.issues[0].message
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("config.toml")).unwrap(),
            original
        );

        save_status_rules(&directory, &default_rules()).unwrap();
        let text = std::fs::read_to_string(directory.join("config.toml")).unwrap();
        assert!(
            text.contains("\"not a rule\""),
            "unreadable entry must survive a save"
        );
        let reloaded = load_status_rules(&directory).unwrap();
        assert_eq!(reloaded.rules, default_rules());
        assert_eq!(reloaded.issues.len(), 1);

        std::fs::write(directory.join("config.toml"), "[status_rules]\nrules = 3\n").unwrap();
        assert!(load_status_rules(&directory).is_err());
        std::fs::write(directory.join("config.toml"), "status_rules = 3\n").unwrap();
        assert!(load_status_rules(&directory).is_err());
    }

    #[test]
    fn broken_enabled_rules_are_disabled_on_load_and_stay_saveable() {
        let directory = scratch_dir();
        let mut rules = default_rules();
        let example_index = rules
            .rules
            .iter()
            .position(|rule| rule.id == "screen-error-example")
            .unwrap();
        let example = &mut rules.rules[example_index];
        example.enabled = true;
        example.any_of[0].all_of[0].condition = broken_pattern();
        rules.rules[1].id = rules.rules[0].id.clone();
        rules.rules[2].name = "n".repeat(60);
        rules.rules[3]
            .any_of
            .push(ilium_core::status_rule_expression::AllOf { all_of: Vec::new() });
        // Bypass the validating saver to write the broken table directly.
        let document: toml::Value = toml::Value::Table(
            [(
                "status_rules".to_owned(),
                toml::Value::try_from(&rules).unwrap(),
            )]
            .into_iter()
            .collect(),
        );
        std::fs::write(
            directory.join("config.toml"),
            toml::to_string(&document).unwrap(),
        )
        .unwrap();

        let loaded = load_status_rules(&directory).unwrap();
        let example = loaded
            .rules
            .rules
            .iter()
            .find(|rule| rule.name == "Screen mentions an error")
            .unwrap();
        assert!(!example.enabled);
        assert_eq!(loaded.rules.rules[1].id, "approval-2");
        assert_eq!(loaded.rules.rules[2].name.chars().count(), 48);
        assert_eq!(loaded.rules.rules[3].any_of.len(), 1);
        let messages: Vec<&str> = loaded
            .issues
            .iter()
            .map(|issue| issue.message.as_str())
            .collect();
        assert!(
            messages
                .iter()
                .any(|message| message.contains("disabled on load")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("\"approval-2\"")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message.contains("shortened")),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("empty AND group")),
            "{messages:?}"
        );
        assert!(loaded.issues.iter().all(|issue| !issue.is_error()));
        assert!(loaded
            .issues
            .iter()
            .any(|issue| issue.rule == Some(example_index)
                && issue.message.starts_with("Disabled rule:")));
        // The repair keeps the problem's kind and the offending term's path.
        let repair = loaded
            .issues
            .iter()
            .find(|issue| {
                issue.rule == Some(example_index) && issue.message.contains("disabled on load")
            })
            .unwrap();
        assert_eq!(repair.kind, IssueKind::ScreenPatternInvalid);
        assert_eq!(
            repair.path,
            Some(TermPath {
                group: 0,
                term: Some(0)
            })
        );

        save_status_rules(&directory, &loaded.rules).unwrap();
        let reloaded = load_status_rules(&directory).unwrap();
        assert_eq!(reloaded.rules, loaded.rules);
        assert!(reloaded.issues.iter().all(|issue| !issue.is_error()));
        assert!(reloaded
            .issues
            .iter()
            .all(|issue| !issue.message.contains("disabled on load")));
    }

    fn screen_rule(
        id: &str,
        pattern: &str,
    ) -> ilium_core::status_rule_expression::Rule<
        Condition,
        ilium_core::status_rule_expression::StatusVisual,
    > {
        let mut rule = default_rules()
            .rules
            .into_iter()
            .find(|rule| rule.id == "screen-error-example")
            .unwrap();
        rule.id = id.to_owned();
        rule.name = id.to_owned();
        rule.enabled = true;
        rule.any_of[0].all_of[0].condition = Condition::ScreenMatches(ScreenPattern {
            pattern: pattern.to_owned(),
            ignore_case: false,
            region: ScreenRegion::All,
        });
        rule
    }

    fn write_unvalidated(directory: &std::path::Path, rules: &StatusRules) {
        let document = toml::Value::Table(
            [(
                "status_rules".to_owned(),
                toml::Value::try_from(rules).unwrap(),
            )]
            .into_iter()
            .collect(),
        );
        std::fs::write(
            directory.join("config.toml"),
            toml::to_string(&document).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn too_many_screen_patterns_disable_the_lowest_priority_rules_on_load() {
        let directory = scratch_dir();
        let mut rules = default_rules();
        // Sixteen distinct patterns fit; a rule reusing one of them still
        // fits; the two after that each need a seventeenth.
        for index in 0..16 {
            rules.rules.push(screen_rule(
                &format!("screen-{index}"),
                &format!("p{index}"),
            ));
        }
        rules.rules.push(screen_rule("reuses-first", "p0"));
        rules.rules.push(screen_rule("seventeenth", "p16"));
        rules.rules.push(screen_rule("eighteenth", "p17"));
        assert!(has_errors(&validate_status_rules(&rules)));
        write_unvalidated(&directory, &rules);

        let loaded = load_status_rules(&directory).unwrap();
        let enabled = |id: &str| {
            loaded
                .rules
                .rules
                .iter()
                .find(|rule| rule.id == id)
                .unwrap()
                .enabled
        };
        assert!(enabled("screen-0") && enabled("screen-15") && enabled("reuses-first"));
        assert!(!enabled("seventeenth") && !enabled("eighteenth"));
        assert_eq!(
            loaded.rules.rules.len(),
            rules.rules.len(),
            "no rule is lost"
        );
        assert_eq!(
            loaded
                .issues
                .iter()
                .filter(|issue| issue.message.contains("distinct screen patterns"))
                .count(),
            2
        );
        assert!(!has_errors(&loaded.issues));
        save_status_rules(&directory, &loaded.rules).unwrap();
        assert_eq!(load_status_rules(&directory).unwrap().rules, loaded.rules);
    }

    #[test]
    fn rules_beyond_the_limit_are_kept_in_the_file_but_not_loaded() {
        let directory = scratch_dir();
        let mut rules = default_rules();
        let mut index = 0;
        while rules.rules.len() < 70 {
            let mut rule = rules.rules[0].clone();
            rule.id = format!("extra-{index}");
            rules.rules.push(rule);
            index += 1;
        }
        let overflow: Vec<String> = rules.rules[64..]
            .iter()
            .map(|rule| rule.id.clone())
            .collect();
        write_unvalidated(&directory, &rules);

        let loaded = load_status_rules(&directory).unwrap();
        assert_eq!(loaded.rules.rules.len(), 64);
        assert!(loaded
            .issues
            .iter()
            .any(|issue| issue.message.starts_with("6 rules beyond the limit")));

        // Two saves of a shrunken set must still keep all six set-aside rules.
        let mut fewer = loaded.rules.clone();
        fewer.rules.truncate(60);
        save_status_rules(&directory, &fewer).unwrap();
        save_status_rules(&directory, &fewer).unwrap();
        let document: toml::Value =
            toml::from_str(&std::fs::read_to_string(directory.join("config.toml")).unwrap())
                .unwrap();
        let kept: Vec<String> = document["status_rules"]["unloaded_rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(kept, overflow);
        let reloaded = load_status_rules(&directory).unwrap();
        assert_eq!(reloaded.rules.rules, fewer.rules);
        assert!(reloaded
            .issues
            .iter()
            .any(|issue| issue.message.starts_with("6 rules beyond the limit")));
    }

    #[test]
    fn schema_version_is_written_defaulted_and_a_newer_one_is_reported() {
        let directory = scratch_dir();
        save_status_rules(&directory, &default_rules()).unwrap();
        let text = std::fs::read_to_string(directory.join("config.toml")).unwrap();
        assert!(text.contains("version = 1"), "{text}");

        std::fs::write(
            directory.join("config.toml"),
            "[status_rules]\ndefaults_version = 1\nrules = []\n",
        )
        .unwrap();
        let loaded = load_status_rules(&directory).unwrap();
        assert_eq!(loaded.rules.version, STATUS_RULES_SCHEMA_VERSION);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);

        std::fs::write(
            directory.join("config.toml"),
            "[status_rules]\nversion = 9\ndefaults_version = 1\nrules = []\n",
        )
        .unwrap();
        let loaded = load_status_rules(&directory).unwrap();
        assert_eq!(loaded.rules.version, STATUS_RULES_SCHEMA_VERSION);
        assert!(loaded
            .issues
            .iter()
            .any(|issue| !issue.is_error() && issue.message.contains("format 9")));
    }

    #[test]
    fn unreadable_fallback_is_dropped_with_a_warning() {
        let directory = scratch_dir();
        std::fs::write(
            directory.join("config.toml"),
            "[status_rules]\ndefaults_version = 1\nrules = []\n[status_rules.fallback]\nkind = 7\n",
        )
        .unwrap();
        let loaded = load_status_rules(&directory).unwrap();
        assert!(loaded.rules.fallback.is_none());
        assert!(loaded
            .issues
            .iter()
            .any(|issue| issue.rule.is_none() && issue.message.contains("fallback")));
    }
}
