//! Command-line shape of `ilium ctl`.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use ilium_control::guide;
use ilium_control::operation::WorktreeAction;
use ilium_control::Format;

use super::CtlFailure;

#[derive(Args, Debug, Clone)]
pub(crate) struct CtlArgs {
    /// Session to address, within the project given by --cwd. Inside an Ilium
    /// pane the pane's own session is the default.
    #[arg(long, global = true)]
    pub(crate) session: Option<String>,
    /// Address one live session by its socket path (see `ilium ctl sessions`).
    #[arg(long, global = true, value_name = "PATH", conflicts_with = "session")]
    pub(crate) socket: Option<PathBuf>,
    /// Read every live session; writes always require an explicit session.
    #[arg(long, global = true)]
    pub(crate) all_sessions: bool,
    /// Output format of reads: json, jsonl, yaml or markdown; events also accepts compact.
    #[arg(long, global = true)]
    pub(crate) format: Option<String>,
    /// Explicit local automation grant: read, act or admin. Outside default: read.
    #[arg(long, global = true)]
    pub(crate) access: Option<String>,
    /// Use a token instead of selecting one from this session's owner-only token file.
    #[arg(long, global = true)]
    pub(crate) token: Option<String>,
    #[command(subcommand)]
    pub(crate) command: CtlCommand,
}

/// Which session a command addresses.
#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) access: Option<String>,
    pub(crate) token: Option<String>,
    pub(crate) session: Option<String>,
    pub(crate) socket: Option<PathBuf>,
}

impl CtlArgs {
    pub(crate) fn across_sessions(&self) -> bool {
        self.all_sessions
            || match &self.command {
                CtlCommand::Events(options) | CtlCommand::Digest(options) => options.all,
                CtlCommand::Tree { all, .. } | CtlCommand::Find { all, .. } => *all,
                _ => false,
            }
    }

    pub(crate) fn format(&self) -> Result<Format, CtlFailure> {
        Format::parse(self.format.as_deref().unwrap_or("json")).ok_or_else(|| {
            CtlFailure::usage(format!(
                "unknown format {:?}; use json, jsonl, yaml or markdown",
                self.format
            ))
        })
    }

    pub(crate) fn target(&self) -> Target {
        Target {
            access: self.access.clone(),
            token: self
                .token
                .clone()
                .or_else(|| std::env::var("ILIUM_CONTROL_TOKEN").ok()),
            session: self.session.clone(),
            socket: self.socket.clone(),
        }
    }
}

#[derive(Args, Debug, Clone)]
pub(crate) struct MonitoringArgs {
    /// Read every live session with an independent cursor for each socket.
    #[arg(long)]
    pub(crate) all: bool,
    #[arg(long)]
    pub(crate) since: Option<String>,
    #[arg(long)]
    pub(crate) follow: bool,
    #[arg(long = "as")]
    pub(crate) consumer: Option<String>,
    #[arg(long)]
    pub(crate) min_severity: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub(crate) kinds: Vec<String>,
    #[arg(long)]
    pub(crate) project: Option<String>,
    #[arg(long)]
    pub(crate) agent: Option<String>,
    #[arg(long)]
    pub(crate) pane: Option<u64>,
    #[arg(long, default_value_t = 12)]
    pub(crate) max_items: usize,
    #[arg(long, default_value_t = 4000)]
    pub(crate) max_chars: usize,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum CtlCommand {
    /// The complete monitoring loop and safety rules in at most 1500 characters.
    AgentGuide,
    /// Acknowledge a previously returned monitoring cursor for one consumer.
    Ack {
        cursor: String,
        #[arg(long = "as")]
        consumer: Option<String>,
    },
    /// Read or follow a bounded page of session monitoring events.
    Events(MonitoringArgs),
    /// Priority-sorted changes: needs-you, failed-or-stuck, finished and working count.
    Digest(MonitoringArgs),
    /// How to remote-control Ilium: essentials with example commands, or one
    /// topic (essentials, reading, creating, controlling, arranging, http).
    Guide { topic: Option<String> },
    /// One example JSON body of every operation `ilium ctl op` and the HTTP
    /// API accept.
    Ops,
    /// List live sessions on this machine.
    Sessions,
    /// Identify the pane this command runs in and its session.
    Whoami,
    /// The whole session tree (or one project) in the chosen format.
    Tree {
        /// Read every live session.
        #[arg(long)]
        all: bool,
        /// Levels of children to include.
        #[arg(long)]
        depth: Option<usize>,
        /// Show only this project (id, absolute path, directory or display name).
        #[arg(long)]
        project: Option<String>,
        /// Leave out per-pane detail objects.
        #[arg(long)]
        no_details: bool,
    },
    /// One project and everything in it.
    Project {
        selector: String,
        #[arg(long)]
        depth: Option<usize>,
        #[arg(long)]
        no_details: bool,
    },
    /// Any node (project, group, folder, pane) by id.
    Node {
        id: String,
        #[arg(long)]
        depth: Option<usize>,
        #[arg(long)]
        no_details: bool,
    },
    /// Full details of one pane; --screen adds its current screen text.
    Pane {
        id: String,
        /// Minimal state and verified latest assistant message, without screen or prompts.
        #[arg(long, conflicts_with_all = ["screen", "lines"])]
        brief: bool,
        #[arg(long)]
        screen: bool,
        /// With --screen: only the last N lines.
        #[arg(long)]
        lines: Option<usize>,
    },
    /// A flat list of panes matching every given filter.
    Find {
        /// Read every live session.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        agent: Option<String>,
        /// Agent state, comma separated: working, idle, done, waiting_approval...
        #[arg(long)]
        state: Option<String>,
        /// shell, agent, editor, board, agent_unavailable
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        name_contains: Option<String>,
    },
    /// Current screen text of a terminal pane.
    Screen {
        id: String,
        #[arg(long)]
        lines: Option<usize>,
    },
    /// Block on live events until any/all selected panes match --state or close.
    Wait {
        #[arg(required = true, num_args = 1..)]
        ids: Vec<String>,
        /// Complete when any selected pane matches (the default).
        #[arg(long, conflicts_with = "all")]
        any: bool,
        /// Complete only when every selected pane matches.
        #[arg(long)]
        all: bool,
        /// Comma separated target states, e.g. idle,done.
        #[arg(long, default_value = "idle,done")]
        state: String,
        /// Give up after this many seconds (exit 3). Default: wait forever.
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Rename a project, group or pane. Fixes the title against auto-naming.
    Rename { id: u64, title: String },
    /// Close a pane.
    Close {
        id: u64,
        /// What to do with the pane's worktree: keep, remove, remove-and-branch.
        #[arg(long, default_value = "keep", value_parser = parse_worktree_action)]
        worktree: WorktreeAction,
    },
    /// Move a node up/down among its siblings, or under another container.
    Move {
        id: u64,
        #[arg(long, conflicts_with_all = ["down", "to"])]
        up: bool,
        #[arg(long, conflicts_with = "to")]
        down: bool,
        /// New parent: a project or group id.
        #[arg(long)]
        to: Option<u64>,
        /// Position among the new parent's children (default: last).
        #[arg(long, requires = "to")]
        index: Option<usize>,
    },
    /// Add a directory-backed project.
    NewProject { path: PathBuf },
    /// Add a group under a project or group.
    NewGroup {
        name: String,
        #[arg(long, conflicts_with = "parent")]
        project: Option<String>,
        #[arg(long)]
        parent: Option<u64>,
    },
    /// Show two or more panes side by side as a split view.
    NewSplitView {
        name: String,
        /// Pane ids, in display order.
        #[arg(required = true, num_args = 2..)]
        panes: Vec<u64>,
        #[arg(long, conflicts_with = "parent")]
        project: Option<String>,
        #[arg(long)]
        parent: Option<u64>,
        /// Stack the panes top to bottom instead of left to right.
        #[arg(long)]
        horizontal: bool,
    },
    /// Replace the process of a pane with a shell command line.
    ReplaceCommand { id: u64, command: String },
    /// Open a kanban board pane backed by a folder or a Markdown file.
    NewBoard {
        name: String,
        path: PathBuf,
        /// The path is one Markdown file instead of a folder of columns.
        #[arg(long)]
        markdown: bool,
        #[arg(long, conflicts_with = "parent")]
        project: Option<String>,
        #[arg(long)]
        parent: Option<u64>,
    },
    /// Stop a pane's agent, keeping a frozen placeholder that RESUME restarts.
    Freeze { id: u64, resume_command: String },
    /// Restart a frozen pane with its resume command.
    Unfreeze { id: u64 },
    /// Add a file-browser folder root under a project or group.
    NewFolder {
        path: PathBuf,
        #[arg(long, conflicts_with = "parent")]
        project: Option<String>,
        #[arg(long)]
        parent: Option<u64>,
    },
    /// Change the directory new entries of a project start in.
    ChangeFolder { project: String, path: PathBuf },
    /// Create a pane: shell, command, editor or agent; optionally in a new worktree.
    NewPane(NewPaneArgs),
    /// Type text into a pane and press Enter.
    Send {
        id: u64,
        text: String,
        #[arg(long)]
        no_enter: bool,
    },
    /// Send named keys to a pane: enter esc tab up down ctrl-c text:LITERAL ...
    Keys {
        id: u64,
        #[arg(required = true)]
        keys: Vec<String>,
    },
    /// Type text into a pane after a delay.
    Schedule {
        id: u64,
        /// Delay in seconds.
        #[arg(long = "in")]
        delay_seconds: u64,
        text: String,
        #[arg(long)]
        no_enter: bool,
    },
    /// Queue a prompt for when the pane's agent next finishes.
    Queue {
        id: u64,
        text: String,
        #[arg(long, conflicts_with = "forever")]
        times: Option<u32>,
        #[arg(long)]
        forever: bool,
    },
    /// Drop every queued prompt of a pane.
    QueueClear { id: u64 },
    /// Bookmark a node (pin it in the sidebar).
    Bookmark {
        id: u64,
        #[arg(long)]
        off: bool,
    },
    /// Expand a project, group or folder in the sidebar.
    Expand { id: u64 },
    /// Collapse a project, group or folder in the sidebar.
    Collapse { id: u64 },
    /// Lock a container or folder closed (or unlock with --off).
    LockClosed {
        id: u64,
        #[arg(long)]
        off: bool,
    },
    /// Stop the process in a pane, keeping the pane.
    Terminate { id: u64 },
    /// Apply a restructure plan from a JSON file to the session or one project.
    Restructure {
        plan: PathBuf,
        #[arg(long)]
        project: Option<String>,
    },
    /// Undo the last restructure (of one project with --project).
    RevertRestructure {
        #[arg(long)]
        project: Option<String>,
    },
    /// End the session: stops every pane process.
    KillSession,
    /// Apply one operation given as JSON (argument, or `-` for standard input).
    Op { json: String },
}

#[derive(Args, Debug, Default, Clone)]
pub(crate) struct NewPaneArgs {
    /// Project to place the pane in: id, path, directory or display name. An
    /// absolute path that is not yet a project is added as one first.
    #[arg(long, conflicts_with = "parent")]
    pub(crate) project: Option<String>,
    /// A specific project or group node id instead.
    #[arg(long)]
    pub(crate) parent: Option<u64>,
    /// Start the process in this absolute directory.
    #[arg(long)]
    pub(crate) workdir: Option<PathBuf>,
    /// Start a built-in agent: claude, codex or agy.
    #[arg(long)]
    pub(crate) agent: Option<String>,
    /// First prompt typed into the agent.
    #[arg(long, requires = "agent")]
    pub(crate) prompt: Option<String>,
    /// Open this file in the built-in editor.
    #[arg(long, conflicts_with_all = ["agent", "command"])]
    pub(crate) editor: Option<PathBuf>,
    /// Create the agent in a new Git worktree on --branch.
    #[arg(long, requires_all = ["branch", "agent"])]
    pub(crate) worktree: bool,
    #[arg(long, requires = "worktree")]
    pub(crate) branch: Option<String>,
    /// Starting ref for the worktree (default: the repository's default base).
    #[arg(long, requires = "worktree")]
    pub(crate) base: Option<String>,
    /// Keep a command pane open after the command exits.
    #[arg(long)]
    pub(crate) keep_open: bool,
    /// Command to run instead of a shell.
    #[arg(last = true, value_name = "CMD")]
    pub(crate) command: Vec<String>,
}

fn parse_worktree_action(value: &str) -> Result<WorktreeAction, String> {
    match value {
        "keep" => Ok(WorktreeAction::Keep),
        "remove" => Ok(WorktreeAction::Remove),
        "remove-and-branch" => Ok(WorktreeAction::RemoveAndBranch),
        other => Err(format!(
            "{other:?} is not keep, remove or remove-and-branch"
        )),
    }
}

impl CtlCommand {
    /// Commands that only read state.
    pub(crate) fn is_read(&self) -> bool {
        matches!(
            self,
            Self::Sessions
                | Self::Whoami
                | Self::Tree { .. }
                | Self::Project { .. }
                | Self::Node { .. }
                | Self::Pane { .. }
                | Self::Find { .. }
                | Self::Screen { .. }
                | Self::Wait { .. }
        )
    }
}

/// Output for commands that need no session at all (help text).
pub(crate) fn offline(command: &CtlCommand, format: Format) -> Result<Option<String>, CtlFailure> {
    match command {
        CtlCommand::AgentGuide => Ok(Some(guide::AGENT_GUIDE.to_owned())),
        CtlCommand::Guide { topic } => match topic.as_deref() {
            None => Ok(Some(guide::ESSENTIALS.to_owned())),
            Some("topics") => Ok(Some(guide::topic_list())),
            Some(name) => guide::topic(name)
                .map(|topic| Some(topic.body.to_owned()))
                .ok_or_else(|| {
                    CtlFailure::usage(format!("no guide topic {name:?}\n{}", guide::topic_list()))
                }),
        },
        CtlCommand::Ops => {
            let documents: Vec<serde_json::Value> = ilium_control::operation_examples()
                .iter()
                .filter_map(|example| serde_json::to_value(example).ok())
                .collect();
            let value = serde_json::json!({"operations": documents});
            Ok(Some(ilium_control::render_document(
                &value,
                format,
                ilium_control::Shape::Generic,
            )))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    /// Parses `ilium <args>` through the real top-level command.
    fn parse(arguments: &[&str]) -> Result<crate::Cli, clap::Error> {
        let mut full = vec!["ilium"];
        full.extend_from_slice(arguments);
        crate::Cli::try_parse_from(full)
    }

    fn ctl_command(arguments: &[&str]) -> CtlCommand {
        match parse(arguments).expect("arguments parse").command {
            Some(crate::Command::Ctl(args)) => args.command,
            _ => panic!("not a ctl command"),
        }
    }

    #[test]
    fn events_accepts_delta_follow_filters_and_output_budgets() {
        assert!(parse(&[
            "ctl",
            "events",
            "--since",
            "epoch:9",
            "--follow",
            "--as",
            "manager",
            "--min-severity",
            "warning",
            "--kinds",
            "error,needs_input",
            "--project",
            "project",
            "--agent",
            "codex",
            "--pane",
            "42",
            "--max-items",
            "12",
            "--max-chars",
            "4000",
            "--format",
            "json",
        ])
        .is_ok());
    }

    #[test]
    fn events_defaults_to_compact_without_changing_other_read_defaults() {
        let Some(crate::Command::Ctl(events)) = parse(&["ctl", "events"]).unwrap().command else {
            panic!("expected ctl events");
        };
        assert!(events.format.is_none());
        let Some(crate::Command::Ctl(tree)) = parse(&["ctl", "tree"]).unwrap().command else {
            panic!("expected ctl tree");
        };
        assert_eq!(tree.format().unwrap(), ilium_control::Format::Json);
    }

    #[test]
    fn new_pane_workdir_does_not_collide_with_the_global_cwd() {
        let command = ctl_command(&[
            "--cwd",
            "/tmp/project",
            "ctl",
            "new-pane",
            "--parent",
            "3",
            "--workdir",
            "/tmp/project/sub",
        ]);
        let CtlCommand::NewPane(args) = command else {
            panic!("expected new-pane");
        };
        assert_eq!(args.parent, Some(3));
        assert_eq!(args.workdir, Some(PathBuf::from("/tmp/project/sub")));
    }

    #[test]
    fn new_pane_command_follows_the_double_dash() {
        let CtlCommand::NewPane(args) = ctl_command(&[
            "ctl",
            "new-pane",
            "--keep-open",
            "--",
            "sh",
            "-c",
            "echo hi",
        ]) else {
            panic!("expected new-pane");
        };
        assert!(args.keep_open);
        assert_eq!(args.command, ["sh", "-c", "echo hi"]);
    }

    #[test]
    fn worktree_needs_a_branch_and_an_agent() {
        assert!(parse(&["ctl", "new-pane", "--worktree"]).is_err());
        assert!(parse(&["ctl", "new-pane", "--worktree", "--branch", "b"]).is_err());
        assert!(parse(&[
            "ctl",
            "new-pane",
            "--agent",
            "claude",
            "--worktree",
            "--branch",
            "b"
        ])
        .is_ok());
    }

    #[test]
    fn move_flags_conflict_and_format_validates() {
        assert!(parse(&["ctl", "move", "4", "--up", "--down"]).is_err());
        assert!(parse(&["ctl", "move", "4", "--index", "1"]).is_err());
        assert!(parse(&["ctl", "move", "4", "--to", "2", "--index", "1"]).is_ok());
        let Some(crate::Command::Ctl(args)) = parse(&["ctl", "--format", "toml", "tree"])
            .expect("parses")
            .command
        else {
            panic!("not ctl");
        };
        assert!(args.format().is_err());
    }

    #[test]
    fn offline_commands_need_no_session() {
        let output = offline(&CtlCommand::Guide { topic: None }, Format::Json)
            .expect("guide renders")
            .expect("guide is offline");
        assert!(output.contains("ilium ctl"));
        assert!(offline(
            &CtlCommand::Guide {
                topic: Some("nope".into())
            },
            Format::Json
        )
        .is_err());
        let ops = offline(&CtlCommand::Ops, Format::Json)
            .expect("ops")
            .expect("offline");
        assert!(ops.contains("\"op\": \"rename\""));
        assert!(offline(
            &CtlCommand::Tree {
                all: false,
                depth: None,
                project: None,
                no_details: false
            },
            Format::Json
        )
        .expect("ok")
        .is_none());
    }
}
