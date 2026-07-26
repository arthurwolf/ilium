//! Markdown "rendered mode" for editor panes -- ported from `mdfried`
//! (<https://github.com/benjajaja/mdfried>): standalone images and, by
//! default, headers are rasterized to pixel images and displayed via
//! `ratatui-image`'s terminal graphics protocols (Kitty/Sixel/iTerm2,
//! half-block fallback). Rendered-mode's toolbar can instead show bold,
//! styled text headers for terminals without graphics support. See
//! `document.rs` for the scope this port deliberately leaves out (no remote
//! image fetch, no mermaid/PDF, no per-terminal font matching).

pub mod chapter;
pub mod checkbox;
pub mod document;
pub mod raster;
pub mod render;
pub mod view;
