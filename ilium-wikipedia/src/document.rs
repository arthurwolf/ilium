use std::collections::{HashMap, HashSet};

use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node, Selector};

pub(crate) const MAX_HTML_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLOCKS: usize = 20_000;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMAGES: usize = 256;
const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub link: bool,
    pub superscript: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageFloat {
    Left,
    Right,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub url: String,
    pub caption: Vec<Span>,
    pub width: u32,
    pub height: u32,
    pub float: ImageFloat,
}

/// Geometry of an original table cell anchored in the normalized row grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSpan {
    pub row: usize,
    pub column: usize,
    pub row_span: usize,
    pub column_span: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableImage {
    pub row: usize,
    pub column: usize,
    pub image: ImageRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Span>>,
    },
    Table {
        rows: Vec<Vec<Vec<Span>>>,
        spans: Vec<TableSpan>,
        images: Vec<TableImage>,
        infobox: bool,
    },
    Image(ImageRef),
}

#[derive(Debug, Clone)]
pub struct Document {
    pub title: String,
    pub url: String,
    /// Revision from the original response, when the HTML exposes it.
    pub revision: Option<String>,
    /// UTC Main Page selection date; stale cache retains its original date.
    pub date: String,
    pub blocks: Vec<Block>,
    pub images: HashMap<String, image::RgbaImage>,
    /// Persistent offline/decode diagnostics; transient event backpressure cannot lose them.
    pub warnings: Vec<String>,
}

fn selector(css: &str) -> Result<Selector, String> {
    Selector::parse(css).map_err(|error| format!("invalid internal selector: {error}"))
}

fn ignored(element: ElementRef<'_>) -> bool {
    matches!(
        element.value().name(),
        "script" | "style" | "noscript" | "nav" | "link" | "meta"
    ) || element.value().attr("aria-hidden") == Some("true")
        || element
            .value()
            .classes()
            .any(|class| matches!(class, "mw-editsection" | "noprint" | "mw-empty-elt"))
}

fn compact_whitespace(text: &str) -> String {
    let mut previous_space = false;
    text.chars()
        .filter_map(|character| {
            if character.is_whitespace() {
                if previous_space {
                    return None;
                }
                previous_space = true;
                Some(' ')
            } else {
                previous_space = false;
                Some(character)
            }
        })
        .collect()
}

fn collect_inline(
    node: NodeRef<'_, Node>,
    style: &Span,
    output: &mut Vec<Span>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("Wikipedia HTML nesting exceeds safety limit".into());
    }
    if let Node::Text(text) = node.value() {
        let mut text = compact_whitespace(text);
        if output.last().is_some_and(|span| span.text.ends_with(' ')) {
            text = text.trim_start_matches(' ').to_owned();
        }
        if !text.is_empty() {
            let mut span = style.clone();
            span.text = text;
            if let Some(previous) = output.last_mut().filter(|previous| {
                previous.bold == span.bold
                    && previous.italic == span.italic
                    && previous.link == span.link
                    && previous.superscript == span.superscript
            }) {
                previous.text.push_str(&span.text);
            } else {
                output.push(span);
            }
        }
        return Ok(());
    }
    let mut style = style.clone();
    if let Some(element) = ElementRef::wrap(node) {
        if ignored(element) || element.value().name() == "img" {
            return Ok(());
        }
        let name = element.value().name();
        if matches!(name, "td" | "th")
            && output.last().is_some_and(|span| !span.text.ends_with('\n'))
        {
            output.push(Span {
                text: " ".into(),
                ..style.clone()
            });
        }
        if name == "tr" && !output.is_empty() {
            output.push(Span {
                text: "\n".into(),
                ..style.clone()
            });
        }
        match name {
            "b" | "strong" | "th" | "dt" => style.bold = true,
            "i" | "em" => style.italic = true,
            "a" => style.link = element.value().attr("href").is_some(),
            "sup" => style.superscript = true,
            "br" => {
                output.push(Span {
                    text: "\n".into(),
                    ..style
                });
                return Ok(());
            }
            "p" | "div" | "li" | "dd" if !output.is_empty() => {
                output.push(Span {
                    text: "\n".into(),
                    ..style.clone()
                });
            }
            _ => {}
        }
    }
    for child in node.children() {
        collect_inline(child, &style, output, depth + 1)?;
    }
    Ok(())
}

fn inline(element: ElementRef<'_>) -> Result<Vec<Span>, String> {
    let mut spans = Vec::new();
    collect_inline(*element, &Span::default(), &mut spans, 0)?;
    if let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    if let Some(last) = spans.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    spans.retain(|span| !span.text.is_empty());
    Ok(spans)
}

struct Parser {
    blocks: Vec<Block>,
    text_bytes: usize,
    image_nodes: HashSet<ego_tree::NodeId>,
    table_cells: usize,
    table_cell_limit: Option<usize>,
}

impl Parser {
    fn push(&mut self, block: Block) -> Result<(), String> {
        let text_bytes: usize = match &block {
            Block::Paragraph(spans) | Block::Heading { spans, .. } => {
                spans.iter().map(|s| s.text.len()).sum()
            }
            Block::List { items, .. } => items.iter().flatten().map(|s| s.text.len()).sum(),
            Block::Table { rows, images, .. } => {
                rows.iter()
                    .flatten()
                    .flatten()
                    .map(|s| s.text.len())
                    .sum::<usize>()
                    + images
                        .iter()
                        .flat_map(|image| &image.image.caption)
                        .map(|s| s.text.len())
                        .sum::<usize>()
            }
            Block::Image(image) => image.caption.iter().map(|s| s.text.len()).sum(),
        };
        self.text_bytes += text_bytes;
        if self.text_bytes > MAX_TEXT_BYTES || self.blocks.len() >= MAX_BLOCKS {
            return Err(
                "Wikipedia document exceeds block/text safety limits; article was not truncated"
                    .into(),
            );
        }
        self.blocks.push(block);
        Ok(())
    }

    fn images(
        &mut self,
        container: ElementRef<'_>,
        default_float: ImageFloat,
    ) -> Result<Vec<ImageRef>, String> {
        let mut references = Vec::new();
        let image_selector = selector("img")?;
        let candidates: Vec<_> = if container.value().name() == "img" {
            vec![container]
        } else {
            container.select(&image_selector).collect()
        };
        for image in candidates {
            let Some(source) = image.value().attr("src") else {
                continue;
            };
            let source = if source.starts_with("//") {
                format!("https:{source}")
            } else {
                source.to_owned()
            };
            if !crate::fetch::valid_wikimedia_url(&source) || !self.image_nodes.insert(image.id()) {
                continue;
            }
            if self.image_nodes.len() > MAX_IMAGES {
                return Err(
                    "Wikipedia article exceeds image safety limit; article was not truncated"
                        .into(),
                );
            }
            let figure = image
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|ancestor| {
                    ancestor.value().name() == "figure"
                        || ancestor.value().classes().any(|class| class == "thumb")
                });
            let caption = if let Some(figure) = figure {
                figure
                    .select(&selector("figcaption, .thumbcaption")?)
                    .next()
                    .map(inline)
                    .transpose()?
                    .unwrap_or_default()
            } else {
                image
                    .value()
                    .attr("alt")
                    .filter(|alt| !alt.is_empty())
                    .map(|alt| {
                        vec![Span {
                            text: alt.to_owned(),
                            ..Span::default()
                        }]
                    })
                    .unwrap_or_default()
            };
            let float = figure.map_or(default_float, |figure| {
                if figure
                    .value()
                    .classes()
                    .any(|class| matches!(class, "mw-halign-left" | "tleft"))
                {
                    ImageFloat::Left
                } else if figure
                    .value()
                    .classes()
                    .any(|class| matches!(class, "mw-halign-right" | "tright"))
                {
                    ImageFloat::Right
                } else if figure
                    .value()
                    .classes()
                    .any(|class| matches!(class, "mw-halign-center" | "mw-halign-none"))
                {
                    ImageFloat::None
                } else {
                    default_float
                }
            });
            references.push(ImageRef {
                url: source,
                caption,
                width: image
                    .value()
                    .attr("width")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(220),
                height: image
                    .value()
                    .attr("height")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(150),
                float,
            });
        }
        Ok(references)
    }

    fn push_images(&mut self, container: ElementRef<'_>, float: ImageFloat) -> Result<(), String> {
        for image in self.images(container, float)? {
            self.push(Block::Image(image))?;
        }
        Ok(())
    }

    fn walk(&mut self, element: ElementRef<'_>, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("Wikipedia HTML nesting exceeds safety limit".into());
        }
        if ignored(element) {
            return Ok(());
        }
        let name = element.value().name();
        match name {
            "p" | "pre" => {
                let spans = inline(element)?;
                if !spans.is_empty() {
                    self.push(Block::Paragraph(spans))?;
                }
                self.push_images(element, ImageFloat::None)?;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.push(Block::Heading {
                    level: name.as_bytes()[1] - b'0',
                    spans: inline(element)?,
                })?;
            }
            "ul" | "ol" | "dl" => {
                let items = element
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|child| matches!(child.value().name(), "li" | "dt" | "dd"))
                    .map(inline)
                    .collect::<Result<Vec<_>, _>>()?;
                if !items.is_empty() {
                    self.push(Block::List {
                        ordered: name == "ol",
                        items,
                    })?;
                }
                self.push_images(element, ImageFloat::None)?;
            }
            "table" => {
                let infobox = element.value().classes().any(|class| class == "infobox");
                let mut rows: Vec<Vec<Vec<Span>>> = Vec::new();
                let mut spans = Vec::new();
                let mut images = Vec::new();
                let mut covered = HashSet::new();
                let table_rows: Vec<_> = element
                    .select(&selector("tr")?)
                    .filter(|row| {
                        row.ancestors()
                            .filter_map(ElementRef::wrap)
                            .find(|ancestor| ancestor.value().name() == "table")
                            .map(|ancestor| ancestor.id())
                            == Some(element.id())
                    })
                    .collect();
                for (source_row_index, row) in table_rows.iter().copied().enumerate() {
                    if row
                        .ancestors()
                        .filter_map(ElementRef::wrap)
                        .find(|e| e.value().name() == "table")
                        .map(|e| e.id())
                        != Some(element.id())
                    {
                        continue;
                    }
                    let row_index = rows.len();
                    let mut cells = Vec::new();
                    let mut column = 0;
                    for cell in row
                        .children()
                        .filter_map(ElementRef::wrap)
                        .filter(|e| matches!(e.value().name(), "td" | "th"))
                    {
                        while covered.contains(&(row_index, column)) {
                            cells.push(Vec::new());
                            column += 1;
                        }
                        let remaining_group_rows = table_rows[source_row_index..]
                            .iter()
                            .take_while(|next| {
                                next.parent().map(|node| node.id())
                                    == row.parent().map(|node| node.id())
                            })
                            .count();
                        let requested_row_span = cell
                            .value()
                            .attr("rowspan")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1);
                        let row_span = if requested_row_span == 0 {
                            remaining_group_rows
                        } else {
                            requested_row_span.min(remaining_group_rows)
                        }
                        .max(1);
                        let column_span = cell
                            .value()
                            .attr("colspan")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                            .max(1);
                        if row_span.saturating_mul(column_span) + covered.len() > 100_000
                            || row_span > 1000
                            || column_span > 1000
                            || column + column_span > 1000
                            || row_index + row_span > MAX_BLOCKS
                        {
                            return Err("Wikipedia table exceeds geometry safety limit".into());
                        }
                        spans.push(TableSpan {
                            row: row_index,
                            column,
                            row_span,
                            column_span,
                        });
                        cells.push(inline(cell)?);
                        for image in self.images(
                            cell,
                            if infobox {
                                ImageFloat::Right
                            } else {
                                ImageFloat::None
                            },
                        )? {
                            images.push(TableImage {
                                row: row_index,
                                column,
                                image,
                            });
                        }
                        for delta_row in 0..row_span {
                            for delta_column in 0..column_span {
                                if delta_row != 0 || delta_column != 0 {
                                    covered.insert((row_index + delta_row, column + delta_column));
                                }
                            }
                        }
                        cells.resize_with(column + column_span, Vec::new);
                        column += column_span;
                    }
                    rows.push(cells);
                }
                let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                if rows.len().saturating_mul(width) > 100_000 || covered.len() > 100_000 {
                    return Err("Wikipedia table exceeds cell safety limit".into());
                }
                let normalized_cells = rows
                    .len()
                    .checked_mul(width)
                    .ok_or("Wikipedia normalized table size overflow")?;
                self.table_cells = self
                    .table_cells
                    .checked_add(normalized_cells)
                    .ok_or("Wikipedia document table size overflow")?;
                if self
                    .table_cell_limit
                    .is_some_and(|limit| self.table_cells > limit)
                {
                    return Err("Wikipedia document exceeds normalized table cell limit".into());
                }
                for row in &mut rows {
                    row.resize_with(width, Vec::new);
                }
                if let Some(caption) = element.select(&selector("caption")?).next() {
                    self.push(Block::Paragraph(inline(caption)?))?;
                }
                if !rows.is_empty() {
                    self.push(Block::Table {
                        rows,
                        spans,
                        images,
                        infobox,
                    })?;
                }
            }
            "figure" => self.push_images(element, ImageFloat::Right)?,
            "div" if element.value().classes().any(|class| class == "thumb") => {
                self.push_images(element, ImageFloat::Right)?
            }
            "img" => {
                self.push_images(element, ImageFloat::None)?;
            }
            _ => {
                let mut loose: Vec<Span> = Vec::new();
                for child in element.children() {
                    if let Some(child_element) = ElementRef::wrap(child) {
                        if matches!(
                            child_element.value().name(),
                            "p" | "pre"
                                | "h1"
                                | "h2"
                                | "h3"
                                | "h4"
                                | "h5"
                                | "h6"
                                | "ul"
                                | "ol"
                                | "dl"
                                | "table"
                                | "figure"
                                | "div"
                                | "section"
                                | "blockquote"
                                | "img"
                        ) {
                            if loose.iter().any(|span| !span.text.trim().is_empty()) {
                                self.push(Block::Paragraph(std::mem::take(&mut loose)))?;
                            } else {
                                loose.clear();
                            }
                            self.walk(child_element, depth + 1)?;
                        } else {
                            collect_inline(child, &Span::default(), &mut loose, depth + 1)?;
                            let images = self.images(child_element, ImageFloat::None)?;
                            if !images.is_empty() {
                                if loose.iter().any(|span| !span.text.trim().is_empty()) {
                                    self.push(Block::Paragraph(std::mem::take(&mut loose)))?;
                                } else {
                                    loose.clear();
                                }
                                for image in images {
                                    self.push(Block::Image(image))?;
                                }
                            }
                        }
                    } else {
                        collect_inline(child, &Span::default(), &mut loose, depth + 1)?;
                    }
                }
                if loose.iter().any(|span| !span.text.trim().is_empty()) {
                    self.push(Block::Paragraph(loose))?;
                }
            }
        }
        Ok(())
    }
}

pub fn parse_article(title: &str, url: &str, date: &str, html: &str) -> Result<Document, String> {
    if html.len() > MAX_HTML_BYTES {
        return Err("Wikipedia HTML exceeds 16 MiB; article was not truncated".into());
    }
    let html = Html::parse_document(html);
    article_from_dom(title, url, date, &html, None)
}

/// Native source entrypoint. Caller reserves ARTICLE_PARSE_PEAK_BYTES before
/// entry and retains source/result admissions separately. The installed
/// tokenizer's fixed atom-cache baseline is admitted for process lifetime.
pub fn parse_article_bounded(
    title: &str,
    url: &str,
    date: &str,
    html: &str,
    limits: crate::ArticleLimits,
) -> Result<Document, String> {
    if title.len() > 512 || url.len() > 4096 || date.len() > 64 {
        return Err("Wikipedia article metadata exceeds source limits".into());
    }
    let dom = crate::bounded_html::parse(html, limits)?;
    preflight_caption_copies(&dom)?;
    article_from_dom(title, url, date, &dom, Some(limits.table_cells))
}
// A figure caption may be copied for each of its image references. Bound
// this amplification before images() constructs any caption Vec/String.
// Count every descendant, including ignored nodes, as a conservative bound
// on inline spans; admitted documents use the unchanged native extractor.
fn preflight_caption_copies(html: &Html) -> Result<(), String> {
    let image_selector = selector("img")?;
    let caption_selector = selector("figcaption, .thumbcaption")?;
    let mut copied_bytes = 0usize;
    let mut copied_nodes = 0usize;
    for image in html.select(&image_selector) {
        let figure = image
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find(|ancestor| {
                ancestor.value().name() == "figure"
                    || ancestor.value().classes().any(|class| class == "thumb")
            });
        if let Some(caption) = figure.and_then(|figure| figure.select(&caption_selector).next()) {
            for node in caption.descendants() {
                copied_nodes += 1;
                if let Node::Text(text) = node.value() {
                    copied_bytes = copied_bytes
                        .checked_add(text.len())
                        .ok_or("Wikipedia caption size overflow")?;
                }
                if copied_nodes > 131_072 || copied_bytes > MAX_TEXT_BYTES {
                    return Err(
                        "Wikipedia repeated caption allocation exceeds source limits".into(),
                    );
                }
            }
        } else {
            copied_bytes = copied_bytes
                .checked_add(image.value().attr("alt").map_or(0, str::len))
                .ok_or("Wikipedia caption size overflow")?;
            if copied_bytes > MAX_TEXT_BYTES {
                return Err("Wikipedia repeated caption allocation exceeds source limits".into());
            }
        }
    }
    Ok(())
}
fn article_from_dom(
    title: &str,
    url: &str,
    date: &str,
    html: &Html,
    table_cell_limit: Option<usize>,
) -> Result<Document, String> {
    let root = html
        .select(&selector(".mw-parser-output")?)
        .next()
        .or_else(|| html.select(&selector("body").ok()?).next())
        .ok_or("Wikipedia HTML has no article body")?;
    let revision = html
        .select(&selector("meta[property='mw:revisionId']")?)
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(str::to_owned)
        .or_else(|| {
            html.select(&selector("html").ok()?)
                .next()
                .and_then(|element| element.value().attr("about"))
                .or_else(|| root.value().attr("about"))
                .and_then(|v| v.rsplit('/').next())
                .filter(|v| v.chars().all(|c| c.is_ascii_digit()) && !v.is_empty())
                .map(str::to_owned)
        });
    let mut parser = Parser {
        blocks: Vec::new(),
        text_bytes: 0,
        image_nodes: HashSet::new(),
        table_cells: 0,
        table_cell_limit,
    };
    parser.walk(root, 0)?;
    if parser.blocks.is_empty() {
        return Err("Wikipedia returned no semantic article content".into());
    }
    Ok(Document {
        title: title.into(),
        url: url.into(),
        revision,
        date: date.into(),
        blocks: parser.blocks,
        images: HashMap::new(),
        warnings: Vec::new(),
    })
}

fn article_title(href: &str) -> Option<String> {
    let url = url::Url::parse("https://en.wikipedia.org/wiki/Main_Page")
        .ok()?
        .join(href)
        .ok()?;
    if url.host_str() != Some("en.wikipedia.org") {
        return None;
    }
    let path = url.path().strip_prefix("/wiki/")?;
    let title = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .ok()?
        .replace('_', " ");
    if title.is_empty()
        || title == "Main Page"
        || title.split_once(':').is_some_and(|(namespace, _)| {
            matches!(
                namespace.to_ascii_lowercase().as_str(),
                "media"
                    | "special"
                    | "talk"
                    | "user"
                    | "user talk"
                    | "wikipedia"
                    | "wikipedia talk"
                    | "project"
                    | "project talk"
                    | "file"
                    | "file talk"
                    | "image"
                    | "image talk"
                    | "mediawiki"
                    | "mediawiki talk"
                    | "template"
                    | "template talk"
                    | "help"
                    | "help talk"
                    | "category"
                    | "category talk"
                    | "portal"
                    | "portal talk"
                    | "draft"
                    | "draft talk"
                    | "timedtext"
                    | "timedtext talk"
                    | "module"
                    | "module talk"
                    | "education program"
                    | "education program talk"
                    | "gadget"
                    | "gadget talk"
                    | "gadget definition"
                    | "gadget definition talk"
            )
        })
    {
        return None;
    }
    Some(title)
}

pub fn main_page_titles(html: &str) -> Result<Vec<String>, String> {
    if html.len() > MAX_HTML_BYTES {
        return Err("Wikipedia Main Page exceeds HTML safety limit".into());
    }
    let html = Html::parse_document(html);
    let mut titles = Vec::new();
    let mut seen = HashSet::new();
    for panel in html.select(&selector("#mp-tfa, #mp-dyk, #mp-itn, #mp-otd")?) {
        for link in panel.select(&selector("a[href]")?) {
            if let Some(title) = link.value().attr("href").and_then(article_title) {
                if seen.insert(title.clone()) {
                    titles.push(title);
                }
            }
        }
    }
    if titles.is_empty() {
        return Err("Wikipedia Main Page contained no daily article links".into());
    }
    Ok(titles)
}
