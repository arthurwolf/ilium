//! The pane selection `ilium recap` shares with `ilium panes` and `ilium broadcast` (the same
//! hand-written option parser and matching rules as the fleet plugin, which keeps its own copy
//! because a plugin is one self-contained component).
//!
//! Semantics, in one place:
//!
//! - Every selector kind narrows the result (they combine with AND).
//! - Repeated values of one selector widen it (they combine with OR), and list-valued selectors
//!   also accept comma-separated values.
//! - `--match` and `--regex` are the text filter: a pane passes when any of them matches any of
//!   the searched fields. `--invert` turns the text filter around (a pane passes when none of
//!   them matches). It applies only to the text filter, never to the structural selectors.
//!
//! Matching is pure: no host call is made here, which is what the unit tests below exercise.

use std::path::{Component, Path, PathBuf};

use regex::{Regex, RegexBuilder};
use serde_json::{json, Value};

/// What a pane is, as the selectors and the JSONL records name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// A terminal with a detected agent CLI that can receive input.
    Agent,
    /// A terminal without a detected agent.
    Shell,
    /// An editor pane.
    Editor,
    /// A kanban board pane.
    Board,
    /// A restored agent pane whose agent is not running.
    UnavailableAgent,
    /// A pane whose body a plugin draws (a plugin pane kind).
    Plugin,
}

impl PaneKind {
    /// The kind a record names, `None` for a word that is no kind.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "agent" => Self::Agent,
            "shell" => Self::Shell,
            "editor" => Self::Editor,
            "board" => Self::Board,
            "unavailable-agent" => Self::UnavailableAgent,
            "plugin" => Self::Plugin,
            _ => return None,
        })
    }

    /// The name in records and in `--kind`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Shell => "shell",
            Self::Editor => "editor",
            Self::Board => "board",
            Self::UnavailableAgent => "unavailable-agent",
            Self::Plugin => "plugin",
        }
    }
}

/// A live agent's current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStateName {
    Working,
    WaitingApproval,
    WaitingSubagents,
    Settling,
    Idle,
    Done,
}

impl AgentStateName {
    /// The state a record names, `None` for a word that is no state.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "working" => Self::Working,
            "waiting-approval" => Self::WaitingApproval,
            "waiting-subagents" => Self::WaitingSubagents,
            "settling" => Self::Settling,
            "idle" => Self::Idle,
            "done" => Self::Done,
            _ => return None,
        })
    }

    /// The name in records and in `--state`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::WaitingApproval => "waiting-approval",
            Self::WaitingSubagents => "waiting-subagents",
            Self::Settling => "settling",
            Self::Idle => "idle",
            Self::Done => "done",
        }
    }

    /// Idle and done agents have no turn in progress: input reaches an empty composer instead of
    /// interrupting or queueing behind work.
    pub const fn is_idle(self) -> bool {
        matches!(self, Self::Idle | Self::Done)
    }
}

/// `--state` values: `idle` also covers `done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateSelector {
    Working,
    WaitingApproval,
    WaitingSubagents,
    Settling,
    Idle,
    Done,
}

impl StateSelector {
    fn parse(value: &str) -> Result<Self, String> {
        Ok(match value {
            "working" => Self::Working,
            "waiting-approval" => Self::WaitingApproval,
            "waiting-subagents" => Self::WaitingSubagents,
            "settling" => Self::Settling,
            "idle" => Self::Idle,
            "done" => Self::Done,
            other => {
                return Err(format!(
                    "invalid value {other:?} for --state (working, waiting-approval, \
                     waiting-subagents, settling, idle, done)"
                ))
            }
        })
    }

    fn accepts(self, state: AgentStateName) -> bool {
        match self {
            Self::Working => state == AgentStateName::Working,
            Self::WaitingApproval => state == AgentStateName::WaitingApproval,
            Self::WaitingSubagents => state == AgentStateName::WaitingSubagents,
            Self::Settling => state == AgentStateName::Settling,
            Self::Idle => state.is_idle(),
            Self::Done => state == AgentStateName::Done,
        }
    }
}

/// Where the text filter looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextField {
    /// The pane's title and its short form.
    Name,
    /// The pane's project directory.
    Project,
    /// The directory the pane was launched in.
    Cwd,
    /// The agent's name (Claude, Codex, ...).
    Agent,
    /// The session name.
    Session,
}

const ALL_TEXT_FIELDS: [TextField; 5] = [
    TextField::Name,
    TextField::Project,
    TextField::Cwd,
    TextField::Agent,
    TextField::Session,
];

impl TextField {
    fn parse(value: &str) -> Result<Self, String> {
        Ok(match value {
            "name" => Self::Name,
            "project" => Self::Project,
            "cwd" => Self::Cwd,
            "agent" => Self::Agent,
            "session" => Self::Session,
            other => {
                return Err(format!(
                    "invalid value {other:?} for --field (name, project, cwd, agent, session)"
                ))
            }
        })
    }
}

/// Everything the selectors look at for one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneFacts {
    pub session_name: String,
    /// The endpoint identity of the session, the name `sessions.send` takes.
    pub session_id: String,
    pub pane_id: u64,
    pub name: String,
    pub short_name: Option<String>,
    /// The enclosing project's root, if the pane sits in a project.
    pub project: Option<PathBuf>,
    /// The directory the pane was launched in (its project root for panes that recorded none).
    pub cwd: Option<PathBuf>,
    pub kind: PaneKind,
    /// The agent's display name, for live and unavailable agents.
    pub agent: Option<String>,
    /// Only live agents have a state.
    pub state: Option<AgentStateName>,
    /// The pane this command runs in.
    pub is_self: bool,
}

impl PaneFacts {
    /// The project directory selectors compare against: the project root, or the launch
    /// directory for a pane outside any project.
    fn project_or_cwd(&self) -> Option<&Path> {
        self.project.as_deref().or(self.cwd.as_deref())
    }

    /// The record of this pane in the JSONL output.
    pub fn to_json(&self) -> Value {
        json!({
            "session": self.session_name,
            "pane_id": self.pane_id,
            "name": self.name,
            "kind": self.kind.name(),
            "agent": self.agent,
            "state": self.state.map(AgentStateName::name),
            "project": self.project.as_ref().map(|path| path.to_string_lossy()),
            "cwd": self.cwd.as_ref().map(|path| path.to_string_lossy()),
            "is_self": self.is_self,
        })
    }

    /// One pane of a `list-panes` reply of the session named `session_name`. `None` when the
    /// record has no pane id.
    pub fn from_record(record: &Value, session_name: &str, session_id: &str) -> Option<Self> {
        let text = |field: &str| record.get(field).and_then(Value::as_str).map(str::to_owned);
        let path = |field: &str| text(field).map(PathBuf::from);
        Some(Self {
            session_name: session_name.to_owned(),
            session_id: session_id.to_owned(),
            pane_id: record.get("pane_id").and_then(Value::as_u64)?,
            name: text("name").unwrap_or_default(),
            short_name: text("short_name"),
            project: path("project"),
            cwd: path("cwd"),
            kind: text("kind")
                .and_then(|kind| PaneKind::from_name(&kind))
                .unwrap_or(PaneKind::Shell),
            agent: text("agent"),
            state: text("state").and_then(|state| AgentStateName::from_name(&state)),
            is_self: record
                .get("is_self")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Option parsing
// ---------------------------------------------------------------------------------------------

/// One option a command accepts.
#[derive(Debug, Clone, Copy)]
pub struct OptionSpec {
    pub long: &'static str,
    pub short: Option<char>,
    /// Whether a value follows (`--name value`, `--name=value`).
    pub takes_value: bool,
}

const fn option(long: &'static str, short: Option<char>, takes_value: bool) -> OptionSpec {
    OptionSpec {
        long,
        short,
        takes_value,
    }
}

/// The options of the shared selection (`--cwd` anchors relative projects and `--here`).
pub const FILTER_OPTIONS: [OptionSpec; 12] = [
    option("match", Some('m'), true),
    option("regex", Some('e'), true),
    option("field", None, true),
    option("invert", Some('v'), false),
    option("project", Some('p'), true),
    option("exclude-project", None, true),
    option("here", None, false),
    option("session", Some('s'), true),
    option("agent", Some('a'), true),
    option("state", None, true),
    option("kind", None, true),
    option("pane", None, true),
];

/// The `--cwd` option every command with a selection also takes.
pub const CWD_OPTION: OptionSpec = option("cwd", None, true);

/// The words of a command line, split into options and positional words.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedArguments {
    /// Each option given, by its long name, with its value (empty for a flag), in order.
    pub options: Vec<(String, String)>,
    /// The words that are no option, in order. A lone `-` is one of them.
    pub words: Vec<String>,
}

impl ParsedArguments {
    /// Whether a flag (or any option) was given.
    pub fn has(&self, long: &str) -> bool {
        self.options.iter().any(|(name, _)| name == long)
    }

    /// Every value of an option, in order.
    pub fn values(&self, long: &str) -> Vec<&str> {
        self.options
            .iter()
            .filter(|(name, _)| name == long)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    /// The last value of an option.
    pub fn last(&self, long: &str) -> Option<&str> {
        self.values(long).last().copied()
    }

    /// Every value of a list option: repeated and comma-separated values, empty ones dropped.
    pub fn list(&self, long: &str) -> Vec<String> {
        self.values(long)
            .into_iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

/// Splits `arguments` along `specs`. A word after `--` is never an option; a lone `-` is a word.
pub fn parse_arguments(
    arguments: &[String],
    specs: &[OptionSpec],
) -> Result<ParsedArguments, String> {
    let mut parsed = ParsedArguments::default();
    let mut index = 0;
    let mut options_ended = false;
    while index < arguments.len() {
        let argument = &arguments[index];
        index += 1;
        if options_ended || argument == "-" || !argument.starts_with('-') {
            parsed.words.push(argument.clone());
            continue;
        }
        if argument == "--" {
            options_ended = true;
            continue;
        }
        let (spec, inline_value) = if let Some(long) = argument.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (long, None),
            };
            let spec = specs
                .iter()
                .find(|spec| spec.long == name)
                .ok_or_else(|| format!("unexpected argument '--{name}'"))?;
            (spec, value)
        } else {
            let mut characters = argument.chars().skip(1);
            let short = characters
                .next()
                .ok_or_else(|| format!("unexpected argument '{argument}'"))?;
            let spec = specs
                .iter()
                .find(|spec| spec.short == Some(short))
                .ok_or_else(|| format!("unexpected argument '-{short}'"))?;
            let rest: String = characters.collect();
            let value = (!rest.is_empty()).then_some(rest);
            (spec, value)
        };
        if spec.takes_value {
            let value = match inline_value {
                Some(value) => value,
                None => {
                    let next = arguments
                        .get(index)
                        .ok_or_else(|| format!("a value is required for '--{}'", spec.long))?;
                    index += 1;
                    next.clone()
                }
            };
            parsed.options.push((spec.long.to_owned(), value));
        } else if inline_value.is_some() && argument.starts_with("--") {
            return Err(format!("'--{}' takes no value", spec.long));
        } else {
            parsed.options.push((spec.long.to_owned(), String::new()));
        }
    }
    Ok(parsed)
}

// ---------------------------------------------------------------------------------------------
// The filter
// ---------------------------------------------------------------------------------------------

/// A `--project` value, resolved once.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectSelector {
    /// Projects at or below this absolute directory.
    Directory(PathBuf),
    /// Projects whose folder has this name (case-insensitive).
    FolderName(String),
}

impl ProjectSelector {
    fn parse(value: &str, cwd: &Path, home: Option<&Path>) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("empty project value".to_owned());
        }
        let is_directory = value.contains('/')
            || value.contains(std::path::MAIN_SEPARATOR)
            || matches!(value, "." | ".." | "~")
            || value.starts_with('~');
        if !is_directory {
            return Ok(Self::FolderName(value.to_owned()));
        }
        let expanded = match value.strip_prefix('~') {
            Some(rest) => {
                let home =
                    home.ok_or_else(|| format!("cannot expand {value:?}: no home directory"))?;
                home.join(rest.trim_start_matches(['/', std::path::MAIN_SEPARATOR]))
            }
            None => PathBuf::from(value),
        };
        let absolute = if expanded.is_absolute() {
            expanded
        } else {
            cwd.join(expanded)
        };
        // A component has no file system to canonicalise against, so the selector is normalised
        // lexically; sessions record canonical project roots.
        Ok(Self::Directory(lexically_normalized(&absolute)))
    }

    fn accepts(&self, facts: &PaneFacts) -> bool {
        let Some(project) = facts.project_or_cwd() else {
            return false;
        };
        match self {
            Self::Directory(directory) => project.starts_with(directory),
            Self::FolderName(name) => project
                .file_name()
                .is_some_and(|folder| folder.to_string_lossy().eq_ignore_ascii_case(name)),
        }
    }
}

fn lexically_normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

/// The selectors of one command line, before they are compiled.
#[derive(Debug, Clone, Default)]
pub struct PaneFilterArgs {
    pub matches: Vec<String>,
    pub regexes: Vec<String>,
    pub fields: Vec<String>,
    pub invert: bool,
    pub projects: Vec<String>,
    pub excluded_projects: Vec<String>,
    pub here: bool,
    pub sessions: Vec<String>,
    pub agents: Vec<String>,
    pub states: Vec<String>,
    pub kinds: Vec<String>,
    pub pane_ids: Vec<String>,
}

impl PaneFilterArgs {
    /// The selectors of a parsed command line.
    pub fn from_parsed(parsed: &ParsedArguments) -> Self {
        Self {
            matches: parsed
                .values("match")
                .into_iter()
                .map(str::to_owned)
                .collect(),
            regexes: parsed
                .values("regex")
                .into_iter()
                .map(str::to_owned)
                .collect(),
            fields: parsed.list("field"),
            invert: parsed.has("invert"),
            projects: parsed.list("project"),
            excluded_projects: parsed.list("exclude-project"),
            here: parsed.has("here"),
            sessions: parsed.list("session"),
            agents: parsed.list("agent"),
            states: parsed.list("state"),
            kinds: parsed.list("kind"),
            pane_ids: parsed.list("pane"),
        }
    }

    /// Validates and resolves the selectors. `cwd` anchors relative project paths and `--here`;
    /// `home` expands a leading `~`.
    pub fn compile(&self, cwd: &Path, home: Option<&Path>) -> Result<PaneFilter, String> {
        let states = self
            .states
            .iter()
            .map(|state| StateSelector::parse(state))
            .collect::<Result<Vec<_>, _>>()?;
        let fields = self
            .fields
            .iter()
            .map(|field| TextField::parse(field))
            .collect::<Result<Vec<_>, _>>()?;
        let kinds = self
            .kinds
            .iter()
            .map(|kind| {
                PaneKind::from_name(kind).ok_or_else(|| {
                    format!(
                        "invalid value {kind:?} for --kind (agent, shell, editor, board, \
                         unavailable-agent, plugin)"
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let pane_ids = self
            .pane_ids
            .iter()
            .map(|id| {
                id.parse::<u64>()
                    .map_err(|_| format!("invalid value {id:?} for --pane (a pane id)"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if self.invert && self.matches.is_empty() && self.regexes.is_empty() {
            return Err("--invert needs at least one --match or --regex to invert".to_owned());
        }
        let regexes = self
            .regexes
            .iter()
            .map(|pattern| {
                RegexBuilder::new(pattern)
                    .case_insensitive(true)
                    .build()
                    .map_err(|error| format!("invalid --regex {pattern:?}: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let parse_projects = |values: &[String]| {
            values
                .iter()
                .map(|value| {
                    ProjectSelector::parse(value, cwd, home)
                        .map_err(|error| format!("invalid project {value:?}: {error}"))
                })
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(PaneFilter {
            substrings: self
                .matches
                .iter()
                .map(|text| text.to_lowercase())
                .collect(),
            regexes,
            fields: if fields.is_empty() {
                ALL_TEXT_FIELDS.to_vec()
            } else {
                fields
            },
            invert: self.invert,
            projects: parse_projects(&self.projects)?,
            excluded_projects: parse_projects(&self.excluded_projects)?,
            here: self.here.then(|| lexically_normalized(cwd)),
            sessions: self.sessions.clone(),
            agents: self
                .agents
                .iter()
                .map(|agent| agent.to_lowercase())
                .collect(),
            states,
            kinds,
            pane_ids,
        })
    }
}

/// A compiled [`PaneFilterArgs`].
#[derive(Debug, Clone)]
pub struct PaneFilter {
    substrings: Vec<String>,
    regexes: Vec<Regex>,
    fields: Vec<TextField>,
    invert: bool,
    projects: Vec<ProjectSelector>,
    excluded_projects: Vec<ProjectSelector>,
    /// The normalised `--cwd` when `--here` was given.
    here: Option<PathBuf>,
    sessions: Vec<String>,
    agents: Vec<String>,
    states: Vec<StateSelector>,
    kinds: Vec<PaneKind>,
    pane_ids: Vec<u64>,
}

impl PaneFilter {
    pub fn accepts(&self, facts: &PaneFacts) -> bool {
        self.accepts_structure(facts) && self.accepts_text(facts)
    }

    fn accepts_structure(&self, facts: &PaneFacts) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&facts.kind) {
            return false;
        }
        if !self.sessions.is_empty() && !self.sessions.contains(&facts.session_name) {
            return false;
        }
        if !self.pane_ids.is_empty() && !self.pane_ids.contains(&facts.pane_id) {
            return false;
        }
        if !self.agents.is_empty() {
            let Some(agent) = &facts.agent else {
                return false;
            };
            if !self.agents.contains(&agent.to_lowercase()) {
                return false;
            }
        }
        if !self.states.is_empty() {
            let Some(state) = facts.state else {
                return false;
            };
            if !self.states.iter().any(|selector| selector.accepts(state)) {
                return false;
            }
        }
        if !self.projects.is_empty() && !self.projects.iter().any(|project| project.accepts(facts))
        {
            return false;
        }
        if self
            .excluded_projects
            .iter()
            .any(|project| project.accepts(facts))
        {
            return false;
        }
        if let Some(cwd) = &self.here {
            let Some(project) = facts.project_or_cwd() else {
                return false;
            };
            if !cwd.starts_with(project) {
                return false;
            }
        }
        true
    }

    fn accepts_text(&self, facts: &PaneFacts) -> bool {
        if self.substrings.is_empty() && self.regexes.is_empty() {
            return true;
        }
        let haystacks = self.haystacks(facts);
        let is_match = haystacks.iter().any(|haystack| {
            let lowered = haystack.to_lowercase();
            self.substrings
                .iter()
                .any(|needle| lowered.contains(needle.as_str()))
                || self.regexes.iter().any(|regex| regex.is_match(haystack))
        });
        is_match != self.invert
    }

    fn haystacks(&self, facts: &PaneFacts) -> Vec<String> {
        let mut haystacks = Vec::new();
        for field in &self.fields {
            match field {
                TextField::Name => {
                    haystacks.push(facts.name.clone());
                    haystacks.extend(facts.short_name.clone());
                }
                TextField::Project => haystacks.extend(
                    facts
                        .project
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                ),
                TextField::Cwd => haystacks.extend(
                    facts
                        .cwd
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                ),
                TextField::Agent => haystacks.extend(facts.agent.clone()),
                TextField::Session => haystacks.push(facts.session_name.clone()),
            }
        }
        haystacks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn agent_pane(
        pane_id: u64,
        name: &str,
        project: &str,
        agent: &str,
        state: AgentStateName,
    ) -> PaneFacts {
        PaneFacts {
            session_name: "default".to_owned(),
            session_id: "default".to_owned(),
            pane_id,
            name: name.to_owned(),
            short_name: None,
            project: Some(PathBuf::from(project)),
            cwd: Some(PathBuf::from(project)),
            kind: PaneKind::Agent,
            agent: Some(agent.to_owned()),
            state: Some(state),
            is_self: false,
        }
    }

    fn shell_pane(pane_id: u64, name: &str, project: &str) -> PaneFacts {
        PaneFacts {
            kind: PaneKind::Shell,
            agent: None,
            state: None,
            ..agent_pane(pane_id, name, project, "", AgentStateName::Idle)
        }
    }

    fn compile(args: PaneFilterArgs) -> PaneFilter {
        args.compile(Path::new("/"), Some(Path::new("/home/me")))
            .expect("valid filter")
    }

    fn compile_line(line: &[&str]) -> Result<PaneFilter, String> {
        let mut specs = FILTER_OPTIONS.to_vec();
        specs.push(CWD_OPTION);
        let parsed = parse_arguments(&words(line), &specs)?;
        let cwd = parsed.last("cwd").unwrap_or("/").to_owned();
        PaneFilterArgs::from_parsed(&parsed).compile(Path::new(&cwd), Some(Path::new("/home/me")))
    }

    #[test]
    fn an_empty_filter_keeps_every_pane() {
        let filter = compile(PaneFilterArgs::default());
        assert!(filter.accepts(&agent_pane(1, "fix", "/w/a", "Codex", AgentStateName::Idle)));
        assert!(filter.accepts(&shell_pane(2, "zsh", "/w/a")));
    }

    #[test]
    fn match_is_a_case_insensitive_substring_over_all_fields_by_default() {
        let filter = compile(PaneFilterArgs {
            matches: vec!["RENDER".to_owned()],
            ..PaneFilterArgs::default()
        });
        assert!(filter.accepts(&agent_pane(
            1,
            "fix render loop",
            "/w/a",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(filter.accepts(&agent_pane(
            2,
            "other",
            "/w/renderer",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(
            3,
            "other",
            "/w/a",
            "Codex",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn field_limits_the_text_search() {
        let filter = compile_line(&["--match", "alpha", "--field", "name"]).expect("filter");
        assert!(filter.accepts(&agent_pane(
            1,
            "Alpha task",
            "/w/x",
            "Claude",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(
            2,
            "unrelated",
            "/w/alpha",
            "Claude",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn invert_flips_only_the_text_filter() {
        let filter =
            compile_line(&["--match", "alpha", "--invert", "--kind", "agent"]).expect("filter");
        assert!(filter.accepts(&agent_pane(
            1,
            "beta",
            "/w/x",
            "Claude",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(
            2,
            "alpha",
            "/w/x",
            "Claude",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&shell_pane(3, "beta", "/w/x")));
    }

    #[test]
    fn invert_without_a_pattern_is_refused() {
        assert!(compile_line(&["--invert"]).is_err());
    }

    #[test]
    fn an_invalid_regex_is_refused() {
        assert!(compile_line(&["--regex", "("]).is_err());
    }

    #[test]
    fn state_idle_also_covers_done() {
        let filter = compile_line(&["--state", "idle"]).expect("filter");
        assert!(filter.accepts(&agent_pane(1, "a", "/w/x", "Claude", AgentStateName::Done)));
        assert!(filter.accepts(&agent_pane(2, "b", "/w/x", "Claude", AgentStateName::Idle)));
        assert!(!filter.accepts(&agent_pane(
            3,
            "c",
            "/w/x",
            "Claude",
            AgentStateName::Working
        )));
        assert!(!filter.accepts(&shell_pane(4, "d", "/w/x")));
    }

    #[test]
    fn an_unknown_state_is_refused() {
        assert!(compile_line(&["--state", "sleeping"]).is_err());
    }

    #[test]
    fn repeated_and_comma_separated_values_widen_one_selector() {
        let filter = compile_line(&["--agent", "claude", "-a", "codex,gemini"]).expect("filter");
        assert!(filter.accepts(&agent_pane(1, "a", "/w/x", "Claude", AgentStateName::Idle)));
        assert!(filter.accepts(&agent_pane(2, "b", "/w/x", "Codex", AgentStateName::Idle)));
        assert!(!filter.accepts(&agent_pane(3, "c", "/w/x", "Other", AgentStateName::Idle)));
    }

    #[test]
    fn a_project_with_a_separator_is_a_directory_and_a_bare_one_a_folder_name() {
        let by_directory = compile_line(&["--project", "/w/team"]).expect("filter");
        assert!(by_directory.accepts(&agent_pane(
            1,
            "a",
            "/w/team/api",
            "Claude",
            AgentStateName::Idle
        )));
        assert!(!by_directory.accepts(&agent_pane(
            2,
            "b",
            "/w/teammate",
            "Claude",
            AgentStateName::Idle
        )));
        let by_name = compile_line(&["--project", "LUMEN"]).expect("filter");
        assert!(by_name.accepts(&agent_pane(
            3,
            "c",
            "/elsewhere/lumen",
            "Claude",
            AgentStateName::Idle
        )));
        assert!(!by_name.accepts(&agent_pane(
            4,
            "d",
            "/w/team/api",
            "Claude",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn exclude_project_drops_the_projects_it_names() {
        let filter = compile_line(&["--exclude-project", "/w/team"]).expect("filter");
        assert!(!filter.accepts(&agent_pane(
            1,
            "a",
            "/w/team/api",
            "Claude",
            AgentStateName::Idle
        )));
        assert!(filter.accepts(&agent_pane(
            2,
            "b",
            "/w/other",
            "Claude",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn here_keeps_the_project_that_contains_cwd() {
        let filter = compile_line(&["--cwd", "/w/team/api", "--here"]).expect("filter");
        assert!(filter.accepts(&agent_pane(
            1,
            "a",
            "/w/team",
            "Claude",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(
            2,
            "b",
            "/w/other",
            "Claude",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn a_relative_project_is_anchored_at_cwd() {
        let filter = compile_line(&["--cwd", "/w", "--project", "./team"]).expect("filter");
        assert!(filter.accepts(&agent_pane(
            1,
            "a",
            "/w/team/api",
            "Claude",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn a_tilde_project_expands_to_the_home_directory() {
        let filter = compile_line(&["--project", "~/dev"]).expect("filter");
        assert!(filter.accepts(&agent_pane(
            1,
            "a",
            "/home/me/dev/x",
            "Claude",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn pane_ids_and_sessions_narrow() {
        let filter = compile_line(&["--pane", "1,3", "--session", "default"]).expect("filter");
        assert!(filter.accepts(&agent_pane(1, "a", "/w/x", "Claude", AgentStateName::Idle)));
        assert!(!filter.accepts(&agent_pane(2, "b", "/w/x", "Claude", AgentStateName::Idle)));
    }

    #[test]
    fn the_parser_keeps_words_dashes_and_double_dash_payloads() {
        let parsed = parse_arguments(
            &words(&["--match=x", "hello", "-", "--", "-v", "--here"]),
            &FILTER_OPTIONS,
        )
        .expect("parsed");
        assert_eq!(parsed.last("match"), Some("x"));
        assert_eq!(parsed.words, words(&["hello", "-", "-v", "--here"]));
        assert!(!parsed.has("here"));
    }

    #[test]
    fn the_parser_refuses_an_unknown_option_and_a_missing_value() {
        assert!(parse_arguments(&words(&["--nope"]), &FILTER_OPTIONS).is_err());
        assert!(parse_arguments(&words(&["--match"]), &FILTER_OPTIONS).is_err());
    }

    #[test]
    fn a_short_option_takes_an_attached_or_a_separate_value() {
        let attached = parse_arguments(&words(&["-mfoo"]), &FILTER_OPTIONS).expect("parsed");
        assert_eq!(attached.last("match"), Some("foo"));
        let separate =
            parse_arguments(&words(&["-m", "foo", "-v"]), &FILTER_OPTIONS).expect("parsed");
        assert_eq!(separate.last("match"), Some("foo"));
        assert!(separate.has("invert"));
    }

    #[test]
    fn a_record_of_a_list_panes_reply_becomes_facts() {
        let record = json!({
            "pane_id": 7, "name": "tests", "kind": "agent", "agent": "Codex",
            "state": "idle", "project": "/w/a", "cwd": "/w/a", "is_self": false,
        });
        let facts = PaneFacts::from_record(&record, "review", "review-id").expect("facts");
        assert_eq!(facts.pane_id, 7);
        assert_eq!(facts.kind, PaneKind::Agent);
        assert_eq!(facts.state, Some(AgentStateName::Idle));
        assert_eq!(facts.to_json()["session"], "review");
        assert!(PaneFacts::from_record(&json!({"name": "x"}), "s", "s").is_none());
    }
}
