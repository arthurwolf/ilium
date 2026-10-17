//! A user-level rig: one user profile with its XDG directories, CLI, server, clients, clock,
//! network and dialogs, driven the way the user-facing manual pages describe
//! (user-plugins-*.md USR-001..USR-220, reference-plugin-policy.md, reference-plugin-files.md).

use std::path::{Path, PathBuf};

use crate::cli::CliOutput;
use crate::lifecycle::StartMode;

pub use ilium_plugin_api::platform::Platform;

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
        unimplemented!("host::testing::user_rig::KeyPair::generate")
    }

    /// The full fingerprint (64 lowercase hex characters, DEV-112).
    pub fn fingerprint(&self) -> String {
        unimplemented!("host::testing::user_rig::KeyPair::fingerprint")
    }

    /// The short fingerprint (first four, an ellipsis, last four).
    pub fn short_fingerprint(&self) -> String {
        unimplemented!("host::testing::user_rig::KeyPair::short_fingerprint")
    }

    /// The public key file path as a string.
    pub fn public_key_path_string(&self) -> String {
        unimplemented!("host::testing::user_rig::KeyPair::public_key_path_string")
    }

    /// The private key bytes.
    pub fn private_key_bytes(&self) -> Vec<u8> {
        unimplemented!("host::testing::user_rig::KeyPair::private_key_bytes")
    }
}

/// A plugin package to install, built in memory.
#[derive(Debug, Clone)]
pub struct PackageSpec {
    id: String,
    version: String,
    options: Vec<String>,
}

impl PackageSpec {
    /// A package of `id` at `version`.
    pub fn new(id: &str, version: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::new")
    }

    /// Grants the manifest declares for one side.
    pub fn grants(self, side: &str, grants: &[&str]) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::grants")
    }

    /// The display name.
    pub fn display_name(self, name: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::display_name")
    }

    /// Signs the package.
    pub fn signed_with(self, key: &KeyPair) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::signed_with")
    }

    /// The plugin writes a data file on start.
    pub fn start_writes_file(self, name: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::start_writes_file")
    }

    /// The plugin traps on activation.
    pub fn activation_traps(self) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::activation_traps")
    }

    /// The plugin traps when rendering a view.
    pub fn render_traps(self, view: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::render_traps")
    }

    /// The plugin traps in an export.
    pub fn export_traps(self, export: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::export_traps")
    }

    /// Corrupts one file hash of the package.
    pub fn corrupt_file_hash(self) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::corrupt_file_hash")
    }

    /// Corrupts the archive bytes.
    pub fn corrupt_archive(self) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::corrupt_archive")
    }

    /// Corrupts the signature.
    pub fn corrupt_signature(self) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::corrupt_signature")
    }

    /// The API version the manifest declares.
    pub fn api_version(self, version: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::api_version")
    }

    /// The plugins this package replaces.
    pub fn replaces(self, ids: &[&str]) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::replaces")
    }

    /// A pane kind the plugin provides.
    pub fn provides_pane(self, kind: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::provides_pane")
    }

    /// A service the plugin provides.
    pub fn provides_service(self, service: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::provides_service")
    }

    /// A service of another plugin this one consumes.
    pub fn consumes_service(self, provider: &str, service: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::consumes_service")
    }

    /// An extension point the plugin defines.
    pub fn defines_point(self, point: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::defines_point")
    }

    /// A filter point the plugin defines.
    pub fn defines_filter_point(self, point: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::defines_filter_point")
    }

    /// A veto point the plugin defines.
    pub fn defines_veto_point(self, point: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::defines_veto_point")
    }

    /// A contribution to another plugin's point.
    pub fn contributes_to(self, owner: &str, point: &str) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::contributes_to")
    }

    /// A contribution with an explicit priority.
    pub fn contributes_with_priority(self, owner: &str, point: &str, priority: u32) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::contributes_with_priority")
    }

    /// A hook handler on another plugin's point.
    pub fn hook(self, owner: &str, point: &str, behaviour: HookBehaviour) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::hook")
    }

    /// The plugin adds tree decorations.
    pub fn adds_decorations(self) -> PackageSpec {
        unimplemented!("host::testing::user_rig::PackageSpec::adds_decorations")
    }

    /// The package bytes.
    pub fn build_bytes(&self) -> Vec<u8> {
        unimplemented!("host::testing::user_rig::PackageSpec::build_bytes")
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
pub struct PendingInstall {
    id: String,
}

/// A long call started with [`UserRig::start_long_call`].
#[derive(Debug)]
pub struct CallHandle {
    id: u64,
}

/// The user rig.
#[derive(Debug)]
pub struct UserRig {
    platform: Platform,
    root: PathBuf,
    now_ms: u64,
}

impl UserRig {
    /// Replacements the install recorded, as `(replaced id, replacing id)` (USR-043).
    pub fn replacements(&self) -> Vec<(String, String)> {
        unimplemented!("host::testing::user_rig::UserRig::replacements")
    }

    /// A rig on Linux.
    pub fn new() -> UserRig {
        unimplemented!("host::testing::user_rig::UserRig::new")
    }

    /// A rig on `platform`.
    pub fn new_on(platform: Platform) -> UserRig {
        unimplemented!("host::testing::user_rig::UserRig::new_on")
    }

    /// A rig whose XDG roots are isolated and recorded (USR-200).
    pub fn new_with_xdg_isolation() -> UserRig {
        unimplemented!("host::testing::user_rig::UserRig::new_with_xdg_isolation")
    }

    // ---- CLI and install ------------------------------------------------------------------

    /// Runs `ilium plugin <args>`.
    pub fn cli(&mut self, args: &[&str]) -> CliOutput {
        unimplemented!("host::testing::user_rig::UserRig::cli")
    }

    /// Installs a package file with extra arguments.
    pub fn install_file(&mut self, package: &PackageSpec, args: &[&str]) -> CliOutput {
        unimplemented!("host::testing::user_rig::UserRig::install_file")
    }

    /// Starts an install without waiting for its review.
    pub fn install_file_nonblocking(
        &mut self,
        package: &PackageSpec,
        args: &[&str],
    ) -> PendingInstall {
        unimplemented!("host::testing::user_rig::UserRig::install_file_nonblocking")
    }

    /// Installs a package through the Plugins settings screen.
    pub fn install_from_plugins_screen(&mut self, package: &PackageSpec) -> CliOutput {
        unimplemented!("host::testing::user_rig::UserRig::install_from_plugins_screen")
    }

    /// The rows of `ilium plugin list`.
    pub fn list(&mut self) -> Vec<PluginRow> {
        unimplemented!("host::testing::user_rig::UserRig::list")
    }

    /// Sets the CI environment flag.
    pub fn set_ci(&mut self, is_ci: bool) {
        unimplemented!("host::testing::user_rig::UserRig::set_ci")
    }

    /// Points the rig at a registry URL.
    pub fn configure_registry(&mut self, url: &str) {
        unimplemented!("host::testing::user_rig::UserRig::configure_registry")
    }

    /// How often the install instructions of a package ran.
    pub fn package_instructions_run(&self, id: &str) -> u32 {
        unimplemented!("host::testing::user_rig::UserRig::package_instructions_run")
    }

    // ---- Paths ----------------------------------------------------------------------------

    /// The data directory.
    pub fn data_dir(&self) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::data_dir")
    }

    /// The config directory.
    pub fn config_dir(&self) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::config_dir")
    }

    /// The cache directory.
    pub fn cache_dir(&self) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::cache_dir")
    }

    /// The state directory.
    pub fn state_dir(&self) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::state_dir")
    }

    /// The scratch directory of the rig itself.
    pub fn scratch_dir(&self) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::scratch_dir")
    }

    /// The grant store file.
    pub fn grants_path(&self) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::grants_path")
    }

    /// The trust store file.
    pub fn trust_store_path(&self) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::trust_store_path")
    }

    /// The audit directory holding `audit.jsonl` and its rotations.
    pub fn audit_dir(&self) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::audit_dir")
    }

    /// Files the host created outside the XDG roots.
    pub fn files_outside_xdg_roots(&self) -> Vec<PathBuf> {
        unimplemented!("host::testing::user_rig::UserRig::files_outside_xdg_roots")
    }

    /// Every path the host created.
    pub fn paths_created_by_the_host(&self) -> Vec<PathBuf> {
        unimplemented!("host::testing::user_rig::UserRig::paths_created_by_the_host")
    }

    /// Sets the umask of the host process.
    pub fn set_umask(&mut self, mask: u32) {
        unimplemented!("host::testing::user_rig::UserRig::set_umask")
    }

    /// Sets the mode of a file.
    pub fn set_file_mode(&mut self, path: &Path, mode: u32) {
        unimplemented!("host::testing::user_rig::UserRig::set_file_mode")
    }

    /// Ages a file by `days`.
    pub fn age_file(&mut self, path: &Path, days: u32) {
        unimplemented!("host::testing::user_rig::UserRig::age_file")
    }

    /// Opens (creates) a project and returns its path.
    pub fn open_project(&mut self, name: &str) -> PathBuf {
        unimplemented!("host::testing::user_rig::UserRig::open_project")
    }

    /// Switches the server to another project.
    pub fn switch_project(&mut self, path: &Path) {
        unimplemented!("host::testing::user_rig::UserRig::switch_project")
    }

    // ---- Server and clock -----------------------------------------------------------------

    /// Starts the server.
    pub fn start_server(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::start_server")
    }

    /// Starts the server in a mode.
    pub fn start_server_mode(&mut self, mode: StartMode) {
        unimplemented!("host::testing::user_rig::UserRig::start_server_mode")
    }

    /// Restarts the server.
    pub fn restart_server(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::restart_server")
    }

    /// Stops the server.
    pub fn stop_server(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::stop_server")
    }

    /// Advances the clock.
    pub fn advance_ms(&mut self, ms: u64) {
        unimplemented!("host::testing::user_rig::UserRig::advance_ms")
    }

    /// Kills the host at a point of the next install or uninstall.
    pub fn kill_host_at(&mut self, point: KillPoint) {
        unimplemented!("host::testing::user_rig::UserRig::kill_host_at")
    }

    /// The startup trace of the last server start.
    pub fn startup_trace(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::startup_trace")
    }

    /// The uninstall trace of a plugin.
    pub fn uninstall_trace(&self, id: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::uninstall_trace")
    }

    /// Uninstalls a plugin and reports which host steps observers saw.
    pub fn uninstall_observing_steps(&mut self, id: &str) -> UninstallSteps {
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
    pub fn attach_client(&mut self, answers: Vec<DialogAnswer>) {
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
        unimplemented!("host::testing::user_rig::UserRig::grant_entries")
    }

    /// Trusts a publisher key.
    pub fn trust(&mut self, key: &KeyPair) {
        unimplemented!("host::testing::user_rig::UserRig::trust")
    }

    /// The trusted key fingerprints.
    pub fn trusted_keys(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::trusted_keys")
    }

    /// The grant-store bytes expected after a grant is given.
    pub fn expected_grants_after(&self, id: &str, grant: &str) -> Vec<u8> {
        unimplemented!("host::testing::user_rig::UserRig::expected_grants_after")
    }

    /// Sets a plugin setting.
    pub fn set_plugin_setting(&mut self, id: &str, key: &str, value: &str) {
        unimplemented!("host::testing::user_rig::UserRig::set_plugin_setting")
    }

    /// A plugin setting.
    pub fn plugin_setting(&self, id: &str, key: &str) -> Option<String> {
        unimplemented!("host::testing::user_rig::UserRig::plugin_setting")
    }

    /// Writes a file into a plugin's cache directory.
    pub fn write_cache_file(&mut self, id: &str, name: &str) {
        unimplemented!("host::testing::user_rig::UserRig::write_cache_file")
    }

    /// Writes a storage bucket of a plugin.
    pub fn write_storage_bucket(&mut self, id: &str) {
        unimplemented!("host::testing::user_rig::UserRig::write_storage_bucket")
    }

    /// Whether a file exists in a plugin's data directory.
    pub fn data_file_exists(&self, id: &str, name: &str) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::data_file_exists")
    }

    /// Reads a file of a plugin's data directory.
    pub fn read_data_file(&self, id: &str, name: &str) -> Option<String> {
        unimplemented!("host::testing::user_rig::UserRig::read_data_file")
    }

    // ---- Calls ----------------------------------------------------------------------------

    /// A plugin side makes a call needing `grant`.
    pub fn call_needing(&mut self, id: &str, side: &str, grant: &str) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::call_needing")
    }

    /// A plugin side makes a call needing a grant its manifest never listed.
    pub fn call_needing_unlisted(&mut self, id: &str, side: &str, grant: &str) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::call_needing_unlisted")
    }

    /// Starts a long call needing `grant`.
    pub fn start_long_call(&mut self, id: &str, side: &str, grant: &str) -> CallHandle {
        unimplemented!("host::testing::user_rig::UserRig::start_long_call")
    }

    /// Finishes a long call.
    pub fn finish_call(&mut self, call: CallHandle) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::finish_call")
    }

    /// The granted list a plugin side's instance saw.
    pub fn granted_list_seen_by_instance(&self, id: &str, side: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::granted_list_seen_by_instance")
    }

    /// A plugin makes an HTTP request.
    pub fn plugin_http(&mut self, id: &str, url: &str) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::plugin_http")
    }

    /// A plugin opens a raw socket.
    pub fn plugin_raw_socket(&mut self, id: &str, address: &str) -> CallOutcome {
        unimplemented!("host::testing::user_rig::UserRig::plugin_raw_socket")
    }

    /// Makes the fake network redirect one URL to another.
    pub fn script_redirect(&mut self, from: &str, to: &str) {
        unimplemented!("host::testing::user_rig::UserRig::script_redirect")
    }

    /// A plugin asks to open a link.
    pub fn plugin_opens_link(&mut self, id: &str, url: &str) {
        unimplemented!("host::testing::user_rig::UserRig::plugin_opens_link")
    }

    /// The links actually opened.
    pub fn opened_links(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::opened_links")
    }

    /// A plugin requests a URL.
    pub fn plugin_requests(&mut self, id: &str, url: &str) {
        unimplemented!("host::testing::user_rig::UserRig::plugin_requests")
    }

    /// The hosts connections were attempted to.
    pub fn connections_attempted(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::connections_attempted")
    }

    /// A plugin shows a toast.
    pub fn plugin_toast(&mut self, id: &str, text: &str) {
        unimplemented!("host::testing::user_rig::UserRig::plugin_toast")
    }

    /// The toasts visible now.
    pub fn visible_toasts(&self) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::visible_toasts")
    }

    /// Dismisses every toast.
    pub fn clear_toasts(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::clear_toasts")
    }

    /// A plugin locks user input.
    pub fn plugin_locks_input(&mut self, id: &str) {
        unimplemented!("host::testing::user_rig::UserRig::plugin_locks_input")
    }

    /// Whether user input is locked.
    pub fn input_locked(&self) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::input_locked")
    }

    /// Whether the clipboard-access indicator is visible.
    pub fn clipboard_indicator_visible(&self) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::clipboard_indicator_visible")
    }

    /// A plugin logs one line at a level.
    pub fn plugin_logs(&mut self, id: &str, level: &str, line: &str) {
        unimplemented!("host::testing::user_rig::UserRig::plugin_logs")
    }

    /// A plugin logs `bytes` bytes.
    pub fn plugin_logs_bytes(&mut self, id: &str, bytes: usize) {
        unimplemented!("host::testing::user_rig::UserRig::plugin_logs_bytes")
    }

    /// The journal lines of a plugin.
    pub fn log_lines(&self, id: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::log_lines")
    }

    /// Makes a plugin trap on its next call.
    pub fn inject_trap(&mut self, id: &str) {
        unimplemented!("host::testing::user_rig::UserRig::inject_trap")
    }

    /// How often a plugin's instance started.
    pub fn instance_starts(&self, id: &str) -> u32 {
        unimplemented!("host::testing::user_rig::UserRig::instance_starts")
    }

    /// The failure count of a plugin.
    pub fn failure_count(&self, id: &str) -> u32 {
        unimplemented!("host::testing::user_rig::UserRig::failure_count")
    }

    /// The N14 notice text for a plugin, if shown.
    pub fn n14_text(&self, id: &str) -> Option<String> {
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
        unimplemented!("host::testing::user_rig::UserRig::network_requests")
    }

    /// How often the fetcher was called.
    pub fn fetch_calls(&self) -> usize {
        unimplemented!("host::testing::user_rig::UserRig::fetch_calls")
    }

    /// Turns request logging on or off.
    pub fn set_request_logging(&mut self, on: bool) {
        unimplemented!("host::testing::user_rig::UserRig::set_request_logging")
    }

    /// Removes every network interface.
    pub fn deny_network(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::deny_network")
    }

    // ---- Extensions and panes -------------------------------------------------------------

    /// The order in which a point's contributors were called.
    pub fn call_point_order(&mut self, owner: &str, point: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::call_point_order")
    }

    /// Moves a contributor up in the Extensions screen.
    pub fn extensions_move_up(&mut self, owner: &str, point: &str, plugin: &str) {
        unimplemented!("host::testing::user_rig::UserRig::extensions_move_up")
    }

    /// Moves a contributor down in the Extensions screen.
    pub fn extensions_move_down(&mut self, owner: &str, point: &str, plugin: &str) {
        unimplemented!("host::testing::user_rig::UserRig::extensions_move_down")
    }

    /// Toggles a contributor in the Extensions screen.
    pub fn extensions_toggle(&mut self, owner: &str, point: &str, plugin: &str, on: bool) {
        unimplemented!("host::testing::user_rig::UserRig::extensions_toggle")
    }

    /// Records the current extension order of a plugin.
    pub fn extensions_remember_order(&mut self, plugin: &str) {
        unimplemented!("host::testing::user_rig::UserRig::extensions_remember_order")
    }

    /// The extension order entries kept for a plugin.
    pub fn extension_order_entries(&self, plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::user_rig::UserRig::extension_order_entries")
    }

    /// Whether a contributor is active on a point.
    pub fn extension_active(&self, owner: &str, point: &str, plugin: &str) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::extension_active")
    }

    /// The label the Extensions screen shows for a contributor.
    pub fn extension_label(&self, owner: &str, point: &str, plugin: &str) -> String {
        unimplemented!("host::testing::user_rig::UserRig::extension_label")
    }

    /// Resets a contributor after breaches.
    pub fn extension_reset(&mut self, owner: &str, point: &str, plugin: &str) {
        unimplemented!("host::testing::user_rig::UserRig::extension_reset")
    }

    /// Whether a reset is offered for a contributor.
    pub fn extension_reset_offered(&self, owner: &str, point: &str, plugin: &str) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::extension_reset_offered")
    }

    /// Runs a filter point and returns the filtered value.
    pub fn run_filter_point(&mut self, owner: &str, point: &str, input: &str) -> String {
        unimplemented!("host::testing::user_rig::UserRig::run_filter_point")
    }

    /// Runs a veto point.
    pub fn run_veto_point(&mut self, owner: &str, point: &str) -> VetoOutcome {
        unimplemented!("host::testing::user_rig::UserRig::run_veto_point")
    }

    /// Calls a service of a provider from a consumer; returns the result word.
    pub fn call_service(&mut self, consumer: &str, provider: &str, service: &str) -> String {
        unimplemented!("host::testing::user_rig::UserRig::call_service")
    }

    /// The decorations a plugin currently draws.
    pub fn decoration_count(&self, id: &str) -> usize {
        unimplemented!("host::testing::user_rig::UserRig::decoration_count")
    }

    /// Opens a plugin pane.
    pub fn open_plugin_pane(&mut self, id: &str, kind: &str) -> PaneId {
        unimplemented!("host::testing::user_rig::UserRig::open_plugin_pane")
    }

    /// Whether a pane is in the tree.
    pub fn pane_exists(&self, pane: PaneId) -> bool {
        unimplemented!("host::testing::user_rig::UserRig::pane_exists")
    }

    /// The content kind of a pane (`placeholder` once its plugin is gone).
    pub fn pane_content_kind(&self, pane: PaneId) -> String {
        unimplemented!("host::testing::user_rig::UserRig::pane_content_kind")
    }

    // ---- Policy and audit -----------------------------------------------------------------

    /// Writes the system policy file.
    pub fn write_system_policy(&mut self, toml: &str) {
        unimplemented!("host::testing::user_rig::UserRig::write_system_policy")
    }

    /// Sets the mode of the system policy file.
    pub fn set_system_policy_mode(&mut self, mode: u32) {
        unimplemented!("host::testing::user_rig::UserRig::set_system_policy_mode")
    }

    /// Makes the system policy file unreadable.
    pub fn make_system_policy_unreadable(&mut self) {
        unimplemented!("host::testing::user_rig::UserRig::make_system_policy_unreadable")
    }

    /// The audit records of one event kind.
    pub fn audit_records_of(&self, event: &str) -> Vec<serde_json::Value> {
        unimplemented!("host::testing::user_rig::UserRig::audit_records_of")
    }
}

impl Default for UserRig {
    fn default() -> UserRig {
        UserRig::new()
    }
}
