//! Video files, folders and URLs played as dithered Braille.
//!
//! ffmpeg (found through `PATH`) decodes to raw frames of exactly
//! `2 * columns x 4 * rows` dots; an owned worker thread reads them from the
//! child's pipe into a small bounded queue and `render` shows the newest frame
//! that is due. Nothing here blocks the render thread.
//!
//! Data-source notes: no network service is used beyond the URL the user
//! types; ffmpeg opens it directly (http/https only, see
//! <https://ffmpeg.org/ffmpeg-protocols.html>). ffmpeg and ffprobe are
//! external programs under the LGPL/GPL, invoked as child processes only.

mod command;
mod convert;
mod discover;
mod ffmpeg;
mod player;
mod render;
mod schedule;
mod settings;
#[cfg(test)]
mod tests;

pub use render::VideoScene;
pub use settings::VideoSettings;
