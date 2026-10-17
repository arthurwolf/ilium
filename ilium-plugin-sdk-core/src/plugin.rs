//! Plugin traits and the host handle (manual: rust-sdk.md "Traits", "Calling the host").

use crate::generated::anim::FrameInput;
use crate::generated::exports::ExportLifecycle;
use crate::generated::host::InstanceState;
use crate::generated::types::{Activation, DeactivationReason, Event, PluginError, Submission};

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

/// Every plugin struct implements `Default` and the lifecycle trait (rust-sdk.md "Plugin structure").
pub trait Plugin: Default + Lifecycle {}
impl<T: Default + Lifecycle> Plugin for T {}

/// All host imports in one trait (rust-sdk.md "Calling the host"). The skeleton keeps it empty: the
/// implementation phase makes it a supertrait of every generated `Host*` trait with a blanket impl.
pub trait HostCalls {}
impl<T: ?Sized> HostCalls for T {}

/// The host handle that `host()` returns; the testkit FakeHost implements it (reference-test-harness.md).
pub trait HostApi: HostCalls {}
impl<T: ?Sized + HostCalls> HostApi for T {}

/// The host handle of the current instance (rust-sdk.md "Calling the host").
pub fn host() -> Box<dyn HostApi> {
    unimplemented!("rust-sdk.host")
}

/// Installs the host handle for the current thread; the testkit calls this in native mode.
pub fn set_host(_handle: Box<dyn HostApi>) {
    unimplemented!("rust-sdk.set-host")
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
}
