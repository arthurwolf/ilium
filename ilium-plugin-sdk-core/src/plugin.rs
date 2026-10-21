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

    /// REQ: rust-sdk.md "Traits" (`Lifecycle`: `deactivate` has a default that saves no state)
    #[test]
    fn default_deactivate_saves_no_state_for_every_delivered_reason() {
        let mut plugin = QuietPlugin::default();
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

    /// REQ: rust-sdk.md "Exports and entry points" (`lifecycle` is always exported) and exports.rs
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
            Err(plugin_error(ErrorCode::InvalidArgument, "settings do not parse"))
        }
    }

    /// REQ: exports.rs `ExportLifecycle::activate` ("An error stops the activation and counts as a failure")
    #[test]
    fn export_lifecycle_activate_returns_the_plugin_error_unchanged() {
        let mut plugin = RefusingPlugin;
        let outcome = ExportLifecycle::activate(&mut plugin, activation_for_test());
        assert_eq!(
            outcome,
            Err(plugin_error(ErrorCode::InvalidArgument, "settings do not parse"))
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

    /// REQ: exports.rs `ExportLifecycle::deactivate` ("The returned bytes arrive in the next activation as
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

    /// REQ: rust-sdk.md "Calling the host" (named at plugin.rs:64; no heading of that name in the page) and
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

    /// REQ: rust-sdk.md "The ilium_plugin macro" (named at plugin.rs:108; no heading of that name in the page):
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

    /// REQ: exports.rs `ExportLifecycle::activate` ("An error stops the activation and counts as a failure"),
    /// reached through the native dispatch of `NativeEntry::native_activate`
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

    /// REQ: reference-test-harness.md [DEV-562] (`set_palette` reaches the renderer before the next frame) and
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
