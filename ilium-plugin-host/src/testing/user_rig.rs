//! A user-level rig: one user profile with its XDG directories, CLI, server, clients, clock,
//! network and dialogs, driven the way the user-facing manual pages describe
//! (user-plugins-*.md USR-001..USR-220, reference-plugin-policy.md, reference-plugin-files.md).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::audit::{AuditActor, AuditContext};
use crate::cli::local_install::factory;
use crate::cli::{CliEnv, CliOutput};
use crate::install::{Consent, ConsentDecision, ConsentRequest};
use crate::lifecycle::{LifecycleManager, StartMode};
use crate::registry_client::{FetchError, Fetcher};
use crate::signing::{SignedAt, SigningKey};
use crate::trust::{TrustDocument, TrustRepository};

pub use ilium_plugin_api::platform::Platform;

/// Fetcher for a rig with no network interface: every request fails as unreachable, and each
/// requested URL is recorded so the rig can report what was attempted.
struct UnreachableFetcher {
    requests: Arc<Mutex<Vec<String>>>,
}

impl Fetcher for UnreachableFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        // A poisoned lock still holds every URL recorded so far, so recording continues through it.
        let mut requests = self.requests.lock().unwrap_or_else(PoisonError::into_inner);
        requests.push(url.to_owned());
        Err(FetchError::Unreachable)
    }
}

/// Consent that approves exactly the grants a package requests, as the default review does.
struct ApproveRequested;

impl Consent for ApproveRequested {
    fn decide(&self, request: &ConsentRequest) -> ConsentDecision {
        ConsentDecision::Approve(request.requested.clone())
    }
}

/// Collects every path below `directory`, depth first, without following a missing directory.
fn collect_paths(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_directory = path.is_dir();
        found.push(path.clone());
        if is_directory {
            collect_paths(&path, found);
        }
    }
}

/// A pane the rig opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u64);

/// A row of `ilium plugin list` (core-plugin-settings.md state words).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginRow {
    /// The plugin id.
    pub id: String,
    /// The installed version.
    pub version: String,
    /// `T1`, `T2`, ...
    pub tier: String,
    /// The state word of the table.
    pub state: String,
    /// The reason code when the plugin is not running.
    pub reason: Option<String>,
    /// The message shown next to the reason.
    pub message: Option<String>,
    /// Whether the publisher is not trusted.
    pub untrusted: bool,
}

/// One row of a review dialog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewRow {
    /// The grant string.
    pub grant: String,
    /// Whether the grant is high-risk.
    pub high: bool,
    /// Whether the row is checked.
    pub checked: bool,
    /// Whether the row cannot be toggled.
    pub locked: bool,
    /// The note under the row.
    pub note: Option<String>,
}

/// A review dialog as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewDialog {
    /// The rows, in display order.
    pub rows: Vec<ReviewRow>,
}

/// How an attached client answers each dialog, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAnswer {
    /// Accept exactly these grants.
    Review(Vec<String>),
    /// Accept the review with its default checks.
    ReviewAcceptDefaults,
    /// Decline the review.
    ReviewDecline,
    /// Pause the review (install without starting).
    ReviewPause,
    /// Decline a publisher trust prompt.
    TrustDecline,
    /// Accept a publisher trust prompt.
    TrustAccept,
    /// Accept a replacement.
    ReplaceYes,
    /// Install without replacing.
    ReplaceInstallWithout,
    /// Allow a link to open.
    LinkAllow,
    /// Decline a link.
    LinkDecline,
    /// Allow a first-use host.
    HostAllow,
    /// Decline a first-use host.
    HostDecline,
    /// Pause on a first-use host.
    HostPause,
    /// Confirm.
    ConfirmYes,
    /// Do not confirm.
    ConfirmNo,
}

/// One grant entry of the grant store as the user sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GrantEntry {
    /// The grant string.
    pub grant: String,
    /// `server` or `client`.
    pub side: String,
    /// `granted`, `declined`, ...
    pub state: String,
}

/// How a call that needs a grant ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallOutcome {
    /// The call went through.
    Ok,
    /// The host refused it with an error code.
    Refused {
        /// The error code.
        code: String,
        /// The reason, when the host names one.
        reason: Option<String>,
    },
    /// The instance trapped.
    Trapped {
        /// The trap code.
        code: String,
    },
    /// The grant was revoked while the call ran.
    Revoked,
}

/// Where the host process is killed during an install or uninstall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KillPoint {
    /// After the uninstall mark is written.
    AfterUninstallMark,
    /// Before the temporary file is written.
    BeforeTempWrite,
    /// After the temporary file is written, before the rename.
    AfterTempWriteBeforeRename,
    /// After the rename.
    AfterRename,
}

/// What a hook handler does when called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookBehaviour {
    /// Trap.
    Trap,
    /// Return an error.
    ReturnError,
    /// Sleep for the given milliseconds.
    SleepMs(u64),
    /// Append text after the given milliseconds.
    AppendAfterMs(u64, &'static str),
}

/// A signing key pair of a publisher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPair {
    publisher: String,
    public_key_path: PathBuf,
    private_key: Vec<u8>,
}

impl KeyPair {
    /// Generates a key pair for `publisher`.
    pub fn generate(publisher: &str) -> KeyPair {
        // Distinct per call: a process-wide counter plus the clock and process id.
        static GENERATION: AtomicU64 = AtomicU64::new(0);
        let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let mut hasher = Sha256::new();
        hasher.update(publisher.as_bytes());
        hasher.update(generation.to_le_bytes());
        hasher.update(nanos.to_le_bytes());
        hasher.update(std::process::id().to_le_bytes());
        let seed: [u8; 32] = hasher.finalize().into();
        let mut pair = KeyPair {
            publisher: publisher.to_owned(),
            public_key_path: PathBuf::new(),
            private_key: SigningKey::from_seed(seed).0,
        };
        pair.public_key_path =
            PathBuf::from(format!("{publisher}-{}.pub", pair.short_fingerprint()));
        pair
    }

    /// The full fingerprint (64 lowercase hex characters, DEV-112).
    pub fn fingerprint(&self) -> String {
        SigningKey(self.private_key.clone()).key_id()
    }

    /// The short fingerprint (the first 8 digits of the fingerprint).
    pub fn short_fingerprint(&self) -> String {
        self.fingerprint().chars().take(8).collect()
    }

    /// The public key file path as a string.
    pub fn public_key_path_string(&self) -> String {
        self.public_key_path.display().to_string()
    }

    /// The private key bytes.
    pub fn private_key_bytes(&self) -> Vec<u8> {
        self.private_key.clone()
    }
}

/// One declaration or behaviour a [`PackageSpec`] builder was given, in the order it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PackageDeclaration {
    StartWritesFile(String),
    ActivationTraps,
    RenderTraps(String),
    ExportTraps(String),
    Pane(String),
    Service(String),
    ConsumesService { provider: String, service: String },
    DefinesPoint(String),
    DefinesFilterPoint(String),
    DefinesVetoPoint(String),
    ContributesTo { owner: String, point: String },
    ContributesWithPriority { owner: String, point: String, priority: u32 },
    Hook { owner: String, point: String, behaviour: HookBehaviour },
    AddsDecorations,
}

/// A plugin package to install, built in memory.
#[derive(Debug, Clone)]
pub struct PackageSpec {
    id: String,
    version: String,
    display_name: Option<String>,
    /// Manifest capabilities per side (`server`, `client`).
    grants: BTreeMap<String, Vec<String>>,
    signed_with: Option<KeyPair>,
    api_version: String,
    replaces: Vec<String>,
    corrupt_archive: bool,
    corrupt_signature: bool,
    /// Builder declarations, kept in the order given; `build_bytes` does not serialize them yet.
    declarations: Vec<PackageDeclaration>,
}

impl PackageSpec {
    /// A package of `id` at `version`.
    pub fn new(id: &str, version: &str) -> PackageSpec {
        PackageSpec {
            id: id.to_owned(),
            version: version.to_owned(),
            display_name: None,
            grants: BTreeMap::new(),
            signed_with: None,
            api_version: "0.1.0".to_owned(),
            replaces: Vec::new(),
            corrupt_archive: false,
            corrupt_signature: false,
            declarations: Vec::new(),
        }
    }

    /// Grants the manifest declares for one side.
    pub fn grants(mut self, side: &str, grants: &[&str]) -> PackageSpec {
        self.grants.insert(
            side.to_owned(),
            grants.iter().map(|grant| (*grant).to_owned()).collect(),
        );
        self
    }

    /// The display name.
    pub fn display_name(mut self, name: &str) -> PackageSpec {
        self.display_name = Some(name.to_owned());
        self
    }

    /// Signs the package.
    pub fn signed_with(mut self, key: &KeyPair) -> PackageSpec {
        self.signed_with = Some(key.clone());
        self
    }

    /// The plugin writes a data file on start.
    pub fn start_writes_file(mut self, name: &str) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::StartWritesFile(name.to_owned()));
        self
    }

    /// The plugin traps on activation.
    pub fn activation_traps(mut self) -> PackageSpec {
        self.declarations.push(PackageDeclaration::ActivationTraps);
        self
    }

    /// The plugin traps when rendering a view.
    pub fn render_traps(mut self, view: &str) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::RenderTraps(view.to_owned()));
        self
    }

    /// The plugin traps in an export.
    pub fn export_traps(mut self, export: &str) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::ExportTraps(export.to_owned()));
        self
    }

    /// Corrupts one file hash of the package.
    pub fn corrupt_file_hash(self) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::corrupt_file_hash")
    }

    /// Corrupts the archive bytes.
    pub fn corrupt_archive(mut self) -> PackageSpec {
        self.corrupt_archive = true;
        self
    }

    /// Corrupts the signature.
    pub fn corrupt_signature(mut self) -> PackageSpec {
        self.corrupt_signature = true;
        self
    }

    /// The API version the manifest declares.
    pub fn api_version(mut self, version: &str) -> PackageSpec {
        self.api_version = version.to_owned();
        self
    }

    /// The plugins this package replaces.
    pub fn replaces(mut self, ids: &[&str]) -> PackageSpec {
        self.replaces = ids.iter().map(|id| (*id).to_owned()).collect();
        self
    }

    /// A pane kind the plugin provides.
    pub fn provides_pane(mut self, kind: &str) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::Pane(kind.to_owned()));
        self
    }

    /// A service the plugin provides.
    pub fn provides_service(mut self, service: &str) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::Service(service.to_owned()));
        self
    }

    /// A service of another plugin this one consumes.
    pub fn consumes_service(mut self, provider: &str, service: &str) -> PackageSpec {
        self.declarations.push(PackageDeclaration::ConsumesService {
            provider: provider.to_owned(),
            service: service.to_owned(),
        });
        self
    }

    /// An extension point the plugin defines.
    pub fn defines_point(mut self, point: &str) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::DefinesPoint(point.to_owned()));
        self
    }

    /// A filter point the plugin defines.
    pub fn defines_filter_point(mut self, point: &str) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::DefinesFilterPoint(point.to_owned()));
        self
    }

    /// A veto point the plugin defines.
    pub fn defines_veto_point(mut self, point: &str) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::DefinesVetoPoint(point.to_owned()));
        self
    }

    /// A contribution to another plugin's point.
    pub fn contributes_to(mut self, owner: &str, point: &str) -> PackageSpec {
        self.declarations.push(PackageDeclaration::ContributesTo {
            owner: owner.to_owned(),
            point: point.to_owned(),
        });
        self
    }

    /// A contribution with an explicit priority.
    pub fn contributes_with_priority(
        mut self,
        owner: &str,
        point: &str,
        priority: u32,
    ) -> PackageSpec {
        self.declarations
            .push(PackageDeclaration::ContributesWithPriority {
                owner: owner.to_owned(),
                point: point.to_owned(),
                priority,
            });
        self
    }

    /// A hook handler on another plugin's point.
    pub fn hook(mut self, owner: &str, point: &str, behaviour: HookBehaviour) -> PackageSpec {
        self.declarations.push(PackageDeclaration::Hook {
            owner: owner.to_owned(),
            point: point.to_owned(),
            behaviour,
        });
        self
    }

    /// The plugin adds tree decorations.
    pub fn adds_decorations(mut self) -> PackageSpec {
        self.declarations.push(PackageDeclaration::AddsDecorations);
        self
    }

    /// The package bytes.
    pub fn build_bytes(&self) -> Vec<u8> {
        let publisher = self
            .signed_with
            .as_ref()
            .map_or("Example Developer", |key| key.publisher.as_str());
        let mut manifest = serde_json::json!({
            "api": format!("ilium:plugin@{}", self.api_version),
            "id": self.id,
            "name": self.display_name.clone().unwrap_or_else(|| self.id.clone()),
            "version": self.version,
            "publisher": publisher,
            "replaces": self.replaces,
            "server": {"entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": []},
        });
        if !self.grants.is_empty() {
            manifest["capabilities"] = serde_json::json!(self.grants);
        }
        // Serializing a JSON value cannot fail.
        let manifest_text = serde_json::to_string_pretty(&manifest).expect("manifest serializes");
        let files = vec![
            ("manifest.json".to_owned(), manifest_text.into_bytes()),
            (
                "server.wasm".to_owned(),
                crate::testing::wat::wat_component("(component)"),
            ),
        ];
        let mut bytes = crate::package::write_package(&files)
            .unwrap_or_else(|diagnostic| panic!("rig package failed: {diagnostic:?}"));
        if let Some(key) = &self.signed_with {
            bytes = crate::signing::sign(
                &bytes,
                &SigningKey(key.private_key.clone()),
                SignedAt("2026-10-23T00:00:00Z".to_owned()),
            )
            .unwrap_or_else(|diagnostic| panic!("rig signing failed: {diagnostic:?}"));
        }
        if self.corrupt_archive {
            let middle = bytes.len() / 2;
            bytes[middle] ^= 0xff;
        }
        if self.corrupt_signature {
            if let Some(last) = bytes.last_mut() {
                *last ^= 0xff;
            }
        }
        bytes
    }
}

/// A file-system operation the host made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsOperation {
    /// `write`, `rename`, `remove`, ...
    pub kind: String,
    /// The path operated on.
    pub path: PathBuf,
    /// The destination of a rename, or empty.
    pub to: PathBuf,
}

/// Which host steps an uninstall cascade observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UninstallSteps {
    /// The host step at which the registry entry was removed.
    pub registry_removal_step: usize,
    /// The host step at which dependents were cascaded.
    pub cascade_step: usize,
}

/// The outcome of a veto point run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VetoOutcome {
    /// Whether the vetoable action went ahead.
    pub action_went_ahead: bool,
    /// Whether a handler vetoed it.
    pub vetoed: bool,
}

/// An install started without waiting for its review.
#[derive(Debug)]
pub struct PendingInstall;

/// A long call started with [`UserRig::start_long_call`].
#[derive(Debug)]
pub struct CallHandle;

/// The user rig.
#[derive(Debug)]
pub struct UserRig {
    /// Temporary root holding the XDG roots, scratch and working directories of this rig.
    root: PathBuf,
    platform: Platform,
    is_ci: bool,
    /// Milliseconds the rig clock has been advanced past the wall clock.
    clock_offset_ms: u64,
    /// URLs the rig's fetcher was asked for, in order; shared with the fetcher `cli_env` hands out.
    requests: Arc<Mutex<Vec<String>>>,
    /// The registry URL set by `configure_registry`; the fetcher does not reach it yet.
    registry_url: Option<String>,
    /// The project set by `switch_project`.
    project_path: Option<PathBuf>,
    /// The kill point set by `kill_host_at`; no install consults it yet.
    kill_point: Option<KillPoint>,
    /// Plugin settings keyed by `(plugin id, setting key)`.
    plugin_settings: BTreeMap<(String, String), String>,
    /// Toast texts currently shown, oldest first.
    toasts: Vec<String>,
    /// Whether a plugin has locked user input.
    input_locked: bool,
    /// Journal lines per plugin id, oldest first.
    journal_lines: BTreeMap<String, Vec<String>>,
}

impl Drop for UserRig {
    fn drop(&mut self) {
        // The root was created by this rig; removing it is best effort.
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl UserRig {
    /// Replacements the install recorded, as `(replaced id, replacing id)` (USR-043).
    pub fn replacements(&self) -> Vec<(String, String)> {
        unimplemented!("host::testing::user_rig::UserRig::replacements")
    }

    /// A rig on Linux.
    pub fn new() -> UserRig {
        UserRig::create(Platform::Linux)
    }

    /// A rig on `platform`.
    pub fn new_on(platform: Platform) -> UserRig {
        UserRig::create(platform)
    }

    /// A rig whose XDG roots are isolated and recorded (USR-200).
    pub fn new_with_xdg_isolation() -> UserRig {
        UserRig::create(Platform::Linux)
    }

    /// Creates a fresh, empty rig root with its XDG roots and scratch folder.
    fn create(platform: Platform) -> UserRig {
        static NEXT_RIG: AtomicU64 = AtomicU64::new(0);
        let index = NEXT_RIG.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("ilium-user-rig-{}-{index}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for directory in ["data", "config", "cache", "state", "scratch", "home"] {
            let path = root.join(directory);
            std::fs::create_dir_all(&path)
                .unwrap_or_else(|error| panic!("rig directory {}: {error}", path.display()));
        }
        UserRig {
            root,
            platform,
            is_ci: false,
            clock_offset_ms: 0,
            requests: Arc::new(Mutex::new(Vec::new())),
            registry_url: None,
            project_path: None,
            kill_point: None,
            plugin_settings: BTreeMap::new(),
            toasts: Vec::new(),
            input_locked: false,
            journal_lines: BTreeMap::new(),
        }
    }

    /// The rig clock: the wall clock plus whatever `advance_ms` added.
    fn now(&self) -> SystemTime {
        SystemTime::now()
            .checked_add(Duration::from_millis(self.clock_offset_ms))
            .unwrap_or(SystemTime::UNIX_EPOCH)
    }

    /// The environment every `ilium plugin` run of this rig uses.
    fn cli_env(&self) -> CliEnv {
        let mut env = CliEnv::new(
            self.data_dir(),
            self.state_dir(),
            self.config_dir(),
            self.root.join("home"),
            self.now(),
            self.is_ci,
            Arc::new(UnreachableFetcher {
                requests: Arc::clone(&self.requests),
            }),
        );
        env.cache_dir = self.cache_dir();
        env.platform = self.platform;
        env
    }

    // ---- CLI and install ------------------------------------------------------------------

    /// Runs `ilium plugin <args>`.
    pub fn cli(&mut self, args: &[&str]) -> CliOutput {
        let mut owned: Vec<String> = args.iter().map(|argument| (*argument).to_owned()).collect();
        if owned.first().map(String::as_str) == Some("plugin") {
            owned.remove(0);
        }
        crate::cli::run(&owned, &self.cli_env())
    }

    /// Installs a package file with extra arguments.
    pub fn install_file(&mut self, package: &PackageSpec, args: &[&str]) -> CliOutput {
        let package_path = self
            .scratch_dir()
            .join(format!("{}-{}.iliumpkg", package.id, package.version));
        std::fs::write(&package_path, package.build_bytes())
            .unwrap_or_else(|error| panic!("write package {}: {error}", package_path.display()));
        let consent: Arc<dyn Consent + Send + Sync> = Arc::new(ApproveRequested);
        let audit_context = AuditContext {
            session: "user-rig".to_owned(),
            actor: AuditActor {
                kind: "cli".to_owned(),
                pane: None,
                plugin: None,
            },
        };
        let mut env = self.cli_env();
        env.cwd = self.root.clone();
        let backend = match factory(&env, consent, audit_context) {
            Ok(backend) => backend,
            Err(diagnostic) => {
                return CliOutput {
                    exit_code: 1,
                    stdout_jsonl: Vec::new(),
                    stderr: diagnostic.message.clone(),
                    stdout_raw: String::new(),
                    stdout_text: String::new(),
                };
            }
        };
        let env = env.with_installer(backend);
        let mut owned = vec!["install".to_owned(), package_path.display().to_string()];
        owned.extend(args.iter().map(|argument| (*argument).to_owned()));
        crate::cli::run(&owned, &env)
    }

    /// Starts an install without waiting for its review.
    pub fn install_file_nonblocking(
        &mut self,
        _package: &PackageSpec,
        _args: &[&str],
    ) -> PendingInstall {
        unimplemented!("host::testing::user_rig::UserRig::install_file_nonblocking")
    }

    /// Installs a package through the Plugins settings screen.
    pub fn install_from_plugins_screen(&mut self, _package: &PackageSpec) -> CliOutput {
        unimplemented!("host::testing::user_rig::UserRig::install_from_plugins_screen")
    }

    /// The rows of `ilium plugin list`.
    pub fn list(&mut self) -> Vec<PluginRow> {
        // The rig attaches no session, so `list` reports session.unreachable and yields no rows.
        let output = self.cli(&["list", "--json"]);
        output
            .stdout_jsonl
            .iter()
            .filter(|record| record["type"] == "plugin")
            .map(|record| PluginRow {
                id: record["id"].as_str().unwrap_or_default().to_owned(),
                version: record["version"].as_str().unwrap_or_default().to_owned(),
                tier: record["tier"].as_str().unwrap_or_default().to_owned(),
                state: record["state"].as_str().unwrap_or_default().to_owned(),
                reason: record["reason"].as_str().map(str::to_owned),
                message: record["message"].as_str().map(str::to_owned),
                untrusted: record["untrusted"].as_bool().unwrap_or(false),
            })
            .collect()
    }

    /// Sets the CI environment flag.
    pub fn set_ci(&mut self, is_ci: bool) {
        self.is_ci = is_ci;
    }

    /// Points the rig at a registry URL.
    pub fn configure_registry(&mut self, url: &str) {
        self.registry_url = Some(url.to_owned());
    }

    /// How often the install instructions of a package ran.
    pub fn package_instructions_run(&self, _id: &str) -> u32 {
        unimplemented!("host::testing::user_rig::UserRig::package_instructions_run")
    }

    // ---- Paths ----------------------------------------------------------------------------

    /// The data directory.
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// The config directory.
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    /// The cache directory.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// The state directory.
    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    /// The scratch directory of the rig itself.
    pub fn scratch_dir(&self) -> PathBuf {
        self.root.join("scratch")
    }

    /// The grant store file.
    pub fn grants_path(&self) -> PathBuf {
        self.state_dir().join("grants.json")
    }

    /// The trust store file.
    pub fn trust_store_path(&self) -> PathBuf {
        self.state_dir().join("trust.json")
    }

    /// The audit directory holding `audit.jsonl` and its rotations.
    pub fn audit_dir(&self) -> PathBuf {
        self.state_dir().join("audit")
    }

    /// Files the host created outside the XDG roots.
    pub fn files_outside_xdg_roots(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        collect_paths(&self.root.join("home"), &mut found);
        found.into_iter().filter(|path| path.is_file()).collect()
    }

    /// Every path the host created.
    pub fn paths_created_by_the_host(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        collect_paths(&self.root, &mut found);
        found
    }

    /// Sets the umask of the host process.
    pub fn set_umask(&mut self, _mask: u32) {
        unimplemented!("host::testing::user_rig::UserRig::set_umask")
    }

    /// Sets the mode of a file.
    pub fn set_file_mode(&mut self, _path: &Path, _mode: u32) {
        unimplemented!("host::testing::user_rig::UserRig::set_file_mode")
    }

    /// Ages a file by `days`.
    pub fn age_file(&mut self, path: &Path, days: u32) {
        let aged = SystemTime::now()
            .checked_sub(Duration::from_secs(u64::from(days) * 86_400))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let file = std::fs::File::open(path)
            .unwrap_or_else(|error| panic!("age_file {}: {error}", path.display()));
        file.set_modified(aged)
            .unwrap_or_else(|error| panic!("age_file {}: {error}", path.display()));
    }

    /// Opens (creates) a project and returns its path.
    pub fn open_project(&mut self, name: &str) -> PathBuf {
        let path = self.scratch_dir().join("projects").join(name);
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|error| panic!("open_project {}: {error}", path.display()));
        path
    }

    /// Switches the server to another project.
    pub fn switch_project(&mut self, path: &Path) {
        self.project_path = Some(path.to_path_buf());
    }

    // ---- Server and clock -----------------------------------------------------------------

    /// Starts the server.
    pub fn start_server(&mut self) {
        self.start_server_mode(StartMode::Normal);
    }

    /// Starts the server in a mode.
    pub fn start_server_mode(&mut self, mode: StartMode) {
        // Production lifecycle start is still a stub: this call is the intended red state.
        let _report = LifecycleManager.start(mode);
    }

    /// Restarts the server.
    pub fn restart_server(&mut self) {
        self.stop_server();
        self.start_server();
    }

    /// Stops the server.
    pub fn stop_server(&mut self) {
        // Production lifecycle shutdown is still a stub: this call is the intended red state.
        let _report = LifecycleManager.shutdown(Duration::from_secs(5));
    }

    /// Advances the clock.
    pub fn advance_ms(&mut self, ms: u64) {
        self.clock_offset_ms = self.clock_offset_ms.saturating_add(ms);
    }

    /// Kills the host at a point of the next install or uninstall.
    pub fn kill_host_at(&mut self, point: KillPoint) {
        self.kill_point = Some(point);
    }

    /// The startup trace of the last server start.
    pub fn startup_trace(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::startup_trace")
    }

    /// The uninstall trace of a plugin.
    pub fn uninstall_trace(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::uninstall_trace")
    }

    /// Uninstalls a plugin and reports which host steps observers saw.
    pub fn uninstall_observing_steps(&mut self, _id: &str) -> UninstallSteps {
        unimplemented!("host::testing::user_rig::UserRig::uninstall_observing_steps")
    }

    /// Starts recording file-system operations.
    pub fn record_fs_operations(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::record_fs_operations")
    }

    /// The recorded file-system operations.
    pub fn fs_operations(&self) -> Vec<FsOperation> {
        unimplemented!("host::testing::user_rig::UserRig::fs_operations")
    }

    // ---- Dialogs and prompts --------------------------------------------------------------

    /// Attaches a client that answers dialogs in order.
    pub fn attach_client(&mut self, _answers: Vec<DialogAnswer>) {
        unimplemented!("host::testing::user_rig::UserRig::attach_client")
    }

    /// The review dialogs shown so far.
    pub fn dialogs_shown(&self) -> Vec<ReviewDialog> {
        unimplemented!("host::testing::user_rig::UserRig::dialogs_shown")
    }

    /// The trust prompts shown so far.
    pub fn trust_prompts(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::trust_prompts")
    }

    /// The replacement prompts shown so far.
    pub fn replace_prompts(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::replace_prompts")
    }

    /// The confirmations shown so far.
    pub fn confirmations_shown(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::confirmations_shown")
    }

    /// The link prompts shown so far.
    pub fn link_prompts(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::link_prompts")
    }

    // ---- Grants, trust and state ----------------------------------------------------------

    /// The grant entries of a plugin.
    pub fn grant_entries(&self, id: &str) -> Vec<GrantEntry> {
        // The grant store file keeps the granted grants per side; a missing or unreadable file
        // holds no grants.
        let Ok(text) = std::fs::read_to_string(self.grants_path()) else {
            return Vec::new();
        };
        let Ok(document) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for side in ["server", "client"] {
            let Some(grants) = document["plugins"][id]["grants"][side].as_array() else {
                continue;
            };
            for grant in grants.iter().filter_map(serde_json::Value::as_str) {
                entries.push(GrantEntry {
                    grant: grant.to_owned(),
                    side: side.to_owned(),
                    state: "granted".to_owned(),
                });
            }
        }
        entries
    }

    /// Trusts a publisher key.
    pub fn trust(&mut self, _key: &KeyPair) {
        unimplemented!("host::testing::user_rig::UserRig::trust")
    }

    /// The trusted key fingerprints.
    pub fn trusted_keys(&self) -> Vec<String> {
        // A missing or unreadable trust store reads as no trusted keys.
        TrustRepository::new(self.trust_store_path(), TrustDocument::default())
            .and_then(|repository| repository.snapshot_at(self.now()))
            .map(|snapshot| {
                snapshot
                    .keys()
                    .iter()
                    .map(|resolved| resolved.entry.key_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The grant-store bytes expected after a grant is given.
    pub fn expected_grants_after(&self, _id: &str, _grant: &str) -> Vec<u8> {
        unimplemented!("host::testing::user_rig::UserRig::expected_grants_after")
    }

    /// Sets a plugin setting.
    pub fn set_plugin_setting(&mut self, id: &str, key: &str, value: &str) {
        self.plugin_settings
            .insert((id.to_owned(), key.to_owned()), value.to_owned());
    }

    /// A plugin setting.
    pub fn plugin_setting(&self, id: &str, key: &str) -> Option<String> {
        self.plugin_settings
            .get(&(id.to_owned(), key.to_owned()))
            .cloned()
    }

    /// Writes a file into a plugin's cache directory.
    pub fn write_cache_file(&mut self, _id: &str, _name: &str) {
        unimplemented!("host::testing::user_rig::UserRig::write_cache_file")
    }

    /// Writes a storage bucket of a plugin.
    pub fn write_storage_bucket(&mut self, _id: &str) {
        unimplemented!("host::testing::user_rig::UserRig::write_storage_bucket")
    }

    /// Whether a file exists in a plugin's data directory.
    pub fn data_file_exists(&self, _id: &str, _name: &str) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::data_file_exists")
    }

    /// Reads a file of a plugin's data directory.
    pub fn read_data_file(&self, _id: &str, _name: &str) -> Option<String> {
        unimplemented!("host::testing::user_rig::UserRig::read_data_file")
    }

    // ---- Calls ----------------------------------------------------------------------------

    /// A plugin side makes a call needing `grant`.
    pub fn call_needing(&mut self, _id: &str, _side: &str, _grant: &str) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::call_needing")
    }

    /// A plugin side makes a call needing a grant its manifest never listed.
    pub fn call_needing_unlisted(&mut self, _id: &str, _side: &str, _grant: &str) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::call_needing_unlisted")
    }

    /// Starts a long call needing `grant`.
    pub fn start_long_call(&mut self, _id: &str, _side: &str, _grant: &str) -> CallHandle {
        unimplemented!("host::testing::user_rig::UserRig::start_long_call")
    }

    /// Finishes a long call.
    pub fn finish_call(&mut self, _call: CallHandle) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::finish_call")
    }

    /// The granted list a plugin side's instance saw.
    pub fn granted_list_seen_by_instance(&self, _id: &str, _side: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::granted_list_seen_by_instance")
    }

    /// A plugin makes an HTTP request.
    pub fn plugin_http(&mut self, _id: &str, _url: &str) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::plugin_http")
    }

    /// A plugin opens a raw socket.
    pub fn plugin_raw_socket(&mut self, _id: &str, _address: &str) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::plugin_raw_socket")
    }

    /// Makes the fake network redirect one URL to another.
    pub fn script_redirect(&mut self, _from: &str, _to: &str) {
        unimplemented!("host::testing::user_rig::UserRig::script_redirect")
    }

    /// A plugin asks to open a link.
    pub fn plugin_opens_link(&mut self, _id: &str, _url: &str) {
        unimplemented!("host::testing::user_rig::UserRig::plugin_opens_link")
    }

    /// The links actually opened.
    pub fn opened_links(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::opened_links")
    }

    /// A plugin requests a URL.
    pub fn plugin_requests(&mut self, _id: &str, _url: &str) {
        unimplemented!("host::testing::user_rig::UserRig::plugin_requests")
    }

    /// The hosts connections were attempted to.
    pub fn connections_attempted(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::connections_attempted")
    }

    /// A plugin shows a toast.
    pub fn plugin_toast(&mut self, _id: &str, text: &str) {
        self.toasts.push(text.to_owned());
    }

    /// The toasts visible now.
    pub fn visible_toasts(&self) -> Vec<String> {
        self.toasts.clone()
    }

    /// Dismisses every toast.
    pub fn clear_toasts(&mut self) {
        self.toasts.clear();
    }

    /// A plugin locks user input.
    pub fn plugin_locks_input(&mut self, _id: &str) {
        self.input_locked = true;
    }

    /// Whether user input is locked.
    pub fn input_locked(&self) -> bool {
        self.input_locked
    }

    /// Whether the clipboard-access indicator is visible.
    pub fn clipboard_indicator_visible(&self) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::clipboard_indicator_visible")
    }

    /// A plugin logs one line at a level.
    pub fn plugin_logs(&mut self, id: &str, level: &str, line: &str) {
        self.journal_lines
            .entry(id.to_owned())
            .or_default()
            .push(format!("{level}: {line}"));
    }

    /// A plugin logs `bytes` bytes. The rig records a write as one journal line of that length,
    /// so a zero-byte write leaves no line.
    pub fn plugin_logs_bytes(&mut self, id: &str, bytes: usize) {
        if bytes == 0 {
            return;
        }
        self.journal_lines
            .entry(id.to_owned())
            .or_default()
            .push("x".repeat(bytes));
    }

    /// The journal lines of a plugin.
    pub fn log_lines(&self, id: &str) -> Vec<String> {
        self.journal_lines.get(id).cloned().unwrap_or_default()
    }

    /// Makes a plugin trap on its next call.
    pub fn inject_trap(&mut self, _id: &str) {
        unimplemented!("host::testing::user_rig::UserRig::inject_trap")
    }

    /// How often a plugin's instance started.
    pub fn instance_starts(&self, _id: &str) -> u32 {
        unimplemented!("host::testing::user_rig::UserRig::instance_starts")
    }

    /// The failure count of a plugin.
    pub fn failure_count(&self, _id: &str) -> u32 {
        unimplemented!("host::testing::user_rig::UserRig::failure_count")
    }

    /// The N14 notice text for a plugin, if shown.
    pub fn n14_text(&self, _id: &str) -> Option<String> {
        unimplemented!("host::testing::user_rig::UserRig::n14_text")
    }

    /// The notices shown so far.
    pub fn notices(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::notices")
    }

    /// The notices that persist across restarts.
    pub fn persistent_notices(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::persistent_notices")
    }

    // ---- Network observation --------------------------------------------------------------

    /// The network requests made.
    pub fn network_requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How often the fetcher was called.
    pub fn fetch_calls(&self) -> usize {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Turns request logging on or off.
    pub fn set_request_logging(&mut self, _on: bool) {
        unimplemented!("host::testing::user_rig::UserRig::set_request_logging")
    }

    /// Removes every network interface.
    pub fn deny_network(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::deny_network")
    }

    // ---- Extensions and panes -------------------------------------------------------------

    /// The order in which a point's contributors were called.
    pub fn call_point_order(&mut self, _owner: &str, _point: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::call_point_order")
    }

    /// Moves a contributor up in the Extensions screen.
    pub fn extensions_move_up(&mut self, _owner: &str, _point: &str, _plugin: &str) {
        unimplemented!("host::testing::user_rig::UserRig::extensions_move_up")
    }

    /// Moves a contributor down in the Extensions screen.
    pub fn extensions_move_down(&mut self, _owner: &str, _point: &str, _plugin: &str) {
        unimplemented!("host::testing::user_rig::UserRig::extensions_move_down")
    }

    /// Toggles a contributor in the Extensions screen.
    pub fn extensions_toggle(&mut self, _owner: &str, _point: &str, _plugin: &str, _on: bool) {
        unimplemented!("host::testing::user_rig::UserRig::extensions_toggle")
    }

    /// Records the current extension order of a plugin.
    pub fn extensions_remember_order(&mut self, _plugin: &str) {
        unimplemented!("host::testing::user_rig::UserRig::extensions_remember_order")
    }

    /// The extension order entries kept for a plugin.
    pub fn extension_order_entries(&self, _plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::extension_order_entries")
    }

    /// Whether a contributor is active on a point.
    pub fn extension_active(&self, _owner: &str, _point: &str, _plugin: &str) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::extension_active")
    }

    /// The label the Extensions screen shows for a contributor.
    pub fn extension_label(&self, _owner: &str, _point: &str, _plugin: &str) -> String {
        unimplemented!("host::testing::user_rig::UserRig::extension_label")
    }

    /// Resets a contributor after breaches.
    pub fn extension_reset(&mut self, _owner: &str, _point: &str, _plugin: &str) {
        unimplemented!("host::testing::user_rig::UserRig::extension_reset")
    }

    /// Whether a reset is offered for a contributor.
    pub fn extension_reset_offered(&self, _owner: &str, _point: &str, _plugin: &str) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::extension_reset_offered")
    }

    /// Runs a filter point and returns the filtered value.
    pub fn run_filter_point(&mut self, _owner: &str, _point: &str, _input: &str) -> String {
        unimplemented!("host::testing::user_rig::UserRig::run_filter_point")
    }

    /// Runs a veto point.
    pub fn run_veto_point(&mut self, _owner: &str, _point: &str) -> VetoOutcome {
        unimplemented!("host::testing::user_rig::UserRig::run_veto_point")
    }

    /// Calls a service of a provider from a consumer; returns the result word.
    pub fn call_service(&mut self, _consumer: &str, _provider: &str, _service: &str) -> String {
        unimplemented!("host::testing::user_rig::UserRig::call_service")
    }

    /// The decorations a plugin currently draws.
    pub fn decoration_count(&self, _id: &str) -> usize {
        unimplemented!("host::testing::user_rig::UserRig::decoration_count")
    }

    /// Opens a plugin pane.
    pub fn open_plugin_pane(&mut self, _id: &str, _kind: &str) -> PaneId {
        unimplemented!("host::testing::user_rig::UserRig::open_plugin_pane")
    }

    /// Whether a pane is in the tree.
    pub fn pane_exists(&self, _pane: PaneId) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::pane_exists")
    }

    /// The content kind of a pane (`placeholder` once its plugin is gone).
    pub fn pane_content_kind(&self, _pane: PaneId) -> String {
        unimplemented!("host::testing::user_rig::UserRig::pane_content_kind")
    }

    // ---- Policy and audit -----------------------------------------------------------------

    /// Writes the system policy file.
    pub fn write_system_policy(&mut self, _toml: &str) {
        unimplemented!("host::testing::user_rig::UserRig::write_system_policy")
    }

    /// Sets the mode of the system policy file.
    pub fn set_system_policy_mode(&mut self, _mode: u32) {
        unimplemented!("host::testing::user_rig::UserRig::set_system_policy_mode")
    }

    /// Makes the system policy file unreadable.
    pub fn make_system_policy_unreadable(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::make_system_policy_unreadable")
    }

    /// The audit records of one event kind.
    pub fn audit_records_of(&self, event: &str) -> Vec<serde_json::Value> {
        // A missing or unreadable audit log holds no records, as the production reader reports.
        crate::stores::AuditLog::open(&self.state_dir())
            .map(|log| log.entries())
            .unwrap_or_default()
            .iter()
            .filter(|entry| entry.kind == event)
            .filter_map(|entry| serde_json::from_str(&entry.line).ok())
            .collect()
    }
}

impl Default for UserRig {
    fn default() -> UserRig {
        UserRig::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CallOutcome, DialogAnswer, HookBehaviour, KeyPair, PackageSpec, PaneId, Platform, UserRig,
    };

    /// REQ: DEV-112
    #[test]
    fn key_fingerprint_is_64_lowercase_hex_characters() {
        let key = KeyPair::generate("example-publisher");
        let fingerprint = key.fingerprint();
        assert_eq!(fingerprint.len(), 64);
        assert!(fingerprint
            .chars()
            .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character)));
    }

    /// REQ: DEV-112
    #[test]
    fn short_fingerprint_is_the_first_eight_characters_of_the_fingerprint() {
        let key = KeyPair::generate("example-publisher");
        let short = key.short_fingerprint();
        assert_eq!(short.len(), 8);
        assert_eq!(short, key.fingerprint()[..8].to_string());
    }

    #[test]
    fn generated_keys_for_different_generations_have_distinct_fingerprints() {
        let first = KeyPair::generate("example-publisher");
        let second = KeyPair::generate("example-publisher");
        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn key_public_key_path_string_is_not_empty() {
        let key = KeyPair::generate("example-publisher");
        assert!(!key.public_key_path_string().is_empty());
    }

    #[test]
    fn key_private_key_bytes_are_not_empty() {
        let key = KeyPair::generate("example-publisher");
        assert!(!key.private_key_bytes().is_empty());
    }

    #[test]
    fn minimal_package_builds_non_empty_bytes() {
        let bytes = PackageSpec::new("minimal-plugin", "0.1.0").build_bytes();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn package_with_empty_grant_list_still_builds() {
        let bytes = PackageSpec::new("no-grants", "0.1.0")
            .grants("client", &[])
            .build_bytes();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn package_with_hook_behaviour_of_zero_delay_still_builds() {
        let bytes = PackageSpec::new("hooky", "1.0.0")
            .hook(
                "owner-plugin",
                "some.point",
                HookBehaviour::AppendAfterMs(0, ""),
            )
            .build_bytes();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn corrupting_the_archive_changes_the_package_bytes() {
        let clean = PackageSpec::new("corrupt-archive", "1.0.0").build_bytes();
        let corrupted = PackageSpec::new("corrupt-archive", "1.0.0")
            .corrupt_archive()
            .build_bytes();
        assert_ne!(clean, corrupted);
    }

    #[test]
    fn corrupting_a_file_hash_changes_the_package_bytes() {
        let clean = PackageSpec::new("corrupt-hash", "1.0.0").build_bytes();
        let corrupted = PackageSpec::new("corrupt-hash", "1.0.0")
            .corrupt_file_hash()
            .build_bytes();
        assert_ne!(clean, corrupted);
    }

    #[test]
    fn corrupting_the_signature_changes_the_package_bytes() {
        let key = KeyPair::generate("example-publisher");
        let signed = PackageSpec::new("corrupt-sig", "1.0.0")
            .signed_with(&key)
            .build_bytes();
        let corrupted = PackageSpec::new("corrupt-sig", "1.0.0")
            .signed_with(&key)
            .corrupt_signature()
            .build_bytes();
        assert_ne!(signed, corrupted);
    }

    #[test]
    fn signed_package_differs_from_unsigned_package() {
        let key = KeyPair::generate("example-publisher");
        let unsigned = PackageSpec::new("signed-check", "1.0.0").build_bytes();
        let signed = PackageSpec::new("signed-check", "1.0.0")
            .signed_with(&key)
            .build_bytes();
        assert_ne!(unsigned, signed);
    }

    #[test]
    fn fresh_rig_trusts_no_keys() {
        let rig = UserRig::new();
        assert!(rig.trusted_keys().is_empty());
    }

    #[test]
    fn default_rig_matches_new_rig_with_no_trusted_keys() {
        let rig = UserRig::default();
        assert!(rig.trusted_keys().is_empty());
    }

    #[test]
    fn rig_on_macos_starts_with_no_trusted_keys() {
        let rig = UserRig::new_on(Platform::MacOs);
        assert!(rig.trusted_keys().is_empty());
    }

    #[test]
    fn trusting_a_key_lists_its_fingerprint() {
        let key = KeyPair::generate("example-publisher");
        let mut rig = UserRig::new();
        rig.trust(&key);
        assert!(rig.trusted_keys().contains(&key.fingerprint()));
    }

    /// REQ: USR-200
    #[test]
    fn xdg_isolated_rig_writes_no_plugin_files_outside_xdg_roots() {
        let mut rig = UserRig::new_with_xdg_isolation();
        rig.start_server();
        assert!(rig.files_outside_xdg_roots().is_empty());
    }

    /// REQ: USR-200
    #[test]
    fn xdg_roots_config_data_cache_and_state_are_four_distinct_directories() {
        let rig = UserRig::new_with_xdg_isolation();
        let roots = [
            rig.config_dir(),
            rig.data_dir(),
            rig.cache_dir(),
            rig.state_dir(),
        ];
        for (index, root) in roots.iter().enumerate() {
            for other in roots.iter().skip(index + 1) {
                assert_ne!(root, other);
            }
        }
    }

    /// REQ: USR-043
    #[test]
    fn replacement_accepted_with_yes_is_recorded() {
        let mut rig = UserRig::new();
        rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
        rig.install_file(&PackageSpec::new("old-a", "1.0.0"), &[]);
        rig.attach_client(vec![DialogAnswer::ReplaceYes]);
        rig.install_file(
            &PackageSpec::new("new-a", "2.0.0").replaces(&["old-a"]),
            &[],
        );
        assert_eq!(
            rig.replacements(),
            vec![("old-a".to_string(), "new-a".to_string())]
        );
        assert_eq!(rig.replace_prompts().len(), 1);
    }

    /// REQ: USR-043
    #[test]
    fn replacement_declined_with_install_without_records_no_replacement() {
        let mut rig = UserRig::new();
        rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
        rig.install_file(&PackageSpec::new("old-b", "1.0.0"), &[]);
        rig.attach_client(vec![DialogAnswer::ReplaceInstallWithout]);
        rig.install_file(
            &PackageSpec::new("new-b", "2.0.0").replaces(&["old-b"]),
            &[],
        );
        assert!(rig.replacements().is_empty());
    }

    /// REQ: USR-043
    #[test]
    fn replacement_list_of_four_entries_is_accepted_at_the_limit() {
        let mut rig = UserRig::new();
        let output = rig.install_file(
            &PackageSpec::new("replacer-four", "1.0.0").replaces(&["a", "b", "c", "d"]),
            &[],
        );
        assert_eq!(output.exit_code, 0);
    }

    /// REQ: USR-043
    #[test]
    fn replacement_list_of_five_entries_is_refused() {
        let mut rig = UserRig::new();
        let output = rig.install_file(
            &PackageSpec::new("replacer-five", "1.0.0").replaces(&["a", "b", "c", "d", "e"]),
            &[],
        );
        assert_ne!(output.exit_code, 0);
        assert!(rig.replace_prompts().is_empty());
    }

    /// REQ: USR-043
    #[test]
    fn empty_replaces_list_shows_no_replace_prompt() {
        let mut rig = UserRig::new();
        rig.install_file(
            &PackageSpec::new("replacer-none", "1.0.0").replaces(&[]),
            &[],
        );
        assert!(rig.replace_prompts().is_empty());
        assert!(rig.replacements().is_empty());
    }

    #[test]
    fn call_for_unlisted_grant_is_refused() {
        let mut rig = UserRig::new();
        rig.attach_client(vec![DialogAnswer::ReviewAcceptDefaults]);
        rig.install_file(
            &PackageSpec::new("calc-plugin", "1.0.0").grants("client", &["clipboard-read"]),
            &[],
        );
        let outcome = rig.call_needing_unlisted("calc-plugin", "client", "network");
        assert!(matches!(outcome, CallOutcome::Refused { .. }));
    }

    #[test]
    fn fresh_rig_has_no_visible_toasts() {
        let rig = UserRig::new();
        assert!(rig.visible_toasts().is_empty());
    }

    #[test]
    fn plugin_toast_is_visible_until_cleared() {
        let mut rig = UserRig::new();
        rig.plugin_toast("toast-plugin", "Export finished");
        assert!(rig
            .visible_toasts()
            .iter()
            .any(|text| text == "Export finished"));
        rig.clear_toasts();
        assert!(rig.visible_toasts().is_empty());
    }

    #[test]
    fn clearing_toasts_on_empty_rig_is_a_no_op() {
        let mut rig = UserRig::new();
        rig.clear_toasts();
        assert!(rig.visible_toasts().is_empty());
    }

    #[test]
    fn input_is_not_locked_on_a_fresh_rig() {
        let rig = UserRig::new();
        assert!(!rig.input_locked());
    }

    #[test]
    fn unknown_pane_does_not_exist() {
        let rig = UserRig::new();
        assert!(!rig.pane_exists(PaneId(0)));
    }

    #[test]
    fn fresh_rig_has_no_opened_links() {
        let rig = UserRig::new();
        assert!(rig.opened_links().is_empty());
    }

    #[test]
    fn zero_byte_log_write_leaves_no_journal_lines() {
        let mut rig = UserRig::new();
        rig.plugin_logs_bytes("log-plugin", 0);
        assert!(rig.log_lines("log-plugin").is_empty());
    }

    #[test]
    fn plugin_setting_round_trips_its_value() {
        let mut rig = UserRig::new();
        rig.set_plugin_setting("settings-plugin", "volume", "7");
        assert_eq!(
            rig.plugin_setting("settings-plugin", "volume"),
            Some("7".to_string())
        );
    }

    #[test]
    fn unset_plugin_setting_reads_as_none() {
        let rig = UserRig::new();
        assert_eq!(rig.plugin_setting("settings-plugin", "volume"), None);
    }

    #[test]
    fn fresh_rig_has_no_audit_records_for_any_event() {
        let rig = UserRig::new();
        assert!(rig.audit_records_of("install").is_empty());
    }
}
