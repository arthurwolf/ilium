//! The pane selection shared by every command that acts on panes across
//! running sessions (`ilium panes`, `ilium broadcast`).
//!
//! One clap argument group, [`PaneFilterArgs`], is flattened into each such
//! command, so every selector means the same thing everywhere. The group is
//! compiled once into a [`PaneFilter`] and applied to [`PaneFacts`], a plain
//! description of one pane taken from a session's tree. Matching is pure: no
//! I/O, no server, which is what the unit tests below exercise.
//!
//! Semantics, in one place:
//!
//! - Every selector kind narrows the result (they combine with AND).
//! - Repeated values of one selector widen it (they combine with OR), and
//!   list-valued selectors also accept comma-separated values.
//! - `--match` and `--regex` are the text filter: a pane passes when any of
//!   them matches any of the searched fields. `--invert` turns the text
//!   filter around (a pane passes when none of them matches). It applies
//!   only to the text filter, never to the structural selectors.

use std::path::{Component, Path, PathBuf};

use clap::{Args, ValueEnum};
use ilium_core::{AgentTurn, NodeId, NodeKind, PaneStatus, Tree};
use regex::{Regex, RegexBuilder};

/// Pane selectors shared by `ilium panes` and `ilium broadcast`.
#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Pane selection (shared by `panes` and `broadcast`)")]
pub(crate) struct PaneFilterArgs {
    /// Keep panes whose searched fields contain TEXT (case-insensitive).
    /// Repeat for alternatives: a pane passes when any --match or --regex
    /// matches.
    #[arg(short = 'm', long = "match", value_name = "TEXT")]
    pub(crate) matches: Vec<String>,

    /// Keep panes whose searched fields match the regular expression
    /// PATTERN (case-insensitive, Rust `regex` syntax, unanchored).
    #[arg(short = 'e', long = "regex", value_name = "PATTERN")]
    pub(crate) regexes: Vec<String>,

    /// Fields that --match and --regex search. Default: all of them.
    #[arg(
        long = "field",
        value_enum,
        value_delimiter = ',',
        value_name = "FIELD"
    )]
    pub(crate) fields: Vec<TextField>,

    /// Invert the text filter: keep panes that no --match or --regex
    /// matches. Requires at least one --match or --regex.
    #[arg(short = 'v', long)]
    pub(crate) invert: bool,

    /// Keep panes in these projects. A value with a path separator (or `.`,
    /// `..`, `~`) is a directory: it selects projects at or below it. A bare
    /// value is a project folder name, compared case-insensitively.
    #[arg(
        short = 'p',
        long = "project",
        value_delimiter = ',',
        value_name = "PROJECT"
    )]
    pub(crate) projects: Vec<String>,

    /// Drop panes in these projects (same syntax as --project).
    #[arg(
        long = "exclude-project",
        value_delimiter = ',',
        value_name = "PROJECT"
    )]
    pub(crate) excluded_projects: Vec<String>,

    /// Keep only panes of the project that contains --cwd (the current
    /// directory by default).
    #[arg(long)]
    pub(crate) here: bool,

    /// Keep panes in these session names (for example `default`).
    #[arg(
        short = 's',
        long = "session",
        value_delimiter = ',',
        value_name = "NAME"
    )]
    pub(crate) sessions: Vec<String>,

    /// Keep panes running these agents: claude, codex, antigravity, or a
    /// custom agent's name (case-insensitive).
    #[arg(
        short = 'a',
        long = "agent",
        value_delimiter = ',',
        value_name = "AGENT"
    )]
    pub(crate) agents: Vec<String>,

    /// Keep agent panes in these states. `idle` also covers `done` (an
    /// agent that finished a turn nobody has looked at yet).
    #[arg(
        long = "state",
        value_enum,
        value_delimiter = ',',
        value_name = "STATE"
    )]
    pub(crate) states: Vec<StateSelector>,

    /// Keep panes of these kinds.
    #[arg(long = "kind", value_enum, value_delimiter = ',', value_name = "KIND")]
    pub(crate) kinds: Vec<PaneKind>,

    /// Keep these pane ids (as printed by `ilium panes`). Pane ids are only
    /// unique within one session; combine with --session or --project when
    /// several sessions run.
    #[arg(long = "pane", value_delimiter = ',', value_name = "ID")]
    pub(crate) pane_ids: Vec<u64>,
}

/// Where the text filter looks.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextField {
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

/// What a pane is, as the selectors and the JSONL records name it.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneKind {
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
}

impl PaneKind {
    fn of(status: &PaneStatus) -> Self {
        match status {
            PaneStatus::Agent(_) => Self::Agent,
            PaneStatus::PlainShell => Self::Shell,
            PaneStatus::Editor { .. } => Self::Editor,
            PaneStatus::Board => Self::Board,
            PaneStatus::AgentUnavailable(_) => Self::UnavailableAgent,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Shell => "shell",
            Self::Editor => "editor",
            Self::Board => "board",
            Self::UnavailableAgent => "unavailable-agent",
        }
    }
}

/// A live agent's current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentStateName {
    Working,
    WaitingApproval,
    WaitingSubagents,
    Settling,
    Idle,
    Done,
}

impl AgentStateName {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::WaitingApproval => "waiting-approval",
            Self::WaitingSubagents => "waiting-subagents",
            Self::Settling => "settling",
            Self::Idle => "idle",
            Self::Done => "done",
        }
    }

    /// Idle and done agents have no turn in progress: input reaches an empty
    /// composer instead of interrupting or queueing behind work.
    pub(crate) const fn is_idle(self) -> bool {
        matches!(self, Self::Idle | Self::Done)
    }
}

/// `--state` values.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateSelector {
    Working,
    WaitingApproval,
    WaitingSubagents,
    Settling,
    /// Idle, including done.
    Idle,
    Done,
}

impl StateSelector {
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

/// Everything the selectors look at for one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneFacts {
    pub(crate) session_name: String,
    pub(crate) pane_id: NodeId,
    pub(crate) name: String,
    pub(crate) short_name: Option<String>,
    /// The enclosing project's root, if the pane sits in a project.
    pub(crate) project: Option<PathBuf>,
    /// The directory the pane was launched in (its project root for panes
    /// that recorded none).
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) kind: PaneKind,
    /// The agent's display name, for live and unavailable agents.
    pub(crate) agent: Option<String>,
    /// Only live agents have a state.
    pub(crate) state: Option<AgentStateName>,
    /// The pane this command runs in.
    pub(crate) is_self: bool,
}

impl PaneFacts {
    /// The project directory selectors compare against: the project root,
    /// or the launch directory for a pane outside any project.
    fn project_or_cwd(&self) -> Option<&Path> {
        self.project.as_deref().or(self.cwd.as_deref())
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "session": self.session_name,
            "pane_id": self.pane_id.0,
            "name": self.name,
            "kind": self.kind.name(),
            "agent": self.agent,
            "state": self.state.map(AgentStateName::name),
            "project": self.project.as_ref().map(|path| path.to_string_lossy()),
            "cwd": self.cwd.as_ref().map(|path| path.to_string_lossy()),
            "is_self": self.is_self,
        })
    }
}

/// Describes every pane of one session's tree, in tree order.
pub(crate) fn pane_facts(tree: &Tree, session_name: &str) -> Vec<PaneFacts> {
    tree.pane_ids_in_tree_order()
        .into_iter()
        .filter_map(|pane_id| {
            let node = tree.get(pane_id)?;
            let NodeKind::Pane { status, .. } = &node.kind else {
                return None;
            };
            let state = status.agent_state().map(|agent| match agent.turn {
                AgentTurn::Working => AgentStateName::Working,
                AgentTurn::WaitingApproval => AgentStateName::WaitingApproval,
                AgentTurn::WaitingSubagents => AgentStateName::WaitingSubagents,
                AgentTurn::Settling => AgentStateName::Settling,
                AgentTurn::Idle if agent.completion_unread => AgentStateName::Done,
                AgentTurn::Idle => AgentStateName::Idle,
            });
            Some(PaneFacts {
                session_name: session_name.to_owned(),
                pane_id,
                name: node.name.clone(),
                short_name: node.short_name.clone(),
                project: tree.project_path_for(pane_id).map(Path::to_path_buf),
                cwd: tree.pane_cwd(pane_id).map(Path::to_path_buf),
                kind: PaneKind::of(status),
                agent: status
                    .known_agent_state()
                    .map(|agent| agent.class.label().to_owned()),
                state,
                is_self: false,
            })
        })
        .collect()
}

/// A `--project` value, resolved once.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectSelector {
    /// Projects at or below this absolute directory.
    Directory(PathBuf),
    /// Projects whose folder has this name (case-insensitive).
    FolderName(String),
}

impl ProjectSelector {
    fn parse(value: &str, cwd: &Path) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("empty project value".to_owned());
        }
        let is_directory = value.contains(std::path::MAIN_SEPARATOR)
            || value.contains('/')
            || matches!(value, "." | ".." | "~")
            || value.starts_with('~');
        if !is_directory {
            return Ok(Self::FolderName(value.to_owned()));
        }
        let expanded = match value.strip_prefix('~') {
            Some(rest) => {
                let home = directories::BaseDirs::new()
                    .map(|base| base.home_dir().to_path_buf())
                    .ok_or_else(|| format!("cannot expand {value:?}: no home directory"))?;
                home.join(rest.trim_start_matches(['/', std::path::MAIN_SEPARATOR]))
            }
            None => PathBuf::from(value),
        };
        let absolute = if expanded.is_absolute() {
            expanded
        } else {
            cwd.join(expanded)
        };
        // Sessions record canonical project roots, so a selector must be
        // canonical too. A directory that no longer exists can still name a
        // session started before it moved; normalise it lexically instead.
        let resolved = ilium_platform::paths::canonicalize(&absolute)
            .unwrap_or_else(|_| lexically_normalized(&absolute));
        Ok(Self::Directory(resolved))
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

/// A compiled [`PaneFilterArgs`].
#[derive(Debug, Clone)]
pub(crate) struct PaneFilter {
    substrings: Vec<String>,
    regexes: Vec<Regex>,
    fields: Vec<TextField>,
    invert: bool,
    projects: Vec<ProjectSelector>,
    excluded_projects: Vec<ProjectSelector>,
    /// The canonical `--cwd` when `--here` was given.
    here: Option<PathBuf>,
    sessions: Vec<String>,
    agents: Vec<String>,
    states: Vec<StateSelector>,
    kinds: Vec<PaneKind>,
    pane_ids: Vec<NodeId>,
}

impl PaneFilterArgs {
    /// Validates and resolves the selectors. `cwd` anchors relative project
    /// paths and `--here`.
    pub(crate) fn compile(&self, cwd: &Path) -> Result<PaneFilter, String> {
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
                    ProjectSelector::parse(value, cwd)
                        .map_err(|error| format!("invalid project {value:?}: {error}"))
                })
                .collect::<Result<Vec<_>, _>>()
        };
        let here = if self.here {
            Some(
                ilium_platform::paths::canonicalize(cwd)
                    .map_err(|error| format!("cannot resolve --cwd {cwd:?}: {error}"))?,
            )
        } else {
            None
        };
        Ok(PaneFilter {
            substrings: self
                .matches
                .iter()
                .map(|text| text.to_lowercase())
                .collect(),
            regexes,
            fields: if self.fields.is_empty() {
                ALL_TEXT_FIELDS.to_vec()
            } else {
                self.fields.clone()
            },
            invert: self.invert,
            projects: parse_projects(&self.projects)?,
            excluded_projects: parse_projects(&self.excluded_projects)?,
            here,
            sessions: self.sessions.clone(),
            agents: self
                .agents
                .iter()
                .map(|agent| agent.to_lowercase())
                .collect(),
            states: self.states.clone(),
            kinds: self.kinds.clone(),
            pane_ids: self.pane_ids.iter().copied().map(NodeId).collect(),
        })
    }
}

impl PaneFilter {
    pub(crate) fn accepts(&self, facts: &PaneFacts) -> bool {
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

    fn agent_pane(
        pane_id: u64,
        name: &str,
        project: &str,
        agent: &str,
        state: AgentStateName,
    ) -> PaneFacts {
        PaneFacts {
            session_name: "default".to_owned(),
            pane_id: NodeId(pane_id),
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
        args.compile(Path::new("/")).expect("valid filter")
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
    fn repeated_text_patterns_are_alternatives_and_field_limits_the_search() {
        let filter = compile(PaneFilterArgs {
            matches: vec!["alpha".to_owned()],
            regexes: vec![r"^beta\d$".to_owned()],
            fields: vec![TextField::Name],
            ..PaneFilterArgs::default()
        });
        assert!(filter.accepts(&agent_pane(
            1,
            "Alpha task",
            "/w/a",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(filter.accepts(&agent_pane(
            2,
            "BETA7",
            "/w/a",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(
            3,
            "beta77",
            "/w/a",
            "Codex",
            AgentStateName::Idle
        )));
        // The project path contains "alpha" but only names are searched.
        assert!(!filter.accepts(&agent_pane(
            4,
            "gamma",
            "/w/alpha",
            "Codex",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn invert_keeps_only_panes_the_text_filter_rejects() {
        let filter = compile(PaneFilterArgs {
            regexes: vec!["review".to_owned()],
            invert: true,
            agents: vec!["codex".to_owned()],
            ..PaneFilterArgs::default()
        });
        assert!(!filter.accepts(&agent_pane(
            1,
            "code review",
            "/w/a",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(filter.accepts(&agent_pane(
            2,
            "build",
            "/w/a",
            "Codex",
            AgentStateName::Idle
        )));
        // Inversion never applies to the structural selectors.
        assert!(!filter.accepts(&agent_pane(
            3,
            "build",
            "/w/a",
            "Claude",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn invert_without_a_text_pattern_and_bad_regexes_are_refused() {
        let invert_only = PaneFilterArgs {
            invert: true,
            ..PaneFilterArgs::default()
        };
        assert!(invert_only.compile(Path::new("/")).is_err());
        let bad_regex = PaneFilterArgs {
            regexes: vec!["(".to_owned()],
            ..PaneFilterArgs::default()
        };
        assert!(bad_regex
            .compile(Path::new("/"))
            .unwrap_err()
            .contains("invalid --regex"));
    }

    #[test]
    fn projects_select_by_directory_subtree_or_by_folder_name() {
        let filter = compile(PaneFilterArgs {
            projects: vec!["/w/team".to_owned(), "Lumen".to_owned()],
            ..PaneFilterArgs::default()
        });
        assert!(filter.accepts(&agent_pane(
            1,
            "x",
            "/w/team",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(filter.accepts(&agent_pane(
            2,
            "x",
            "/w/team/api",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(filter.accepts(&agent_pane(
            3,
            "x",
            "/elsewhere/lumen",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(
            4,
            "x",
            "/w/teammate",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(
            5,
            "x",
            "/w/other",
            "Codex",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn relative_project_directories_resolve_against_cwd() {
        let selector = ProjectSelector::parse("../b", Path::new("/no/such/root/a")).expect("parse");
        assert_eq!(
            selector,
            ProjectSelector::Directory(PathBuf::from("/no/such/root/b"))
        );
        assert_eq!(
            ProjectSelector::parse("ilium", Path::new("/")).expect("parse"),
            ProjectSelector::FolderName("ilium".to_owned())
        );
    }

    #[test]
    fn excluded_projects_win_over_included_ones() {
        let filter = compile(PaneFilterArgs {
            projects: vec!["/w".to_owned()],
            excluded_projects: vec!["/w/private".to_owned()],
            ..PaneFilterArgs::default()
        });
        assert!(filter.accepts(&agent_pane(
            1,
            "x",
            "/w/public",
            "Codex",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(
            2,
            "x",
            "/w/private",
            "Codex",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn here_keeps_the_project_that_contains_cwd() {
        let directory = tempfile::tempdir().expect("tempdir");
        let project = ilium_platform::paths::canonicalize(directory.path()).expect("canonical");
        let nested = project.join("src");
        std::fs::create_dir_all(&nested).expect("nested");
        let filter = PaneFilterArgs {
            here: true,
            ..PaneFilterArgs::default()
        }
        .compile(&nested)
        .expect("valid filter");
        let inside = agent_pane(
            1,
            "x",
            &project.to_string_lossy(),
            "Codex",
            AgentStateName::Idle,
        );
        assert!(filter.accepts(&inside));
        assert!(!filter.accepts(&agent_pane(
            2,
            "x",
            "/elsewhere",
            "Codex",
            AgentStateName::Idle
        )));
    }

    #[test]
    fn agent_state_kind_session_and_pane_selectors_narrow_together() {
        let filter = compile(PaneFilterArgs {
            agents: vec!["CLAUDE".to_owned(), "codex".to_owned()],
            states: vec![StateSelector::Idle],
            sessions: vec!["default".to_owned()],
            pane_ids: vec![1, 2, 3, 4],
            ..PaneFilterArgs::default()
        });
        assert!(filter.accepts(&agent_pane(1, "x", "/w", "Claude", AgentStateName::Idle)));
        // `idle` covers `done`.
        assert!(filter.accepts(&agent_pane(2, "x", "/w", "Codex", AgentStateName::Done)));
        assert!(!filter.accepts(&agent_pane(3, "x", "/w", "Codex", AgentStateName::Working)));
        assert!(!filter.accepts(&agent_pane(
            4,
            "x",
            "/w",
            "Antigravity",
            AgentStateName::Idle
        )));
        assert!(!filter.accepts(&agent_pane(5, "x", "/w", "Codex", AgentStateName::Idle)));
        assert!(!filter.accepts(&shell_pane(1, "x", "/w")));
        let mut other_session = agent_pane(1, "x", "/w", "Claude", AgentStateName::Idle);
        other_session.session_name = "review".to_owned();
        assert!(!filter.accepts(&other_session));

        let done_only = compile(PaneFilterArgs {
            states: vec![StateSelector::Done],
            ..PaneFilterArgs::default()
        });
        assert!(!done_only.accepts(&agent_pane(1, "x", "/w", "Codex", AgentStateName::Idle)));
        assert!(done_only.accepts(&agent_pane(1, "x", "/w", "Codex", AgentStateName::Done)));

        let shells = compile(PaneFilterArgs {
            kinds: vec![PaneKind::Shell],
            ..PaneFilterArgs::default()
        });
        assert!(shells.accepts(&shell_pane(9, "zsh", "/w")));
        assert!(!shells.accepts(&agent_pane(9, "x", "/w", "Codex", AgentStateName::Idle)));
    }

    #[test]
    fn pane_facts_describe_agents_shells_and_projects_from_a_tree() {
        use ilium_core::{AgentActivity, AgentClass, AgentState, PaneContentKind};

        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/w/proj")).expect("project");
        let agent = tree
            .add_pane(project, "agent title", PaneContentKind::Terminal)
            .expect("agent pane");
        let shell = tree
            .add_pane(project, "zsh", PaneContentKind::Terminal)
            .expect("shell pane");
        tree.set_pane_status(
            agent,
            AgentState::from_activity(AgentClass::Codex, AgentActivity::Done, None).into_status(),
        )
        .expect("status");

        let facts = pane_facts(&tree, "default");
        assert_eq!(facts.len(), 2);
        let agent_facts = facts.iter().find(|pane| pane.pane_id == agent).unwrap();
        assert_eq!(agent_facts.kind, PaneKind::Agent);
        assert_eq!(agent_facts.agent.as_deref(), Some("Codex"));
        assert_eq!(agent_facts.state, Some(AgentStateName::Done));
        assert_eq!(agent_facts.project.as_deref(), Some(Path::new("/w/proj")));
        assert_eq!(agent_facts.to_json()["state"], "done");
        let shell_facts = facts.iter().find(|pane| pane.pane_id == shell).unwrap();
        assert_eq!(shell_facts.kind, PaneKind::Shell);
        assert_eq!(shell_facts.state, None);
        assert_eq!(shell_facts.to_json()["kind"], "shell");
    }
}
