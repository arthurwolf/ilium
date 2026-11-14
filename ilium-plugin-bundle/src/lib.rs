//! The core plugins of Ilium as WebAssembly components.
//!
//! `build.rs` compiles every crate under `plugins/core/` to `wasm32-wasip2`, one component per
//! side the plugin's `manifest.json` declares, stamps each with the `ilium:api-version` section
//! and writes the catalogue included below. This crate only exposes that catalogue; loading,
//! validating and precompiling the components is the plugin host's work.

/// One help topic a core plugin ships (`contributes.help` of its manifest).
#[derive(Debug, Clone, Copy)]
pub struct HelpTopic {
    /// The topic id inside the plugin; the global id is `<plugin id>/<id>`.
    pub id: &'static str,
    /// The title of the manifest entry.
    pub title: &'static str,
    /// The text of the help file.
    pub markdown: &'static str,
}

/// The schema file of one service a core plugin provides (gap G13).
#[derive(Debug, Clone, Copy)]
pub struct ServiceSchema {
    /// The service name, `<plugin id>/<name>`.
    pub service: &'static str,
    /// The text of the schema file: method name to `params` and `result` schemas.
    pub text: &'static str,
}

/// One core plugin: its manifest and the component of each side it has.
#[derive(Debug, Clone, Copy)]
pub struct BundledPlugin {
    /// The plugin id of the manifest, for example `ilium.core.layout`.
    pub id: &'static str,
    /// The directory name under `plugins/core/`.
    pub directory: &'static str,
    /// The text of `manifest.json`.
    pub manifest: &'static str,
    /// The server component, when the manifest has a server side and it was built.
    pub server: Option<&'static [u8]>,
    /// The client component, when the manifest has a client side and it was built.
    pub client: Option<&'static [u8]>,
    /// The scene component, when the manifest has a scene side and it was built.
    pub scene: Option<&'static [u8]>,
    /// The help topics of the manifest whose file exists in the plugin directory.
    pub help: &'static [HelpTopic],
    /// The schema files of the services the plugin provides.
    pub schemas: &'static [ServiceSchema],
}

impl BundledPlugin {
    /// The component of one side (`"server"`, `"client"` or `"scene"`).
    pub fn component(&self, side: &str) -> Option<&'static [u8]> {
        match side {
            "server" => self.server,
            "client" => self.client,
            "scene" => self.scene,
            _ => None,
        }
    }
}

/// A component the catalogue does not hold: (plugin directory, side).
pub type Absent = (&'static str, &'static str);

include!(concat!(env!("OUT_DIR"), "/catalogue.rs"));

/// Every core plugin, in directory order.
pub fn plugins() -> &'static [BundledPlugin] {
    PLUGINS
}

/// The core plugin with this id.
pub fn find(id: &str) -> Option<&'static BundledPlugin> {
    PLUGINS.iter().find(|plugin| plugin.id == id)
}
