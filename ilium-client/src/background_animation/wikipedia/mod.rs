//! Wikipedia presentation lives in the client; downloads and HTML in the adapter.
mod settings;
pub use settings::{Palette, RenderMode, WikipediaSettings};

#[cfg(test)]
mod integration_tests;
mod presentation;
pub(super) mod render;
mod runtime;
mod scroll;
pub(super) use presentation::WikipediaPresentation;
pub(super) use runtime::WikipediaRuntime;
