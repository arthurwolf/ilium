//! Plugin traits and the host handle (manual: rust-sdk.md "Traits", "Calling the host").

use crate::generated::anim::FrameInput;
use crate::generated::exports::{ExportLifecycle, HookCall, HookReply, ServiceCall, ServiceReply};
use crate::generated::host::{
    HostAudio, HostCatalogs, HostClientRequests, HostClipboard, HostCodec, HostCommands,
    HostGeocode, HostHelp, HostHttpApi, HostIntentsHost, HostKernelSettings, HostKeymap,
    HostLocation, HostMedia, HostPanes, HostPlugins, HostSelfDescribe, HostSelfLimits,
    HostSessions, HostSounds, HostTelemetry, HostTree, HostUiHost, HostVector, HostWorkspace,
};
use crate::generated::host::{
    HostCli, HostClock, HostExtensions, HostFs, HostHttp, HostInference, HostJobs, HostJournal,
    HostKeys, HostLog, HostProcess, HostServices, HostSettings, HostStorage, HostTimers,
    HostTopics, InstanceState,
};
use crate::generated::primitives::{HostAgent, HostBasic, HostEditor, HostTerminal};
use crate::generated::types::{
    Activation, DeactivationReason, ErrorCode, Event, PluginError, Submission,
};

/// The lifecycle state of an instance, as `host::plugins` reports it (reference-wit.md section 5.31).
pub type LifecycleState = InstanceState;

/// Page trait `Lifecycle` (rust-sdk.md "Traits"): activate, and deactivate with a default that saves no state.
pub trait Lifecycle {
    /// Called once per instance before any event.
    fn activate(&mut self, context: Activation) -> Result<(), PluginError>;

    /// Called with the reason; the returned bytes arrive as `restored` in the next activation.
    fn deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
}
impl<T: Lifecycle> ExportLifecycle for T {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        Lifecycle::activate(self, context)
    }
    fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
        Lifecycle::deactivate(self, reason)
    }
}
/// Page trait `Annotator` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportAnnotator as Annotator;
/// Page trait `CatalogProvider` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportCatalogProvider as CatalogProvider;
/// Page trait `Detector` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportDetector as Detector;
/// Page trait `Handler`: `on_events` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportHandler as Handler;
/// Page trait `HookHandler` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportHookHandler as HookHandler;
/// Page trait `InferenceProvider` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportInferenceProvider as InferenceProvider;
/// Page trait `JobRunner` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportJobRunner as JobRunner;
/// Page trait `MenuProvider` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportMenuProvider as MenuProvider;
/// Page trait `Migrator` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportMigrator as Migrator;
/// Page trait `Renderer` for scene renderers in component mode (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportRenderer as Renderer;
/// Page trait `ServiceProvider` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportServiceProvider as ServiceProvider;
/// Page trait `SettingsValidate` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportSettingsValidate as SettingsValidate;
/// Page trait `TokenProvider` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportTokenProvider as TokenProvider;
/// Page trait `Views`: `render` and `on_action` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportViews as Views;
/// Page trait `VirtualRowsProvider` (rust-sdk.md "Traits").
pub use crate::generated::exports::ExportVirtualRowsProvider as VirtualRowsProvider;

/// Routes an event batch to the plugin's `Handler` when it has one. `#[ilium_plugin]` writes
/// `(&mut HandlerProbe(plugin)).dispatch_events(batch)` with both traits below in scope: method
/// lookup finds `DispatchToHandler` first (it needs no further reference) and falls back to
/// `DispatchToNobody` only when the plugin type does not implement `Handler`.
#[doc(hidden)]
pub struct HandlerProbe<'a, T>(pub &'a mut T);

/// The `Handler` arm of `HandlerProbe`.
#[doc(hidden)]
pub trait DispatchToHandler {
    /// Forwards the batch to `Handler::on_events`.
    fn dispatch_events(&mut self, batch: Vec<Event>) -> Vec<Submission>;
}
impl<T: Handler> DispatchToHandler for HandlerProbe<'_, T> {
    fn dispatch_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        Handler::on_events(self.0, batch)
    }
}

/// The fallback arm of `HandlerProbe`: the plugin handles no event.
#[doc(hidden)]
pub trait DispatchToNobody {
    /// Drops the batch and submits nothing.
    fn dispatch_events(&mut self, batch: Vec<Event>) -> Vec<Submission>;
}
impl<T> DispatchToNobody for &mut HandlerProbe<'_, T> {
    fn dispatch_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// Every plugin struct implements `Default` and the lifecycle trait (rust-sdk.md "Plugin structure").
pub trait Plugin: Default + Lifecycle {}
impl<T: Default + Lifecycle> Plugin for T {}

/// All host imports in one trait (rust-sdk.md "Calling the host"): every generated `Host*` trait the
/// SDK exposes, so `host().fetch(..)`, `host().read(..)` and the other calls resolve on the handle.
pub trait HostCalls:
    HostLog
    + HostClock
    + HostTimers
    + HostStorage
    + HostSettings
    + HostFs
    + HostHttp
    + HostJobs
    + HostProcess
    + HostInference
    + HostCli
    + HostKeys
    + HostTopics
    + HostServices
    + HostExtensions
    + HostAgent
    + HostJournal
    + HostTree
    + HostPanes
    + HostIntentsHost
    + HostCommands
    + HostUiHost
    + HostSessions
    + HostKernelSettings
    + HostKeymap
    + HostHelp
    + HostPlugins
    + HostWorkspace
    + HostClipboard
    + HostTelemetry
    + HostSounds
    + HostClientRequests
    + HostHttpApi
    + HostCatalogs
    + HostVector
    + HostCodec
    + HostGeocode
    + HostLocation
    + HostMedia
    + HostAudio
    + HostSelfLimits
    + HostSelfDescribe
    + HostTerminal
    + HostBasic
    + HostEditor
{
}
impl<T: ?Sized> HostCalls for T where
    T: HostLog
        + HostClock
        + HostTimers
        + HostStorage
        + HostSettings
        + HostFs
        + HostHttp
        + HostJobs
        + HostProcess
        + HostInference
        + HostCli
        + HostKeys
        + HostTopics
        + HostServices
        + HostExtensions
        + HostAgent
        + HostJournal
        + HostTree
        + HostPanes
        + HostIntentsHost
        + HostCommands
        + HostUiHost
        + HostSessions
        + HostKernelSettings
        + HostKeymap
        + HostHelp
        + HostPlugins
        + HostWorkspace
        + HostClipboard
        + HostTelemetry
        + HostSounds
        + HostClientRequests
        + HostHttpApi
        + HostCatalogs
        + HostVector
        + HostCodec
        + HostGeocode
        + HostLocation
        + HostMedia
        + HostAudio
        + HostSelfLimits
        + HostSelfDescribe
        + HostTerminal
        + HostBasic
        + HostEditor
{
}

/// The host handle that `host()` returns; the testkit FakeHost implements it (reference-test-harness.md).
pub trait HostApi: HostCalls {}
impl<T: ?Sized + HostCalls> HostApi for T {}

/// The host handle of the current instance (rust-sdk.md "Calling the host").
pub fn host() -> Box<dyn HostApi> {
    let installed = INSTALLED_HOST.with(|slot| slot.borrow().clone());
    Box::new(HostHandle { installed })
}

/// Error every fallible call returns while no backend is installed.
pub fn no_host() -> PluginError {
    PluginError::new(
        ErrorCode::Unsupported,
        "no host is installed on this thread",
    )
}

type Backend = std::rc::Rc<std::cell::RefCell<Box<dyn HostApi>>>;

/// Handle returned by `host()`; clones share the installed host of the thread.
pub struct HostHandle {
    installed: Option<Backend>,
}

impl HostHandle {
    /// A handle with no backend: every call answers as on a thread without a host. The wasm32
    /// glue uses it for the host interfaces a component has no grant for.
    pub fn detached() -> Self {
        HostHandle { installed: None }
    }

    /// True when a host was installed with `set_host` on this thread.
    pub fn is_installed(&self) -> bool {
        self.installed.is_some()
    }

    /// The installed backend that receives the calls, when there is one.
    pub(crate) fn backend(&self) -> Option<&Backend> {
        self.installed.as_ref()
    }
}

thread_local! {
    static INSTALLED_HOST: std::cell::RefCell<Option<Backend>> =
        const { std::cell::RefCell::new(None) };
}

/// Installs the host handle for the current thread; the testkit calls this in native mode. A
/// `Box<dyn HostApi>` becomes the backend of every call; any other value only marks a host as
/// present and its calls report `unsupported`.
pub fn set_host(handle: Box<dyn std::any::Any>) {
    let backend = match handle.downcast::<Box<dyn HostApi>>() {
        Ok(backend) => backend,
        Err(_) => Box::new(Box::new(HostHandle { installed: None }) as Box<dyn HostApi>),
    };
    INSTALLED_HOST.with(|slot| {
        *slot.borrow_mut() = Some(std::rc::Rc::new(std::cell::RefCell::new(*backend)));
    });
}

/// Removes the installed host of the current thread.
pub fn clear_host() {
    INSTALLED_HOST.with(|slot| *slot.borrow_mut() = None);
}

/// Pixel buffer a native scene renderer draws into (reference-test-harness.md "FakeScene").
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NativeSurface {
    /// Columns of the surface.
    pub columns: u16,
    /// Rows of the surface.
    pub rows: u16,
    /// One `(glyph, foreground rgb, background rgb)` per cell, row-major.
    pub cells: Vec<(char, u32, u32)>,
}

/// Scene renderer for native mode; `FakeScene::new::<R>` drives it (reference-test-harness.md "FakeScene").
pub trait NativeRenderer: Sized {
    /// Builds the renderer from the scene spec JSON.
    fn new(spec_json: &str) -> Self;
    /// Draws one frame into the surface.
    fn render(&mut self, frame: &FrameInput, surface: &mut NativeSurface);
    /// Receives the shared palette as RGB values (palette contract in CLAUDE.md).
    fn set_palette(&mut self, palette_rgb: Vec<u32>);
    /// Target frame rate.
    fn frames_per_second(&self) -> u32;
}
/// Alias used by the plan (test-plan.md section 1.3).
pub use NativeRenderer as SceneRenderer;

/// Pixels per terminal column of the Braille field a scene draws into.
pub const FIELD_PIXELS_PER_COLUMN: u32 = 2;
/// Pixels per terminal row of the Braille field a scene draws into.
pub const FIELD_PIXELS_PER_ROW: u32 = 4;

/// The pixel field of one frame: two pixels per terminal column and four per row (the Braille
/// field of the ambient scenes), one grey value per pixel (0 is no ink, 255 full ink), and an
/// optional colour per terminal cell. `render` of a [`SceneProgram`] fills it; the component glue
/// writes it to the host's frame surface, and the native harness turns it into Braille cells.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SceneField {
    /// Columns of the field in terminal cells.
    pub columns: u16,
    /// Rows of the field in terminal cells.
    pub rows: u16,
    /// One grey value per pixel, row by row; `width() * height()` long.
    pub pixels: Vec<u8>,
    /// One `0xRRGGBB` value per terminal cell, row by row; only read when the program
    /// reports `uses_cell_colors`.
    pub cell_colors: Vec<u32>,
    /// The glyph plane: terminal text the host draws in place of the Braille dots of a cell,
    /// as `(column, row, character)`. Cleared before every frame; the component glue writes it
    /// with `write-glyphs`. Colours come from `cell_colors`.
    pub glyphs: Vec<(u16, u16, char)>,
    /// The pointer over the field, set by the component glue before `render` when the program
    /// reports `wants_pointer` and the pointer is inside the field; `None` otherwise.
    pub pointer: Option<FieldPointer>,
    /// What the workspace draws under the field (true where text is drawn), set by the
    /// component glue before `render` when the program reports `wants_occupancy`; `None`
    /// otherwise.
    pub occupancy: Option<FieldOccupancy>,
}

/// The pointer a scene sees: its cell, the dot inside the cell and whether a button is down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldPointer {
    pub column: u16,
    pub row: u16,
    /// Horizontal dot inside the cell, 0..2.
    pub sub_x: u8,
    /// Vertical dot inside the cell, 0..4.
    pub sub_y: u8,
    pub pressed: bool,
}

/// What the workspace draws under the field. `cells` holds the occupied plane (one value per
/// terminal cell, row by row, true where something other than empty space is drawn) and, when
/// it is twice as long, a second plane of the same shape marking the cells that hold a
/// foreground character anchor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldOccupancy {
    pub columns: u16,
    pub rows: u16,
    pub cells: Vec<bool>,
}

impl FieldOccupancy {
    /// Cells per plane.
    fn plane(&self) -> usize {
        usize::from(self.columns) * usize::from(self.rows)
    }

    /// Whether something is drawn in cell (`column`, `row`); false outside the field.
    pub fn is_occupied(&self, column: u16, row: u16) -> bool {
        column < self.columns
            && row < self.rows
            && self
                .cells
                .get(usize::from(row) * usize::from(self.columns) + usize::from(column))
                .copied()
                .unwrap_or(false)
    }

    /// Whether cell (`column`, `row`) holds a foreground character anchor; false when the host
    /// sent no second plane.
    pub fn is_character(&self, column: u16, row: u16) -> bool {
        column < self.columns
            && row < self.rows
            && self.cells.len() >= 2 * self.plane()
            && self
                .cells
                .get(
                    self.plane()
                        + usize::from(row) * usize::from(self.columns)
                        + usize::from(column),
                )
                .copied()
                .unwrap_or(false)
    }
}

impl SceneField {
    /// A cleared field of `columns` x `rows` cells.
    pub fn new(columns: u16, rows: u16) -> Self {
        let mut field = Self::default();
        field.resize(columns, rows);
        field
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        usize::from(self.columns) * FIELD_PIXELS_PER_COLUMN as usize
    }

    /// Height in pixels.
    pub fn height(&self) -> usize {
        usize::from(self.rows) * FIELD_PIXELS_PER_ROW as usize
    }

    /// Resizes to `columns` x `rows` cells and clears every pixel and colour.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.columns = columns;
        self.rows = rows;
        self.pixels.clear();
        self.pixels.resize(self.width() * self.height(), 0);
        self.cell_colors.clear();
        self.cell_colors
            .resize(usize::from(columns) * usize::from(rows), 0x00ff_ffff);
        self.glyphs.clear();
    }

    /// Clears every pixel to no ink and drops the glyph plane.
    pub fn clear(&mut self) {
        self.pixels.fill(0);
        self.glyphs.clear();
    }

    /// Puts a character of the glyph plane into cell (`column`, `row`); a cell outside the
    /// field is ignored.
    pub fn put_glyph(&mut self, column: u16, row: u16, character: char) {
        if column < self.columns && row < self.rows {
            self.glyphs.push((column, row, character));
        }
    }

    /// The Braille character of cell (`column`, `row`): a dot is raised where the pixel is
    /// at least half inked. A blank cell is a space.
    pub fn braille(&self, column: u16, row: u16) -> char {
        // Dot numbers of the Braille block, by (sub column, sub row).
        const BITS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
        let width = self.width();
        let mut bits = 0u32;
        for (sub_x, column_bits) in BITS.iter().enumerate() {
            for (sub_y, bit) in column_bits.iter().enumerate() {
                let x = usize::from(column) * FIELD_PIXELS_PER_COLUMN as usize + sub_x;
                let y = usize::from(row) * FIELD_PIXELS_PER_ROW as usize + sub_y;
                if self
                    .pixels
                    .get(y * width + x)
                    .is_some_and(|ink| *ink >= 128)
                {
                    bits |= bit;
                }
            }
        }
        if bits == 0 {
            ' '
        } else {
            char::from_u32(0x2800 + bits).unwrap_or(' ')
        }
    }

    /// The field as native cells: `(glyph, cell colour, 0)`, where the glyph is the glyph-plane
    /// character of the cell when the program wrote one, else the Braille character.
    pub fn to_native_cells(&self) -> Vec<(char, u32, u32)> {
        let mut cells = Vec::with_capacity(usize::from(self.columns) * usize::from(self.rows));
        for row in 0..self.rows {
            for column in 0..self.columns {
                let index = usize::from(row) * usize::from(self.columns) + usize::from(column);
                let colour = self.cell_colors.get(index).copied().unwrap_or(0x00ff_ffff);
                cells.push((self.braille(column, row), colour, 0));
            }
        }
        for (column, row, character) in &self.glyphs {
            let index = usize::from(*row) * usize::from(self.columns) + usize::from(*column);
            if let Some(cell) = cells.get_mut(index) {
                cell.0 = *character;
            }
        }
        cells
    }
}

/// A scene that draws into the pixel field of the host frame surface. This is the contract of
/// the scene component: `#[ilium_plugin(side = "scene")]` exports `ilium:exports/renderer` and
/// forwards each call of one `scene-renderer` resource to a value of the plugin type.
///
/// PALETTE (future plugin contract): the palette arrives in the spec (`palette`), in every
/// [`FrameInput`] and through `set_palette`; a scene with natural colours must follow it.
pub trait SceneProgram: Sized {
    /// Builds the scene from the spec JSON: `scene` (id), `settings`, `palette`, `mode`.
    fn new(spec_json: &str) -> Self;
    /// Draws one frame. `frame.width` and `frame.height` are the field size in pixels and
    /// equal `field.width()` and `field.height()`.
    fn render(&mut self, frame: &FrameInput, field: &mut SceneField);
    /// Receives a later change of the shared palette.
    fn set_palette(&mut self, palette_rgb: Vec<u32>);
    /// Target frame rate; the host never asks for more than the manifest's cap.
    fn frames_per_second(&self) -> u32;
    /// Whether the scene is ready, still preparing what it draws from, or failed.
    fn readiness(&self) -> crate::generated::anim::Readiness {
        crate::generated::anim::Readiness::Ready
    }
    /// Whether the scene wants the pointer position.
    fn wants_pointer(&self) -> bool {
        false
    }
    /// Whether the scene wants the screen occupancy.
    fn wants_occupancy(&self) -> bool {
        false
    }
    /// Whether `render` fills `field.cell_colors`.
    fn uses_cell_colors(&self) -> bool {
        false
    }
    /// Applies new settings; `false` asks the host to restart the scene.
    fn reconfigure(&mut self, _settings_json: &str) -> bool {
        false
    }
    /// One status line for the Settings screen, when the scene has something to say.
    fn status(&self) -> Option<String> {
        None
    }
}

/// What `#[ilium_plugin]` emits in native mode (rust-sdk.md "The ilium_plugin macro").
pub trait NativeEntry: Sized {
    /// The side the plugin declared: "server", "client" or "scene".
    const SIDE: &'static str;
    /// The export interface names the plugin declared.
    const EXPORTS: &'static [&'static str];
    /// Dispatches `activate` to the plugin's lifecycle implementation.
    fn native_activate(&mut self, context: Activation) -> Result<(), PluginError>;
    /// Dispatches `deactivate`.
    fn native_deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>>;
    /// Dispatches `on-events`.
    fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission>;
    /// Dispatches `job-runner.run`; `None` when the plugin does not export `job-runner`.
    fn native_run_job(
        &mut self,
        kind: String,
        input: Vec<u8>,
    ) -> Option<Result<Vec<u8>, PluginError>> {
        let _ = (kind, input);
        None
    }
    /// Dispatches `service-provider.handle`; `None` when the plugin does not export `service-provider`.
    fn native_handle_service(&mut self, call: ServiceCall) -> Option<ServiceReply> {
        let _ = call;
        None
    }
    /// Dispatches `hook-handler.handle-hook`; `None` when the plugin does not export `hook-handler`.
    fn native_handle_hook(&mut self, call: HookCall) -> Option<Result<HookReply, PluginError>> {
        let _ = call;
        None
    }
    /// Dispatches `views.render`; `None` when the plugin does not export `views`.
    fn native_render(
        &mut self,
        view: crate::types::ViewTarget,
        area: crate::types::Area,
    ) -> Option<crate::types::UiTree> {
        let _ = (view, area);
        None
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::generated::types::{ErrorCode, PluginId};

    fn activation_for_test() -> Activation {
        Activation {
            plugin: PluginId("test.plugin".to_string()),
            api: "ilium:plugin@0.1.0".to_string(),
            instance: 1,
            side: "server".to_string(),
            settings_json: "{}".to_string(),
            granted: Vec::new(),
            restored: None,
        }
    }

    fn plugin_error(code: ErrorCode, message: &str) -> PluginError {
        PluginError {
            code,
            message: message.to_string(),
            retry_after_ms: None,
        }
    }

    /// Overrides only `activate`, so `deactivate` runs the trait default.
    #[derive(Default)]
    struct QuietPlugin;

    impl Lifecycle for QuietPlugin {
        fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }
    }

    /// Manual: `Lifecycle`: `deactivate` has a default that saves no state
    #[test]
    fn default_deactivate_saves_no_state_for_every_delivered_reason() {
        let mut plugin = QuietPlugin;
        for reason in [
            DeactivationReason::Shutdown,
            DeactivationReason::Disabled,
            DeactivationReason::Upgrade,
            DeactivationReason::Revoked,
            DeactivationReason::Granted,
        ] {
            assert_eq!(Lifecycle::deactivate(&mut plugin, reason), None);
        }
    }

    #[derive(Default)]
    struct CountingPlugin {
        activations: u32,
    }

    impl Lifecycle for CountingPlugin {
        fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            self.activations += 1;
            Ok(())
        }
    }

    /// `ExportLifecycle::activate` ("Called once per instance, before any event")
    #[test]
    fn export_lifecycle_activate_reaches_the_plugin_once() {
        let mut plugin = CountingPlugin::default();
        ExportLifecycle::activate(&mut plugin, activation_for_test()).expect("activation succeeds");
        assert_eq!(plugin.activations, 1);
    }

    struct RefusingPlugin;

    impl Lifecycle for RefusingPlugin {
        fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Err(plugin_error(
                ErrorCode::InvalidArgument,
                "settings do not parse",
            ))
        }
    }

    /// Manual: "An error stops the activation and counts as a failure"
    #[test]
    fn export_lifecycle_activate_returns_the_plugin_error_unchanged() {
        let mut plugin = RefusingPlugin;
        let outcome = ExportLifecycle::activate(&mut plugin, activation_for_test());
        assert_eq!(
            outcome,
            Err(plugin_error(
                ErrorCode::InvalidArgument,
                "settings do not parse"
            ))
        );
    }

    #[derive(Default)]
    struct KeepingPlugin;

    impl Lifecycle for KeepingPlugin {
        fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }

        fn deactivate(&mut self, reason: DeactivationReason) -> Option<Vec<u8>> {
            match reason {
                DeactivationReason::Upgrade => Some(vec![1, 2, 3]),
                _ => None,
            }
        }
    }

    /// restored") and reference-test-harness.md [DEV-424] (the default reason is `shutdown`, which keeps no bytes here)
    #[test]
    fn export_lifecycle_deactivate_returns_restore_bytes_from_the_plugin() {
        let mut plugin = KeepingPlugin;
        assert_eq!(
            ExportLifecycle::deactivate(&mut plugin, DeactivationReason::Upgrade),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            ExportLifecycle::deactivate(&mut plugin, DeactivationReason::Shutdown),
            None
        );
    }

    fn accept_host_api(_handle: &dyn HostApi) {}

    /// plugin.rs `set_host` ("Installs the host handle for the current thread")
    #[test]
    fn set_host_installs_the_handle_that_host_returns() {
        set_host(Box::new(()));
        let handle = host();
        accept_host_api(&*handle);
    }

    struct SceneEntry;

    impl NativeEntry for SceneEntry {
        const SIDE: &'static str = "scene";
        const EXPORTS: &'static [&'static str] = &["renderer"];

        fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }

        fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
            None
        }

        fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
            Vec::new()
        }
    }

    /// the side is "server", "client" or "scene"
    #[test]
    fn native_entry_side_is_server_client_or_scene() {
        assert!(matches!(SceneEntry::SIDE, "server" | "client" | "scene"));
        assert_eq!(SceneEntry::EXPORTS, &["renderer"]);
    }

    struct DeniedEntry;

    impl NativeEntry for DeniedEntry {
        const SIDE: &'static str = "server";
        const EXPORTS: &'static [&'static str] = &[];

        fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Err(plugin_error(ErrorCode::NotPermitted, "grant not requested"))
        }

        fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
            None
        }

        fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
            Vec::new()
        }
    }

    /// reached through the native dispatch of `NativeEntry::native_activate`
    /// Manual: "An error stops the activation and counts as a failure"
    #[test]
    fn native_activate_error_reaches_the_caller() {
        let mut entry = DeniedEntry;
        let outcome = NativeEntry::native_activate(&mut entry, activation_for_test());
        assert_eq!(
            outcome,
            Err(plugin_error(ErrorCode::NotPermitted, "grant not requested"))
        );
    }

    #[derive(Default)]
    struct SolidRenderer {
        palette_rgb: Vec<u32>,
    }

    impl NativeRenderer for SolidRenderer {
        fn new(_spec_json: &str) -> Self {
            Self::default()
        }

        fn render(&mut self, _frame: &FrameInput, surface: &mut NativeSurface) {
            let colour = self.palette_rgb.first().copied().unwrap_or(0);
            for cell in surface.cells.iter_mut() {
                cell.1 = colour;
            }
        }

        fn set_palette(&mut self, palette_rgb: Vec<u32>) {
            self.palette_rgb = palette_rgb;
        }

        fn frames_per_second(&self) -> u32 {
            30
        }
    }

    fn blank_surface(columns: u16, rows: u16) -> NativeSurface {
        NativeSurface {
            columns,
            rows,
            cells: vec![(' ', 0, 0); usize::from(columns) * usize::from(rows)],
        }
    }

    /// [DEV-560] (`frame` returns the surface cells in the scene size)
    #[test]
    fn palette_set_before_a_frame_colours_that_frame() {
        let mut renderer = SolidRenderer::new("{}");
        renderer.set_palette(vec![0x00ff00]);
        let mut surface = blank_surface(2, 1);
        renderer.render(&FrameInput::default(), &mut surface);
        assert_eq!(surface.cells.len(), 2);
        assert!(surface.cells.iter().all(|cell| cell.1 == 0x00ff00));
    }
}
