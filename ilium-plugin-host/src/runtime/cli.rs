//! CLI invocations of the runtime (api-cli, capability-cli-contribute).
//!
//! The CLI router (the server for `ilium <name>`, the CLI process itself for a command that
//! needs no server) asks [`HostRuntime::cli_commands`] which subcommands exist, starts one with
//! [`HostRuntime::start_cli`], drains what it printed with [`HostRuntime::take_cli_output`] after
//! each pump and ends it early with [`HostRuntime::cancel_cli`] when the caller goes away.
//!
//! A plugin owns a subcommand only while it is enabled and holds the `cli.contribute` grant; a
//! declined or disabled plugin's name is unknown. The pane-selector argument group is resolved
//! here, against the kernel tree, so the plugin never parses `--pane` or `--all-panes`: it
//! receives `argv` without those options and calls `selected-panes`.

use ilium_plugin_api::generated::host::cli::PaneSelection;
use ilium_plugin_api::generated::types::events::{CliInvokedEvent, Event};
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::tree::NodeClass;
use ilium_plugin_api::kernel::TreeRead;
use serde_json::Value;

use super::{HostRuntime, PluginState, RuntimeError};
use crate::imports::cli::{CliChunk, CliSession, OutputForm, StdinMode};

/// The grant a plugin needs to contribute subcommands.
const CLI_GRANT: &str = "cli.contribute";
/// The argument group that adds `--pane` and `--all-panes`.
pub const PANE_SELECTOR_GROUP: &str = "pane-selector";

/// One subcommand a plugin contributes (`contributes.cli` of its manifest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCommand {
    /// The owning plugin.
    pub plugin: String,
    /// The subcommand name.
    pub name: String,
    /// Further names for the same command.
    pub aliases: Vec<String>,
    /// The line shown in `ilium --help`.
    pub summary: String,
    /// `jsonl`, `text` or `mixed`.
    pub output: String,
    /// `none`, `optional` or `required`.
    pub stdin: String,
    /// Whether the command needs a running server (the default).
    pub requires_server: bool,
    /// The argument groups the command takes.
    pub argument_groups: Vec<String>,
}

impl CliCommand {
    /// Reads one manifest entry; `None` when it has no usable name.
    pub fn from_manifest(plugin: &str, entry: &Value) -> Option<Self> {
        let text = |key: &str| entry.get(key).and_then(Value::as_str);
        let strings = |key: &str| -> Vec<String> {
            entry
                .get(key)
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let name = text("name").filter(|name| is_command_name(name))?;
        Some(Self {
            plugin: plugin.to_owned(),
            name: name.to_owned(),
            aliases: strings("aliases")
                .into_iter()
                .filter(|alias| is_command_name(alias))
                .collect(),
            summary: text("summary").unwrap_or_default().to_owned(),
            output: text("output").unwrap_or("jsonl").to_owned(),
            stdin: text("stdin").unwrap_or("none").to_owned(),
            requires_server: entry
                .get("requires_server")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            argument_groups: strings("argument_groups"),
        })
    }

    /// Whether `word` is this command's name or one of its aliases.
    pub fn answers_to(&self, word: &str) -> bool {
        self.name == word || self.aliases.iter().any(|alias| alias == word)
    }

    /// Whether the command takes the pane-selector group.
    pub fn takes_pane_selector(&self) -> bool {
        self.argument_groups
            .iter()
            .any(|group| group == PANE_SELECTOR_GROUP)
    }
}

/// `[a-z][a-z0-9-]{0,31}` (capability-cli-contribute, "Manifest").
pub fn is_command_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && name.len() <= 32
        && characters.all(|next| next.is_ascii_lowercase() || next.is_ascii_digit() || next == '-')
}

/// What the caller supplies to run a subcommand.
#[derive(Debug, Clone, Default)]
pub struct CliStart {
    /// The plugin that must own the command; empty when any owner will do.
    pub plugin: String,
    /// The subcommand name or one of its aliases.
    pub command: String,
    /// The words after the name, as the shell passed them.
    pub args: Vec<String>,
    /// The bytes for standard input.
    pub stdin: Vec<u8>,
    /// The caller's working directory, absolute.
    pub cwd: String,
    /// Whether the caller's standard output is a terminal.
    pub stdout_tty: bool,
    /// Whether the caller's standard error is a terminal.
    pub stderr_tty: bool,
    /// The pane the command ran in (`ILIUM_PANE_ID`).
    pub caller_pane: Option<u64>,
}

/// Why a subcommand did not start.
#[derive(Debug, thiserror::Error)]
pub enum CliStartError {
    /// No enabled plugin with the grant contributes this name.
    #[error("unknown command {0}")]
    Unknown(String),
    /// The arguments are wrong (a bad `--pane` value); the caller exits with 2.
    #[error("{0}")]
    Usage(String),
    /// The owner could not run.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// `--pane` and `--all-panes` as typed, before they are resolved against the tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneRequest {
    /// Every `--pane` value: an id, a title or `.`.
    pub terms: Vec<String>,
    /// `--all-panes` was given.
    pub all: bool,
}

/// Splits the pane-selector options off `args`. An option without its value is a usage error.
pub fn split_pane_selector(args: &[String]) -> Result<(Vec<String>, PaneRequest), String> {
    let mut rest = Vec::new();
    let mut request = PaneRequest::default();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "--all-panes" {
            request.all = true;
        } else if word == "--pane" {
            match words.next() {
                Some(value) => request.terms.push(value.clone()),
                None => return Err("--pane needs a pane id, a title or .".to_owned()),
            }
        } else if let Some(value) = word.strip_prefix("--pane=") {
            request.terms.push(value.to_owned());
        } else {
            rest.push(word.clone());
        }
    }
    Ok((rest, request))
}

/// Resolves the request to node ids. `None` when the caller gave neither option.
pub fn resolve_panes(
    tree: &dyn TreeRead,
    caller: Option<NodeId>,
    request: &PaneRequest,
) -> Result<Option<PaneSelection>, String> {
    if request.terms.is_empty() && !request.all {
        return Ok(None);
    }
    let panes: Vec<_> = tree
        .nodes()
        .into_iter()
        .filter(|node| node.class == NodeClass::Pane)
        .collect();
    if request.all {
        return Ok(Some(PaneSelection {
            panes: panes.iter().map(|node| node.id).collect(),
            all: true,
        }));
    }
    let mut chosen: Vec<NodeId> = Vec::new();
    for term in &request.terms {
        let matches: Vec<NodeId> = if term == "." {
            match caller {
                Some(pane) if panes.iter().any(|node| node.id == pane) => vec![pane],
                _ => return Err("--pane . needs a caller pane (ILIUM_PANE_ID)".to_owned()),
            }
        } else if let Some(pane) = term
            .parse::<u64>()
            .ok()
            .map(NodeId)
            .filter(|id| panes.iter().any(|node| node.id == *id))
        {
            vec![pane]
        } else {
            panes
                .iter()
                .filter(|node| &node.title == term)
                .map(|node| node.id)
                .collect()
        };
        if matches.is_empty() {
            return Err(format!("no pane matches {term}"));
        }
        for pane in matches {
            if !chosen.contains(&pane) {
                chosen.push(pane);
            }
        }
    }
    Ok(Some(PaneSelection {
        panes: chosen,
        all: false,
    }))
}

impl HostRuntime {
    /// The subcommands of every enabled plugin that holds the `cli.contribute` grant, in plugin
    /// id order and manifest order.
    pub fn cli_commands(&self) -> Vec<CliCommand> {
        let mut commands = Vec::new();
        for (id, record) in &self.plugins {
            if record.state == PluginState::Disabled
                || !record.granted.iter().any(|grant| grant == CLI_GRANT)
            {
                continue;
            }
            let entries = record
                .manifest
                .pointer("/contributes/cli")
                .and_then(Value::as_array);
            commands.extend(
                entries
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| CliCommand::from_manifest(id, entry)),
            );
        }
        commands
    }

    /// Starts a subcommand: resolves the name, activates the owner when it still waits for its
    /// first use, resolves the pane selector and delivers `cli-invoked`. Returns the invocation
    /// id. The caller pumps afterwards and drains the output.
    pub fn start_cli(&mut self, start: CliStart) -> Result<u64, CliStartError> {
        let command = self
            .cli_commands()
            .into_iter()
            .find(|command| {
                command.answers_to(&start.command)
                    && (start.plugin.is_empty() || start.plugin == command.plugin)
            })
            .ok_or_else(|| CliStartError::Unknown(start.command.clone()))?;
        if self.state(&command.plugin) == Some(PluginState::Loaded) {
            self.activate(&command.plugin)?;
        }
        if self.state(&command.plugin) != Some(PluginState::Active) {
            return Err(RuntimeError::NotActive(command.plugin).into());
        }
        let caller_pane = start.caller_pane.map(NodeId);
        let (argv, selection) = if command.takes_pane_selector() {
            let (rest, request) = split_pane_selector(&start.args).map_err(CliStartError::Usage)?;
            let selection = resolve_panes(self.kernel.tree(), caller_pane, &request)
                .map_err(CliStartError::Usage)?;
            (rest, selection)
        } else {
            (start.args.clone(), None)
        };
        let invocation = self.cli_book.open(CliSession {
            plugin: command.plugin.clone(),
            output: OutputForm::from_manifest(Some(&command.output)),
            stdin_mode: StdinMode::from_manifest(Some(&command.stdin)),
            stdin: start.stdin,
            stdout_tty: start.stdout_tty,
            stderr_tty: start.stderr_tty,
            caller_pane,
            selection,
        });
        let event = Event::CliInvoked(CliInvokedEvent {
            invocation,
            name: command.name,
            argv,
            cwd: start.cwd,
        });
        if let Err(error) = self.deliver(&command.plugin, vec![event]) {
            self.cli_book.cancel(invocation);
            return Err(error.into());
        }
        Ok(invocation)
    }

    /// The caller of a running subcommand went away: the plugin gets `cli-cancelled`, and what
    /// it writes afterwards is refused. Nothing happens for an invocation that is not running.
    pub fn cancel_cli(&mut self, invocation: u64) -> Result<(), RuntimeError> {
        let Some(plugin) = self.cli_book.plugin_of(invocation) else {
            return Ok(());
        };
        self.cli_book.cancel(invocation);
        self.deliver(&plugin, vec![Event::CliCancelled(invocation)])
    }

    /// What the subcommands printed and how they ended since the last call, oldest first.
    pub fn take_cli_output(&self) -> Vec<CliChunk> {
        self.cli_book.take_output()
    }

    /// Ends the invocations of a plugin whose instance is gone, so no caller waits forever.
    pub(super) fn abandon_cli(&self, plugin: &str) {
        self.cli_book.abandon_plugin(
            plugin,
            1,
            &format!("ilium: the plugin {plugin} stopped before the command finished"),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use ilium_plugin_api::generated::exports::hook_handler::HookReply;
    use ilium_plugin_api::generated::types::events::{CliInvokedEvent, Event};
    use ilium_plugin_api::generated::types::ids::{NodeId, PluginId};
    use ilium_plugin_api::generated::types::intents::{Intent, Submission};
    use ilium_plugin_api::generated::types::layout::Area;
    use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
    use ilium_plugin_api::generated::types::requests::Outcome;
    use ilium_plugin_api::generated::types::tree::{NodeClass, NodeView};
    use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
    use ilium_plugin_api::kernel::{
        Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
    };
    use ilium_plugin_api::{ProcessKind, Side, Tier};
    use serde_json::{json, Value};

    use super::{
        is_command_name, resolve_panes, split_pane_selector, CliCommand, CliStartError, PaneRequest,
    };
    use crate::calls::{ResolvedLimits, Trap};
    use crate::engine::{EngineConfig, PluginEngine};
    use crate::imports::backed::NullLogSink;
    use crate::imports::cli::CliChunk;
    use crate::instance::{ExportCall, ExportReply, GuestExports, PluginInstance};
    use crate::runtime::{HostRuntime, PluginRecord, PluginState, RuntimeConfig};

    fn words(text: &[&str]) -> Vec<String> {
        text.iter().map(|word| (*word).to_owned()).collect()
    }

    fn pane(id: u64, title: &str) -> NodeView {
        NodeView {
            id: NodeId(id),
            parent: None,
            class: NodeClass::Pane,
            content_kind: None,
            title: title.to_owned(),
            cwd: None,
            agent: None,
            locked: false,
            revision: 1,
            container: None,
        }
    }

    struct Tree(Vec<NodeView>);
    impl TreeRead for Tree {
        fn nodes(&self) -> Vec<TreeNode> {
            self.0.clone()
        }
        fn node(&self, id: NodeId) -> Option<TreeNode> {
            self.0.iter().find(|node| node.id == id).cloned()
        }
        fn epoch(&self) -> u64 {
            0
        }
    }

    fn tree() -> Tree {
        let mut folder = pane(3, "docs");
        folder.class = NodeClass::Folder;
        Tree(vec![
            pane(1, "build"),
            pane(2, "build"),
            folder,
            pane(4, "logs"),
        ])
    }

    // ---- names and manifest entries ------------------------------------------------------

    /// REQ: API-005
    #[test]
    fn command_names_follow_the_manifest_rule() {
        for good in ["a", "recap", "new-pane", "a1-b2", &"a".repeat(32)] {
            assert!(is_command_name(good), "{good}");
        }
        for bad in ["", "1a", "-a", "A", "a_b", "a b", &"a".repeat(33)] {
            assert!(!is_command_name(bad), "{bad}");
        }
    }

    /// REQ: API-005
    #[test]
    fn a_manifest_entry_gets_its_defaults() {
        let entry = json!({"name": "panes", "summary": "List panes"});
        let command = CliCommand::from_manifest("x.y", &entry).expect("entry parses");
        assert_eq!(command.output, "jsonl");
        assert_eq!(command.stdin, "none");
        assert!(
            command.requires_server,
            "a server is required unless declined"
        );
        assert!(command.argument_groups.is_empty());
        assert!(CliCommand::from_manifest("x.y", &json!({"name": "Bad"})).is_none());
        assert!(CliCommand::from_manifest("x.y", &json!({"summary": "no name"})).is_none());
    }

    /// REQ: API-005
    #[test]
    fn aliases_answer_to_the_same_command() {
        let entry = json!({"name": "progress", "aliases": ["wait", "BAD"]});
        let command = CliCommand::from_manifest("x.y", &entry).expect("entry parses");
        assert!(command.answers_to("progress") && command.answers_to("wait"));
        assert!(!command.answers_to("BAD") && !command.answers_to("other"));
    }

    // ---- pane selector -------------------------------------------------------------------

    /// REQ: API-005
    #[test]
    fn the_selector_options_leave_argv_and_become_a_request() {
        let (rest, request) = split_pane_selector(&words(&[
            "go",
            "--pane",
            "build",
            "--pane=4",
            "--all-panes",
            "tail",
        ]))
        .expect("parses");
        assert_eq!(rest, words(&["go", "tail"]));
        assert_eq!(
            request,
            PaneRequest {
                terms: words(&["build", "4"]),
                all: true
            }
        );
        assert!(split_pane_selector(&words(&["--pane"])).is_err());
    }

    /// REQ: API-005
    #[test]
    fn selecting_by_id_title_dot_and_all() {
        let tree = tree();
        let by = |terms: &[&str], all: bool, caller: Option<u64>| {
            resolve_panes(
                &tree,
                caller.map(NodeId),
                &PaneRequest {
                    terms: words(terms),
                    all,
                },
            )
        };
        assert_eq!(by(&[], false, None), Ok(None));
        let all = by(&[], true, None).expect("resolves").expect("selected");
        assert!(all.all);
        assert_eq!(all.panes, vec![NodeId(1), NodeId(2), NodeId(4)]);
        let titled = by(&["build"], false, None)
            .expect("resolves")
            .expect("selected");
        assert_eq!(titled.panes, vec![NodeId(1), NodeId(2)]);
        assert!(!titled.all);
        let mixed = by(&["4", "build", "1"], false, None)
            .expect("resolves")
            .expect("selected");
        assert_eq!(mixed.panes, vec![NodeId(4), NodeId(1), NodeId(2)]);
        let caller = by(&["."], false, Some(4))
            .expect("resolves")
            .expect("selected");
        assert_eq!(caller.panes, vec![NodeId(4)]);
    }

    /// REQ: API-005
    #[test]
    fn a_selector_that_matches_nothing_is_an_error() {
        let tree = tree();
        let request = |terms: &[&str]| PaneRequest {
            terms: words(terms),
            all: false,
        };
        assert!(resolve_panes(&tree, None, &request(&["nope"])).is_err());
        // A folder is not a pane, even by id.
        assert!(resolve_panes(&tree, None, &request(&["3"])).is_err());
        assert!(resolve_panes(&tree, None, &request(&["."])).is_err());
        assert!(resolve_panes(&tree, Some(NodeId(9)), &request(&["."])).is_err());
    }

    // ---- a runtime over a scripted guest -------------------------------------------------

    struct Panes;
    impl PaneRead for Panes {
        fn screen_text(&self, _pane: NodeId) -> Option<String> {
            None
        }
        fn cells(&self, _pane: NodeId, _rows: std::ops::Range<u16>) -> Option<Vec<Vec<Cell>>> {
            None
        }
        fn history(&self, _pane: NodeId, _lines: u32) -> Option<Vec<String>> {
            None
        }
    }
    static PANES: Panes = Panes;

    struct Kernel(Tree);
    impl KernelFacade for Kernel {
        fn apply_intent(&self, _origin: &Origin, _intent: Intent) -> IntentResult {
            Outcome::Ok
        }
        fn tree(&self) -> &dyn TreeRead {
            &self.0
        }
        fn panes(&self) -> &dyn PaneRead {
            &PANES
        }
        fn process_kind(&self) -> ProcessKind {
            ProcessKind::Server
        }
    }

    /// Every event batch the guest received.
    type Seen = Arc<Mutex<VecDeque<Event>>>;

    struct Guest(Seen);
    impl GuestExports for Guest {
        fn activate(
            &mut self,
            _activation: Activation,
        ) -> Result<Result<(), ilium_plugin_api::PluginError>, Trap> {
            Ok(Ok(()))
        }
        fn deactivate(&mut self, _reason: DeactivationReason) -> Result<Option<Vec<u8>>, Trap> {
            Ok(None)
        }
        fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
            if let Ok(mut seen) = self.0.lock() {
                seen.extend(batch);
            }
            Ok(Vec::new())
        }
        fn render(&mut self, _view: ViewTarget, _area: Area) -> Result<UiTree, Trap> {
            Err(Trap {
                code: ilium_plugin_api::Code::from_static("trap.abi"),
                interface: "view".into(),
                function: "render".into(),
            })
        }
        fn call_export(&mut self, _call: ExportCall) -> Result<ExportReply, Trap> {
            Ok(ExportReply::Hook(HookReply::Pass))
        }
    }

    fn runtime() -> HostRuntime {
        let engine = Arc::new(
            PluginEngine::new(EngineConfig {
                epoch_tick_ms: 10,
                cache_dir: None,
                process: ProcessKind::Server,
            })
            .expect("engine"),
        );
        HostRuntime::new(
            engine,
            RuntimeConfig {
                side: Side::Server,
                kernel: Arc::new(Kernel(tree())),
                log: Arc::new(NullLogSink),
                provider: None,
                disabled: Default::default(),
            },
        )
    }

    fn add_plugin(runtime: &mut HostRuntime, id: &str, granted: &[&str], cli: Value) -> Seen {
        let seen = Seen::default();
        let mut instance = PluginInstance::from_guest(Box::new(Guest(seen.clone())));
        instance
            .activate(Activation {
                plugin: PluginId(id.to_owned()),
                api: "ilium:plugin@0.1.0".to_owned(),
                instance: 1,
                side: "server".to_owned(),
                settings_json: "{}".to_owned(),
                granted: Vec::new(),
                restored: None,
            })
            .expect("activates");
        runtime.plugins.insert(
            id.to_owned(),
            PluginRecord {
                manifest: json!({"id": id, "contributes": {"cli": cli}}),
                tier: Tier::T1,
                granted: granted.iter().map(|grant| (*grant).to_owned()).collect(),
                triggers: Vec::new(),
                limits: ResolvedLimits(BTreeMap::new()),
                compiled: None,
                instance: Some(instance),
                state: PluginState::Active,
                saved: None,
                activation_order: 0,
                instance_number: 1,
            },
        );
        seen
    }

    fn start(command: &str, args: &[&str]) -> super::CliStart {
        super::CliStart {
            command: command.to_owned(),
            args: words(args),
            cwd: "/work".to_owned(),
            ..super::CliStart::default()
        }
    }

    fn events(seen: &Seen) -> Vec<Event> {
        seen.lock()
            .map(|seen| seen.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// REQ: API-005
    #[test]
    fn a_granted_command_starts_and_the_plugin_gets_cli_invoked() {
        let mut runtime = runtime();
        let seen = add_plugin(
            &mut runtime,
            "a.b",
            &["cli.contribute"],
            json!([{"name": "go", "aliases": ["run"], "output": "text"}]),
        );
        let invocation = runtime
            .start_cli(start("run", &["x", "y"]))
            .expect("starts");
        assert_eq!(
            events(&seen),
            vec![Event::CliInvoked(CliInvokedEvent {
                invocation,
                name: "go".to_owned(),
                argv: words(&["x", "y"]),
                cwd: "/work".to_owned(),
            })],
            "the plugin sees the declared name, not the alias"
        );
        let second = runtime.start_cli(start("go", &[])).expect("starts again");
        assert!(second > invocation, "ids are unique and rise");
    }

    /// REQ: API-005
    #[test]
    fn a_declined_grant_a_disabled_plugin_and_a_missing_name_are_unknown() {
        let mut runtime = runtime();
        add_plugin(&mut runtime, "a.b", &[], json!([{"name": "hidden"}]));
        add_plugin(
            &mut runtime,
            "c.d",
            &["cli.contribute"],
            json!([{"name": "shown"}]),
        );
        assert!(matches!(
            runtime.start_cli(start("hidden", &[])),
            Err(CliStartError::Unknown(_))
        ));
        assert!(matches!(
            runtime.start_cli(start("absent", &[])),
            Err(CliStartError::Unknown(_))
        ));
        assert_eq!(
            runtime
                .cli_commands()
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec!["shown"]
        );
        runtime.set_enabled("c.d", false).expect("disables");
        assert!(matches!(
            runtime.start_cli(start("shown", &[])),
            Err(CliStartError::Unknown(_))
        ));
    }

    /// REQ: API-005
    #[test]
    fn the_pane_selector_is_resolved_before_the_plugin_runs() {
        let mut runtime = runtime();
        let seen = add_plugin(
            &mut runtime,
            "a.b",
            &["cli.contribute"],
            json!([{"name": "ping", "argument_groups": ["pane-selector"]}]),
        );
        runtime
            .start_cli(start("ping", &["--pane", "logs", "hello"]))
            .expect("starts");
        assert!(matches!(
            events(&seen).as_slice(),
            [Event::CliInvoked(event)] if event.argv == words(&["hello"])
        ));
        let refused = runtime.start_cli(start("ping", &["--pane", "nope"]));
        assert!(matches!(refused, Err(CliStartError::Usage(_))));
        assert_eq!(events(&seen).len(), 1, "a refused start delivers nothing");
    }

    /// REQ: API-005
    #[test]
    fn cancel_tells_the_plugin_once_and_a_stopped_plugin_ends_its_invocations() {
        let mut runtime = runtime();
        let seen = add_plugin(
            &mut runtime,
            "a.b",
            &["cli.contribute"],
            json!([{"name": "go"}]),
        );
        let invocation = runtime.start_cli(start("go", &[])).expect("starts");
        runtime.cancel_cli(invocation).expect("cancels");
        runtime
            .cancel_cli(invocation)
            .expect("a second cancel is ignored");
        let cancels = events(&seen)
            .into_iter()
            .filter(|event| matches!(event, Event::CliCancelled(id) if *id == invocation))
            .count();
        assert_eq!(cancels, 1);
        assert!(
            runtime.take_cli_output().is_empty(),
            "nobody listens after a cancel"
        );

        let running = runtime.start_cli(start("go", &[])).expect("starts");
        runtime
            .deactivate("a.b", DeactivationReason::Shutdown)
            .expect("deactivates");
        let output = runtime.take_cli_output();
        assert!(matches!(
            output.last(),
            Some(CliChunk::Exit { invocation, code: 1 }) if *invocation == running
        ));
    }
}
