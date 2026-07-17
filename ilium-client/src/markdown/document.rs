//! Parses markdown text into a `Document`: a flat list of `Block`s ready
//! for rendering. Headings and standalone images are pulled out as their
//! own blocks (`raster.rs`/`render.rs` turn those into pixel images later);
//! everything else -- paragraphs, lists, blockquotes, code, tables, rules
//! -- becomes styled `ratatui::text::Line`s directly, since that content
//! is never rasterized.
//!
//! Paragraph reflow is left to `ratatui::widgets::Paragraph`'s own word
//! wrap at render time (soft line breaks become a plain space here, not a
//! hard `Line` split), so wrapping stays correct across terminal resizes
//! without this module tracking any width.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// One renderable piece of a parsed document, in source order.
#[derive(Debug, Clone)]
pub enum Block {
    Heading {
        text: String,
        level: u8,
    },
    /// A paragraph containing nothing but a single image -- the only shape
    /// of image this renders as a pixel image (an inline image mixed into
    /// running text is shown as a small placeholder span instead, since
    /// compositing a raster image into the middle of a text line isn't
    /// supported by any terminal graphics protocol).
    Image {
        alt: String,
        path: ImagePath,
    },
    /// Source-authored empty rows between visible Markdown constructs.
    /// Keeping spacing explicit prevents later rendering and viewport
    /// layers from having to guess paragraph margins.
    BlankLines(Arc<Vec<Line<'static>>>),
    Text(Arc<Vec<Line<'static>>>),
}

/// A resolved (but not yet loaded) image reference.
#[derive(Debug, Clone)]
pub enum ImagePath {
    Local(PathBuf),
    /// Anything not resolvable to a local file (a bare `http(s)://` URL, or
    /// a malformed reference) -- rendered as a text placeholder rather
    /// than fetched, so opening a markdown file never makes a network
    /// call. See `ilium/CLAUDE.md` / global IP-reputation policy.
    Unsupported(String),
}

const CODE_BG: Color = Color::Rgb(0x2a, 0x2e, 0x37);
const CODE_FG: Color = Color::Rgb(0xd8, 0xdc, 0xe4);
const QUOTE_FG: Color = Color::Rgb(0x7a, 0x8a, 0xa0);
/// Also used by `raster.rs` (as the header glyph fill color) and
/// `render.rs` (as the plain-text fallback when rasterization fails), so
/// a header reads the same accent color whether it renders as a pixel
/// image or falls back to text.
pub const HEADING_FG: Color = Color::Rgb(0xe0, 0xaf, 0x68);
const LINK_FG: Color = Color::Cyan;
const RULE_WIDTH: usize = 400;

/// Parses `markdown` (whose file lives at `base_dir` -- used to resolve
/// relative image paths) into a flat `Document`.
pub fn parse(markdown: &str, base_dir: &Path) -> Document {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let events: Vec<(Event, Range<usize>)> = Parser::new_ext(markdown, options)
        .into_offset_iter()
        .collect();
    Builder::new(base_dir, markdown).run(&events)
}

#[derive(Debug, Clone, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

/// One active list level: whether it's ordered (and the next number to
/// print) plus its nesting depth (used for indentation).
struct ListLevel {
    next_ordinal: Option<u64>,
    depth: u32,
    /// Loose lists wrap item text in a Paragraph event after the Item event
    /// has already emitted its marker. The first paragraph must reuse that
    /// in-progress line instead of flushing the marker onto a row by itself.
    is_first_paragraph_pending: bool,
    /// `quote_depth` at the moment this item's marker line was started. A
    /// blockquote opening between the marker and the item's first paragraph
    /// (e.g. `- > quoted`) raises `quote_depth` after that line was already
    /// begun, so the first paragraph must add the missing `│ ` prefixes
    /// itself -- the reused-line path skips `start_line_with_prefix`
    /// entirely, which is the only other place that applies them.
    marker_quote_depth: u32,
}

/// Accumulated cell text for one in-progress table, before column widths
/// are known.
#[derive(Default)]
struct TableBuilder {
    alignments: Vec<Alignment>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
}

struct Builder<'a> {
    base_dir: &'a Path,
    source_lines: Vec<&'a str>,
    source_line_starts: Vec<usize>,
    last_source_line: Option<usize>,
    blocks: Vec<Block>,
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    bold: u32,
    italic: u32,
    strike: u32,
    code: u32,
    link: u32,
    quote_depth: u32,
    lists: Vec<ListLevel>,
    code_block: Option<(Option<String>, Vec<String>)>,
    table: Option<TableBuilder>,
    /// Set while inside a paragraph that has emitted nothing but a single
    /// image so far; `Some(false)` once anything else disqualifies it.
    paragraph_is_lone_image: Option<bool>,
    pending_image: Option<(String, String)>,
    /// True only between an `Image` tag's Start and End -- distinguishes
    /// "collecting this image's alt text" from `pending_image` merely being
    /// held while `TagEnd::Paragraph` decides if the image was alone. Text
    /// events after the tag has closed must disqualify a held image rather
    /// than being appended to its alt text.
    in_image: bool,
}

impl<'a> Builder<'a> {
    fn new(base_dir: &'a Path, markdown: &'a str) -> Self {
        let source_line_starts = std::iter::once(0)
            .chain(
                markdown
                    .match_indices('\n')
                    .map(|(byte_index, _)| byte_index + 1),
            )
            .collect();

        Self {
            base_dir,
            source_lines: markdown.split('\n').collect(),
            source_line_starts,
            last_source_line: None,
            blocks: Vec::new(),
            lines: Vec::new(),
            spans: Vec::new(),
            bold: 0,
            italic: 0,
            strike: 0,
            code: 0,
            link: 0,
            quote_depth: 0,
            lists: Vec::new(),
            code_block: None,
            table: None,
            paragraph_is_lone_image: None,
            pending_image: None,
            in_image: false,
        }
    }

    fn run(mut self, events: &[(Event, Range<usize>)]) -> Document {
        for (event, source_range) in events {
            self.preserve_blank_lines_before(source_range.start);
            self.handle(event);
            self.record_source_progress(event, source_range);
        }
        self.flush_line();
        self.flush_text_block();
        Document {
            blocks: self.blocks,
        }
    }

    /// Inserts only separator rows that physically exist in the source.
    /// Event offsets are essential here because CommonMark deliberately
    /// omits blank separators from its semantic event stream.
    fn preserve_blank_lines_before(&mut self, source_offset: usize) {
        let current_source_line = self.source_line_for_offset(source_offset);
        let first_unseen_line = self
            .last_source_line
            .map_or(0, |source_line| source_line.saturating_add(1));
        if first_unseen_line >= current_source_line {
            return;
        }

        let blank_lines: Vec<Line<'static>> = self.source_lines
            [first_unseen_line..current_source_line]
            .iter()
            .filter_map(|line| render_blank_source_line(line))
            .collect();
        if blank_lines.is_empty() {
            return;
        }

        self.flush_text_block();
        self.blocks.push(Block::BlankLines(Arc::new(blank_lines)));
    }

    /// Advances source-line ownership without letting closing events move
    /// backwards: pulldown reports an enclosing tag's full source range on
    /// both its Start and End events.
    fn record_source_progress(&mut self, event: &Event, source_range: &Range<usize>) {
        let occupied_source_offset = match event {
            Event::Start(_) => source_range.start,
            // Container End ranges overlap their children and can include a
            // following separator (notably loose-list Item ranges). Leaf
            // events already advanced through every rendered source row.
            Event::End(_) => return,
            _ => source_range.end.saturating_sub(1).max(source_range.start),
        };
        let occupied_source_line = self.source_line_for_offset(occupied_source_offset);
        self.last_source_line = Some(
            self.last_source_line
                .map_or(occupied_source_line, |previous_source_line| {
                    previous_source_line.max(occupied_source_line)
                }),
        );
    }

    /// Maps a byte offset from pulldown-cmark to its zero-based source row.
    fn source_line_for_offset(&self, source_offset: usize) -> usize {
        self.source_line_starts
            .partition_point(|line_start| *line_start <= source_offset)
            .saturating_sub(1)
    }

    fn current_style(&self) -> Style {
        let mut style = Style::new();
        if self.bold > 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strike > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.code > 0 {
            style = style.fg(CODE_FG).bg(CODE_BG);
        }
        if self.link > 0 {
            style = style.fg(LINK_FG).add_modifier(Modifier::UNDERLINED);
        }
        style
    }

    /// Pushes the current line (with its leading blockquote/list prefix
    /// applied only once, when the line was started) into `self.lines`
    /// and starts a fresh one.
    fn flush_line(&mut self) {
        if !self.spans.is_empty() {
            self.lines.push(Line::from(std::mem::take(&mut self.spans)));
        }
    }

    /// Moves accumulated lines into a `Block::Text`, if any were built.
    fn flush_text_block(&mut self) {
        self.flush_line();
        if !self.lines.is_empty() {
            self.blocks
                .push(Block::Text(Arc::new(std::mem::take(&mut self.lines))));
        }
    }

    /// Starts a new line already carrying its indentation/quote prefix, so
    /// list and blockquote nesting reads correctly even though each item's
    /// wrapped continuation (produced by `Paragraph`'s own wrap) won't
    /// re-apply it.
    fn start_line_with_prefix(&mut self, prefix: Option<Span<'static>>) {
        self.flush_line();
        for _ in 0..self.quote_depth {
            self.spans
                .push(Span::styled("│ ", Style::new().fg(QUOTE_FG)));
        }
        if let Some(prefix) = prefix {
            self.spans.push(prefix);
        }
    }

    /// Indentation for a line that continues the innermost active list
    /// item's content without repeating its marker -- a loose item's second
    /// paragraph, or a line following a hard break inside an item. Matches
    /// the two-space-per-depth indent `Tag::Item` bakes into its own marker
    /// line, stepped one level deeper so continuation text lines up under
    /// where the marker's own text began. `None` outside any list, so a
    /// plain top-level continuation line stays unindented.
    fn list_item_continuation_indent(&self) -> Option<Span<'static>> {
        let depth = self.lists.last()?.depth;
        Some(Span::raw("  ".repeat(depth as usize + 1)))
    }

    fn push_text(&mut self, text: &str) {
        if let Some((_, lines)) = self.code_block.as_mut() {
            let mut parts = text.split('\n');
            if let Some(first) = parts.next() {
                if let Some(last) = lines.last_mut() {
                    last.push_str(first);
                }
            }
            for part in parts {
                lines.push(part.to_string());
            }
            return;
        }
        // Alt text arrives as `Text` events nested inside the `Image` tag
        // itself -- accumulate it there regardless of context (a table
        // cell or the lone-image check below), both of which only concern
        // text *outside* the image. `in_image` (rather than merely
        // `pending_image.is_some()`) is essential here: after the tag
        // closes, `pending_image` may still be held tentatively awaiting
        // `TagEnd::Paragraph`, and text arriving in that window belongs to
        // the paragraph, not the alt text.
        if self.in_image {
            if let Some((alt, _)) = self.pending_image.as_mut() {
                alt.push_str(text);
            }
            return;
        }
        if let Some((alt, dest_url)) = self.pending_image.take() {
            if text.trim().is_empty() {
                // Whitespace after a still-tentative lone image: not real
                // content, keep waiting for the paragraph to close.
                self.pending_image = Some((alt, dest_url));
                return;
            }
            // Real text follows a closed image tag within the same
            // paragraph -- it's no longer a lone image. Resolve the held
            // image to its inline placeholder now rather than letting it be
            // silently dropped if no later event ever revisits it.
            self.paragraph_is_lone_image = Some(false);
            self.spans.push(Span::styled(
                format!("[image: {alt}]"),
                Style::new().fg(Color::DarkGray).italic(),
            ));
        }
        if let Some(table) = self.table.as_mut() {
            table.current_cell.push_str(text);
            return;
        }
        if self.paragraph_is_lone_image == Some(true) {
            if text.trim().is_empty() {
                // Whitespace surrounding a candidate lone image: not real
                // content, so drop it rather than pushing an empty span.
                return;
            }
            // Real text alongside an image disqualifies "lone image" --
            // fall through and push it as an ordinary span below.
            self.paragraph_is_lone_image = Some(false);
        }
        self.spans
            .push(Span::styled(text.to_string(), self.current_style()));
    }

    fn handle(&mut self, event: &Event) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag_end) => self.end_tag(*tag_end),
            Event::Text(text) => self.push_text(text),
            Event::Code(text) => {
                self.code += 1;
                self.push_text(text);
                self.code -= 1;
            }
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => {
                let continuation_indent = self.list_item_continuation_indent();
                self.start_line_with_prefix(continuation_indent);
            }
            Event::Rule => {
                self.flush_text_block();
                self.blocks.push(Block::Text(Arc::new(vec![Line::from(
                    "─".repeat(RULE_WIDTH),
                )])));
            }
            Event::TaskListMarker(checked) => {
                let glyph = if *checked { "[x] " } else { "[ ] " };
                self.spans.push(Span::raw(glyph));
            }
            Event::InlineHtml(_)
            | Event::Html(_)
            | Event::FootnoteReference(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {}
        }
    }

    fn start_tag(&mut self, tag: &Tag) {
        match tag {
            Tag::Paragraph => {
                let marker_quote_depth = self.lists.last().map(|level| level.marker_quote_depth);
                let is_first_list_item_paragraph = self
                    .lists
                    .last_mut()
                    .is_some_and(|level| std::mem::take(&mut level.is_first_paragraph_pending));
                if is_first_list_item_paragraph {
                    // A blockquote (or other container) may have opened
                    // between the item marker and this paragraph, deepening
                    // `quote_depth` after the marker line already started --
                    // add exactly the prefixes that line is still missing.
                    let marker_quote_depth = marker_quote_depth.unwrap_or(0);
                    for _ in marker_quote_depth..self.quote_depth {
                        self.spans
                            .push(Span::styled("│ ", Style::new().fg(QUOTE_FG)));
                    }
                } else {
                    self.start_line_with_prefix(self.list_item_continuation_indent());
                }
                self.paragraph_is_lone_image = Some(true);
            }
            Tag::Heading { .. } => {
                self.flush_text_block();
                self.spans.clear();
            }
            Tag::BlockQuote(_) => {
                self.quote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush_text_block();
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.to_string()),
                    _ => None,
                };
                self.code_block = Some((lang, vec![String::new()]));
            }
            Tag::List(start) => {
                let depth = self.lists.len() as u32;
                self.lists.push(ListLevel {
                    next_ordinal: *start,
                    depth,
                    is_first_paragraph_pending: false,
                    marker_quote_depth: 0,
                });
            }
            Tag::Item => {
                let indent = self
                    .lists
                    .last()
                    .map(|level| "  ".repeat(level.depth as usize))
                    .unwrap_or_default();
                if let Some(level) = self.lists.last_mut() {
                    level.is_first_paragraph_pending = true;
                    level.marker_quote_depth = self.quote_depth;
                }
                let bullet = match self.lists.last_mut() {
                    Some(ListLevel {
                        next_ordinal: Some(ordinal),
                        ..
                    }) => {
                        let text = format!("{indent}{ordinal}. ");
                        *ordinal += 1;
                        text
                    }
                    _ => format!("{indent}• "),
                };
                self.start_line_with_prefix(Some(Span::styled(
                    bullet,
                    Style::new().fg(Color::DarkGray),
                )));
            }
            Tag::Table(alignments) => {
                self.flush_text_block();
                self.table = Some(TableBuilder {
                    alignments: alignments.clone(),
                    ..Default::default()
                });
            }
            Tag::TableHead | Tag::TableRow => {}
            Tag::TableCell => {}
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { .. } => self.link += 1,
            Tag::Image { dest_url, .. } => {
                if let Some((previous_alt, _)) = self.pending_image.take() {
                    // A second image already showed up before this
                    // paragraph closed -- it no longer contains a single
                    // lone image, and the first image's alt text must not
                    // be silently discarded by the overwrite below.
                    self.paragraph_is_lone_image = Some(false);
                    self.spans.push(Span::styled(
                        format!("[image: {previous_alt}]"),
                        Style::new().fg(Color::DarkGray).italic(),
                    ));
                }
                self.pending_image = Some((String::new(), dest_url.to_string()));
                self.in_image = true;
            }
            Tag::Superscript | Tag::Subscript => {}
            Tag::FootnoteDefinition(_)
            | Tag::HtmlBlock
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Paragraph => {
                if self.paragraph_is_lone_image == Some(true) {
                    if let Some((alt, dest_url)) = self.pending_image.take() {
                        self.spans.clear();
                        self.blocks.push(Block::Image {
                            alt,
                            path: resolve_image_path(self.base_dir, &dest_url),
                        });
                        self.paragraph_is_lone_image = None;
                        return;
                    }
                }
                self.paragraph_is_lone_image = None;
                self.flush_line();
            }
            TagEnd::Heading(level) => {
                let text: String = self
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect();
                self.spans.clear();
                self.blocks.push(Block::Heading {
                    text,
                    level: heading_level_number(level),
                });
            }
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.flush_line();
            }
            TagEnd::CodeBlock => {
                if let Some((_, mut lines)) = self.code_block.take() {
                    if lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
                        lines.pop();
                    }
                    self.flush_text_block();
                    let code_lines = lines
                        .into_iter()
                        .map(|line| {
                            Line::from(Span::styled(
                                format!(" {line}"),
                                Style::new().fg(CODE_FG).bg(CODE_BG),
                            ))
                        })
                        .collect();
                    self.blocks.push(Block::Text(Arc::new(code_lines)));
                }
            }
            TagEnd::List(_) => {
                self.lists.pop();
                self.flush_line();
            }
            TagEnd::Item => {
                if let Some(level) = self.lists.last_mut() {
                    level.is_first_paragraph_pending = false;
                }
                self.flush_line();
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.blocks
                        .push(Block::Text(Arc::new(render_table(&table))));
                }
            }
            // The header row's cells are wrapped directly in `TableHead`
            // with no `TableRow` around them (only body rows get one), so
            // it's flushed to `header` here rather than in `TableRow` below.
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.header = std::mem::take(&mut table.current_row);
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.rows.push(std::mem::take(&mut table.current_row));
                }
            }
            TagEnd::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    let cell = std::mem::take(&mut table.current_cell);
                    table.current_row.push(cell);
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => self.link = self.link.saturating_sub(1),
            TagEnd::Image => {
                self.in_image = false;
                if let Some((alt, dest_url)) = self.pending_image.take() {
                    if self.paragraph_is_lone_image == Some(true) {
                        // Re-stash: `TagEnd::Paragraph` (just after this)
                        // performs the actual image-block substitution.
                        self.pending_image = Some((alt, dest_url));
                    } else {
                        // An inline image mixed into running text: shown as
                        // a placeholder span, not rasterized. A table cell's
                        // text lives in `table.current_cell`, not
                        // `self.spans`, so route it there when applicable.
                        let placeholder = format!("[image: {alt}]");
                        if let Some(table) = self.table.as_mut() {
                            table.current_cell.push_str(&placeholder);
                        } else {
                            self.spans.push(Span::styled(
                                placeholder,
                                Style::new().fg(Color::DarkGray).italic(),
                            ));
                        }
                    }
                }
            }
            TagEnd::Superscript | TagEnd::Subscript => {}
            TagEnd::FootnoteDefinition
            | TagEnd::HtmlBlock
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::MetadataBlock(_) => {}
        }
    }
}

/// A blockquote marker-only row is visually empty in rendered Markdown even
/// though its source contains `>` characters, so it deserves the same spacing
/// preservation as an actually empty or whitespace-only row.
fn render_blank_source_line(source_line: &str) -> Option<Line<'static>> {
    let mut remaining = source_line.trim();
    if remaining.is_empty() {
        return Some(Line::default());
    }

    let mut quote_depth = 0u16;
    while let Some(after_quote_marker) = remaining.strip_prefix('>') {
        quote_depth = quote_depth.saturating_add(1);
        remaining = after_quote_marker.trim_start();
        if remaining.is_empty() {
            let quote_prefixes = (0..quote_depth)
                .map(|_| Span::styled("│ ", Style::new().fg(QUOTE_FG)))
                .collect::<Vec<_>>();
            return Some(Line::from(quote_prefixes));
        }
    }

    None
}

fn heading_level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Local files only -- see `ImagePath::Unsupported`.
fn resolve_image_path(base_dir: &Path, dest_url: &str) -> ImagePath {
    if dest_url.contains("://") {
        return ImagePath::Unsupported(dest_url.to_string());
    }
    ImagePath::Local(base_dir.join(dest_url))
}

fn render_table(table: &TableBuilder) -> Vec<Line<'static>> {
    let column_count = table
        .header
        .len()
        .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
    let mut widths = vec![0usize; column_count];
    for row in std::iter::once(&table.header).chain(table.rows.iter()) {
        for (index, cell) in row.iter().enumerate() {
            // `format!("{:>width$}")` below pads by `char` count, not byte
            // count -- using byte length here would over-count any
            // multi-byte UTF-8 cell and misalign the whole column.
            widths[index] = widths[index].max(cell.trim().chars().count()).min(40);
        }
    }

    let mut lines = Vec::new();
    lines.push(render_table_row(
        &table.header,
        &widths,
        &table.alignments,
        true,
    ));
    let separator: String = widths
        .iter()
        .map(|width| "─".repeat(width + 2))
        .collect::<Vec<_>>()
        .join("┼");
    lines.push(Line::from(Span::styled(
        separator,
        Style::new().fg(Color::DarkGray),
    )));
    for row in &table.rows {
        lines.push(render_table_row(row, &widths, &table.alignments, false));
    }
    lines
}

/// Truncates `text` to at most `max_chars` Unicode scalar values. Operates
/// on `chars()` rather than byte slicing so a cut never lands inside a
/// multi-byte UTF-8 sequence.
fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

fn render_table_row(
    row: &[String],
    widths: &[usize],
    alignments: &[Alignment],
    is_header: bool,
) -> Line<'static> {
    let style = if is_header {
        Style::new().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    let mut spans = Vec::new();
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("│", Style::new().fg(Color::DarkGray)));
        }
        let text = row.get(index).map(|cell| cell.trim()).unwrap_or_default();
        // `widths` is capped at 40 (above) but that cap only bounds the
        // padding target -- without truncating here too, any cell longer
        // than the cap still renders at full length and breaks alignment
        // with every other row in its column.
        let text = truncate_chars(text, *width);
        let alignment = alignments.get(index).copied().unwrap_or(Alignment::None);
        let padded = match alignment {
            Alignment::Right => format!(" {text:>width$} "),
            Alignment::Center => format!(" {text:^width$} "),
            Alignment::Left | Alignment::None => format!(" {text:<width$} "),
        };
        spans.push(Span::styled(padded, style));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_becomes_its_own_block() {
        let doc = parse("# Title\n\nBody text.", Path::new("/tmp"));
        assert!(matches!(
            &doc.blocks[0],
            Block::Heading { text, level: 1 } if text == "Title"
        ));
        assert!(matches!(&doc.blocks[1], Block::BlankLines(lines) if lines.len() == 1));
        assert!(matches!(&doc.blocks[2], Block::Text(_)));
    }

    #[test]
    fn lone_image_paragraph_becomes_image_block() {
        let doc = parse("![alt text](pic.png)", Path::new("/tmp"));
        assert_eq!(doc.blocks.len(), 1);
        match &doc.blocks[0] {
            Block::Image { alt, path } => {
                assert_eq!(alt, "alt text");
                assert!(matches!(path, ImagePath::Local(p) if p.ends_with("pic.png")));
            }
            other => panic!("expected Block::Image, got {other:?}"),
        }
    }

    #[test]
    fn inline_image_stays_in_text() {
        let doc = parse("before ![x](pic.png) after", Path::new("/tmp"));
        assert_eq!(doc.blocks.len(), 1);
        assert!(matches!(&doc.blocks[0], Block::Text(_)));
    }

    #[test]
    fn two_adjacent_images_in_one_paragraph_keep_both_alt_texts() {
        // Neither image is "alone" in its paragraph, so both must fall back
        // to inline placeholders instead of one silently overwriting the
        // other's pending alt text.
        let doc = parse("![a](x.png)![b](y.png)", Path::new("/tmp"));
        assert_eq!(doc.blocks.len(), 1);
        let Block::Text(lines) = &doc.blocks[0] else {
            panic!("expected a text block, got {:?}", doc.blocks[0]);
        };
        let text = line_text(&lines[0]);
        assert!(text.contains("[image: a]"), "missing first alt in {text}");
        assert!(text.contains("[image: b]"), "missing second alt in {text}");
    }

    #[test]
    fn image_with_trailing_text_in_the_same_paragraph_stays_inline() {
        // The image is no longer alone once "after" follows it on the same
        // line -- the trailing text must not be silently merged into the
        // image's alt text and must not vanish into a discarded Block::Image.
        let doc = parse("![x](pic.png) after", Path::new("/tmp"));
        assert_eq!(doc.blocks.len(), 1);
        let Block::Text(lines) = &doc.blocks[0] else {
            panic!("expected a text block, got {:?}", doc.blocks[0]);
        };
        let text = line_text(&lines[0]);
        assert!(
            text.contains("[image: x]"),
            "missing alt placeholder in {text}"
        );
        assert!(text.contains("after"), "trailing text was dropped: {text}");
    }

    #[test]
    fn image_inside_table_cell_does_not_become_a_block_image() {
        // Table cells never wrap their inline content in a Paragraph tag,
        // so an image here must render as an inline placeholder inside the
        // cell, not as a standalone `Block::Image`.
        let doc = parse(
            "| ![a](x.png) | b |\n|---|---|\n| 1 | 2 |\n",
            Path::new("/tmp"),
        );
        assert_eq!(doc.blocks.len(), 1);
        let Block::Text(lines) = &doc.blocks[0] else {
            panic!("expected a table text block, got {:?}", doc.blocks[0]);
        };
        assert!(
            line_text(&lines[0]).contains("[image: a]"),
            "expected the placeholder inside the header row, got {}",
            line_text(&lines[0])
        );
    }

    #[test]
    fn bold_text_gets_bold_modifier() {
        let doc = parse("**bold**", Path::new("/tmp"));
        let Block::Text(lines) = &doc.blocks[0] else {
            panic!("expected text block");
        };
        assert!(lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn remote_image_is_unsupported_not_fetched() {
        let doc = parse("![x](https://example.com/pic.png)", Path::new("/tmp"));
        match &doc.blocks[0] {
            Block::Image { path, .. } => {
                assert!(matches!(path, ImagePath::Unsupported(_)));
            }
            other => panic!("expected Block::Image, got {other:?}"),
        }
    }

    #[test]
    fn table_renders_aligned_rows() {
        let doc = parse("| a | b |\n|---|---|\n| 1 | 2 |\n", Path::new("/tmp"));
        assert_eq!(doc.blocks.len(), 1);
        let Block::Text(lines) = &doc.blocks[0] else {
            panic!("expected text block");
        };
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn table_cell_longer_than_the_width_cap_stays_aligned() {
        let long_cell = "x".repeat(80);
        let markdown = format!("| a | b |\n|---|---|\n| {long_cell} | short |\n");
        let doc = parse(&markdown, Path::new("/tmp"));
        let Block::Text(lines) = &doc.blocks[0] else {
            panic!("expected text block");
        };
        // Every rendered row must end up the same width -- if the long
        // cell weren't truncated to the capped column width, its row
        // would be far wider than the header/separator rows.
        let row_widths: Vec<usize> = lines
            .iter()
            .map(|line| line_text(line).chars().count())
            .collect();
        assert_eq!(
            row_widths[0], row_widths[2],
            "long-cell row width {:?} does not match header row width {:?}",
            row_widths[2], row_widths[0]
        );
    }

    #[test]
    fn unicode_table_cell_aligns_with_ascii_cell() {
        let doc = parse("| a | b |\n|---|---|\n| 日本語 | x |\n", Path::new("/tmp"));
        let Block::Text(lines) = &doc.blocks[0] else {
            panic!("expected text block");
        };
        let row_widths: Vec<usize> = lines
            .iter()
            .map(|line| line_text(line).chars().count())
            .collect();
        assert_eq!(
            row_widths[0], row_widths[2],
            "unicode-cell row width {:?} does not match header row width {:?}",
            row_widths[2], row_widths[0]
        );
    }

    #[test]
    fn preserves_the_exact_number_of_blank_lines_between_paragraphs() {
        let document = parse("first\n\n\n\nsecond", Path::new("/tmp"));

        assert_eq!(document.blocks.len(), 3);
        assert!(matches!(document.blocks[0], Block::Text(_)));
        assert!(matches!(&document.blocks[1], Block::BlankLines(lines) if lines.len() == 3));
        assert!(matches!(document.blocks[2], Block::Text(_)));
    }

    #[test]
    fn does_not_invent_spacing_for_a_soft_line_break() {
        let document = parse("first\nsecond", Path::new("/tmp"));

        assert_eq!(document.blocks.len(), 1);
        let Block::Text(lines) = &document.blocks[0] else {
            panic!("expected one text block");
        };
        assert_eq!(line_text(&lines[0]), "first second");
    }

    #[test]
    fn loose_list_keeps_each_marker_with_its_text_and_preserves_the_gap() {
        let document = parse("- first\n\n- second", Path::new("/tmp"));

        assert_eq!(document.blocks.len(), 3);
        let Block::Text(first_item) = &document.blocks[0] else {
            panic!("expected first list item");
        };
        let Block::Text(second_item) = &document.blocks[2] else {
            panic!("expected second list item");
        };
        assert_eq!(line_text(&first_item[0]), "• first");
        assert!(matches!(&document.blocks[1], Block::BlankLines(lines) if lines.len() == 1));
        assert_eq!(line_text(&second_item[0]), "• second");
    }

    #[test]
    fn tight_list_remains_compact_without_synthetic_spacing() {
        let document = parse("- first\n- second", Path::new("/tmp"));

        assert_eq!(document.blocks.len(), 1);
        let Block::Text(items) = &document.blocks[0] else {
            panic!("expected one tight-list text block");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(line_text(&items[0]), "• first");
        assert_eq!(line_text(&items[1]), "• second");
    }

    #[test]
    fn preserves_spacing_on_both_sides_of_a_list() {
        let document = parse("before\n\n- first\n- second\n\nafter", Path::new("/tmp"));

        assert_eq!(document.blocks.len(), 5);
        assert!(matches!(&document.blocks[1], Block::BlankLines(lines) if lines.len() == 1));
        let Block::Text(items) = &document.blocks[2] else {
            panic!("expected list between paragraph spacers");
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(&document.blocks[3], Block::BlankLines(lines) if lines.len() == 1));
    }

    #[test]
    fn fenced_code_keeps_internal_blank_rows_without_a_trailing_sentinel() {
        let document = parse("```\nfirst\n\nsecond\n```\n\nafter", Path::new("/tmp"));

        assert_eq!(document.blocks.len(), 3);
        let Block::Text(code_lines) = &document.blocks[0] else {
            panic!("expected code block");
        };
        assert_eq!(code_lines.len(), 3);
        assert_eq!(line_text(&code_lines[0]), " first");
        assert_eq!(line_text(&code_lines[1]), " ");
        assert_eq!(line_text(&code_lines[2]), " second");
        assert!(matches!(&document.blocks[1], Block::BlankLines(lines) if lines.len() == 1));
    }

    #[test]
    fn blockquote_marker_only_row_is_preserved_as_spacing() {
        let document = parse("> first\n>\n> second", Path::new("/tmp"));

        let quote_spacing = document.blocks.iter().find_map(|block| match block {
            Block::BlankLines(lines) => Some(lines),
            _ => None,
        });
        let quote_spacing = quote_spacing.expect("expected a quoted blank row");
        assert_eq!(quote_spacing.len(), 1);
        assert_eq!(line_text(&quote_spacing[0]), "│ ");
    }

    #[test]
    fn hard_break_stays_two_adjacent_content_rows() {
        let document = parse("first  \nsecond", Path::new("/tmp"));

        assert_eq!(document.blocks.len(), 1);
        let Block::Text(lines) = &document.blocks[0] else {
            panic!("expected one hard-break text block");
        };
        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "first");
        assert_eq!(line_text(&lines[1]), "second");
    }

    #[test]
    fn blockquote_opening_inside_a_list_item_keeps_its_quote_prefix() {
        // The item marker's line starts before `quote_depth` is incremented
        // by the nested blockquote, so the first paragraph (nested two
        // levels deep, inside the blockquote rather than directly inside
        // the item) must still add the prefix that `start_line_with_prefix`
        // never gets to apply for a reused marker line.
        let document = parse("- > quoted", Path::new("/tmp"));
        let Block::Text(lines) = &document.blocks[0] else {
            panic!("expected one text block, got {:?}", document.blocks);
        };
        assert_eq!(line_text(&lines[0]), "• │ quoted");
    }

    #[test]
    fn loose_list_items_second_paragraph_stays_indented_under_the_marker() {
        // Only the marker line bakes in its own indent; a second paragraph
        // in the same item has no marker to reuse, so it must independently
        // pick up the same depth-based indent or it reads as if it had
        // fallen back out of the list entirely.
        let document = parse("- first\n\n  second para", Path::new("/tmp"));

        let Block::Text(first_item) = &document.blocks[0] else {
            panic!(
                "expected first item's paragraph, got {:?}",
                document.blocks[0]
            );
        };
        assert_eq!(line_text(&first_item[0]), "• first");
        let Block::Text(second_paragraph) = &document.blocks[2] else {
            panic!(
                "expected the item's second paragraph, got {:?}",
                document.blocks[2]
            );
        };
        assert_eq!(line_text(&second_paragraph[0]), "  second para");
    }

    #[test]
    fn hard_break_inside_a_list_item_keeps_the_marker_indent() {
        let document = parse("- first  \nsecond", Path::new("/tmp"));

        let Block::Text(lines) = &document.blocks[0] else {
            panic!("expected one text block, got {:?}", document.blocks);
        };
        assert_eq!(line_text(&lines[0]), "• first");
        assert_eq!(line_text(&lines[1]), "  second");
    }

    /// Concatenates styled spans without losing the visible text contract
    /// that list, code, and paragraph tests need to assert.
    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }
}
