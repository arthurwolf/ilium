//! Native Wikipedia layout and ink-only rendering. No network, terminal, or worker ownership.
//!
//! `prepare` replaces a bounded display list; `render` paints only cached 16-row strips.
//! The host owns PageLoader and PageScroll and must release this renderer when hidden.
//! Virtual cells are 8x16 pixels, matching the existing ambient 2:1 cell assumption.
//! Text is terminal text, not a font image. Images remain Braille in either mode.

use cosmic_text::{
    Attrs, Buffer, Color as FontColor, Family, FontSystem, LayoutGlyph, Metrics, Shaping,
    SwashCache, Wrap,
};
use ilium_ambient::raster::{threshold, DitherMode};
use ilium_ambient::SceneSettings;
use ilium_wikipedia::{Block, Document, ImageFloat, ImageRef, Span, TableImage, TableSpan};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Palette, RenderMode, WikipediaSettings};

const FONT: &[u8] = include_bytes!("../../../assets/fonts/CascadiaCode-Regular.otf");
const CW: usize = 8;
const CH: usize = 16;
const DOT: usize = 4;
const STRIP_PX: usize = 256;
const STRIP_DOTS: usize = STRIP_PX / DOT;
const MAX_COLUMNS: u16 = 512;
const MAX_ROWS: u16 = 512;
const MAX_CELLS: usize = 131_072;
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_INLINE_BYTES: usize = 65_536;
const MAX_SOURCE_NODES: usize = 200_000;
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_ITEMS: usize = 100_000;
const MAX_GLYPHS: usize = 250_000;
const MAX_LAYOUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_INDEX_LINKS: usize = 500_000;
const MAX_PAGE_PX: f32 = 2_000_000.0;
const MAX_STRIPS: usize = 64;
const MAX_STRIP_BYTES: usize = 8 * 1024 * 1024;
const MAX_SWASH_ENTRIES: usize = 4096;

impl WikipediaSettings {
    fn geometry(self, columns: u16) -> GeometryKey {
        GeometryKey {
            columns,
            mode: self.render_mode,
            zoom: if self.render_mode == RenderMode::Braille {
                self.zoom_percent
            } else {
                100
            },
        }
    }
    fn ink(self) -> InkKey {
        InkKey(
            self.palette,
            self.greyscale,
            self.hue_degrees,
            self.saturation_percent,
            self.lightness_percent,
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GeometryKey {
    columns: u16,
    mode: RenderMode,
    zoom: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InkKey(Palette, bool, u16, u16, u16);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    Limit(&'static str),
    FontUnavailable,
    TooNarrow,
    NotPrepared,
    InvalidOffset,
    Cancelled,
}
impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(what) => write!(
                f,
                "Wikipedia renderer limit: {what}; no excerpt substituted"
            ),
            Self::FontUnavailable => f.write_str("Bundled Cascadia font could not be loaded"),
            Self::TooNarrow => {
                f.write_str("Wikipedia layout needs more columns or a smaller Braille zoom")
            }
            Self::NotPrepared => f.write_str("No Wikipedia page is prepared"),
            Self::InvalidOffset => {
                f.write_str("Wikipedia scroll offset must be finite and nonnegative")
            }
            Self::Cancelled => f.write_str("Wikipedia layout preparation cancelled"),
        }
    }
}
impl std::error::Error for RenderError {}
type Result<T> = std::result::Result<T, RenderError>;
fn check_cancel(cancel: Option<&AtomicBool>) -> Result<()> {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        Err(RenderError::Cancelled)
    } else {
        Ok(())
    }
}

/// An ink-only terminal grapheme occupying one or two columns. A wide lead
/// reserves a typed continuation; empty symbols are transparent. The compositor
/// still protects native wide spans, cursor, styling and diff flags.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RenderCell {
    symbol: String,
    rgb: [u8; 3],
    bold: bool,
    italic: bool,
    continuation: bool,
}
impl RenderCell {
    pub fn symbol(&self) -> &str {
        if self.symbol.is_empty() {
            " "
        } else {
            &self.symbol
        }
    }
    pub fn rgb(&self) -> [u8; 3] {
        self.rgb
    }
    pub fn bold(&self) -> bool {
        self.bold
    }
    pub fn italic(&self) -> bool {
        self.italic
    }
    pub fn is_ink(&self) -> bool {
        !self.symbol.is_empty()
    }
    pub fn is_continuation(&self) -> bool {
        self.continuation
    }
    fn clear(&mut self) {
        self.symbol.clear();
        self.rgb = [0; 3];
        self.bold = false;
        self.italic = false;
        self.continuation = false;
    }
    fn set(&mut self, symbol: &str, rgb: [u8; 3], style: Style) {
        debug_assert!(is_safe_symbol(symbol));
        self.symbol.clear();
        self.symbol.push_str(symbol);
        self.rgb = rgb;
        self.bold = style.bold;
        self.italic = style.italic;
        self.continuation = false;
    }
    fn reserve_continuation(&mut self) {
        self.clear();
        self.continuation = true;
    }
}
fn forbidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}'
        | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}
/// One complete terminal grapheme, one or two columns, excluding controls,
/// direction overrides, joiners, standalone marks and visually empty cells.
pub fn safe_symbol_width(s: &str) -> Option<usize> {
    let width = UnicodeWidthStr::width(s);
    (matches!(width, 1 | 2)
        && s.graphemes(true).count() == 1
        && !s
            .chars()
            .any(|c| forbidden(c) || matches!(c, '\u{200c}' | '\u{200d}'))
        && !s.chars().all(|c| c.is_whitespace() || c == '\u{2800}'))
    .then_some(width)
}
pub fn is_safe_symbol(s: &str) -> bool {
    safe_symbol_width(s).is_some()
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LayoutStats {
    pub items: usize,
    pub glyphs: usize,
    pub escaped_clusters: usize,
    pub missing_font_glyphs: usize,
    pub unavailable_images: usize,
    pub source_image_bytes: usize,
    pub display_list_bytes: usize,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WorkStats {
    pub layouts: u64,
    pub raster_strips: u64,
    pub packed_viewports: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Body,
    Heading,
    Link,
    Muted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    superscript: bool,
    role: Role,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            superscript: false,
            role: Role::Body,
        }
    }
}
#[derive(Debug, Clone)]
struct Inline {
    text: String,
    style: Style,
}
#[derive(Debug, Default)]
struct Rich {
    parts: Vec<Inline>,
}
impl Rich {
    fn bytes(&self) -> usize {
        self.parts.iter().map(|p| p.text.len()).sum()
    }
    fn suffix(&self, mut skip: usize) -> Self {
        let mut parts = Vec::new();
        for p in &self.parts {
            if skip >= p.text.len() {
                skip -= p.text.len();
                continue;
            }
            let text = p.text[skip..].to_owned();
            skip = 0;
            parts.push(Inline {
                text,
                style: p.style,
            });
        }
        Self { parts }
    }
    fn from_spans(spans: &[Span], mode: RenderMode, role: Role, stats: &mut LayoutStats) -> Self {
        let mut parts = Vec::new();
        let mut previous_space = true;
        for span in spans {
            let source = if mode == RenderMode::Text && span.superscript {
                superscript_text(&span.text)
            } else {
                span.text.clone()
            };
            let mut clean = String::new();
            for c in source.chars() {
                if c == '\n' {
                    clean.push('\n');
                    previous_space = true;
                } else if c.is_whitespace() && !forbidden(c) || matches!(c, '\r' | '\t') {
                    if !previous_space {
                        clean.push(' ');
                        previous_space = true;
                    }
                } else {
                    if forbidden(c) {
                        stats.escaped_clusters += 1;
                        clean.push_str(&format!("\\u{{{:x}}}", c as u32));
                    } else {
                        clean.push(c);
                    }
                    previous_space = false;
                }
            }
            let text = clean;
            if !text.is_empty() {
                parts.push(Inline {
                    text,
                    style: Style {
                        bold: span.bold || role == Role::Heading,
                        italic: span.italic,
                        superscript: span.superscript,
                        role: if span.link { Role::Link } else { role },
                    },
                });
            }
        }
        if mode == RenderMode::Text {
            // Form graphemes across HTML span boundaries before the one-cell policy.
            // A terminal cell has one style: use the style at the grapheme's start.
            let joined: String = parts.iter().map(|p| p.text.as_str()).collect();
            let mut ends = Vec::with_capacity(parts.len());
            let mut end = 0;
            for p in &parts {
                end += p.text.len();
                ends.push(end);
            }
            let mut terminal: Vec<Inline> = Vec::new();
            for (offset, g) in joined.grapheme_indices(true) {
                let i = ends.partition_point(|end| *end <= offset);
                let style = parts[i].style;
                let text = terminal_text(g, stats);
                if let Some(last) = terminal.last_mut().filter(|last| last.style == style) {
                    last.text.push_str(&text);
                } else {
                    terminal.push(Inline { text, style });
                }
            }
            Self { parts: terminal }
        } else {
            Self { parts }
        }
    }
}
fn terminal_text(text: &str, stats: &mut LayoutStats) -> String {
    let mut out = String::new();
    for g in text.graphemes(true) {
        if matches!(g, " " | "\n") || is_safe_symbol(g) {
            out.push_str(g);
        } else {
            stats.escaped_clusters += 1;
            for c in g.chars() {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
        }
    }
    out
}
fn superscript_text(s: &str) -> String {
    let mapped: Option<String> = s
        .chars()
        .map(|c| {
            Some(match c {
                '0' => '⁰',
                '1' => '¹',
                '2' => '²',
                '3' => '³',
                '4' => '⁴',
                '5' => '⁵',
                '6' => '⁶',
                '7' => '⁷',
                '8' => '⁸',
                '9' => '⁹',
                '+' => '⁺',
                '-' => '⁻',
                '=' => '⁼',
                '(' => '⁽',
                ')' => '⁾',
                'n' => 'ⁿ',
                'i' => 'ⁱ',
                '[' | ']' | ',' | ' ' => c,
                _ => return None,
            })
        })
        .collect();
    mapped.unwrap_or_else(|| format!("^{{{s}}}"))
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}
impl Rect {
    fn bottom(self) -> f32 {
        self.y + self.h
    }
}
#[derive(Debug)]
struct Glyph {
    glyph: LayoutGlyph,
    baseline: f32,
    style: Style,
}
#[derive(Debug, Clone)]
struct TextGlyph {
    symbol: String,
    style: Style,
}
#[derive(Debug)]
enum Paint {
    Font(Vec<Glyph>),
    Text(Vec<TextGlyph>),
    Image(String),
    Rule,
}
#[derive(Debug)]
struct Item {
    rect: Rect,
    paint: Paint,
}
#[derive(Debug, Default)]
struct Layout {
    items: Vec<Item>,
    bands: BTreeMap<u32, Vec<usize>>,
    image_contrast: HashMap<String, ImageContrast>,
    bottom: f32,
    stats: LayoutStats,
}
impl Layout {
    fn push(&mut self, item: Item) -> Result<()> {
        if !item.rect.bottom().is_finite() || item.rect.bottom() > MAX_PAGE_PX {
            return Err(RenderError::Limit("page height"));
        }
        let (glyphs, bytes) = match &item.paint {
            Paint::Font(g) => (g.len(), g.capacity() * std::mem::size_of::<Glyph>()),
            Paint::Text(g) => (
                g.len(),
                g.capacity() * std::mem::size_of::<TextGlyph>()
                    + g.iter().map(|c| c.symbol.capacity()).sum::<usize>(),
            ),
            _ => (0, 0),
        };
        if self.items.len() >= MAX_ITEMS || self.stats.glyphs + glyphs > MAX_GLYPHS {
            return Err(RenderError::Limit("display-list item or glyph count"));
        }
        self.stats.display_list_bytes += bytes + std::mem::size_of::<Item>();
        if self.stats.display_list_bytes > MAX_LAYOUT_BYTES / 2 {
            // Reserve the other half for Vec growth and the vertical index.
            return Err(RenderError::Limit("display-list allocation budget"));
        }
        self.stats.glyphs += glyphs;
        self.bottom = self.bottom.max(item.rect.bottom());
        self.items.push(item);
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        let mut links = 0;
        for (i, item) in self.items.iter().enumerate() {
            let first = (item.rect.y.max(0.0) as usize / STRIP_PX) as u32;
            let last = ((item.rect.bottom().ceil().max(1.0) as usize - 1) / STRIP_PX) as u32;
            for band in first..=last {
                links += 1;
                if links > MAX_INDEX_LINKS {
                    return Err(RenderError::Limit("vertical-index links"));
                }
                self.bands.entry(band).or_default().push(i);
            }
        }
        self.stats.items = self.items.len();
        self.stats.display_list_bytes += (self.items.capacity() - self.items.len())
            * std::mem::size_of::<Item>()
            + self
                .bands
                .values()
                .map(|v| v.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>();
        if self.stats.display_list_bytes > MAX_LAYOUT_BYTES {
            return Err(RenderError::Limit("display-list and index payload"));
        }
        Ok(())
    }
    fn cache_image_contrast(
        &mut self,
        document: &Document,
        cancel: Option<&AtomicBool>,
    ) -> Result<()> {
        for (url, image) in &document.images {
            check_cancel(cancel)?;
            if image.width() > 0 && image.height() > 0 {
                self.image_contrast
                    .insert(url.clone(), analyze_image_contrast(image, cancel)?);
            }
        }
        self.stats.display_list_bytes += self.image_contrast.capacity()
            * (std::mem::size_of::<(String, ImageContrast)>() + 2 * std::mem::size_of::<usize>())
            + self
                .image_contrast
                .keys()
                .map(String::capacity)
                .sum::<usize>();
        if self.stats.display_list_bytes > MAX_LAYOUT_BYTES {
            return Err(RenderError::Limit(
                "display-list and image analysis payload",
            ));
        }
        Ok(())
    }
}

struct Fonts {
    system: FontSystem,
    swash: SwashCache,
    family: String,
    cache_bytes: usize,
}
impl Fonts {
    fn new() -> Result<Self> {
        static DATABASE: OnceLock<cosmic_text::fontdb::Database> = OnceLock::new();
        let db = DATABASE
            .get_or_init(|| {
                let mut db = cosmic_text::fontdb::Database::new();
                db.load_font_data(FONT.to_vec());
                // Keep Cascadia first and share system font discovery across page
                // rotations and geometry changes. The database stays immutable.
                db.load_system_fonts();
                db
            })
            .clone();
        let family = db
            .faces()
            .next()
            .and_then(|f| f.families.first())
            .map(|(name, _)| name.clone())
            .ok_or(RenderError::FontUnavailable)?;
        Ok(Self {
            system: FontSystem::new_with_locale_and_db("en-US".into(), db),
            swash: SwashCache::new(),
            family,
            cache_bytes: 0,
        })
    }
    fn lines(&mut self, rich: &Rich, width: f32, size: f32) -> Result<Vec<Draft>> {
        let height = (size * 1.55).ceil();
        let attrs = Attrs::new().family(Family::Name(&self.family));
        let mut buffer = Buffer::new(&mut self.system, Metrics::new(size, height));
        buffer.set_size(&mut self.system, Some(width), None);
        buffer.set_wrap(&mut self.system, Wrap::WordOrGlyph);
        // Only the supplied Regular face is assumed. Bold and italic are synthesized
        // explicitly during rasterization, rather than silently selecting a missing face.
        buffer.set_rich_text(
            &mut self.system,
            rich.parts.iter().enumerate().map(|(i, part)| {
                let fs = if part.style.superscript {
                    size * 0.65
                } else {
                    size
                };
                (
                    part.text.as_str(),
                    attrs.clone().metadata(i).metrics(Metrics::new(fs, height)),
                )
            }),
            &attrs,
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.system, false);
        let mut lines = Vec::new();
        let joined: String = rich.parts.iter().map(|part| part.text.as_str()).collect();
        let mut line_starts = vec![0];
        line_starts.extend(joined.match_indices('\n').map(|(offset, _)| offset + 1));
        let mut used = 0;
        for run in buffer.layout_runs() {
            if run.glyphs.iter().any(|glyph| {
                if glyph.x + glyph.w <= width + 0.5 {
                    return false;
                }
                // WordOrGlyph may leave a shaped trailing space beyond the
                // line box. It has no raster ink and must not reject a real
                // article; any nonblank overhang remains a layout error.
                !run.text.get(glyph.start..glyph.end).is_some_and(|source| {
                    !source.is_empty() && source.chars().all(char::is_whitespace)
                })
            }) {
                return Err(RenderError::TooNarrow);
            }
            used += run.glyphs.len();
            if used > MAX_GLYPHS {
                return Err(RenderError::Limit("one shaped paragraph"));
            }
            let local_end = run
                .glyphs
                .iter()
                .map(|g| g.end)
                .max()
                .unwrap_or(run.text.len());
            let line_start = line_starts[run.line_i];
            let mut end = line_start + local_end;
            if local_end == run.text.len() && joined.as_bytes().get(end) == Some(&b'\n') {
                end += 1;
            }
            let glyphs = run
                .glyphs
                .iter()
                .map(|g| {
                    let style = rich.parts[g.metadata].style;
                    Glyph {
                        glyph: g.clone(),
                        baseline: run.line_y
                            - run.line_top
                            - if style.superscript { size * 0.30 } else { 0.0 },
                        style,
                    }
                })
                .collect();
            lines.push(Draft {
                end,
                height: run.line_height,
                paint: Paint::Font(glyphs),
            });
        }
        Ok(lines)
    }
    fn trim_cache(&mut self) {
        if self.cache_bytes >= 8 * 1024 * 1024
            || self.swash.image_cache.len() >= MAX_SWASH_ENTRIES
            || self.swash.outline_command_cache.len() >= MAX_SWASH_ENTRIES
        {
            self.swash = SwashCache::new();
            self.cache_bytes = 0;
        }
    }
}
struct Draft {
    end: usize,
    height: f32,
    paint: Paint,
}
fn text_lines(rich: &Rich, columns: usize) -> Result<Vec<Draft>> {
    if columns == 0 {
        return Err(RenderError::TooNarrow);
    }
    let mut clusters = Vec::new();
    let mut byte = 0;
    for part in &rich.parts {
        for g in part.text.graphemes(true) {
            if clusters.len() >= MAX_GLYPHS {
                return Err(RenderError::Limit("one text paragraph"));
            }
            byte += g.len();
            clusters.push((
                TextGlyph {
                    symbol: g.to_owned(),
                    style: part.style,
                },
                byte,
            ));
        }
    }
    let mut lines = Vec::new();
    let mut start = 0;
    while start < clusters.len() {
        while start < clusters.len() && clusters[start].0.symbol == " " {
            start += 1;
        }
        if start == clusters.len() {
            break;
        }
        if clusters[start].0.symbol == "\n" {
            lines.push(Draft {
                end: clusters[start].1,
                height: CH as f32,
                paint: Paint::Text(Vec::new()),
            });
            start += 1;
            continue;
        }
        let mut hard_end = start;
        let mut used_columns = 0;
        while hard_end < clusters.len() && clusters[hard_end].0.symbol != "\n" {
            let symbol = &clusters[hard_end].0.symbol;
            let cell_width = if symbol == " " {
                1
            } else {
                safe_symbol_width(symbol).ok_or(RenderError::TooNarrow)?
            };
            if used_columns + cell_width > columns {
                break;
            }
            used_columns += cell_width;
            hard_end += 1;
        }
        if hard_end == start {
            return Err(RenderError::TooNarrow);
        }
        let ended_at_break = clusters.get(hard_end).is_some_and(|c| c.0.symbol == "\n");
        let end =
            if !ended_at_break && hard_end < clusters.len() && clusters[hard_end].0.symbol != " " {
                (start..hard_end)
                    .rev()
                    .find(|&i| clusters[i].0.symbol == " ")
                    .filter(|&i| i > start)
                    .unwrap_or(hard_end)
            } else {
                hard_end
            };
        let mut visible_end = end;
        while visible_end > start && clusters[visible_end - 1].0.symbol == " " {
            visible_end -= 1;
        }
        let paint = Paint::Text(
            clusters[start..visible_end]
                .iter()
                .map(|(g, _)| g.clone())
                .collect(),
        );
        let consumed = end + usize::from(end == hard_end && ended_at_break);
        lines.push(Draft {
            end: clusters[consumed - 1].1,
            height: CH as f32,
            paint,
        });
        start = consumed;
    }
    Ok(lines)
}

#[derive(Clone, Copy)]
struct Float {
    right: bool,
    width: f32,
    bottom: f32,
}
struct Flow {
    x: f32,
    width: f32,
    y: f32,
    float: Option<Float>,
}
impl Flow {
    fn lane(&self) -> (f32, f32) {
        if let Some(f) = self.float.filter(|f| self.y < f.bottom) {
            (
                self.x
                    + if f.right {
                        0.0
                    } else {
                        f.width + CW as f32 * 2.0
                    },
                self.width - f.width - CW as f32 * 2.0,
            )
        } else {
            (self.x, self.width)
        }
    }
    fn clear(&mut self) {
        if let Some(f) = self.float.take() {
            self.y = self.y.max(f.bottom);
        }
    }
}
struct Builder<'a> {
    document: &'a Document,
    key: GeometryKey,
    fonts: Option<&'a mut Fonts>,
    cancel: Option<&'a AtomicBool>,
    layout: Layout,
}
struct TableCellDraft {
    span: TableSpan,
    items: Vec<Item>,
    height: f32,
}
impl Builder<'_> {
    fn base_size(&self) -> f32 {
        32.0 * f32::from(self.key.zoom) / 100.0
    }
    fn em(&self) -> f32 {
        if self.key.mode == RenderMode::Text {
            CW as f32
        } else {
            self.base_size() * 0.60
        }
    }
    fn gap(&self) -> f32 {
        if self.key.mode == RenderMode::Text {
            CH as f32
        } else {
            self.base_size() * 0.5
        }
    }
    fn rich(&mut self, spans: &[Span], role: Role) -> Rich {
        Rich::from_spans(spans, self.key.mode, role, &mut self.layout.stats)
    }
    fn label(&mut self, text: &str, role: Role) -> Rich {
        self.rich(
            &[Span {
                text: text.to_owned(),
                bold: false,
                italic: false,
                link: role == Role::Link,
                superscript: false,
            }],
            role,
        )
    }
    fn inline(&mut self, rich: Rich, flow: &mut Flow, scale: f32) -> Result<()> {
        if rich.bytes() > MAX_INLINE_BYTES {
            return Err(RenderError::Limit("normalized inline text"));
        }
        let mut rest = rich;
        while rest.bytes() > 0 {
            check_cancel(self.cancel)?;
            let (x, width) = flow.lane();
            let padding = if self.key.mode == RenderMode::Text {
                0.0
            } else {
                (self.base_size() * scale * 0.25).ceil()
            };
            let inner = width - padding * 2.0;
            if inner < CW as f32 {
                return Err(RenderError::TooNarrow);
            }
            let size = self.base_size() * scale;
            let drafts = match self.key.mode {
                RenderMode::Braille => self
                    .fonts
                    .as_deref_mut()
                    .ok_or(RenderError::FontUnavailable)?
                    .lines(&rest, inner, size)?,
                RenderMode::Text => text_lines(&rest, (inner / CW as f32).floor() as usize)?,
            };
            if drafts.is_empty() {
                break;
            }
            let mut consumed = 0;
            let mut reflow = false;
            for mut draft in drafts {
                check_cancel(self.cancel)?;
                if flow.lane() != (x, width) {
                    reflow = true;
                    break;
                }
                consumed = consumed.max(draft.end);
                if let Paint::Font(glyphs) = &mut draft.paint {
                    self.layout.stats.missing_font_glyphs +=
                        glyphs.iter().filter(|g| g.glyph.glyph_id == 0).count();
                    for g in glyphs {
                        g.glyph.x += padding;
                    }
                }
                self.layout.push(Item {
                    rect: Rect {
                        x,
                        y: flow.y,
                        w: width,
                        h: draft.height,
                    },
                    paint: draft.paint,
                })?;
                flow.y += draft.height;
            }
            if !reflow {
                break;
            }
            if consumed == 0 {
                return Err(RenderError::TooNarrow);
            }
            rest = rest.suffix(consumed);
        }
        Ok(())
    }
    fn rule(&mut self, x: f32, y: f32, w: f32, h: f32) -> Result<()> {
        if w <= 0.0 || h <= 0.0 {
            return Ok(());
        }
        self.layout.push(Item {
            rect: Rect { x, y, w, h },
            paint: Paint::Rule,
        })
    }
    fn frame(&mut self, rect: Rect) -> Result<()> {
        self.rule(rect.x, rect.y, rect.w, 2.0)?;
        self.rule(rect.x, rect.bottom() - 2.0, rect.w, 2.0)?;
        self.rule(rect.x, rect.y, 2.0, rect.h)?;
        self.rule(rect.x + rect.w - 2.0, rect.y, 2.0, rect.h)
    }
    fn float_width(&self, flow: &Flow) -> Option<f32> {
        // Preserve at least 32 body characters and 22 sidebar characters.
        let side = (flow.width * 0.35).max(self.em() * 22.0);
        let side = (side / CW as f32).ceil() * CW as f32;
        (flow.width - side - 2.0 * CW as f32 >= self.em() * 32.0).then_some(side)
    }
    fn table(
        &mut self,
        rows: &[Vec<Vec<Span>>],
        spans: &[TableSpan],
        images: &[TableImage],
        flow: &mut Flow,
        infobox: bool,
    ) -> Result<()> {
        flow.clear();
        let floated_width = if infobox {
            self.float_width(flow)
        } else {
            None
        };
        let width = floated_width.unwrap_or(flow.width);
        let x = flow.x
            + if floated_width.is_some() {
                flow.width - width
            } else {
                0.0
            };
        let top = flow.y;
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let usable = width - 2.0 * CW as f32;
        let gap = CW as f32;
        let column_width = if columns == 0 {
            0.0
        } else {
            ((usable - gap * columns.saturating_sub(1) as f32) / columns as f32 / CW as f32).floor()
                * CW as f32
        };
        let stack = column_width < self.em() * 6.0;
        let mut anchors = if spans.is_empty() {
            rows.iter()
                .enumerate()
                .flat_map(|(row, cells)| {
                    cells.iter().enumerate().map(move |(column, _)| TableSpan {
                        row,
                        column,
                        row_span: 1,
                        column_span: 1,
                    })
                })
                .collect::<Vec<_>>()
        } else {
            spans.to_vec()
        };
        anchors.sort_by_key(|span| (span.row, span.column));
        let mut occupied = vec![false; rows.len().saturating_mul(columns)];
        for span in &anchors {
            if span.row_span == 0
                || span.column_span == 0
                || span
                    .row
                    .checked_add(span.row_span)
                    .is_none_or(|end| end > rows.len())
                || span
                    .column
                    .checked_add(span.column_span)
                    .is_none_or(|end| end > columns)
                || rows
                    .get(span.row)
                    .and_then(|row| row.get(span.column))
                    .is_none()
            {
                return Err(RenderError::Limit("invalid table span"));
            }
            for row in span.row..span.row + span.row_span {
                for column in span.column..span.column + span.column_span {
                    let slot = &mut occupied[row * columns + column];
                    if *slot {
                        return Err(RenderError::Limit("overlapping table spans"));
                    }
                    *slot = true;
                }
            }
        }
        let mut images_at: HashMap<(usize, usize), Vec<&ImageRef>> = HashMap::new();
        for positioned in images {
            if !anchors
                .iter()
                .any(|span| span.row == positioned.row && span.column == positioned.column)
            {
                return Err(RenderError::Limit("invalid table image anchor"));
            }
            images_at
                .entry((positioned.row, positioned.column))
                .or_default()
                .push(&positioned.image);
        }
        let mut drafts = Vec::with_capacity(anchors.len());
        let mut drafted_items = self.layout.items.len();
        for span in anchors {
            check_cancel(self.cancel)?;
            let cell_width = if stack {
                usable
            } else {
                column_width * span.column_span as f32
                    + gap * span.column_span.saturating_sub(1) as f32
            };
            if cell_width < CW as f32 {
                return Err(RenderError::TooNarrow);
            }
            let first = self.layout.items.len();
            let mut cell_flow = Flow {
                x: 0.0,
                width: cell_width,
                y: 0.0,
                float: None,
            };
            let content = self.rich(&rows[span.row][span.column], Role::Body);
            self.inline(content, &mut cell_flow, 0.85)?;
            if let Some(references) = images_at.get(&(span.row, span.column)) {
                for reference in references {
                    self.image(reference, &mut cell_flow, false)?;
                }
            }
            let items = self.layout.items.split_off(first);
            drafted_items = drafted_items.saturating_add(items.len());
            if drafted_items > MAX_ITEMS {
                return Err(RenderError::Limit("table display-list items"));
            }
            drafts.push(TableCellDraft {
                span,
                items,
                height: cell_flow.y.max(CH as f32),
            });
        }
        let mut y = top + CH as f32;
        if stack {
            for row in 0..rows.len() {
                let before = y;
                for draft in drafts.iter_mut().filter(|draft| draft.span.row == row) {
                    self.place_table_cell(draft, x + CW as f32, y)?;
                    y += draft.height + self.gap();
                }
                y = y.max(before + CH as f32) + CH as f32;
                self.rule(x, y - CH as f32 / 2.0, width, 2.0)?;
            }
        } else {
            let mut row_heights = vec![CH as f32; rows.len()];
            for draft in &drafts {
                if draft.span.row_span == 1 {
                    let slot = &mut row_heights[draft.span.row];
                    *slot = (*slot).max(draft.height + self.gap());
                }
            }
            for draft in &drafts {
                if draft.span.row_span > 1 {
                    let end = draft.span.row + draft.span.row_span;
                    let occupied_height: f32 = row_heights[draft.span.row..end].iter().sum();
                    row_heights[end - 1] += (draft.height + self.gap() - occupied_height).max(0.0);
                }
            }
            let mut row_starts = Vec::with_capacity(rows.len());
            for height in &row_heights {
                row_starts.push(y);
                y += *height;
            }
            for draft in &mut drafts {
                let left = x + CW as f32 + draft.span.column as f32 * (column_width + gap);
                self.place_table_cell(draft, left, row_starts[draft.span.row])?;
            }
            for (index, height) in row_heights.iter().enumerate() {
                let line_y = row_starts[index] + height - CH as f32 / 2.0;
                // A row-spanning cell continues across this boundary. Draw
                // the separator only outside its occupied horizontal lane.
                let mut exclusions: Vec<(f32, f32)> = drafts
                    .iter()
                    .filter(|draft| {
                        draft.span.row <= index && draft.span.row + draft.span.row_span > index + 1
                    })
                    .map(|draft| {
                        let left = x + CW as f32 + draft.span.column as f32 * (column_width + gap);
                        let right = left
                            + column_width * draft.span.column_span as f32
                            + gap * draft.span.column_span.saturating_sub(1) as f32;
                        (left, right)
                    })
                    .collect();
                exclusions.sort_by(|left, right| left.0.total_cmp(&right.0));
                let mut cursor = x;
                for (left, right) in exclusions {
                    self.rule(cursor, line_y, (left - cursor).max(0.0), 2.0)?;
                    cursor = cursor.max(right);
                }
                self.rule(cursor, line_y, (x + width - cursor).max(0.0), 2.0)?;
            }
        }
        let bottom = y + CH as f32;
        self.frame(Rect {
            x,
            y: top,
            w: width,
            h: bottom - top,
        })?;
        if floated_width.is_some() {
            flow.float = Some(Float {
                right: true,
                width,
                bottom: bottom + self.gap(),
            });
        } else {
            flow.y = bottom + self.gap();
        }
        Ok(())
    }
    fn place_table_cell(&mut self, draft: &mut TableCellDraft, x: f32, y: f32) -> Result<()> {
        for mut item in draft.items.drain(..) {
            item.rect.x += x;
            item.rect.y += y;
            if item.rect.bottom() > MAX_PAGE_PX {
                return Err(RenderError::Limit("page height"));
            }
            self.layout.bottom = self.layout.bottom.max(item.rect.bottom());
            self.layout.items.push(item);
        }
        Ok(())
    }
    fn image(&mut self, image: &ImageRef, flow: &mut Flow, allow_float: bool) -> Result<()> {
        flow.clear();
        let wants_float =
            allow_float && matches!(image.float, ImageFloat::Left | ImageFloat::Right);
        let floated_width = if wants_float {
            self.float_width(flow)
        } else {
            None
        };
        let width = floated_width.unwrap_or(flow.width);
        let right = matches!(image.float, ImageFloat::Right);
        let x = flow.x
            + if floated_width.is_some() && right {
                flow.width - width
            } else {
                0.0
            };
        let top = flow.y;
        let usable = width - 2.0 * CW as f32;
        if usable < CW as f32 {
            return Err(RenderError::TooNarrow);
        }
        let decoded = self
            .document
            .images
            .get(&image.url)
            .filter(|p| p.width() > 0 && p.height() > 0);
        let intrinsic = decoded
            .map(|p| (p.width(), p.height()))
            .unwrap_or((image.width.max(1), image.height.max(1)));
        let wanted = if image.width > 0 {
            image.width as f32
        } else {
            intrinsic.0 as f32
        };
        let zoom = if self.key.mode == RenderMode::Braille {
            f32::from(self.key.zoom) / 100.0
        } else {
            1.0
        };
        let iw = (wanted * zoom).min(usable).max(1.0);
        let ih = iw * intrinsic.1 as f32 / intrinsic.0 as f32;
        if !ih.is_finite() || ih > MAX_PAGE_PX {
            return Err(RenderError::Limit("image display height"));
        }
        let mut caption = Flow {
            x: x + CW as f32,
            width: usable,
            y: top + CH as f32,
            float: None,
        };
        if decoded.is_some() {
            self.layout.push(Item {
                rect: Rect {
                    x: x + (width - iw) * 0.5,
                    y: caption.y,
                    w: iw,
                    h: ih,
                },
                paint: Paint::Image(image.url.clone()),
            })?;
            caption.y += ih;
            if self.key.mode == RenderMode::Text {
                caption.y = (caption.y / CH as f32).ceil() * CH as f32;
            }
        } else {
            self.layout.stats.unavailable_images += 1;
            let unavailable = self.label("[Image unavailable]", Role::Muted);
            self.inline(unavailable, &mut caption, 0.8)?;
        }
        caption.y += self.gap();
        let rich = self.rich(&image.caption, Role::Muted);
        self.inline(rich, &mut caption, 0.8)?;
        caption.y += CH as f32;
        self.frame(Rect {
            x,
            y: top,
            w: width,
            h: caption.y - top,
        })?;
        if floated_width.is_some() {
            flow.float = Some(Float {
                right,
                width,
                bottom: caption.y + self.gap(),
            });
        } else {
            flow.y = caption.y + self.gap();
        }
        Ok(())
    }
    fn build(mut self) -> Result<Layout> {
        let document = self.document;
        let width = f32::from(self.key.columns) * CW as f32;
        let margin = if self.key.columns >= 8 {
            CW as f32
        } else {
            0.0
        };
        let mut flow = Flow {
            x: margin,
            width: width - 2.0 * margin,
            y: CH as f32,
            float: None,
        };
        let title = self.label(&document.title, Role::Heading);
        self.inline(title, &mut flow, 1.6)?;
        flow.y += self.gap();
        // Borrow the caller-owned document independently of the mutable builder.
        let document = self.document;
        for block in &document.blocks {
            check_cancel(self.cancel)?;
            match block {
                Block::Paragraph(spans) => {
                    let rich = self.rich(spans, Role::Body);
                    self.inline(rich, &mut flow, 1.0)?;
                    flow.y += self.gap();
                }
                Block::Heading { level, spans } => {
                    flow.y += self.gap();
                    let rich = self.rich(spans, Role::Heading);
                    let scale = match level {
                        0 | 1 => 1.5,
                        2 => 1.3,
                        3 => 1.15,
                        _ => 1.0,
                    };
                    self.inline(rich, &mut flow, scale)?;
                    let (x, w) = flow.lane();
                    self.rule(x, flow.y, w, 2.0)?;
                    flow.y += self.gap();
                }
                Block::List { ordered, items } => {
                    for (i, spans) in items.iter().enumerate() {
                        let (x, width) = flow.lane();
                        let prefix = if *ordered {
                            format!("{}. ", i + 1)
                        } else {
                            "• ".to_owned()
                        };
                        let indent = ((prefix.chars().count() as f32 * self.em() + self.em())
                            / CW as f32)
                            .ceil()
                            * CW as f32;
                        if width <= indent + 2.0 * self.em() {
                            return Err(RenderError::TooNarrow);
                        }
                        let mut marker = Flow {
                            x,
                            width: indent,
                            y: flow.y,
                            float: None,
                        };
                        let mark = self.label(&prefix, Role::Body);
                        self.inline(mark, &mut marker, 1.0)?;
                        // Keep the hanging indent while allowing this same item's later
                        // lines to regain the full lane below an active float.
                        let mut body = Flow {
                            x: flow.x + indent,
                            width: flow.width - indent,
                            y: flow.y,
                            float: flow.float,
                        };
                        let rich = self.rich(spans, Role::Body);
                        self.inline(rich, &mut body, 1.0)?;
                        flow.y = body.y.max(marker.y);
                    }
                    flow.y += self.gap();
                }
                Block::Table {
                    rows,
                    spans,
                    images,
                    infobox,
                } => self.table(rows, spans, images, &mut flow, *infobox)?,
                Block::Image(image) => self.image(image, &mut flow, true)?,
            }
        }
        flow.clear();
        flow.y += self.gap();
        self.layout.bottom = self.layout.bottom.max(flow.y + CH as f32);
        if self.layout.bottom > MAX_PAGE_PX {
            return Err(RenderError::Limit("page height"));
        }
        self.layout.finish()?;
        Ok(self.layout)
    }
}

/// Validate before copying strings, shaping, or pinning a decoded document in the cache.
fn validate_document(document: &Document) -> Result<usize> {
    fn add(total: &mut usize, n: usize, cap: usize, name: &'static str) -> Result<()> {
        *total = total.checked_add(n).ok_or(RenderError::Limit(name))?;
        if *total > cap {
            return Err(RenderError::Limit(name));
        }
        Ok(())
    }
    fn string(text: &str, total: &mut usize) -> Result<()> {
        if text.len() > MAX_INLINE_BYTES {
            return Err(RenderError::Limit("one source string"));
        }
        add(total, text.len(), MAX_TEXT_BYTES, "source text bytes")
    }
    fn spans(spans: &[Span], text: &mut usize, nodes: &mut usize) -> Result<()> {
        add(nodes, spans.len(), MAX_SOURCE_NODES, "source nodes")?;
        let mut group = 0;
        for span in spans {
            string(&span.text, text)?;
            add(
                &mut group,
                span.text.len(),
                MAX_INLINE_BYTES,
                "one inline group",
            )?;
        }
        Ok(())
    }
    let mut text = 0;
    let mut nodes = 0;
    string(&document.title, &mut text)?;
    string(&document.url, &mut text)?;
    string(&document.date, &mut text)?;
    if let Some(revision) = &document.revision {
        string(revision, &mut text)?;
    }
    add(
        &mut nodes,
        document.blocks.len(),
        MAX_SOURCE_NODES,
        "source nodes",
    )?;
    for block in &document.blocks {
        match block {
            Block::Paragraph(s) | Block::Heading { spans: s, .. } => {
                spans(s, &mut text, &mut nodes)?
            }
            Block::List { items, .. } => {
                add(&mut nodes, items.len(), MAX_SOURCE_NODES, "source nodes")?;
                for s in items {
                    spans(s, &mut text, &mut nodes)?;
                }
            }
            Block::Table {
                rows,
                spans: geometry,
                images: positioned,
                ..
            } => {
                add(&mut nodes, rows.len(), MAX_SOURCE_NODES, "source nodes")?;
                for row in rows {
                    add(&mut nodes, row.len(), MAX_SOURCE_NODES, "source nodes")?;
                    for cell in row {
                        spans(cell, &mut text, &mut nodes)?;
                    }
                }
                add(&mut nodes, geometry.len(), MAX_SOURCE_NODES, "source nodes")?;
                add(
                    &mut nodes,
                    positioned.len(),
                    MAX_SOURCE_NODES,
                    "source nodes",
                )?;
                for image in positioned {
                    string(&image.image.url, &mut text)?;
                    spans(&image.image.caption, &mut text, &mut nodes)?;
                }
            }
            Block::Image(i) => {
                string(&i.url, &mut text)?;
                spans(&i.caption, &mut text, &mut nodes)?;
            }
        }
    }
    add(
        &mut nodes,
        document.images.len(),
        MAX_SOURCE_NODES,
        "source nodes",
    )?;
    let mut images = 0;
    for (url, image) in &document.images {
        string(url, &mut text)?;
        add(
            &mut images,
            image.as_raw().len(),
            MAX_IMAGE_BYTES,
            "decoded image bytes",
        )?;
    }
    Ok(images)
}

#[derive(Clone, Copy, Default)]
struct DotInk {
    rgb: [u8; 3],
    alpha: u8,
}
struct Strip {
    band: u32,
    dots: Vec<DotInk>,
}
struct Canvas {
    width: usize,
    origin: i32,
    pixels: Vec<[u8; 4]>,
}
impl Canvas {
    fn new(width: usize, band: u32) -> Self {
        Self {
            width,
            origin: (band as usize * STRIP_PX) as i32,
            pixels: vec![[0; 4]; width * STRIP_PX],
        }
    }
    // Canvas stores premultiplied channels. No implicit white/black page rectangle.
    fn over(&mut self, x: i32, y: i32, rgba: [u8; 4], clip: Rect) {
        if x < 0
            || x as usize >= self.width
            || y < self.origin
            || y >= self.origin + STRIP_PX as i32
            || rgba[3] == 0
            || (x as f32) < clip.x
            || (x as f32) >= clip.x + clip.w
            || (y as f32) < clip.y
            || (y as f32) >= clip.bottom()
        {
            return;
        }
        let p = &mut self.pixels[(y - self.origin) as usize * self.width + x as usize];
        let a = u32::from(rgba[3]);
        for (target, source) in p[..3].iter_mut().zip(&rgba[..3]) {
            *target = ((u32::from(*source) * a + u32::from(*target) * (255 - a) + 127) / 255)
                .min(255) as u8;
        }
        p[3] = (a + (u32::from(p[3]) * (255 - a) + 127) / 255).min(255) as u8;
    }
    fn reduce(self, band: u32) -> Strip {
        let dot_width = self.width / DOT;
        let mut dots = vec![DotInk::default(); dot_width * STRIP_DOTS];
        for y in 0..STRIP_DOTS {
            for x in 0..dot_width {
                let mut sums = [0_u32; 4];
                for dy in 0..DOT {
                    for dx in 0..DOT {
                        let pixel = self.pixels[(y * DOT + dy) * self.width + x * DOT + dx];
                        for (sum, channel) in sums.iter_mut().zip(pixel) {
                            *sum += u32::from(channel);
                        }
                    }
                }
                if sums[3] > 0 {
                    dots[y * dot_width + x] = DotInk {
                        rgb: std::array::from_fn(|c| {
                            ((sums[c] * 255 + sums[3] / 2) / sums[3]).min(255) as u8
                        }),
                        alpha: ((sums[3] + 8) / 16) as u8,
                    };
                }
            }
        }
        Strip { band, dots }
    }
}
fn luma(rgb: [u8; 3]) -> u8 {
    ((54 * u32::from(rgb[0]) + 183 * u32::from(rgb[1]) + 19 * u32::from(rgb[2]) + 128) / 256) as u8
}
fn transform(rgb: [u8; 3], s: WikipediaSettings) -> [u8; 3] {
    s.color(rgb)
}
fn ink(role: Role, s: WikipediaSettings) -> [u8; 3] {
    transform(
        match role {
            Role::Body => [160, 168, 180],
            Role::Heading => [198, 206, 218],
            Role::Link => [102, 160, 230],
            Role::Muted => [122, 138, 156],
        },
        s,
    )
}
/// Bilinear source sampling is alpha-aware, avoiding black fringes on transparent images.
fn sample_image(image: &image::RgbaImage, u: f32, v: f32) -> [u8; 4] {
    let x = u * image.width() as f32 - 0.5;
    let y = v * image.height() as f32 - 0.5;
    let (ix, iy) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let mut sums = [0.0_f32; 4];
    for (dx, dy, weight) in [
        (0, 0, (1.0 - fx) * (1.0 - fy)),
        (1, 0, fx * (1.0 - fy)),
        (0, 1, (1.0 - fx) * fy),
        (1, 1, fx * fy),
    ] {
        let p = image
            .get_pixel(
                (ix + dx).clamp(0, i64::from(image.width()) - 1) as u32,
                (iy + dy).clamp(0, i64::from(image.height()) - 1) as u32,
            )
            .0;
        let a = f32::from(p[3]) * weight;
        for (sum, channel) in sums[..3].iter_mut().zip(&p[..3]) {
            *sum += f32::from(*channel) * a;
        }
        sums[3] += a;
    }
    if sums[3] <= 0.0 {
        return [0; 4];
    }
    [
        (sums[0] / sums[3]).round() as u8,
        (sums[1] / sums[3]).round() as u8,
        (sums[2] / sums[3]).round() as u8,
        sums[3].round() as u8,
    ]
}
/// Bright paper borders mean dark diagram marks are positive ink. Transparent
/// border pixels count as paper; ordinary photos keep light-driven coverage.
fn image_has_light_paper(image: &image::RgbaImage) -> bool {
    let samples = image.width().clamp(1, 64);
    let mut sum = 0_u64;
    for step in 0..samples {
        let x = step * image.width() / samples;
        for y in [0, image.height() - 1] {
            let p = image.get_pixel(x, y).0;
            let alpha = u32::from(p[3]);
            sum += u64::from(
                (u32::from(luma([p[0], p[1], p[2]])) * alpha + 255 * (255 - alpha) + 127) / 255,
            );
        }
    }
    sum / u64::from(samples * 2) >= 190
}
#[derive(Clone, Copy, Debug)]
struct ImageContrast {
    alpha_backed: bool,
    dark_marks: bool,
}
/// Analyze every source alpha once per image. A regular grid would alias a
/// one-pixel checkerboard and incorrectly erase all its black source marks.
fn analyze_image_contrast(
    image: &image::RgbaImage,
    cancel: Option<&AtomicBool>,
) -> Result<ImageContrast> {
    let total = u64::from(image.width()) * u64::from(image.height());
    let threshold = total.div_ceil(16);
    let mut transparent = 0_u64;
    for (index, pixel) in image.as_raw().chunks_exact(4).enumerate() {
        if index % 262_144 == 0 {
            check_cancel(cancel)?;
        }
        transparent += u64::from(pixel[3] < 240);
        if transparent >= threshold {
            break;
        }
    }
    let alpha_backed = transparent >= threshold;
    Ok(ImageContrast {
        alpha_backed,
        dark_marks: !alpha_backed && image_has_light_paper(image),
    })
}
fn raster_strip(
    layout: &mut Layout,
    doc: &Document,
    fonts: &mut Option<Fonts>,
    columns: u16,
    settings: WikipediaSettings,
    band: u32,
) -> Result<Strip> {
    let mut canvas = Canvas::new(usize::from(columns) * CW, band);
    if let Some(indices) = layout.bands.get(&band) {
        for &i in indices {
            let item = &layout.items[i];
            let rect = item.rect;
            match &item.paint {
                Paint::Text(_) => {}
                Paint::Font(glyphs) => {
                    let fonts = fonts.as_mut().ok_or(RenderError::FontUnavailable)?;
                    for g in glyphs {
                        fonts.trim_cache();
                        let physical = g.glyph.physical((rect.x, rect.y + g.baseline), 1.0);
                        let is_new = !fonts.swash.image_cache.contains_key(&physical.cache_key);
                        if let Some(image) =
                            fonts.swash.get_image(&mut fonts.system, physical.cache_key)
                        {
                            if image.data.len() > 4 * 1024 * 1024 {
                                return Err(RenderError::Limit("one font raster"));
                            }
                            if is_new {
                                fonts.cache_bytes += image.data.len();
                            }
                        }
                        let rgb = ink(g.style.role, settings);
                        let thickness = if g.style.bold {
                            (g.glyph.font_size / 32.0).ceil() as i32
                        } else {
                            0
                        };
                        fonts.swash.with_pixels(
                            &mut fonts.system,
                            physical.cache_key,
                            FontColor::rgb(rgb[0], rgb[1], rgb[2]),
                            |x, y, color| {
                                let shear = if g.style.italic {
                                    (-y as f32 * 0.20).round() as i32
                                } else {
                                    0
                                };
                                for dx in 0..=thickness {
                                    canvas.over(
                                        physical.x + x + shear + dx,
                                        physical.y + y,
                                        [rgb[0], rgb[1], rgb[2], color.a()],
                                        rect,
                                    );
                                }
                            },
                        );
                    }
                }
                Paint::Image(url) => {
                    let Some(image) = doc
                        .images
                        .get(url)
                        .filter(|p| p.width() > 0 && p.height() > 0)
                    else {
                        continue;
                    };
                    let contrast = match layout.image_contrast.get(url).copied() {
                        Some(contrast) => contrast,
                        None => {
                            let contrast = analyze_image_contrast(image, None)?;
                            layout.image_contrast.insert(url.clone(), contrast);
                            contrast
                        }
                    };
                    let alpha_backed = contrast.alpha_backed;
                    let dark_marks = contrast.dark_marks;
                    let left = rect.x.floor().max(0.0) as i32;
                    let right = (rect.x + rect.w).ceil().min(canvas.width as f32) as i32;
                    let top = (rect.y.floor() as i32).max(canvas.origin);
                    let bottom = (rect.bottom().ceil() as i32).min(canvas.origin + STRIP_PX as i32);
                    for y in top..bottom {
                        for x in left..right {
                            let p = sample_image(
                                image,
                                (x as f32 + 0.5 - rect.x) / rect.w,
                                (y as f32 + 0.5 - rect.y) / rect.h,
                            );
                            let source = [p[0], p[1], p[2]];
                            let luminance = luma(source);
                            let tone = if alpha_backed {
                                luminance.max(255 - luminance)
                            } else if dark_marks {
                                255 - luminance
                            } else {
                                luminance
                            };
                            let rgb =
                                if (dark_marks || alpha_backed) && luminance < 128 && tone > 128 {
                                    ink(Role::Body, settings)
                                } else {
                                    transform(source, settings)
                                };
                            // Sparse ink follows the source's contrast and alpha. White
                            // diagram paper, opaque black planes and alpha-zero pixels stay clear.
                            let alpha = ((u32::from(p[3]) * u32::from(tone) + 127) / 255) as u8;
                            canvas.over(x, y, [rgb[0], rgb[1], rgb[2], alpha], rect);
                        }
                    }
                }
                Paint::Rule => {
                    let rgb = ink(Role::Muted, settings);
                    for y in (rect.y.floor() as i32).max(canvas.origin)
                        ..(rect.bottom().ceil() as i32).min(canvas.origin + STRIP_PX as i32)
                    {
                        for x in (rect.x.floor() as i32).max(0)
                            ..((rect.x + rect.w).ceil() as i32).min(canvas.width as i32)
                        {
                            canvas.over(x, y, [rgb[0], rgb[1], rgb[2], 255], rect);
                        }
                    }
                }
            }
        }
    }
    Ok(canvas.reduce(band))
}

struct Attempt {
    document: Weak<Document>,
    key: GeometryKey,
    error: Option<RenderError>,
}
/// Client-local cache. This type owns neither a loader nor a scene worker.
#[derive(Default)]
pub struct WikipediaRenderer {
    document: Option<Arc<Document>>,
    attempt: Option<Attempt>,
    layout: Option<Layout>,
    fonts: Option<Fonts>,
    settings: WikipediaSettings,
    columns: u16,
    rows: u16,
    cells: Vec<RenderCell>,
    strips: VecDeque<Strip>,
    strip_bytes: usize,
    last_view: Option<(u16, u32)>,
    work: WorkStats,
}
impl fmt::Debug for WikipediaRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WikipediaRenderer")
            .field("columns", &self.columns)
            .field("rows", &self.rows)
            .field("stats", &self.layout_stats())
            .field("cached_strip_bytes", &self.strip_bytes)
            .finish()
    }
}
impl WikipediaRenderer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn release(&mut self) {
        *self = Self::default();
    }
    #[cfg(test)]
    pub fn width(&self) -> u16 {
        self.columns
    }
    #[cfg(test)]
    pub fn height(&self) -> u16 {
        self.rows
    }
    #[cfg(test)]
    pub fn cells(&self) -> &[RenderCell] {
        &self.cells
    }
    pub fn cell(&self, x: u16, y: u16) -> Option<&RenderCell> {
        if x >= self.columns || y >= self.rows {
            return None;
        }
        self.cells
            .get(usize::from(y) * usize::from(self.columns) + usize::from(x))
    }
    pub fn total_rows(&self) -> u32 {
        self.layout
            .as_ref()
            .map_or(0, |l| (l.bottom / CH as f32).ceil() as u32)
    }
    pub fn layout_stats(&self) -> Option<LayoutStats> {
        self.layout.as_ref().map(|l| l.stats)
    }
    #[cfg(test)]
    pub fn work_stats(&self) -> WorkStats {
        self.work
    }
    /// Payload bytes only: not FontSystem memory, decoded document overhead, or RSS.
    #[cfg(test)]
    pub fn cached_strip_bytes(&self) -> usize {
        self.strip_bytes
    }
    fn clear_ink(&mut self) {
        self.strips.clear();
        self.strip_bytes = 0;
        self.last_view = None;
        for cell in &mut self.cells {
            cell.clear();
        }
    }
    fn clear_output(&mut self) {
        self.cells.clear();
        self.rows = 0;
        self.last_view = None;
    }
    /// Call on every current frame; pointer identity is safe because the cache pins an
    /// immutable Arc<Document>. A new Arc, width, mode or Braille zoom rebuilds layout.
    /// Palette changes invalidate only ink; scroll/dwell changes invalidate neither.
    /// Failed attempts are memoized with Weak (no failed decoded-image retention).
    pub fn prepare(
        &mut self,
        document: Arc<Document>,
        columns: u16,
        settings: &WikipediaSettings,
    ) -> Result<bool> {
        self.prepare_inner(document, columns, settings, None)
    }
    /// Optional worker stop flag, checked between bounded layout units. One cosmic-text
    /// shaping call is not interruptible; the host must retain its worker admission until exit.
    pub fn prepare_cancellable(
        &mut self,
        document: Arc<Document>,
        columns: u16,
        settings: &WikipediaSettings,
        cancel: &AtomicBool,
    ) -> Result<bool> {
        self.prepare_inner(document, columns, settings, Some(cancel))
    }
    fn prepare_inner(
        &mut self,
        document: Arc<Document>,
        columns: u16,
        settings: &WikipediaSettings,
        cancel: Option<&AtomicBool>,
    ) -> Result<bool> {
        if check_cancel(cancel).is_err() {
            self.release();
            return Err(RenderError::Cancelled);
        }
        let settings = settings.normalized();
        let key = settings.geometry(columns);
        let weak = Arc::downgrade(&document);
        let same = self
            .attempt
            .as_ref()
            .is_some_and(|a| a.key == key && a.document.ptr_eq(&weak));
        if self.settings.ink() != settings.ink() {
            self.clear_ink();
        }
        self.settings = settings;
        if same {
            return match self.attempt.as_ref().and_then(|a| a.error.clone()) {
                Some(error) => Err(error),
                None => Ok(false),
            };
        }
        // Release old images, shapes and font caches before constructing a replacement.
        self.document = None;
        self.layout = None;
        self.fonts = None;
        self.clear_ink();
        self.clear_output();
        self.columns = columns;
        self.attempt = Some(Attempt {
            document: weak,
            key,
            error: None,
        });
        self.work.layouts = self.work.layouts.saturating_add(1);
        let build = (|| {
            if columns == 0 {
                return Err(RenderError::TooNarrow);
            }
            if columns > MAX_COLUMNS {
                return Err(RenderError::Limit("viewport columns"));
            }
            let image_bytes = validate_document(&document)?;
            check_cancel(cancel)?;
            let mut fonts = if key.mode == RenderMode::Braille {
                Some(Fonts::new()?)
            } else {
                None
            };
            let mut layout = Builder {
                document: &document,
                key,
                fonts: fonts.as_mut(),
                cancel,
                layout: Layout::default(),
            }
            .build()?;
            layout.cache_image_contrast(&document, cancel)?;
            layout.stats.source_image_bytes = image_bytes;
            Ok((layout, fonts))
        })();
        match build {
            Ok((layout, fonts)) => {
                self.layout = Some(layout);
                self.fonts = fonts;
                self.document = Some(document);
                Ok(true)
            }
            Err(error) => {
                if error == RenderError::Cancelled {
                    self.attempt = None;
                } else if let Some(attempt) = &mut self.attempt {
                    attempt.error = Some(error.clone());
                }
                Err(error)
            }
        }
    }
    fn ensure_strip(&mut self, band: u32) -> Result<()> {
        if self.strips.back().is_some_and(|s| s.band == band) {
            return Ok(());
        }
        if let Some(i) = self.strips.iter().position(|s| s.band == band) {
            if let Some(strip) = self.strips.remove(i) {
                self.strips.push_back(strip);
            }
            return Ok(());
        }
        let strip = raster_strip(
            self.layout.as_mut().ok_or(RenderError::NotPrepared)?,
            self.document.as_deref().ok_or(RenderError::NotPrepared)?,
            &mut self.fonts,
            self.columns,
            self.settings,
            band,
        )?;
        let bytes = strip.dots.capacity() * std::mem::size_of::<DotInk>();
        while self.strips.len() >= MAX_STRIPS || self.strip_bytes + bytes > MAX_STRIP_BYTES {
            if let Some(old) = self.strips.pop_front() {
                self.strip_bytes -= old.dots.capacity() * std::mem::size_of::<DotInk>();
            } else {
                return Err(RenderError::Limit("one cached raster strip"));
            }
        }
        self.strip_bytes += bytes;
        self.strips.push_back(strip);
        self.work.raster_strips = self.work.raster_strips.saturating_add(1);
        Ok(())
    }
    /// `offset_rows` is a physical terminal-row offset. Text snaps to whole rows;
    /// Braille snaps to one dot (1/4 row). Errors leave no stale/partial output.
    pub fn render(&mut self, rows: u16, offset_rows: f64) -> Result<&[RenderCell]> {
        if !offset_rows.is_finite() || offset_rows < 0.0 {
            self.clear_output();
            return Err(RenderError::InvalidOffset);
        }
        if self.layout.is_none() {
            self.clear_output();
            return Err(RenderError::NotPrepared);
        }
        if rows > MAX_ROWS || usize::from(rows) * usize::from(self.columns) > MAX_CELLS {
            self.clear_output();
            return Err(RenderError::Limit("viewport cells or rows"));
        }
        let maximum = self.total_rows().saturating_sub(u32::from(rows));
        let offset = offset_rows.min(f64::from(maximum));
        let top = if self.settings.render_mode == RenderMode::Text {
            offset.floor() as u32 * 4
        } else {
            (offset * 4.0).floor() as u32
        };
        if self.last_view == Some((rows, top)) {
            return Ok(&self.cells);
        }
        self.last_view = None;
        self.rows = rows;
        self.cells.resize_with(
            usize::from(rows) * usize::from(self.columns),
            RenderCell::default,
        );
        for c in &mut self.cells {
            c.clear();
        }
        if let Err(error) = self.paint_view(top) {
            self.clear_output();
            return Err(error);
        }
        self.last_view = Some((rows, top));
        self.work.packed_viewports = self.work.packed_viewports.saturating_add(1);
        Ok(&self.cells)
    }

    /// Keep a last-good layout visible while a replacement layout is admitted.
    /// A resized viewport may ask for more cells than this older frame owns, so
    /// clip only this retained view instead of clearing it as an invalid render.
    pub fn render_retained(&mut self, rows: u16, offset_rows: f64) -> Result<&[RenderCell]> {
        let row_limit = if self.columns == 0 {
            0
        } else {
            (MAX_CELLS / usize::from(self.columns)).min(usize::from(MAX_ROWS)) as u16
        };
        self.render(rows.min(row_limit), offset_rows)
    }
    fn paint_view(&mut self, top: u32) -> Result<()> {
        const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        let width = usize::from(self.columns);
        let mut packed = vec![(0_u8, [0_u32; 3], 0_u32); width];
        for row in 0..usize::from(self.rows) {
            packed.fill((0, [0; 3], 0));
            for (dy, bits) in BITS.iter().enumerate() {
                let global_y = top as usize + row * 4 + dy;
                self.ensure_strip((global_y / STRIP_DOTS) as u32)?;
                let strip = self.strips.back().ok_or(RenderError::NotPrepared)?;
                for (x, (mask, sums, weight)) in packed.iter_mut().enumerate() {
                    for (dx, bit) in bits.iter().enumerate() {
                        let dot = strip.dots[(global_y % STRIP_DOTS) * width * 2 + x * 2 + dx];
                        if f32::from(dot.alpha) / 255.0
                            > threshold(x * 2 + dx, global_y, DitherMode::Ordered)
                        {
                            *mask |= *bit;
                            for (sum, channel) in sums.iter_mut().zip(dot.rgb) {
                                *sum += u32::from(channel) * u32::from(dot.alpha);
                            }
                            *weight += u32::from(dot.alpha);
                        }
                    }
                }
            }
            for (x, (mask, sums, weight)) in packed.iter().enumerate() {
                if *mask == 0 || *weight == 0 {
                    continue;
                }
                let c = char::from_u32(0x2800 + u32::from(*mask)).unwrap_or(' ');
                let mut encoded = [0; 4];
                self.cells[row * width + x].set(
                    c.encode_utf8(&mut encoded),
                    sums.map(|v| ((v + *weight / 2) / *weight) as u8),
                    Style::default(),
                );
            }
        }
        if self.settings.render_mode == RenderMode::Text && self.rows > 0 {
            let layout = self.layout.as_ref().ok_or(RenderError::NotPrepared)?;
            let start_row = top / 4;
            let first_band = top as usize / STRIP_DOTS;
            let last_band = (top as usize + usize::from(self.rows) * 4 - 1) / STRIP_DOTS;
            for (_, indices) in layout.bands.range(first_band as u32..=last_band as u32) {
                for &i in indices {
                    let item = &layout.items[i];
                    let Paint::Text(glyphs) = &item.paint else {
                        continue;
                    };
                    let y = (item.rect.y / CH as f32).round() as u32;
                    if y < start_row || y >= start_row + u32::from(self.rows) {
                        continue;
                    }
                    let left = (item.rect.x / CW as f32).round() as usize;
                    let mut column = left;
                    for g in glyphs {
                        let cell_width = if g.symbol == " " {
                            1
                        } else {
                            safe_symbol_width(&g.symbol).ok_or(RenderError::TooNarrow)?
                        };
                        if column + cell_width > width {
                            return Err(RenderError::TooNarrow);
                        }
                        if g.symbol != " " {
                            let index = (y - start_row) as usize * width + column;
                            self.cells[index].set(
                                &g.symbol,
                                ink(g.style.role, self.settings),
                                g.style,
                            );
                            if cell_width == 2 {
                                self.cells[index + 1].reserve_continuation();
                            }
                        }
                        column += cell_width;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
