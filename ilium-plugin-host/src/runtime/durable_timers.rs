//! The store of durable timers (api-durable-timers).
//!
//! A durable timer (`timers.at`) fires at a wall-clock time even when Ilium was not running at
//! that time. The runtime keeps the timers of each plugin in this store, a small JSON file, and
//! writes it whenever the set changes. A restarted runtime loads it, hands each plugin its timers
//! again when the plugin activates, and activates a plugin that is not running once one of its
//! timers is due. A timer that came due while Ilium was down fires once, with its lateness.
//!
//! The file holds `{"version":1,"plugins":{"<plugin id>":{"<timer name>":<epoch millis>}}}`. A
//! missing or unreadable file is an empty store: the plugins lose their pending timers, nothing
//! else. The store never decides which plugin may arm a timer; the import does that.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// The version written into the file.
const FORMAT_VERSION: u64 = 1;

/// One plugin's timers: name to epoch milliseconds.
pub type PluginTimers = BTreeMap<String, u64>;

/// The durable timers of every plugin of one runtime side, and the file they live in.
#[derive(Debug)]
pub struct DurableTimerStore {
    path: PathBuf,
    plugins: BTreeMap<String, PluginTimers>,
    /// Whether `plugins` differs from the file.
    dirty: bool,
}

impl DurableTimerStore {
    /// Opens the store at `path`. A missing file, or one that does not parse, gives an empty
    /// store; the second case is reported through the returned warning so the runtime can log it.
    pub fn open(path: PathBuf) -> (Self, Option<String>) {
        let (plugins, warning) = match std::fs::read_to_string(&path) {
            Ok(text) => match parse(&text) {
                Ok(plugins) => (plugins, None),
                Err(message) => (
                    BTreeMap::new(),
                    Some(format!(
                        "the durable timer file {} was not read: {message}",
                        path.display()
                    )),
                ),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => (BTreeMap::new(), None),
            Err(error) => (
                BTreeMap::new(),
                Some(format!(
                    "the durable timer file {} was not read: {error}",
                    path.display()
                )),
            ),
        };
        (
            Self {
                path,
                plugins,
                dirty: false,
            },
            warning,
        )
    }

    /// The timers stored for a plugin, as `(name, epoch millis)`.
    pub fn entries_of(&self, plugin: &str) -> Vec<(String, u64)> {
        self.plugins
            .get(plugin)
            .map(|timers| {
                timers
                    .iter()
                    .map(|(name, epoch)| (name.clone(), *epoch))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The plugins that have a timer due at `now_unix_millis`, in id order.
    pub fn plugins_due(&self, now_unix_millis: u64) -> Vec<String> {
        self.plugins
            .iter()
            .filter(|(_, timers)| timers.values().any(|epoch| *epoch <= now_unix_millis))
            .map(|(plugin, _)| plugin.clone())
            .collect()
    }

    /// Whether any plugin has a stored timer.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Whether `plugin` has any stored timer.
    pub fn holds(&self, plugin: &str) -> bool {
        self.plugins.contains_key(plugin)
    }

    /// Replaces the timers of a plugin with what its book holds now. An empty set removes the
    /// plugin from the store.
    pub fn replace(&mut self, plugin: &str, entries: Vec<(String, u64)>) {
        let timers: PluginTimers = entries.into_iter().collect();
        if timers.is_empty() {
            if self.plugins.remove(plugin).is_some() {
                self.dirty = true;
            }
            return;
        }
        if self.plugins.get(plugin) != Some(&timers) {
            self.plugins.insert(plugin.to_owned(), timers);
            self.dirty = true;
        }
    }

    /// Writes the file when the set changed since the last write. The write goes to a temporary
    /// file that replaces the old one, so a crash leaves the previous version whole.
    pub fn save_if_changed(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        write_atomically(&self.path, &render(&self.plugins))?;
        self.dirty = false;
        Ok(())
    }

    /// The file this store lives in.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Reads the file text into the plugin map.
fn parse(text: &str) -> Result<BTreeMap<String, PluginTimers>, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let version = value.get("version").and_then(Value::as_u64);
    if version != Some(FORMAT_VERSION) {
        return Err(format!("unknown version {version:?}"));
    }
    let plugins = value
        .get("plugins")
        .and_then(Value::as_object)
        .ok_or_else(|| "no plugins object".to_owned())?;
    let mut parsed = BTreeMap::new();
    for (plugin, timers) in plugins {
        let Some(timers) = timers.as_object() else {
            continue;
        };
        let held: PluginTimers = timers
            .iter()
            .filter(|(name, _)| !name.is_empty())
            .filter_map(|(name, epoch)| epoch.as_u64().map(|epoch| (name.clone(), epoch)))
            .collect();
        if !held.is_empty() {
            parsed.insert(plugin.clone(), held);
        }
    }
    Ok(parsed)
}

/// The file text of a plugin map.
fn render(plugins: &BTreeMap<String, PluginTimers>) -> String {
    let mut text = json!({ "version": FORMAT_VERSION, "plugins": plugins }).to_string();
    text.push('\n');
    text
}

fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temporary_name = path.as_os_str().to_owned();
    temporary_name.push(".tmp");
    let temporary = PathBuf::from(temporary_name);
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(directory: &tempfile::TempDir) -> DurableTimerStore {
        DurableTimerStore::open(directory.path().join("durable-timers.json")).0
    }

    /// REQ: PRF-027
    #[test]
    fn a_missing_file_is_an_empty_store_without_a_warning() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let (store, warning) =
            DurableTimerStore::open(directory.path().join("durable-timers.json"));
        assert!(store.is_empty());
        assert_eq!(warning, None);
    }

    /// REQ: PRF-027
    #[test]
    fn timers_survive_a_save_and_a_reopen() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut store = store_in(&directory);
        store.replace(
            "ilium.core.fixture",
            vec![("job-17".to_owned(), 5_000), ("job-2".to_owned(), 9_000)],
        );
        store.save_if_changed().expect("saved");
        let reopened = store_in(&directory);
        assert_eq!(
            reopened.entries_of("ilium.core.fixture"),
            vec![("job-17".to_owned(), 5_000), ("job-2".to_owned(), 9_000)]
        );
        assert!(reopened.entries_of("ilium.core.other").is_empty());
    }

    /// REQ: PRF-027
    #[test]
    fn an_unchanged_set_is_not_written_again() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut store = store_in(&directory);
        store.replace("p", vec![("a".to_owned(), 1)]);
        store.save_if_changed().expect("first save");
        std::fs::remove_file(store.path()).expect("remove the file");
        store.replace("p", vec![("a".to_owned(), 1)]);
        store.save_if_changed().expect("second save");
        assert!(!store.path().exists(), "nothing changed, nothing written");
    }

    /// REQ: PRF-027
    #[test]
    fn cancelling_the_last_timer_removes_the_plugin() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut store = store_in(&directory);
        store.replace("p", vec![("a".to_owned(), 1)]);
        store.save_if_changed().expect("saved");
        store.replace("p", Vec::new());
        store.save_if_changed().expect("saved again");
        assert!(!store.holds("p"));
        assert!(store_in(&directory).is_empty());
    }

    /// REQ: PRF-027
    #[test]
    fn only_plugins_with_an_overdue_timer_are_due() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut store = store_in(&directory);
        store.replace("early", vec![("a".to_owned(), 100)]);
        store.replace("late", vec![("a".to_owned(), 900)]);
        store.replace("both", vec![("a".to_owned(), 900), ("b".to_owned(), 50)]);
        assert_eq!(
            store.plugins_due(500),
            vec!["both".to_owned(), "early".to_owned()]
        );
        assert_eq!(store.plugins_due(10), Vec::<String>::new());
    }

    /// REQ: PRF-027
    #[test]
    fn a_damaged_file_is_an_empty_store_with_a_warning() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("durable-timers.json");
        std::fs::write(&path, "{ not json").expect("write");
        let (store, warning) = DurableTimerStore::open(path);
        assert!(store.is_empty());
        assert!(warning.is_some_and(|text| text.contains("was not read")));
    }

    /// REQ: PRF-027
    #[test]
    fn a_file_of_another_version_is_not_trusted() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("durable-timers.json");
        std::fs::write(&path, r#"{"version":2,"plugins":{"p":{"a":1}}}"#).expect("write");
        let (store, warning) = DurableTimerStore::open(path);
        assert!(store.is_empty());
        assert!(warning.is_some());
    }

    /// REQ: PRF-027
    #[test]
    fn entries_with_empty_names_or_non_numbers_are_skipped() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("durable-timers.json");
        std::fs::write(
            &path,
            r#"{"version":1,"plugins":{"p":{"":5,"a":"soon","b":7},"q":"x"}}"#,
        )
        .expect("write");
        let (store, warning) = DurableTimerStore::open(path);
        assert_eq!(warning, None);
        assert_eq!(store.entries_of("p"), vec![("b".to_owned(), 7)]);
        assert!(!store.holds("q"));
    }
}

/// The store as the runtime uses it: a fake guest that records what it is sent.
#[cfg(test)]
mod runtime_tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use ilium_plugin_api::generated::exports::hook_handler::HookReply;
    use ilium_plugin_api::generated::host::timers::HostTimers;
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::ids::{NodeId, PluginId};
    use ilium_plugin_api::generated::types::intents::{Intent, Submission};
    use ilium_plugin_api::generated::types::layout::Area;
    use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
    use ilium_plugin_api::generated::types::requests::Outcome;
    use ilium_plugin_api::generated::types::tree::NodeView;
    use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
    use ilium_plugin_api::kernel::{
        Cell, IntentResult, KernelFacade, Origin, PaneRead, TreeNode, TreeRead,
    };
    use ilium_plugin_api::{ProcessKind, Side, Tier};
    use serde_json::json;

    use crate::calls::{ResolvedLimits, Trap};
    use crate::engine::{EngineConfig, PluginEngine};
    use crate::imports::backed::NullLogSink;
    use crate::imports::timers::{self, TimerBook};
    use crate::instance::{ExportCall, ExportReply, GuestExports, PluginInstance};
    use crate::runtime::{HostRuntime, PluginRecord, PluginState, RuntimeConfig};

    struct EmptyTree;
    impl TreeRead for EmptyTree {
        fn nodes(&self) -> Vec<TreeNode> {
            Vec::<NodeView>::new()
        }
        fn node(&self, _id: NodeId) -> Option<TreeNode> {
            None
        }
        fn epoch(&self) -> u64 {
            0
        }
    }

    struct NoPanes;
    impl PaneRead for NoPanes {
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
    static PANES: NoPanes = NoPanes;
    static TREE: EmptyTree = EmptyTree;

    struct Kernel;
    impl KernelFacade for Kernel {
        fn apply_intent(&self, _origin: &Origin, _intent: Intent) -> IntentResult {
            Outcome::Ok
        }
        fn tree(&self) -> &dyn TreeRead {
            &TREE
        }
        fn panes(&self) -> &dyn PaneRead {
            &PANES
        }
        fn process_kind(&self) -> ProcessKind {
            ProcessKind::Server
        }
    }

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

    fn runtime_over(file: std::path::PathBuf) -> HostRuntime {
        let engine = Arc::new(
            PluginEngine::new(EngineConfig {
                epoch_tick_ms: 10,
                cache_dir: None,
                process: ProcessKind::Server,
            })
            .expect("engine"),
        );
        let mut runtime = HostRuntime::new(
            engine,
            RuntimeConfig {
                side: Side::Server,
                kernel: Arc::new(Kernel),
                log: Arc::new(NullLogSink),
                provider: None,
                disabled: Default::default(),
            },
        );
        runtime.set_durable_timer_file(file);
        runtime
    }

    fn add_active_plugin(runtime: &mut HostRuntime, id: &str) -> Seen {
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
                manifest: json!({"id": id}),
                tier: Tier::T1,
                granted: Vec::new(),
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

    /// The book a plugin holds, with the given durable timers armed in it.
    fn arm(runtime: &HostRuntime, plugin: &str, entries: &[(&str, u64)]) -> TimerBook {
        let book = TimerBook::default();
        timers::restore_durable(
            &book,
            entries
                .iter()
                .map(|(name, epoch)| ((*name).to_owned(), *epoch)),
        );
        runtime
            .timer_books
            .lock()
            .expect("timer books")
            .insert(plugin.to_owned(), Arc::clone(&book));
        book
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            })
    }

    fn fired(seen: &Seen) -> Vec<(String, u64)> {
        seen.lock()
            .expect("seen")
            .iter()
            .filter_map(|event| match event {
                Event::DurableTimerFired(fired) => Some((fired.name.clone(), fired.late_by_millis)),
                _ => None,
            })
            .collect()
    }

    /// REQ: PRF-027
    #[test]
    fn a_timer_armed_by_an_active_plugin_reaches_the_file_on_the_next_pump() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file = directory.path().join("durable-timers.json");
        let mut runtime = runtime_over(file.clone());
        add_active_plugin(&mut runtime, "a.b");
        let due = now_millis() + 3_600_000;
        arm(&runtime, "a.b", &[("remind", due)]);
        assert!(runtime.fire_timers().is_empty());
        let (stored, warning) = super::DurableTimerStore::open(file);
        assert_eq!(warning, None);
        assert_eq!(stored.entries_of("a.b"), vec![("remind".to_owned(), due)]);
    }

    /// REQ: PRF-027
    #[test]
    fn a_restarted_runtime_hands_the_timers_back_and_the_overdue_one_fires_once() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file = directory.path().join("durable-timers.json");
        let overdue = now_millis().saturating_sub(250_000);
        let future = now_millis() + 3_600_000;
        {
            let mut before = runtime_over(file.clone());
            add_active_plugin(&mut before, "a.b");
            arm(&before, "a.b", &[("job-17", overdue), ("job-18", future)]);
            // What the pump does before it would fire anything.
            before.sync_durable_timers();
            // The first runtime goes away without firing: it was a crash, or Ilium was off.
        }
        let mut after = runtime_over(file.clone());
        let seen = add_active_plugin(&mut after, "a.b");
        // What `build_imports` does for a new instance: a fresh book armed from the store.
        let book = TimerBook::default();
        let stored = super::DurableTimerStore::open(file.clone()).0;
        timers::restore_durable(&book, stored.entries_of("a.b"));
        after
            .timer_books
            .lock()
            .expect("timer books")
            .insert("a.b".to_owned(), book);
        assert!(after.fire_timers().is_empty());
        let events = fired(&seen);
        assert_eq!(events.len(), 1, "only the overdue timer fires: {events:?}");
        assert_eq!(events[0].0, "job-17");
        assert!(
            events[0].1 >= 250_000,
            "the lateness is reported: {events:?}"
        );
        assert!(after.fire_timers().is_empty());
        assert_eq!(fired(&seen).len(), 1, "it fires once");
        let remaining = super::DurableTimerStore::open(file).0;
        assert_eq!(
            remaining.entries_of("a.b"),
            vec![("job-18".to_owned(), future)],
            "the fired timer left the file, the future one stays"
        );
    }

    /// REQ: PRF-027
    #[test]
    fn a_stored_timer_of_a_plugin_that_is_not_running_keeps_the_pump_polling() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file = directory.path().join("durable-timers.json");
        let mut store = super::DurableTimerStore::open(file.clone()).0;
        store.replace("a.b", vec![("later".to_owned(), now_millis() + 60_000)]);
        store.save_if_changed().expect("saved");
        let runtime = runtime_over(file);
        let due = runtime.next_timer_due().expect("a poll is scheduled");
        assert!(due <= std::time::Instant::now() + std::time::Duration::from_secs(2));
        let idle = runtime_over(directory.path().join("empty.json"));
        assert_eq!(
            idle.next_timer_due(),
            None,
            "nothing stored, nothing polled"
        );
    }

    /// REQ: PRF-027
    #[test]
    fn a_cancelled_durable_timer_leaves_the_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file = directory.path().join("durable-timers.json");
        let mut runtime = runtime_over(file.clone());
        add_active_plugin(&mut runtime, "a.b");
        let due = now_millis() + 3_600_000;
        let book = arm(&runtime, "a.b", &[("remind", due)]);
        runtime.fire_timers();
        assert!(super::DurableTimerStore::open(file.clone()).0.holds("a.b"));
        // The plugin cancels it: the book no longer holds it.
        let mut import = crate::imports::timers::TimersImport::with_book(book);
        import.cancel_durable("remind".to_owned());
        runtime.fire_timers();
        assert!(!super::DurableTimerStore::open(file).0.holds("a.b"));
    }
}
