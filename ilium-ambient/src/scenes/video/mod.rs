//! Video files, folders and URLs played as dithered Braille.
//!
//! ffmpeg (found through `PATH`) decodes to raw frames of exactly
//! `2 * columns x 4 * rows` dots; an owned worker thread reads them from the
//! child's pipe into a small bounded queue and `render` shows the newest frame
//! that is due. Nothing here blocks the render thread.
//!
//! Custom sources are opened directly by ffmpeg. The Germination series instead
//! fetches pinned catalogue URLs over HTTPS, verifies their encoded bytes, and
//! serves the retained RAM body to ffmpeg and ffprobe over seekable loopback HTTP.
//! Germination media never uses local source files or a disk cache. ffmpeg and
//! ffprobe are external LGPL/GPL programs invoked as child processes only.

mod command;
mod convert;
mod diagnostic;
mod discover;
mod ffmpeg;
mod http_range;
mod player;
mod ram;
mod render;
mod schedule;
mod series;
mod settings;
#[cfg(test)]
mod tests;

pub use render::VideoScene;
pub use settings::{VideoSeries, VideoSettings};
