//! Host implementation of `ilium:host/keymap` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostKeymap` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::keymap::{Binding, HostKeymap};

/// Host side of the `keymap` interface.
#[derive(Debug)]
pub struct KeymapImport;

impl HostKeymap for KeymapImport {
    fn bindings(&mut self) -> Vec<Binding> {
        unimplemented!("host::imports::keymap::bindings")
    }

    fn chord_label(&mut self, _chord: String) -> String {
        unimplemented!("host::imports::keymap::chord_label")
    }
}
