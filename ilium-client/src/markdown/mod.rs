//! Markdown "rendered mode" for editor panes -- ported from `mdfried`
//! (<https://github.com/benjajaja/mdfried>): headers and standalone images
//! are rasterized to pixel images and displayed via `ratatui-image`'s
//! terminal graphics protocols (Kitty/Sixel/iTerm2, half-block fallback);
//! everything else is styled `ratatui` text. See `document.rs` for the
//! scope this port deliberately leaves out (no remote image fetch, no
//! mermaid/PDF, no per-terminal font matching).

pub mod checkbox;
pub mod document;
pub mod raster;
pub mod render;
pub mod view;
