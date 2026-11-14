//! Equips the per-instance imports that reach the machine: `process`, `fs`, `http`,
//! `inference`, `vector`, `audio`, `clipboard` and `media`. `jobs` is equipped by the runtime itself, because a job needs the
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
use crate::imports::clipboard::{ClipboardBackend, ClipboardImport};
use crate::imports::env::ImportEnv;
use crate::imports::fs::{FilePicker, FsImport};
use crate::imports::generated_dispatch::ImportSet;
use crate::imports::http::HttpImport;
use crate::imports::inference::{InferenceBackend, InferenceImport};
use crate::imports::media::{MediaBackend, MediaImport, VideoFiles};
use crate::imports::process::ProcessImport;
use crate::imports::vector::VectorImport;
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
    /// Reads and writes the desktop clipboard for `ilium:host/clipboard`.
    pub clipboard: Option<Arc<dyn ClipboardBackend>>,
    /// Decodes video for `ilium:host/media` (client and scene instances).
    pub media: Option<Arc<dyn MediaBackend>>,
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
    /// The desktop clipboard; `None` leaves `clipboard` unlinked.
    pub clipboard: Option<Arc<dyn ClipboardBackend>>,
    /// The video decoder; `None` leaves `media` unlinked.
    pub media: Option<Arc<dyn MediaBackend>>,
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
            .field("clipboard", &self.clipboard.is_some())
            .field("media", &self.media.is_some())
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
            clipboard: inputs.clipboard,
            media: inputs.media,
            agent_scopes: inputs.agent_scopes,
            ..Self::new(project_dir, data_dir)
        }
    }

    /// `<data dir>/plugins/fs/<plugin>`, the root of the three private `fs` scopes of a plugin.
    pub fn fs_root(&self, plugin: &str) -> PathBuf {
        self.data_dir.join("plugins").join("fs").join(plugin)
    }

    /// `<data dir>/plugins/media-spool/<plugin>`, where `open-video-bytes` writes a clip for the
    /// decoder. Host-private: no `fs` scope reaches it.
    pub fn media_spool(&self, plugin: &str) -> PathBuf {
        self.data_dir
            .join("plugins")
            .join("media-spool")
            .join(plugin)
    }

    /// `<data dir>/plugins/vector/<plugin>/<side>`, where the `vector` indexes of one side of a
    /// plugin persist. Each side numbers its indexes on its own, so the sides keep apart folders.
    pub fn vector_root(&self, plugin: &str, side: Side) -> PathBuf {
        self.data_dir
            .join("plugins")
            .join("vector")
            .join(plugin)
            .join(side.as_str())
    }
}

/// Builds the `process`, `fs`, `http`, `inference`, `vector`, `audio`, `clipboard` and `media`
/// imports of one instance and puts them in `set`. `clipboard` is equipped on the client only and
/// `media` on the client and scene sides; `media` opens files with the instance's `fs` rules, so
/// it is equipped only together with `fs`. `vector` embeds through the inference backend and keeps its indexes
/// under [`Backends::vector_root`]; without a data folder the indexes live in memory only.
/// `process` runs on the server only. An `fs` scope folder that cannot be created leaves `fs`
/// out, so the instance still starts and a call reports the interface as unavailable.
pub fn equip(backends: &Backends, context: &InstanceContext, set: &mut ImportSet) {
    let env = ImportEnv::for_instance(context);
    set.http = Some(Box::new(HttpImport::new(env.clone())));
    if let Some(backend) = &backends.inference {
        set.inference = Some(Box::new(InferenceImport::new(
            env.clone(),
            Some(Arc::clone(backend)),
        )));
    }
    let vector_store = (!backends.data_dir.as_os_str().is_empty())
        .then(|| backends.vector_root(&context.plugin, context.side));
    set.vector = Some(Box::new(VectorImport::new(
        env.clone(),
        backends.inference.clone(),
        vector_store,
    )));
    if let Some(backend) = &backends.audio {
        set.audio = Some(Box::new(AudioImport::new(
            env.clone(),
            Some(Arc::clone(backend)),
        )));
    }
    if context.side == Side::Client {
        if let Some(backend) = &backends.clipboard {
            set.clipboard = Some(Box::new(ClipboardImport::new(
                env.clone(),
                Arc::clone(backend),
            )));
        }
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
    let mut fs = FsImport::new(env.clone());
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
    if context.side != Side::Server {
        if let (Some(backend), Ok(resolver)) = (&backends.media, fs.read_resolver()) {
            let spool = (!backends.data_dir.as_os_str().is_empty())
                .then(|| backends.media_spool(&context.plugin));
            set.media = Some(Box::new(MediaImport::new(
                env,
                Some(Arc::new(resolver) as Arc<dyn VideoFiles>),
                Some(Arc::clone(backend)),
                spool,
            )));
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

    /// Returns one vector per text whose single component is the text length.
    struct LengthEmbeddings;

    impl InferenceBackend for LengthEmbeddings {
        fn complete(
            &self,
            _request: &ilium_plugin_api::generated::host::inference::CompletionRequest,
            _cancel: &crate::imports::env::CancelFlag,
        ) -> Result<
            ilium_plugin_api::generated::host::inference::Completion,
            ilium_plugin_api::generated::types::errors::PluginError,
        > {
            Err(
                ilium_plugin_api::generated::types::errors::PluginError::new(
                    ilium_plugin_api::generated::types::errors::ErrorCode::Unsupported,
                    "embeddings only",
                ),
            )
        }

        fn models(
            &self,
            _cancel: &crate::imports::env::CancelFlag,
        ) -> Result<Vec<String>, ilium_plugin_api::generated::types::errors::PluginError> {
            Ok(Vec::new())
        }

        fn embed(
            &self,
            texts: &[String],
            _purpose: &str,
            _cancel: &crate::imports::env::CancelFlag,
        ) -> Result<Vec<Vec<f32>>, ilium_plugin_api::generated::types::errors::PluginError>
        {
            Ok(texts.iter().map(|text| vec![text.len() as f32]).collect())
        }
    }

    #[test]
    fn an_equipped_vector_import_embeds_through_the_provider_and_persists_its_indexes() {
        use ilium_plugin_api::generated::host::vector::HostVector;
        use ilium_plugin_api::generated::types::events::Event;
        use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
        let scratch = tempfile::tempdir().expect("scratch folder");
        let backends = Backends::server(
            scratch.path().join("project"),
            scratch.path().join("data"),
            ServerBackendInputs {
                inference: Some(Arc::new(LengthEmbeddings)),
                ..ServerBackendInputs::default()
            },
        );
        let context = context(Side::Server, &["inference.embed"]);
        let mut set = ImportSet::default();
        equip(&backends, &context, &mut set);
        let vector = set.vector.as_mut().expect("vector equipped");
        let request = vector
            .embed(
                vec!["abc".to_owned(), "hello".to_owned()],
                "equip-test".to_owned(),
            )
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
        assert_eq!(
            vector.take_vectors(request).expect("vectors"),
            vec![vec![3.0], vec![5.0]]
        );
        vector
            .create_index("notes".to_owned(), 1)
            .expect("index created");
        assert!(
            backends.vector_root("test.equip", Side::Server).is_dir(),
            "the index is written under the plugin's vector folder"
        );
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

    /// A decoder that knows one size and returns one grey frame per run.
    struct OneFrameDecoder;

    struct OneFrame(Option<Vec<u8>>);

    impl crate::imports::media::FrameSource for OneFrame {
        fn next_frame(
            &mut self,
        ) -> Result<Option<Vec<u8>>, ilium_plugin_api::generated::types::errors::PluginError>
        {
            Ok(self.0.take())
        }
    }

    impl MediaBackend for OneFrameDecoder {
        fn probe(
            &self,
            _file: &Path,
        ) -> Result<
            ilium_plugin_api::generated::host::media::VideoInfo,
            ilium_plugin_api::generated::types::errors::PluginError,
        > {
            Ok(ilium_plugin_api::generated::host::media::VideoInfo {
                width: 32,
                height: 18,
                frames_per_second: 30.0,
                duration_millis: Some(1_000),
            })
        }

        fn decode(
            &self,
            request: &crate::imports::media::DecodeRequest,
        ) -> Result<
            Box<dyn crate::imports::media::FrameSource>,
            ilium_plugin_api::generated::types::errors::PluginError,
        > {
            let bytes = (request.width * request.height * 3) as usize;
            Ok(Box::new(OneFrame(Some(vec![128; bytes]))))
        }
    }

    fn client_backends(scratch: &Path) -> Backends {
        let backends = Backends::client(
            scratch.join("project"),
            scratch.join("data"),
            ClientBackendInputs {
                clipboard: Some(Arc::new(
                    crate::imports::clipboard::MemoryClipboard::default(),
                )),
                media: Some(Arc::new(OneFrameDecoder)),
                ..ClientBackendInputs::default()
            },
        );
        std::fs::create_dir_all(&backends.project_dir).expect("project folder");
        backends
    }

    #[test]
    fn the_client_gets_clipboard_and_media_and_the_server_neither() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let backends = client_backends(scratch.path());
        let mut client_set = ImportSet::default();
        equip(&backends, &context(Side::Client, &[]), &mut client_set);
        assert!(client_set.clipboard.is_some());
        assert!(client_set.media.is_some());
        let mut scene_set = ImportSet::default();
        equip(&backends, &context(Side::Scene, &[]), &mut scene_set);
        assert!(scene_set.clipboard.is_none(), "clipboard is client only");
        assert!(scene_set.media.is_some(), "scenes play video too");
        let mut server_set = ImportSet::default();
        equip(&backends, &context(Side::Server, &[]), &mut server_set);
        assert!(server_set.clipboard.is_none());
        assert!(server_set.media.is_none());
    }

    #[test]
    fn an_equipped_clipboard_writes_through_the_backend_on_its_grant() {
        use ilium_plugin_api::generated::host::clipboard::HostClipboard;
        let scratch = tempfile::tempdir().expect("scratch folder");
        let memory = Arc::new(crate::imports::clipboard::MemoryClipboard::default());
        let backends = Backends::client(
            scratch.path().join("project"),
            scratch.path().join("data"),
            ClientBackendInputs {
                clipboard: Some(Arc::clone(&memory) as Arc<dyn ClipboardBackend>),
                ..ClientBackendInputs::default()
            },
        );
        let mut set = ImportSet::default();
        equip(
            &backends,
            &context(Side::Client, &["clipboard.write"]),
            &mut set,
        );
        let clipboard = set.clipboard.as_mut().expect("clipboard equipped");
        clipboard
            .write_text("copied by a plugin".to_owned())
            .expect("written");
        assert_eq!(memory.get().as_deref(), Some("copied by a plugin"));
        // The read grant is not held, so the plugin cannot read it back.
        assert_eq!(clipboard.read_text(), None);
    }

    #[test]
    fn an_equipped_media_import_plays_a_project_file_it_may_read() {
        use ilium_plugin_api::generated::host::media::HostMedia;
        use ilium_plugin_api::generated::types::paths::FsPath;
        let scratch = tempfile::tempdir().expect("scratch folder");
        let backends = client_backends(scratch.path());
        std::fs::write(backends.project_dir.join("intro.mp4"), b"video").expect("clip");
        let clip = FsPath {
            scope: "project".to_owned(),
            path: "intro.mp4".to_owned(),
        };
        let mut set = ImportSet::default();
        equip(
            &backends,
            &context(Side::Client, &["media.video", "fs.read:project"]),
            &mut set,
        );
        let media = set.media.as_mut().expect("media equipped");
        let (video, info) = media.open_video(clip.clone()).expect("opens");
        assert_eq!((info.width, info.height), (32, 18));
        let frame = media
            .next_frame(video, 4, 3)
            .expect("decodes")
            .expect("frame");
        assert_eq!(frame.len(), 4 * 3 * 3);
        // Without `fs.read:project` the same file stays closed.
        let mut unread = ImportSet::default();
        equip(
            &backends,
            &context(Side::Client, &["media.video"]),
            &mut unread,
        );
        let refused = unread
            .media
            .as_mut()
            .expect("media equipped")
            .open_video(clip)
            .expect_err("refused");
        assert_eq!(
            refused.code,
            ilium_plugin_api::generated::types::errors::ErrorCode::NotPermitted
        );
    }
}
