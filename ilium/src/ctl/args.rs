//! Command-line shape of `ilium ctl`.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use ilium_control::guide;
use ilium_control::operation::WorktreeAction;
use ilium_control::Format;

use super::CtlFailure;

#[derive(Args, Debug)]
pub(crate) struct CtlArgs {
    /// Session to address, within the project given by --cwd. Inside an Ilium
    /// pane the pane's own session is the default.
    #[arg(long, global = true)]
    pub(crate) session: Option<String>,
    /// Address one live session by its socket path (see `ilium ctl sessions`).
    #[arg(long, global = true, value_name = "PATH", conflicts_with = "session")]
    pub(crate) socket: Option<PathBuf>,
    /// Output format of read commands: json, jsonl, yaml or markdown.
    #[arg(long, global = true, default_value = "json")]
    pub(crate) format: String,
    #[command(subcommand)]
    pub(crate) command: CtlCommand,
}

/// Which session a command addresses.
pub(crate) struct Target {
    pub(crate) session: Option<String>,
    pub(crate) socket: Option<PathBuf>,
}

impl CtlArgs {
    pub(crate) fn format(&self) -> Result<Format, CtlFailure> {
        Format::parse(&self.format).ok_or_else(|| {
            CtlFailure::usage(format!(
                "unknown format {:?}; use json, jsonl, yaml or markdown",
                self.format
            ))
        })
    }

    pub(crate) fn target(&self) -> Target {
        Target {
            session: self.session.clone(),
            socket: self.socket.clone(),
        }
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum CtlCommand {
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
        #[arg(long)]
        screen: bool,
        /// With --screen: only the last N lines.
        #[arg(long)]
        lines: Option<usize>,
    },
    /// A flat list of panes matching every given filter.
    Find {
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
    /// Block until a pane's agent state is one of --state (or the pane closes).
    Wait {
        id: String,
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

#[derive(Args, Debug, Default)]
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
