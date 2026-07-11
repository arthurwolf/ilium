//! Turns a parsed `document::Document` into a `RenderedDocument`: headers
//! and local images resolved into `ratatui_image::protocol::Protocol`s
//! (ready to hand straight to `ratatui_image::Image` widgets), everything
//! else passed through unchanged as styled text.
//!
//! Runs synchronously on the UI thread. Deliberately so: rendering only
//! happens when the user toggles into Rendered mode or resizes while
//! already in it (see `EditorPane::view_mode` in `editor_pane.rs`) --
//! rare, user-initiated moments, not a per-keystroke cost -- so the
//! `mdfried`-style background worker thread this could otherwise need
//! isn't earning its complexity here.

use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::{FilterType, Resize};

use super::document::{Block, Document, ImagePath};
use super::raster::HeaderRasterizer;

/// One block of a document, ready to render: text passed through as-is,
/// or a header/image already converted to a terminal-graphics `Protocol`.
pub enum RenderedBlock {
    Text(Vec<Line<'static>>),
    /// Source-authored vertical space. It remains a first-class block so
    /// content-height and scroll calculations count the same rows the
    /// viewport leaves unpainted.
    BlankLines(Vec<Line<'static>>),
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
}

/// Content images are capped to this many terminal rows so one huge
/// screenshot can't push the rest of the document off-screen.
const MAX_IMAGE_ROWS: u16 = 24;

pub fn render(
    document: &Document,
    picker: &Picker,
    rasterizer: &mut HeaderRasterizer,
    width_cols: u16,
) -> RenderedDocument {
    let cell_px = super::raster::cell_pixel_size(picker);
    let blocks = document
        .blocks
        .iter()
        .map(|block| render_block(block, picker, rasterizer, width_cols, cell_px))
        .collect();
    RenderedDocument { blocks }
}

fn render_block(
    block: &Block,
    picker: &Picker,
    rasterizer: &mut HeaderRasterizer,
    width_cols: u16,
    cell_px: (u16, u16),
) -> RenderedBlock {
    match block {
        Block::Text(lines) => RenderedBlock::Text(lines.clone()),
        Block::BlankLines(lines) => RenderedBlock::BlankLines(lines.clone()),
        Block::Heading { text, level } => {
            let image = rasterizer.rasterize(text, *level, width_cols, cell_px);
            let size = ratatui::layout::Size::new(width_cols, 2);
            match picker.new_protocol(
                image::DynamicImage::ImageRgba8(image),
                size,
                Resize::Fit(None),
            ) {
                Ok(protocol) => RenderedBlock::Header(protocol),
                Err(_) => RenderedBlock::Placeholder(heading_fallback_line(text, *level)),
            }
        }
        Block::Image { alt, path } => render_image(alt, path, picker, width_cols),
    }
}

fn render_image(alt: &str, path: &ImagePath, picker: &Picker, width_cols: u16) -> RenderedBlock {
    let ImagePath::Local(path) = path else {
        let ImagePath::Unsupported(url) = path else {
            unreachable!("the only other ImagePath variant is Local, matched above");
        };
        return RenderedBlock::Placeholder(Line::styled(
            format!("[image: {alt} -- remote images aren't loaded ({url})]"),
            Style::new().fg(Color::DarkGray).italic(),
        ));
    };

    let dyn_image = image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.decode().ok());

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
        Err(_) => RenderedBlock::Placeholder(Line::styled(
            format!("[image failed to render: {alt}]"),
            Style::new().fg(Color::DarkGray).italic(),
        )),
    }
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
        let rendered = render(&doc, &picker, &mut rasterizer, 40);
        assert_eq!(rendered.blocks.len(), 1);
        assert!(matches!(rendered.blocks[0], RenderedBlock::Header(_)));
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
        let rendered = render(&doc, &picker, &mut rasterizer, 40);
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
        let rendered = render(&document, &picker, &mut rasterizer, 20);

        let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
        terminal
            .draw(|frame| {
                super::super::view::render(frame, Rect::new(0, 0, 20, 3), &rendered, 0);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), "f");
        assert_eq!(buffer.cell((0, 1)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 2)).unwrap().symbol(), "s");
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
        let dir = tempfile_dir();
        let image_path = dir.join("pic.png");
        write_solid_png(&image_path, 100, 40, [65, 105, 225]); // royalblue

        let doc = Document {
            blocks: vec![Block::Image {
                alt: "pic".to_string(),
                path: ImagePath::Local(image_path),
            }],
        };
        let picker = Picker::halfblocks();
        let mut rasterizer = HeaderRasterizer::new();
        let rendered = render(&doc, &picker, &mut rasterizer, 40);
        assert!(matches!(rendered.blocks[0], RenderedBlock::Image(_)));

        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal
            .draw(|frame| {
                super::super::view::render(frame, Rect::new(0, 0, 40, 20), &rendered, 0);
            })
            .unwrap();
        let cell = terminal.backend().buffer().cell((0, 0)).unwrap();
        assert_eq!(cell.bg, Color::Rgb(65, 105, 225));
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("illium-markdown-render-tests")
            .join(format!("{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }
}
