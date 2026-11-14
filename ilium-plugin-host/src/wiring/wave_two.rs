//! Equips the per-instance imports that reach the machine: `process`, `fs`, `http`,
//! `inference` and `audio`. `jobs` is equipped by the runtime itself, because a job needs the
//! runtime's queue.
//!
//! Every import is built fresh for the instance, from the instance's own grants, so what one
//! plugin opens, spawns or streams is never visible to another. Dropping the instance drops the
//! imports, which stops their watches, streams, child processes and provider calls.
//!
//! What talks to a device or a provider is a backend trait the composition root fills in:
//! [`ServerBackendInputs`] for the server (and the CLI's server-side plugins) and
//! [`ClientBackendInputs`] for the client. A backend left out keeps the runtime's default import,
//! which answers `unsupported` for the work it cannot do.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::side::Side;

use crate::imports::audio::{AudioBackend, AudioImport};
use crate::imports::env::ImportEnv;
use crate::imports::fs::{FilePicker, FsImport};
use crate::imports::generated_dispatch::ImportSet;
use crate::imports::http::HttpImport;
use crate::imports::inference::{InferenceBackend, InferenceImport};
use crate::imports::process::ProcessImport;
use crate::runtime::InstanceContext;

/// The read-only `fs` scopes of the agents' own folders, by scope name
/// (`agent-sessions:claude` to `~/.claude/projects`).
pub type AgentScopes = BTreeMap<String, PathBuf>;

/// The agent folders under the user's home folder (api-fs "Scopes"): transcripts and
/// configuration of Claude Code and Codex.
pub fn agent_scopes_in(home: &Path) -> AgentScopes {
    [
        (
            "agent-sessions:claude",
            home.join(".claude").join("projects"),
        ),
        ("agent-sessions:codex", home.join(".codex").join("sessions")),
        ("agent-config:claude", home.join(".claude")),
        ("agent-config:codex", home.join(".codex")),
    ]
    .into_iter()
    .map(|(scope, root)| (scope.to_owned(), root))
    .collect()
}

/// What the server composition root supplies.
#[derive(Default)]
pub struct ServerBackendInputs {
    /// Answers `ilium:host/inference` (the user's provider settings).
    pub inference: Option<Arc<dyn InferenceBackend>>,
    /// The agents' transcript and configuration folders.
    pub agent_scopes: AgentScopes,
}

/// What the client composition root supplies.
#[derive(Default)]
pub struct ClientBackendInputs {
    /// Answers `ilium:host/inference` (the user's provider settings).
    pub inference: Option<Arc<dyn InferenceBackend>>,
    /// Lists sound devices and plays `audio.play`.
    pub audio: Option<Arc<dyn AudioBackend>>,
    /// Shows the picker of `fs.pick`.
    pub picker: Option<Arc<dyn FilePicker>>,
    /// The agents' transcript and configuration folders.
    pub agent_scopes: AgentScopes,
}

/// Where a process keeps what the imports need from the disk, and the devices and providers it
/// reaches them through.
#[derive(Clone, Default)]
pub struct Backends {
    /// The project folder: the `project` scope of `fs` and the working folder of `process`.
    pub project_dir: PathBuf,
    /// The data folder; `fs` keeps its per-plugin scopes under `plugins/fs/<plugin>/`.
    pub data_dir: PathBuf,
    /// Answers `inference`; `None` keeps the runtime's import, which completes `unsupported`.
    pub inference: Option<Arc<dyn InferenceBackend>>,
    /// Lists devices and plays for `audio`.
    pub audio: Option<Arc<dyn AudioBackend>>,
    /// Shows the `fs.pick` picker.
    pub picker: Option<Arc<dyn FilePicker>>,
    /// Read-only `fs` scopes of the agents' folders.
    pub agent_scopes: AgentScopes,
}

impl std::fmt::Debug for Backends {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Backends")
            .field("project_dir", &self.project_dir)
            .field("data_dir", &self.data_dir)
            .field("inference", &self.inference.is_some())
            .field("audio", &self.audio.is_some())
            .field("picker", &self.picker.is_some())
            .field("agent_scopes", &self.agent_scopes)
            .finish()
    }
}

impl Backends {
    /// The backends of a process working in `project_dir` and keeping data in `data_dir`, with
    /// no device or provider.
    pub fn new(project_dir: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            project_dir: project_dir.into(),
            data_dir: data_dir.into(),
            ..Self::default()
        }
    }

    /// The backends of the server.
    pub fn server(
        project_dir: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        inputs: ServerBackendInputs,
    ) -> Self {
        Self {
            inference: inputs.inference,
            agent_scopes: inputs.agent_scopes,
            ..Self::new(project_dir, data_dir)
        }
    }

    /// The backends of the client.
    pub fn client(
        project_dir: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        inputs: ClientBackendInputs,
    ) -> Self {
        Self {
            inference: inputs.inference,
            audio: inputs.audio,
            picker: inputs.picker,
            agent_scopes: inputs.agent_scopes,
            ..Self::new(project_dir, data_dir)
        }
    }

    /// `<data dir>/plugins/fs/<plugin>`, the root of the three private `fs` scopes of a plugin.
    pub fn fs_root(&self, plugin: &str) -> PathBuf {
        self.data_dir.join("plugins").join("fs").join(plugin)
    }
}

/// Builds the `process`, `fs`, `http`, `inference` and `audio` imports of one instance and puts
/// them in `set`. `process` runs on the server only. An `fs` scope folder that cannot be created
/// leaves `fs` out, so the instance still starts and a call reports the interface as unavailable.
pub fn equip(backends: &Backends, context: &InstanceContext, set: &mut ImportSet) {
    let env = ImportEnv::for_instance(context);
    set.http = Some(Box::new(HttpImport::new(env.clone())));
    if let Some(backend) = &backends.inference {
        set.inference = Some(Box::new(InferenceImport::new(
            env.clone(),
            Some(Arc::clone(backend)),
        )));
    }
    if let Some(backend) = &backends.audio {
        set.audio = Some(Box::new(AudioImport::new(
            env.clone(),
            Some(Arc::clone(backend)),
        )));
    }
    let has_project = !backends.project_dir.as_os_str().is_empty();
    if context.side == Side::Server {
        let mut process = ProcessImport::new(env.clone());
        if has_project {
            process.set_scope_root("project", backends.project_dir.clone());
        }
        set.process = Some(Box::new(process));
    }
    let root = backends.fs_root(&context.plugin);
    let mut fs = FsImport::new(env);
    if let Some(picker) = &backends.picker {
        fs.set_file_picker(Arc::clone(picker));
    }
    for (scope, scope_root) in &backends.agent_scopes {
        // An agent that is not installed has no folder; its scope stays closed.
        let _ = fs.configure_read_only_scope(scope, scope_root);
    }
    if fs
        .configure_scope_roots(root.join("package"), root.join("data"), root.join("cache"))
        .is_err()
    {
        return;
    }
    if has_project {
        let grants: Vec<Grant> = context
            .granted
            .iter()
            .filter_map(|grant| Grant::parse(grant).ok())
            .collect();
        if fs
            .configure_project_scope(backends.project_dir.clone(), Arc::new(Mutex::new(grants)))
            .is_err()
        {
            return;
        }
    }
    set.fs = Some(Box::new(fs));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::env::{RequestIds, SessionCounters};
    use crate::runtime::pending_events::PendingEvents;
    use ilium_plugin_api::Tier;
    use std::time::Instant;

    fn context(side: Side, granted: &[&str]) -> InstanceContext {
        InstanceContext {
            plugin: "test.equip".to_owned(),
            side,
            tier: Tier::T1,
            instance: 1,
            granted: granted.iter().map(|grant| (*grant).to_owned()).collect(),
            results: Arc::new(Mutex::new(Default::default())),
            events: PendingEvents::new(),
            ids: RequestIds::new(),
            session: SessionCounters::new(),
            clock_origin: Instant::now(),
        }
    }

    #[test]
    fn the_server_gets_process_fs_and_http_and_the_client_no_process() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let backends = Backends::new(scratch.path().join("project"), scratch.path().join("data"));
        std::fs::create_dir_all(&backends.project_dir).expect("project folder");
        let mut server_set = ImportSet::default();
        equip(&backends, &context(Side::Server, &[]), &mut server_set);
        assert!(server_set.process.is_some());
        assert!(server_set.fs.is_some());
        assert!(server_set.http.is_some());
        let mut client_set = ImportSet::default();
        equip(&backends, &context(Side::Client, &[]), &mut client_set);
        assert!(client_set.process.is_none());
        assert!(client_set.fs.is_some());
        assert!(client_set.http.is_some());
    }

    #[derive(Default)]
    struct EchoInference;

    impl InferenceBackend for EchoInference {
        fn complete(
            &self,
            request: &ilium_plugin_api::generated::host::inference::CompletionRequest,
            _cancel: &crate::imports::env::CancelFlag,
        ) -> Result<
            ilium_plugin_api::generated::host::inference::Completion,
            ilium_plugin_api::generated::types::errors::PluginError,
        > {
            Ok(ilium_plugin_api::generated::host::inference::Completion {
                text: request.purpose.clone(),
                model: "echo".to_owned(),
                input_tokens: 0,
                output_tokens: 0,
            })
        }

        fn models(
            &self,
            _cancel: &crate::imports::env::CancelFlag,
        ) -> Result<Vec<String>, ilium_plugin_api::generated::types::errors::PluginError> {
            Ok(vec!["echo".to_owned()])
        }
    }

    #[test]
    fn an_equipped_inference_backend_answers_the_instance() {
        use ilium_plugin_api::generated::host::inference::{
            ChatMessage, CompletionRequest, HostInference,
        };
        use ilium_plugin_api::generated::types::events::Event;
        use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
        let scratch = tempfile::tempdir().expect("scratch folder");
        let backends = Backends::server(
            scratch.path().join("project"),
            scratch.path().join("data"),
            ServerBackendInputs {
                inference: Some(Arc::new(EchoInference)),
                ..ServerBackendInputs::default()
            },
        );
        let context = context(Side::Server, &["inference"]);
        let mut set = ImportSet::default();
        equip(&backends, &context, &mut set);
        let inference = set.inference.as_mut().expect("inference equipped");
        let request = inference
            .complete(CompletionRequest {
                messages: vec![ChatMessage {
                    role: "user".to_owned(),
                    content: "hello".to_owned(),
                }],
                max_output_tokens: None,
                json_schema: None,
                purpose: "equip-test".to_owned(),
            })
            .expect("accepted");
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let outcome = loop {
            let found = context
                .events
                .drain()
                .into_iter()
                .find_map(|queued| match queued.event {
                    Event::RequestCompleted(result)
                        if result.request == RequestRef::Call(request) =>
                    {
                        Some(result.outcome)
                    }
                    _ => None,
                });
            if let Some(outcome) = found {
                break outcome;
            }
            assert!(Instant::now() < deadline, "no completion");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(outcome, Outcome::Ok);
        let completion = inference.take_completion(request).expect("answer");
        assert_eq!(completion.text, "equip-test");
    }

    #[test]
    fn agent_scopes_open_on_their_grant_through_equip() {
        use ilium_plugin_api::generated::host::fs::HostFs;
        let scratch = tempfile::tempdir().expect("scratch folder");
        let home = scratch.path().join("home");
        std::fs::create_dir_all(home.join(".claude").join("projects")).expect("claude folder");
        let backends = Backends::client(
            scratch.path().join("project"),
            scratch.path().join("data"),
            ClientBackendInputs {
                agent_scopes: agent_scopes_in(&home),
                ..ClientBackendInputs::default()
            },
        );
        let mut set = ImportSet::default();
        equip(
            &backends,
            &context(Side::Client, &["fs.read:agent-sessions:claude"]),
            &mut set,
        );
        let roots = set.fs.as_mut().expect("fs equipped").roots();
        assert!(roots
            .iter()
            .any(|root| root.scope == "agent-sessions:claude"));
        // Codex is not installed in this home folder, so its scope stays closed.
        assert!(roots
            .iter()
            .all(|root| root.scope != "agent-sessions:codex"));
    }

    #[test]
    fn each_plugin_gets_its_own_scope_folders() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let backends = Backends::new(scratch.path().join("project"), scratch.path().join("data"));
        assert_ne!(backends.fs_root("a.plugin"), backends.fs_root("b.plugin"));
        let mut set = ImportSet::default();
        equip(&backends, &context(Side::Server, &[]), &mut set);
        assert!(backends.fs_root("test.equip").join("data").is_dir());
    }
}
