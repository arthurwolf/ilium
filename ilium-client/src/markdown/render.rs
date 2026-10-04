//! Turns a parsed `document::Document` into a `RenderedDocument`: headers
//! and local images resolved into `ratatui_image::protocol::Protocol`s
//! (ready to hand straight to `ratatui_image::Image` widgets), everything
//! else passed through unchanged as styled text.
//!
//! Production preparation runs on the shared CPU bank; local image reads run
//! on its I/O bank. The synchronous entrypoint is retained for focused tests.

use std::sync::Arc;

use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::{FilterType, Resize};

use super::document::{Block, Document, ImagePath};
use super::raster::HeaderRasterizer;

/// How Rendered-mode headings reach the terminal. Rasterized headings use a
/// terminal graphics protocol, while text headings keep the same Markdown
/// structure readable in terminals without graphics support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeadingRendering {
    #[default]
    Rasterized,
    PlainText,
}

/// One block of a document, ready to render: text and authored spacing
/// passed through as-is, or a header/image converted to a graphics protocol.
pub enum RenderedBlock {
    Text(Arc<Vec<Line<'static>>>),
    /// Source-authored vertical space. It remains a first-class block so
    /// content-height and scroll calculations count the same rows the
    /// viewport leaves unpainted.
    BlankLines(Arc<Vec<Line<'static>>>),
    /// Rasterized header image, two terminal rows tall.
    Header(Protocol),
    /// A loaded content image, sized to fit within the document width.
    Image(Protocol),
    /// A header/image that couldn't be rasterized or loaded (decode
    /// failure, missing file, unsupported remote URL) -- shown as plain
    /// text so one broken image doesn't blank out the rest of the
    /// document.
    Placeholder(Line<'static>),
}

pub struct RenderedDocument {
    pub blocks: Vec<RenderedBlock>,
    pub(crate) layout: Option<PreparedLayout>,
}

pub(crate) struct PreparedLayout {
    pub width: u16,
    pub line_display: crate::config::LineDisplay,
    pub buffers: Vec<Option<ratatui::buffer::Buffer>>,
    pub heights: Vec<u16>,
    pub total_height: u16,
}

/// Content images are capped to this many terminal rows so one huge
/// screenshot can't push the rest of the document off-screen.
const MAX_IMAGE_ROWS: u16 = 24;

pub fn render(
    document: &Document,
    picker: &Picker,
    rasterizer: &mut HeaderRasterizer,
    width_cols: u16,
    heading_rendering: HeadingRendering,
) -> RenderedDocument {
    let cell_px = super::raster::cell_pixel_size(picker);
    let blocks = document
        .blocks
        .iter()
        .map(|block| {
            render_block(
                block,
                picker,
                rasterizer,
                width_cols,
                cell_px,
                heading_rendering,
            )
        })
        .collect();
    RenderedDocument {
        blocks,
        layout: None,
    }
}

fn render_block(
    block: &Block,
    picker: &Picker,
    rasterizer: &mut HeaderRasterizer,
    width_cols: u16,
    cell_px: (u16, u16),
    heading_rendering: HeadingRendering,
) -> RenderedBlock {
    match block {
        Block::Text(lines) => RenderedBlock::Text(Arc::clone(lines)),
        Block::BlankLines(lines) => RenderedBlock::BlankLines(Arc::clone(lines)),
        Block::Heading { text, level } => {
            if heading_rendering == HeadingRendering::PlainText {
                return RenderedBlock::Text(Arc::new(vec![plain_heading_line(text)]));
            }
            if !safe_geometry(width_cols, cell_px) || text.len() > 1024 {
                return RenderedBlock::Text(Arc::new(vec![plain_heading_line(text)]));
            }
            let image = rasterizer.rasterize(text, *level, width_cols, cell_px);
            let size = ratatui::layout::Size::new(width_cols, 2);
            match picker.new_protocol(
                image::DynamicImage::ImageRgba8(image),
                size,
                Resize::Fit(None),
            ) {
                Ok(protocol) => RenderedBlock::Header(protocol),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        heading = %text,
                        level,
                        "failed to encode rasterized heading as a terminal graphics protocol, \
                         falling back to plain text"
                    );
                    RenderedBlock::Placeholder(heading_fallback_line(text, *level))
                }
            }
        }
        Block::Image { alt, path } => render_image(alt, path, picker, width_cols),
    }
}

fn render_image(alt: &str, path: &ImagePath, picker: &Picker, width_cols: u16) -> RenderedBlock {
    // A `match` (not nested let-else + unreachable!) so a future third
    // `ImagePath` variant fails to compile here instead of panicking at
    // runtime the first time this function runs against it.
    let path = match path {
        ImagePath::Local(path) => path,
        ImagePath::Unsupported(url) => {
            return RenderedBlock::Placeholder(Line::styled(
                format!("[image: {alt} -- remote images aren't loaded ({url})]"),
                Style::new().fg(Color::DarkGray).italic(),
            ));
        }
    };

    let bytes = read_image_bytes(path);
    render_loaded_image(alt, path, bytes.as_deref(), picker, width_cols)
}

fn render_loaded_image(
    alt: &str,
    path: &std::path::Path,
    bytes: Option<&[u8]>,
    picker: &Picker,
    width_cols: u16,
) -> RenderedBlock {
    let dyn_image = bytes.and_then(decode_image);
    let Some(dyn_image) = dyn_image else {
        return RenderedBlock::Placeholder(Line::styled(
            format!("[image unavailable: {alt} ({})]", path.display()),
            Style::new().fg(Color::DarkGray).italic(),
        ));
    };

    let available = ratatui::layout::Size::new(width_cols, MAX_IMAGE_ROWS);
    let resize = Resize::Fit(Some(FilterType::Lanczos3));
    let size = resize.size_for(&dyn_image, picker.font_size(), available);
    match picker.new_protocol(dyn_image, size, resize) {
        Ok(protocol) => RenderedBlock::Image(protocol),
        Err(error) => {
            tracing::warn!(
                %error,
                path = %path.display(),
                "failed to encode markdown image as a terminal graphics protocol"
            );
            RenderedBlock::Placeholder(Line::styled(
                format!("[image failed to render: {alt}]"),
                Style::new().fg(Color::DarkGray).italic(),
            ))
        }
    }
}

/// Reads only regular, bounded local files. A growing file is bounded by take,
/// rather than trusting its earlier metadata. Never opens FIFOs/devices.
pub(crate) fn read_image_bytes(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read;
    const LIMIT: usize = 4 * 1024 * 1024;
    let file = ilium_platform::secure_fs::open_regular_file(path).ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() > LIMIT as u64 {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((LIMIT + 1) as u64).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= LIMIT).then_some(bytes)
}

pub(crate) fn decode_image(bytes: &[u8]) -> Option<image::DynamicImage> {
    use std::io::Cursor;
    let mut dimension_reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut dimension_limits = image::Limits::default();
    dimension_limits.max_image_width = Some(4096);
    dimension_limits.max_image_height = Some(4096);
    dimension_limits.max_alloc = Some(32 * 1024 * 1024);
    dimension_reader.limits(dimension_limits);
    let dimensions = dimension_reader.into_dimensions().ok()?;
    if dimensions.0 > 4096
        || dimensions.1 > 4096
        || u64::from(dimensions.0) * u64::from(dimensions.1) > 4 * 1024 * 1024
    {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().ok()
}

fn safe_geometry(width: u16, cell: (u16, u16)) -> bool {
    width > 0 && width <= 512 && cell.0 > 0 && cell.0 <= 32 && cell.1 > 0 && cell.1 <= 64
}

/// No filesystem access here: encoding and rasterization are CPU work.
pub(crate) fn render_prepared(
    document: &Document,
    images: &std::collections::HashMap<std::path::PathBuf, Vec<u8>>,
    picker: &Picker,
    rasterizer: &mut HeaderRasterizer,
    width: u16,
    heading: HeadingRendering,
    cancelled: impl Fn() -> bool,
) -> Result<RenderedDocument, &'static str> {
    let cell = super::raster::cell_pixel_size(picker);
    let mut blocks = Vec::with_capacity(document.blocks.len());
    let mut graphics = 0usize;
    let mut graphics_pixels = 0usize;
    for block in &document.blocks {
        if cancelled() {
            return Err("document preparation cancelled");
        }
        let rows = if matches!(block, Block::Image { .. }) {
            24
        } else {
            2
        };
        let pixels = usize::from(width) * usize::from(cell.0) * usize::from(cell.1) * rows;
        let graphics_admitted = safe_geometry(width, cell)
            && graphics < 16
            && graphics_pixels.saturating_add(pixels) <= 262144;
        let prepared = match block {
            Block::Image {
                alt,
                path: ImagePath::Local(path),
            } if graphics_admitted => render_loaded_image(
                alt,
                path,
                images.get(path).map(Vec::as_slice),
                picker,
                width,
            ),
            Block::Image {
                path: ImagePath::Unsupported(_),
                ..
            } => render_block(block, picker, rasterizer, width, cell, heading),
            Block::Image { alt, .. } => {
                RenderedBlock::Placeholder(Line::from(format!("[image unavailable: {alt}]")))
            }
            Block::Heading { text, .. } if !graphics_admitted => {
                RenderedBlock::Text(Arc::new(vec![plain_heading_line(text)]))
            }
            _ => render_block(block, picker, rasterizer, width, cell, heading),
        };
        if matches!(prepared, RenderedBlock::Image(_) | RenderedBlock::Header(_)) {
            graphics += 1;
            graphics_pixels += pixels;
        }
        blocks.push(prepared);
    }
    Ok(RenderedDocument {
        blocks,
        layout: None,
    })
}

/// Uses Ratatui's exact wrapping engine once on the worker, then the UI only
/// copies visible prepared cells. Total output cells are capped before allocation.
pub(crate) fn prepare_layout(
    document: &mut RenderedDocument,
    width: u16,
    display: crate::config::LineDisplay,
) -> Result<(), &'static str> {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget, Wrap},
    };
    let mut layout = PreparedLayout {
        width,
        line_display: display,
        buffers: Vec::new(),
        heights: Vec::new(),
        total_height: 0,
    };
    let mut cells = 0usize;
    for block in &document.blocks {
        let lines = match block {
            RenderedBlock::Text(lines) | RenderedBlock::BlankLines(lines) => Some(lines.as_slice()),
            RenderedBlock::Placeholder(line) => Some(std::slice::from_ref(line)),
            _ => None,
        };
        let (height, buffer) = if let Some(lines) = lines {
            let mut paragraph = Paragraph::new(lines.to_vec());
            if display == crate::config::LineDisplay::Wrap {
                paragraph = paragraph.wrap(Wrap { trim: false });
            }
            let height = u16::try_from(paragraph.line_count(width.max(1)))
                .map_err(|_| "document layout has too many rows")?;
            cells = cells
                .checked_add(usize::from(width) * usize::from(height))
                .ok_or("document layout overflow")?;
            if cells > 131072 {
                return Err("document layout exceeds prepared-cell budget; use source view");
            }
            let area = Rect::new(0, 0, width, height);
            let mut buffer = Buffer::empty(area);
            paragraph.render(area, &mut buffer);
            (height, Some(buffer))
        } else {
            match block {
                RenderedBlock::Image(protocol) | RenderedBlock::Header(protocol) => {
                    (protocol.size().height, None)
                }
                _ => (0, None),
            }
        };
        layout.heights.push(height);
        layout.buffers.push(buffer);
        layout.total_height = layout.total_height.saturating_add(height);
    }
    document.layout = Some(layout);
    Ok(())
}

/// Creates the graphics-free heading shown when the current terminal cannot
/// display raster protocols. Bold accent text preserves hierarchy without
/// adding Markdown syntax back into the rendered document.
fn plain_heading_line(text: &str) -> Line<'static> {
    Line::styled(
        text.to_string(),
        Style::new()
            .fg(super::document::HEADING_FG)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )
}

fn heading_fallback_line(text: &str, level: u8) -> Line<'static> {
    Line::styled(
        format!("{} {text}", "#".repeat(level.max(1) as usize)),
        Style::new()
            .fg(super::document::HEADING_FG)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::raster::HeaderRasterizer;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::Terminal;

    /// A solid-color PNG a real markdown image link can point at, without
    /// depending on an external image tool -- written straight through
    /// the `image` crate this module already links against.
    fn write_solid_png(path: &std::path::Path, width: u32, height: u32, rgb: [u8; 3]) {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb(rgb));
        image.save(path).expect("write test fixture PNG");
    }

    #[test]
    fn heading_rasterizes_to_a_protocol_block() {
        let doc = Document {
            blocks: vec![Block::Heading {
                text: "Title".to_string(),
                level: 1,
            }],
        };
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let rendered = render(
            &doc,
            &picker,
            &mut rasterizer,
            40,
            HeadingRendering::Rasterized,
        );
        assert_eq!(rendered.blocks.len(), 1);
        assert!(matches!(rendered.blocks[0], RenderedBlock::Header(_)));
    }

    #[test]
    fn plain_text_heading_avoids_the_graphics_protocol_and_remains_bold() {
        let doc = Document {
            blocks: vec![Block::Heading {
                text: "Portable title".to_string(),
                level: 1,
            }],
        };
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let rendered = render(
            &doc,
            &picker,
            &mut rasterizer,
            40,
            HeadingRendering::PlainText,
        );

        let RenderedBlock::Text(lines) = &rendered.blocks[0] else {
            panic!("plain-text heading must not create a graphics protocol");
        };
        assert_eq!(lines[0].spans[0].content, "Portable title");
        assert!(
            lines[0]
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD),
            "heading style: {:?}",
            lines[0].style
        );
    }

    #[test]
    fn missing_local_image_falls_back_to_placeholder_text() {
        let doc = Document {
            blocks: vec![Block::Image {
                alt: "gone".to_string(),
                path: ImagePath::Local(std::path::PathBuf::from("/no/such/file.png")),
            }],
        };
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let rendered = render(
            &doc,
            &picker,
            &mut rasterizer,
            40,
            HeadingRendering::Rasterized,
        );
        assert!(matches!(rendered.blocks[0], RenderedBlock::Placeholder(_)));
    }

    /// Exercises the complete parse -> render -> terminal-buffer path so a
    /// semantic-only parser test cannot pass while the viewport still stacks
    /// the resulting blocks without their source spacing.
    #[test]
    fn source_blank_line_paints_an_empty_terminal_row() {
        let document =
            super::super::document::parse("first\n\nsecond", std::path::Path::new("/tmp"));
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let rendered = render(
            &document,
            &picker,
            &mut rasterizer,
            20,
            HeadingRendering::Rasterized,
        );

        let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
        terminal
            .draw(|frame| {
                super::super::view::render(
                    frame,
                    Rect::new(0, 0, 20, 3),
                    &rendered,
                    0,
                    crate::config::LineDisplay::Wrap,
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), "f");
        assert_eq!(buffer.cell((0, 1)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 2)).unwrap().symbol(), "s");
    }

    #[test]
    fn heading_to_body_spacing_survives_rasterization() {
        let document =
            super::super::document::parse("# Heading\n\nbody", std::path::Path::new("/tmp"));
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let rendered = render(
            &document,
            &picker,
            &mut rasterizer,
            20,
            HeadingRendering::Rasterized,
        );

        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|frame| {
                super::super::view::render(
                    frame,
                    Rect::new(0, 0, 20, 4),
                    &rendered,
                    0,
                    crate::config::LineDisplay::Wrap,
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 2)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 3)).unwrap().symbol(), "b");
    }

    #[test]
    fn blank_line_after_code_block_has_no_code_background() {
        let document =
            super::super::document::parse("```\ncode\n```\n\nafter", std::path::Path::new("/tmp"));
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let rendered = render(
            &document,
            &picker,
            &mut rasterizer,
            20,
            HeadingRendering::Rasterized,
        );

        let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
        terminal
            .draw(|frame| {
                super::super::view::render(
                    frame,
                    Rect::new(0, 0, 20, 3),
                    &rendered,
                    0,
                    crate::config::LineDisplay::Wrap,
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_ne!(buffer.cell((0, 0)).unwrap().bg, Color::Reset);
        assert_eq!(buffer.cell((0, 1)).unwrap().bg, Color::Reset);
        assert_eq!(buffer.cell((0, 2)).unwrap().symbol(), "a");
    }

    /// Regression test for a real image actually painting pixels: builds a
    /// solid-color PNG, renders a one-block document containing it, and
    /// checks the `TestBackend` buffer for the exact color at the image's
    /// row -- `ratatui-image`'s halfblocks encoder represents a solid-color
    /// region as a plain space character with matching fg/bg, which reads
    /// as "blank" in any symbol-only dump (this bit a manual test pass
    /// during development), so the color check (not the symbol) is what
    /// actually proves the image rendered.
    #[test]
    fn local_image_paints_its_color_into_the_buffer() {
        // `TempDir` removes the scratch directory (and the fixture PNG
        // written into it) on drop, instead of the previous fixed path
        // under the OS temp dir that was never cleaned up.
        let directory = tempfile::tempdir().expect("create scratch dir");
        let image_path = directory.path().join("pic.png");
        write_solid_png(&image_path, 100, 40, [65, 105, 225]); // royalblue

        let doc = Document {
            blocks: vec![Block::Image {
                alt: "pic".to_string(),
                path: ImagePath::Local(image_path),
            }],
        };
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let rendered = render(
            &doc,
            &picker,
            &mut rasterizer,
            40,
            HeadingRendering::Rasterized,
        );
        assert!(matches!(rendered.blocks[0], RenderedBlock::Image(_)));

        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal
            .draw(|frame| {
                super::super::view::render(
                    frame,
                    Rect::new(0, 0, 40, 20),
                    &rendered,
                    0,
                    crate::config::LineDisplay::Wrap,
                );
            })
            .unwrap();
        let cell = terminal.backend().buffer().cell((0, 0)).unwrap();
        assert_eq!(cell.bg, Color::Rgb(65, 105, 225));
    }
}

#[cfg(test)]
mod preparation_limit_tests {
    use super::*;
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};
    #[test]
    fn prepared_wrapping_paints_exactly_the_original_ratatui_layout() {
        let document = super::super::document::parse(
            "one two three four\n\n**Wide 中文** body\n\n> quote",
            std::path::Path::new("."),
        );
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let reference = render(
            &document,
            &picker,
            &mut rasterizer,
            8,
            HeadingRendering::PlainText,
        );
        let mut prepared = render(
            &document,
            &picker,
            &mut rasterizer,
            8,
            HeadingRendering::PlainText,
        );
        prepare_layout(&mut prepared, 8, crate::config::LineDisplay::Wrap).unwrap();
        let paint = |document: &RenderedDocument| {
            let mut terminal = Terminal::new(TestBackend::new(8, 20)).unwrap();
            terminal
                .draw(|frame| {
                    super::super::view::render(
                        frame,
                        Rect::new(0, 0, 8, 20),
                        document,
                        0,
                        crate::config::LineDisplay::Wrap,
                    )
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        assert_eq!(paint(&reference), paint(&prepared));
        assert_eq!(
            super::super::view::content_height(&reference, 8, crate::config::LineDisplay::Wrap),
            super::super::view::content_height(&prepared, 8, crate::config::LineDisplay::Wrap)
        );
    }
    #[test]
    fn malformed_and_oversized_images_and_cell_layout_are_rejected() {
        assert!(decode_image(b"not an image").is_none());
        let mut bitmap = vec![0u8; 54];
        bitmap[0..2].copy_from_slice(b"BM");
        bitmap[14..18].copy_from_slice(&40u32.to_le_bytes());
        bitmap[18..22].copy_from_slice(&100000u32.to_le_bytes());
        bitmap[22..26].copy_from_slice(&100000u32.to_le_bytes());
        bitmap[26..28].copy_from_slice(&1u16.to_le_bytes());
        bitmap[28..30].copy_from_slice(&24u16.to_le_bytes());
        assert!(decode_image(&bitmap).is_none());
        let mut document = RenderedDocument {
            blocks: vec![RenderedBlock::Text(Arc::new(
                (0..1000).map(|_| Line::from("body")).collect(),
            ))],
            layout: None,
        };
        assert!(prepare_layout(&mut document, 512, crate::config::LineDisplay::Clip).is_err());
        assert!(document.layout.is_none());
        assert!(!safe_geometry(u16::MAX, (u16::MAX, u16::MAX)));
    }
}
