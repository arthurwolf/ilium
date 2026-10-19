//! Smart-copy-lane testing hooks on `FakeHost` (project files the picker may open).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.

use ilium_plugin_sdk::testing::FakeHost;

/// Handle on the project files the picker checks for existence.
pub struct Files<'a>(pub &'a mut FakeHost);

impl Files<'_> {
    /// Marks `path` as an existing file.
    pub fn add_existing(&mut self, path: &str) {
        self.0.fs().seed(path, &[]);
    }
}

/// Smart-copy-lane hooks. Implemented for `FakeHost`.
pub trait SmartCopyHostExt {
    /// The project files the picker may resolve.
    fn files(&mut self) -> Files<'_>;
}

impl SmartCopyHostExt for FakeHost {
    fn files(&mut self) -> Files<'_> {
        Files(self)
    }
}
