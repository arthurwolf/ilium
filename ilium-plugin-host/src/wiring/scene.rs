//! The imports of plugin scene guests (foundation gap G10).
//!
//! A scene component runs on the ambient worker, outside the runtime thread, so it gets no
//! `HostRuntime` instance. [`SceneImportServices`] builds what the runtime would build for any
//! other instance, per scene instance and from the scene side of the manifest only:
//!
//! - `log`, `clock`, `codec`, `fonts` and `gpu` (CPU kernels), as every guest gets them;
//! - `storage`, `fs`, `media` and `audio` through the same provider the client's own instances
//!   use (`wiring::import_provider`, `wave_two::equip`), so the private `package`, `data` and
//!   `cache` scopes of the plugin, its storage buckets, the ffmpeg-backed video decoder and the
//!   sound devices are the ones the plugin's client side sees. A scene gets no `project` scope,
//!   no picker, no clipboard and no agent folders; the extra read-only scopes are
//!   [`MINECRAFT_SAVES_SCOPE`] and [`MINECRAFT_JAVA_SCOPE`] (`fs.read:minecraft-saves`,
//!   `fs.read:minecraft-java`) and, per instance, one read-only `picked/<n>` scope for each
//!   folder or file of a settings row the scene declared with the `picker` keyword (opened by
//!   `fs.read:picked`; a scene never calls `pick` itself);
//! - `http` that never waits inside a frame (`take-response` reports a running fetch as not
//!   ready), and `jobs`, whose `start` queues a job that [`SceneBackground`] runs in a fresh job
//!   instance of the same component on a worker thread; the job instance gets waiting `http`
//!   (`jobs.wait-request` and blocking fetches belong there, never in `render`);
//! - `location` backed by the observer position of the shared location row, and `geocode`.
//!
//! Every interface the `ilium:anim/base` world does not import is removed, then every interface
//! the scene grants do not unlock (`restrict_to_grants`), and grants whose catalogue row does not
//! allow the scene side are ignored. Each import still checks its grant (host, scope, device) on
//! every call. Dropping the scene drops its imports, which stops its fetches, streams, decoders
//! and captures, and cancels its jobs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::grants::{self, Grant};
use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::{ErrorCode, PluginError, Side, Tier};
use serde_json::Value;

use crate::calls::ResolvedLimits;
use crate::engine::{CompiledComponent, PluginEngine};
use crate::imports::audio::AudioBackend;
use crate::imports::backed::{LogSink, NullLogSink, ResultQueue, SinkLog};
use crate::imports::clock::ClockImport;
use crate::imports::codec::CodecImport;
use crate::imports::env::{ImportEnv, RequestIds, SessionCounters};
use crate::imports::fonts::FontsImport;
use crate::imports::generated_dispatch::{ImportSet, IMPORT_INTERFACES};
use crate::imports::geocode::{GeocodeBackend, GeocodeImport};
use crate::imports::gpu::GpuImport;
use crate::imports::http::HttpImport;
use crate::imports::jobs::{JobBook, JobsImport, QueuedJob};
use crate::imports::location::ObserverLocation;
use crate::imports::media::MediaBackend;
use crate::instance::{ExportCall, ExportReply, PluginInstance};
use crate::runtime::pending_events::PendingEvents;
use crate::runtime::{declared_jobs, job_error_of, restrict_to_grants, InstanceContext};
use crate::scene_guest::{scene_linker, SceneSurface};
use crate::storage::StorageContext;
use crate::store_data::GuestIdentity;
use crate::wasm_guest::WasmGuest;

use super::wave_two::{AgentScopes, Backends};
use super::{import_provider, storage_root, ProcessImports};

/// The read-only `fs` scope of the Minecraft Java saves folder (`fs.read:minecraft-saves`), which
/// the saved-world scenes (Minecraft maps, voxel landscape) read region files from.
pub const MINECRAFT_SAVES_SCOPE: &str = "minecraft-saves";

/// The read-only `fs` scope of the Minecraft Java folder itself (`fs.read:minecraft-java`): the
/// installed version archives and resource packs the voxel landscape draws its textures from.
pub const MINECRAFT_JAVA_SCOPE: &str = "minecraft-java";

/// The interfaces the `ilium:anim/base` world imports (reference-wit.md section 7); a scene
/// instance is never equipped with any other.
pub const SCENE_IMPORTS: [&str; 15] = [
    "ilium:host/log@0.1.0",
    "ilium:host/telemetry@0.1.0",
    "ilium:host/clock@0.1.0",
    "ilium:host/storage@0.1.0",
    "ilium:host/codec@0.1.0",
    "ilium:host/fs@0.1.0",
    "ilium:host/http@0.1.0",
    "ilium:host/jobs@0.1.0",
    "ilium:host/audio@0.1.0",
    "ilium:host/media@0.1.0",
    "ilium:host/location@0.1.0",
    "ilium:host/geocode@0.1.0",
    "ilium:host/gpu@0.1.0",
    "ilium:anim/surface@0.1.0",
    "ilium:anim/fonts@0.1.0",
];

/// The devices and folders the client lends its scenes.
#[derive(Default)]
pub struct SceneBackendInputs {
    /// Sound devices for `audio` (capture, loopback, playback).
    pub audio: Option<Arc<dyn AudioBackend>>,
    /// The video decoder for `media`.
    pub media: Option<Arc<dyn MediaBackend>>,
    /// The Minecraft Java `saves` folder, the root of [`MINECRAFT_SAVES_SCOPE`]; `None` (or a
    /// folder that does not exist) leaves the scope closed.
    pub minecraft_saves: Option<PathBuf>,
    /// The Minecraft Java folder, the root of [`MINECRAFT_JAVA_SCOPE`]; `None` leaves it closed.
    pub minecraft_java: Option<PathBuf>,
    /// The place and address search behind `geocode`; `None` answers every request with no
    /// place.
    pub geocode: Option<Arc<dyn GeocodeBackend>>,
}

/// What every scene instance of one client is equipped from. Cloning shares the id allocator
/// and instance numbering.
#[derive(Clone)]
pub struct SceneImportServices {
    /// Storage, `fs`, `media` and `audio`; `None` keeps scenes off the disk (tests, and a client
    /// without a data folder).
    disk: Option<ProcessImports>,
    log: Arc<dyn LogSink>,
    clock_origin: Instant,
    ids: RequestIds,
    session: SessionCounters,
    next_instance: Arc<AtomicU64>,
    /// The provider of `geocode`, shared by every instance (each keeps its own requests).
    geocode: Option<Arc<dyn GeocodeBackend>>,
}

impl std::fmt::Debug for SceneImportServices {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SceneImportServices")
            .field("disk", &self.disk)
            .finish_non_exhaustive()
    }
}

impl SceneImportServices {
    /// The services of a client keeping plugin data in `data_dir`; storage buckets are scoped to
    /// `scope`, and storage refusals and guest output go to `log`.
    pub fn new(
        data_dir: &Path,
        scope: StorageContext,
        log: Arc<dyn LogSink>,
        inputs: SceneBackendInputs,
    ) -> Self {
        let mut read_only = AgentScopes::new();
        if let Some(saves) = inputs.minecraft_saves {
            read_only.insert(MINECRAFT_SAVES_SCOPE.to_owned(), saves);
        }
        if let Some(java) = inputs.minecraft_java {
            read_only.insert(MINECRAFT_JAVA_SCOPE.to_owned(), java);
        }
        let geocode = inputs.geocode;
        let backends = Backends {
            audio: inputs.audio,
            media: inputs.media,
            agent_scopes: read_only,
            // No project folder: a scene never reaches the project scope.
            ..Backends::new(PathBuf::new(), data_dir)
        };
        let disk = ProcessImports {
            storage_root: storage_root(data_dir),
            scope,
            basic: None,
            log: Arc::clone(&log),
            backends,
        };
        Self {
            disk: Some(disk),
            geocode,
            ..Self::without_disk(log)
        }
    }

    /// Services without storage, `fs`, `media` or `audio`: those calls trap as unavailable.
    pub fn without_disk(log: Arc<dyn LogSink>) -> Self {
        Self {
            disk: None,
            log,
            clock_origin: Instant::now(),
            ids: RequestIds::new(),
            session: SessionCounters::new(),
            next_instance: Arc::new(AtomicU64::new(1)),
            geocode: None,
        }
    }

    /// The imports, identity and background work of one new scene instance.
    pub fn equip(
        &self,
        request: &SceneInstanceRequest<'_>,
        engine: &Arc<PluginEngine>,
        compiled: &CompiledComponent,
        limits: &ResolvedLimits,
    ) -> SceneImports {
        let granted = scene_grants(request.granted);
        let tier = match request.manifest.get("limits_tier").and_then(Value::as_str) {
            Some("first-party") => Tier::T1,
            _ => Tier::T2,
        };
        let context = InstanceContext {
            plugin: request.plugin.to_owned(),
            side: Side::Scene,
            tier,
            instance: self.next_instance.fetch_add(1, Ordering::Relaxed),
            granted: granted.clone(),
            results: ResultQueue::default(),
            events: PendingEvents::new(),
            ids: self.ids.clone(),
            session: self.session.clone(),
            clock_origin: self.clock_origin,
        };
        let env = ImportEnv::for_instance(&context);
        let book = JobBook::new();
        let lent_picks = lent_picks(request.picked);
        let setting_values = request.setting_values.to_vec();
        let mut imports = self.base_set(&context, request.observer, &lent_picks, &setting_values);
        // A scene draws inside a render frame: its fetches never block it.
        imports.http = Some(Box::new(
            HttpImport::new(env.clone())
                .with_setting_hosts(&setting_values)
                .without_waiting(),
        ));
        imports.jobs = Some(Box::new(JobsImport::new(
            env,
            book.clone(),
            declared_jobs(request.manifest),
        )));
        restrict_to_grants(&mut imports, &granted);
        SceneImports {
            imports,
            identity: GuestIdentity {
                plugin: request.plugin.to_owned(),
                side: Side::Scene,
                granted,
            },
            background: SceneBackground {
                services: self.clone(),
                context,
                book,
                observer: request.observer,
                lent_picks,
                setting_values,
                engine: Arc::clone(engine),
                compiled: compiled.clone(),
                limits: limits.clone(),
                workers: Mutex::new(Vec::new()),
            },
        }
    }

    /// Everything but `jobs` (and the frame-bound `http` of the render instance); restricted to
    /// the interfaces of the scene world, not yet to the grants. `lent_picks` are this
    /// instance's read-only `picked/<n>` scopes and their roots; `setting_values` are the
    /// `(key, value)` pairs whose hosts its `http:setting:<key>` grants lend.
    fn base_set(
        &self,
        context: &InstanceContext,
        observer: Option<(f64, f64)>,
        lent_picks: &AgentScopes,
        setting_values: &[(String, String)],
    ) -> ImportSet {
        let env = ImportEnv::for_instance(context);
        let mut set = ImportSet {
            log: Some(Box::new(SinkLog::new(
                Arc::clone(&self.log),
                &context.plugin,
                Side::Scene,
            ))),
            clock: Some(Box::new(ClockImport::with_origin(self.clock_origin))),
            codec: Some(Box::new(CodecImport)),
            fonts: Some(Box::new(FontsImport::default())),
            gpu: Some(Box::new(GpuImport)),
            geocode: Some(Box::new(match &self.geocode {
                Some(backend) => GeocodeImport::with_backend(Arc::clone(backend)),
                None => GeocodeImport::default(),
            })),
            location: Some(Box::new(ObserverLocation::new(env.clone(), observer))),
            http: Some(Box::new(
                HttpImport::new(env).with_setting_hosts(setting_values),
            )),
            ..ImportSet::default()
        };
        if let Some(disk) = &self.disk {
            let mut disk = disk.clone();
            // A folder or file that does not exist (any more) leaves its scope closed.
            disk.backends.agent_scopes.extend(
                lent_picks
                    .iter()
                    .map(|(scope, root)| (scope.clone(), root.clone())),
            );
            let provider = import_provider(disk);
            provider(context, &mut set);
        }
        keep_scene_interfaces(&mut set);
        set
    }
}

impl Default for SceneImportServices {
    fn default() -> Self {
        Self::without_disk(Arc::new(NullLogSink))
    }
}

/// What a scene instance is built for.
#[derive(Debug, Clone, Copy)]
pub struct SceneInstanceRequest<'a> {
    /// The plugin id.
    pub plugin: &'a str,
    /// The grants of the scene side of the manifest (`capabilities.scene`).
    pub granted: &'a [String],
    /// The whole manifest, for `contributes.jobs` and `limits_tier`.
    pub manifest: &'a Value,
    /// The observer position (latitude, longitude) of the shared location row.
    pub observer: Option<(f64, f64)>,
    /// The folders and files of the scene's `picker` settings rows, in row order: path `n` is
    /// lent as the read-only scope `picked/<n>`, opened by `fs.read:picked`.
    pub picked: &'a [PathBuf],
    /// The `(key, value)` text settings of the scene: the hosts in the value of `key` are
    /// reachable when the scene holds `http:setting:<key>` (api-http-streams "Hosts from a
    /// setting"). Values of other keys lend nothing.
    pub setting_values: &'a [(String, String)],
}

/// The read-only `picked/<n>` scopes of one instance. A path that is not absolute is left out,
/// and the others keep their row index.
fn lent_picks(picked: &[PathBuf]) -> AgentScopes {
    picked
        .iter()
        .enumerate()
        .filter(|(_, root)| root.is_absolute())
        .map(|(index, root)| (format!("picked/{index}"), root.clone()))
        .collect()
}

/// The equipment of one scene instance.
pub struct SceneImports {
    /// The imports of the render instance.
    pub imports: ImportSet,
    /// The identity whose grants unlock the imports.
    pub identity: GuestIdentity,
    /// The jobs and event queue; hand it to `SceneGuest::with_background`.
    pub background: SceneBackground,
}

/// The grants a scene may hold: those whose catalogue row allows the scene side. Unknown or
/// malformed grants are dropped.
pub fn scene_grants(granted: &[String]) -> Vec<String> {
    granted
        .iter()
        .filter(|grant| {
            Grant::parse(grant).is_ok_and(|parsed| {
                grants::info(parsed.family()).is_some_and(|row| row.sides.contains(&Side::Scene))
            })
        })
        .cloned()
        .collect()
}

fn keep_scene_interfaces(set: &mut ImportSet) {
    for interface in IMPORT_INTERFACES {
        if !SCENE_IMPORTS.contains(&interface.instance) {
            set.remove(interface.instance);
        }
    }
}

/// The background work of one scene instance: its job book, the event queue its imports write
/// to, and what a job instance is built from. Dropping it cancels every job of the scene.
pub struct SceneBackground {
    services: SceneImportServices,
    context: InstanceContext,
    book: JobBook,
    observer: Option<(f64, f64)>,
    /// The instance's lent `picked/<n>` scopes, lent to its job instances too.
    lent_picks: AgentScopes,
    /// The setting values whose hosts `http:setting:<key>` lends, to its job instances too.
    setting_values: Vec<(String, String)>,
    engine: Arc<PluginEngine>,
    compiled: CompiledComponent,
    limits: ResolvedLimits,
    /// The job threads still running. Each ends with its job: the guest polls `is-cancelled`,
    /// and the job deadline interrupts one that does not.
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl std::fmt::Debug for SceneBackground {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SceneBackground")
            .field("plugin", &self.context.plugin)
            .field("instance", &self.context.instance)
            .finish_non_exhaustive()
    }
}

impl SceneBackground {
    /// The plugin id.
    pub fn plugin(&self) -> &str {
        &self.context.plugin
    }

    /// The grants the scene instance holds.
    pub fn granted(&self) -> &[String] {
        &self.context.granted
    }

    /// How many job threads of the scene are still running.
    pub fn running_jobs(&self) -> usize {
        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        workers.retain(|worker| !worker.is_finished());
        workers.len()
    }

    /// Starts every job the scene queued; returns how many started. A job that cannot start
    /// ends at once with the reason, which the scene reads through `jobs.wait-request` or the
    /// `request-completed` event.
    pub fn start_queued_jobs(&self) -> usize {
        let mut started = 0;
        for job in self.book.take_queued() {
            match self.launch_job(&job) {
                Ok(()) => started += 1,
                Err(failure) => {
                    let sink = self.context.events.sink(&job.plugin, job.instance);
                    self.book.finish(job.request, Err(failure), &sink);
                }
            }
        }
        started
    }

    /// Takes what the scene's imports queued since the last call: request completions, job
    /// progress, audio frames, stream chunks.
    pub fn take_events(&self) -> Vec<Event> {
        let mut events: Vec<Event> = {
            let mut results = self
                .context
                .results
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            results
                .drain(..)
                .filter(|(plugin, _)| *plugin == self.context.plugin)
                .map(|(_, result)| Event::RequestCompleted(result))
                .collect()
        };
        events.extend(
            self.context
                .events
                .drain()
                .into_iter()
                .filter(|queued| {
                    queued.plugin == self.context.plugin && queued.instance == self.context.instance
                })
                .map(|queued| queued.event),
        );
        events
    }

    fn launch_job(&self, job: &QueuedJob) -> Result<(), PluginError> {
        let failure = |code: ErrorCode, message: String| PluginError {
            code,
            message,
            retry_after_ms: None,
        };
        let mut limits = self.limits.clone();
        limits.0.insert(LimitKey::JobDeadlineMs, job.deadline_ms);
        // The job's own cap, never above what the tier allows jobs.
        let memory_mib = limits
            .0
            .get(&LimitKey::JobMemoryMib)
            .map_or(job.memory_mib, |ceiling| (*ceiling).min(job.memory_mib));
        limits.0.insert(LimitKey::MemoryMib, memory_mib);
        let mut set = self
            .services
            .base_set(
                &self.context,
                self.observer,
                &self.lent_picks,
                &self.setting_values,
            );
        set.jobs = Some(Box::new(JobsImport::for_job(
            ImportEnv::for_instance(&self.context),
            self.book.clone(),
            job.request,
        )));
        restrict_to_grants(&mut set, &self.context.granted);
        if set.jobs.is_none() {
            return Err(failure(
                ErrorCode::NotPermitted,
                "the scene holds no jobs.run grant".to_owned(),
            ));
        }
        // A job instance draws nothing; its surface is a buffer nobody reads.
        let surface = Arc::new(Mutex::new(SceneSurface::default()));
        let linker = scene_linker(&self.engine, &self.compiled, &surface)
            .map_err(|error| failure(ErrorCode::Internal, error.to_string()))?;
        let guest = WasmGuest::instantiate_linked(
            Arc::clone(&self.engine),
            &self.compiled,
            limits,
            &linker,
            set,
            GuestIdentity {
                plugin: self.context.plugin.clone(),
                side: Side::Scene,
                granted: self.context.granted.clone(),
            },
        )
        .map_err(|error| failure(ErrorCode::Internal, error.to_string()))?;
        let mut instance = PluginInstance::for_job(Box::new(guest));
        let sink = self.context.events.sink(&job.plugin, job.instance);
        let book = self.book.clone();
        let (request, kind, input) = (job.request, job.kind.clone(), job.input.clone());
        // Counted as a running worker from before the thread starts until the job ends.
        let guard = sink.worker();
        let worker = std::thread::Builder::new()
            .name(format!("scene-job-{}", request.0))
            .spawn(move || {
                let _guard = guard;
                let result = match instance.call_export(ExportCall::Job { kind, input }) {
                    Ok(ExportReply::Job(result)) => result,
                    Ok(_) => Err(PluginError {
                        code: ErrorCode::Internal,
                        message: "the job instance answered with another reply".to_owned(),
                        retry_after_ms: None,
                    }),
                    Err(trap) => Err(job_error_of(&trap)),
                };
                book.finish(request, result, &sink);
            })
            .map_err(|error| failure(ErrorCode::Internal, error.to_string()))?;
        let mut workers = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        workers.retain(|running| !running.is_finished());
        workers.push(worker);
        Ok(())
    }
}

impl Drop for SceneBackground {
    fn drop(&mut self) {
        // Cancels every job of this scene instance (the book is the instance's own). The threads
        // are not joined, so the render thread never waits for a job; each ends within its
        // deadline.
        self.book.forget_plugin(&self.context.plugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_api::generated::host::fs::{HostFs, WriteMode};
    use ilium_plugin_api::generated::host::location::HostLocation;
    use ilium_plugin_api::generated::types::paths::FsPath;

    /// The smallest component binary: the preamble of an empty component.
    const EMPTY_COMPONENT: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];

    fn path(scope: &str, path: &str) -> FsPath {
        FsPath {
            scope: scope.to_owned(),
            path: path.to_owned(),
        }
    }

    fn engine() -> Arc<PluginEngine> {
        Arc::new(
            PluginEngine::new(crate::engine::EngineConfig {
                epoch_tick_ms: 10,
                cache_dir: None,
                process: ilium_plugin_api::ProcessKind::Client,
            })
            .expect("real engine"),
        )
    }

    fn equip(
        services: &SceneImportServices,
        plugin: &str,
        granted: &[&str],
        manifest: &Value,
        observer: Option<(f64, f64)>,
    ) -> SceneImports {
        let engine = engine();
        let compiled = engine.compile(&EMPTY_COMPONENT).expect("component");
        let granted: Vec<String> = granted.iter().map(|grant| (*grant).to_owned()).collect();
        services.equip(
            &SceneInstanceRequest {
                plugin,
                granted: &granted,
                manifest,
                observer,
                picked: &[],
                setting_values: &[],
            },
            &engine,
            &compiled,
            &ResolvedLimits(Default::default()),
        )
    }

    fn linked(set: &ImportSet) -> Vec<&'static str> {
        set.linked()
    }

    #[test]
    fn grants_outside_the_scene_side_are_ignored() {
        let granted = [
            "http:api.example.org".to_owned(),
            "process.run:ffmpeg".to_owned(),
            "fs.pick".to_owned(),
            "fs.read:minecraft-saves".to_owned(),
            "audio.loopback".to_owned(),
            "not a grant".to_owned(),
        ];
        assert_eq!(
            scene_grants(&granted),
            vec![
                "http:api.example.org".to_owned(),
                "fs.read:minecraft-saves".to_owned(),
                "audio.loopback".to_owned(),
            ]
        );
    }

    #[test]
    fn a_scene_without_grants_gets_only_the_ungated_interfaces_of_its_world() {
        let services = SceneImportServices::default();
        let equipped = equip(&services, "test.scene", &[], &Value::Null, None);
        let names = linked(&equipped.imports);
        for interface in &names {
            assert!(
                SCENE_IMPORTS.contains(interface),
                "{interface} is not a scene import"
            );
        }
        for gated in [
            "ilium:host/http@0.1.0",
            "ilium:host/jobs@0.1.0",
            "ilium:host/location@0.1.0",
            "ilium:host/geocode@0.1.0",
            "ilium:host/gpu@0.1.0",
        ] {
            assert!(!names.contains(&gated), "{gated} linked without its grant");
        }
        for always in [
            "ilium:host/log@0.1.0",
            "ilium:host/clock@0.1.0",
            "ilium:host/codec@0.1.0",
            "ilium:anim/fonts@0.1.0",
        ] {
            assert!(names.contains(&always), "{always} missing");
        }
        assert_eq!(equipped.identity.side, Side::Scene);
    }

    #[test]
    fn granted_interfaces_are_linked_and_never_the_ones_of_other_worlds() {
        let services = SceneImportServices::default();
        let equipped = equip(
            &services,
            "test.scene",
            &[
                "http:api.example.org",
                "jobs.run",
                "location.observer",
                "location.geocode",
                // Not a scene grant: neither the grant nor its interface reaches the scene.
                "process.run:ffmpeg",
            ],
            &Value::Null,
            Some((52.37, 4.89)),
        );
        let names = linked(&equipped.imports);
        for wanted in [
            "ilium:host/http@0.1.0",
            "ilium:host/jobs@0.1.0",
            "ilium:host/location@0.1.0",
            "ilium:host/geocode@0.1.0",
        ] {
            assert!(names.contains(&wanted), "{wanted} missing");
        }
        assert!(!names.contains(&"ilium:host/process@0.1.0"));
        assert!(!names.contains(&"ilium:host/vector@0.1.0"));
        assert!(!equipped
            .identity
            .granted
            .iter()
            .any(|grant| grant.starts_with("process.run")));
        let mut imports = equipped.imports;
        let location = imports.location.as_mut().expect("location linked");
        assert_eq!(location.current(), Some((52.37, 4.89)));
    }

    #[test]
    fn scenes_on_disk_get_private_scopes_apart_from_other_plugins() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let services = SceneImportServices::new(
            scratch.path(),
            StorageContext::default(),
            Arc::new(NullLogSink),
            SceneBackendInputs::default(),
        );
        let first = equip(
            &services,
            "test.first",
            &["fs.write:cache", "fs.read:cache"],
            &Value::Null,
            None,
        );
        let second = equip(
            &services,
            "test.second",
            &["fs.read:cache"],
            &Value::Null,
            None,
        );
        assert_ne!(
            first.background.context.instance,
            second.background.context.instance
        );
        let mut first_imports = first.imports;
        let mut second_imports = second.imports;
        // The cache scope of each plugin is its own folder under the client's data folder.
        let tile = scratch.path().join("plugins/fs/test.first/cache/tile.bin");
        std::fs::write(&tile, [1, 2, 3]).expect("cached tile");
        let first_fs = first_imports.fs.as_mut().expect("fs linked");
        assert_eq!(
            first_fs
                .read(path("cache", "tile.bin"), 0, 1024)
                .expect("cache read"),
            vec![1, 2, 3]
        );
        assert!(first_fs
            .write(path("cache", "next.bin"), vec![4], WriteMode::Replace)
            .is_ok());
        // Another plugin's scene reads its own cache folder, never this one.
        let second_fs = second_imports.fs.as_mut().expect("fs linked");
        assert!(second_fs.read(path("cache", "tile.bin"), 0, 1024).is_err());
        assert!(second_fs
            .read(path("cache", "../../test.first/cache/tile.bin"), 0, 1024)
            .is_err());
        assert!(first_imports.storage.is_some(), "storage opened");
    }

    #[test]
    fn the_minecraft_saves_scope_opens_on_its_read_grant_only() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let saves = scratch.path().join("saves");
        std::fs::create_dir_all(saves.join("World")).expect("saves folder");
        std::fs::write(saves.join("World").join("level.dat"), b"nbt").expect("level file");
        let services = SceneImportServices::new(
            &scratch.path().join("data"),
            StorageContext::default(),
            Arc::new(NullLogSink),
            SceneBackendInputs {
                minecraft_saves: Some(saves),
                ..SceneBackendInputs::default()
            },
        );
        let mut granted = equip(
            &services,
            "test.voxels",
            &["fs.read:minecraft-saves"],
            &Value::Null,
            None,
        )
        .imports;
        let fs = granted.fs.as_mut().expect("fs linked");
        assert_eq!(
            fs.read(path(MINECRAFT_SAVES_SCOPE, "World/level.dat"), 0, 1024)
                .expect("save readable"),
            b"nbt".to_vec()
        );
        assert!(fs
            .write(
                path(MINECRAFT_SAVES_SCOPE, "World/level.dat"),
                Vec::new(),
                WriteMode::Replace
            )
            .is_err());
        let mut refused = equip(
            &services,
            "test.voxels",
            &["fs.read:cache"],
            &Value::Null,
            None,
        )
        .imports;
        let fs = refused.fs.as_mut().expect("fs linked");
        assert!(fs
            .read(path(MINECRAFT_SAVES_SCOPE, "World/level.dat"), 0, 1024)
            .is_err());
    }

    /// A provider that knows one place for every text.
    struct OnePlace;

    impl GeocodeBackend for OnePlace {
        fn search(
            &self,
            query: &ilium_plugin_api::generated::host::geocode::GeocodeQuery,
        ) -> Result<Vec<ilium_plugin_api::generated::types::geo::GeoPlace>, String> {
            Ok(vec![ilium_plugin_api::generated::types::geo::GeoPlace {
                point: ilium_plugin_api::generated::types::geo::GeoPoint {
                    lat: 52.37,
                    lon: 4.89,
                },
                label: query.text.clone(),
                country_code: None,
                kind: None,
            }])
        }

        fn reverse(
            &self,
            _point: ilium_plugin_api::generated::types::geo::GeoPoint,
        ) -> Result<Vec<ilium_plugin_api::generated::types::geo::GeoPlace>, String> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn the_client_geocoder_answers_the_geocode_import_of_every_instance() {
        use ilium_plugin_api::generated::host::geocode::{GeocodeQuery, HostGeocode};
        let scratch = tempfile::tempdir().expect("scratch folder");
        let services = SceneImportServices::new(
            scratch.path(),
            StorageContext::default(),
            Arc::new(NullLogSink),
            SceneBackendInputs {
                geocode: Some(Arc::new(OnePlace)),
                ..SceneBackendInputs::default()
            },
        );
        for plugin in ["test.first", "test.second"] {
            let mut imports =
                equip(&services, plugin, &["location.geocode"], &Value::Null, None).imports;
            let geocode = imports.geocode.as_mut().expect("geocode linked");
            let request = geocode
                .search(GeocodeQuery {
                    text: "Amsterdam".to_owned(),
                    near: None,
                    limit: 3,
                    language: None,
                })
                .expect("accepted");
            let deadline = Instant::now() + std::time::Duration::from_secs(5);
            let places = loop {
                if let Some(places) = geocode.take_places(request) {
                    break places;
                }
                assert!(Instant::now() < deadline, "{plugin}: no answer");
                std::thread::sleep(std::time::Duration::from_millis(5));
            };
            assert_eq!(places.len(), 1);
            assert_eq!(places[0].label, "Amsterdam");
        }
    }

    #[test]
    fn paths_of_picker_rows_become_read_only_picked_scopes() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let pictures = scratch.path().join("Pictures");
        std::fs::create_dir_all(&pictures).expect("picture folder");
        std::fs::write(pictures.join("cat.png"), b"png").expect("picture");
        let services = SceneImportServices::new(
            &scratch.path().join("data"),
            StorageContext::default(),
            Arc::new(NullLogSink),
            SceneBackendInputs::default(),
        );
        let engine = engine();
        let compiled = engine.compile(&EMPTY_COMPONENT).expect("component");
        // Row 1 is relative: left out, and row 2 keeps its index.
        let picked = vec![
            pictures.clone(),
            PathBuf::from("Pictures"),
            pictures.join("cat.png"),
        ];
        let equip_with = |granted: &[&str]| {
            let granted: Vec<String> = granted.iter().map(|grant| (*grant).to_owned()).collect();
            services.equip(
                &SceneInstanceRequest {
                    plugin: "test.images",
                    granted: &granted,
                    manifest: &Value::Null,
                    observer: None,
                    picked: &picked,
                    setting_values: &[],
                },
                &engine,
                &compiled,
                &ResolvedLimits(Default::default()),
            )
        };
        let equipped = equip_with(&["fs.read:picked"]);
        assert_eq!(
            equipped.background.lent_picks.keys().collect::<Vec<_>>(),
            vec!["picked/0", "picked/2"]
        );
        let mut imports = equipped.imports;
        let fs = imports.fs.as_mut().expect("fs linked");
        let listed: Vec<String> = fs
            .list_dir(path("picked/0", ""), 16)
            .expect("folder listed")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(listed, vec!["cat.png".to_owned()]);
        assert_eq!(
            fs.read(path("picked/2", ""), 0, 1024)
                .expect("picked file read"),
            b"png".to_vec()
        );
        assert!(fs
            .write(path("picked/0", "cat.png"), Vec::new(), WriteMode::Replace)
            .is_err());
        let mut refused = equip_with(&["fs.read:cache"]).imports;
        let fs = refused.fs.as_mut().expect("fs linked");
        assert!(fs.read(path("picked/0", "cat.png"), 0, 1024).is_err());
    }

    #[test]
    fn a_job_that_cannot_start_ends_at_once_and_dropping_the_scene_cancels_queued_jobs() {
        use ilium_plugin_api::generated::host::jobs::HostJobs;
        let manifest = serde_json::json!({
            "contributes": { "jobs": [ { "kind": "decode", "deadline_ms": 1000, "memory_mib": 64 } ] }
        });
        let services = SceneImportServices::default();
        let equipped = equip(&services, "test.jobs", &["jobs.run"], &manifest, None);
        let mut imports = equipped.imports;
        let background = equipped.background;
        let jobs = imports.jobs.as_mut().expect("jobs linked");
        let request = jobs
            .start("decode".to_owned(), vec![1])
            .expect("job queued");
        assert!(jobs.start("unknown".to_owned(), Vec::new()).is_err());
        // The component exports no job-runner: the job instance starts, the run traps, and the
        // job ends with the trap's error rather than staying queued.
        background.start_queued_jobs();
        let mut finished = false;
        for _ in 0..200 {
            if background.running_jobs() == 0 {
                finished = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(finished, "the job thread ended");
        assert!(jobs.wait_request(request, 1000).is_err());
        let completed = background
            .take_events()
            .into_iter()
            .any(|event| matches!(event, Event::RequestCompleted(_)));
        assert!(completed, "request-completed queued for the scene");
        let second = jobs
            .start("decode".to_owned(), vec![2])
            .expect("second job queued");
        drop(background);
        assert!(jobs.take_output(second).is_none());
    }
}
