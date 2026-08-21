//! Finds the physical Markdown heading section containing a source line.
//!
//! The editor context menu needs raw source bytes for a clipboard operation,
//! while the renderer works from semantic blocks. Keeping heading-boundary
//! discovery here lets the UI select a chapter without coupling clipboard
//! behavior to either rendering or terminal coordinates.

use std::ops::Range;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Returns the raw source range for the innermost heading section containing
/// `source_line_index` (zero-based), or `None` when that line is outside all
/// Markdown headings. A section starts at its heading and ends immediately
/// before the next heading at the same or a higher level.
pub fn source_range_for_chapter_containing_line(
    markdown: &str,
    source_line_index: usize,
) -> Option<Range<usize>> {
    let headings = markdown_headings(markdown);
    let containing_heading_index = headings
        .iter()
        .rposition(|heading| heading.source_line_index <= source_line_index)?;
    let containing_heading = &headings[containing_heading_index];
    let end = headings
        .iter()
        .skip(containing_heading_index + 1)
        .find(|heading| heading.level <= containing_heading.level)
        .map(|heading| heading.source_range.start)
        .unwrap_or(markdown.len());

    Some(containing_heading.source_range.start..end)
}

/// One parsed heading plus the raw source location the editor must preserve.
struct MarkdownHeading {
    level: u8,
    source_line_index: usize,
    source_range: Range<usize>,
}

/// Parses semantic headings so fenced code and escaped Markdown never become
/// false chapter boundaries merely because they look like a `#` prefix.
///
/// Also drops headings nested inside a block quote or list item at any depth
/// (`> ## Not a chapter`, `- ## Not a chapter`, or a heading indented under a
/// list item on its own line): treating them as real headings would truncate
/// the enclosing chapter at an arbitrary mid-container point and, if picked
/// as the containing heading, hand back a range missing its own `>`/list
/// prefix. Container membership is tracked from the event stream itself
/// (`BlockQuote`/`Item` start and end events) rather than inferred from
/// source bytes, since a nested heading's own line can contain nothing but
/// whitespace before it when the container marker sits on an earlier line.
fn markdown_headings(markdown: &str) -> Vec<MarkdownHeading> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;

    let mut headings = Vec::new();
    let mut container_depth: usize = 0;

    for (event, source_range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::Item) => container_depth += 1,
            // Saturating: depth only ever reaches zero via a matching Start, so this is
            // never actually clamped -- it just avoids a panic if that invariant ever slips.
            Event::End(TagEnd::BlockQuote(_) | TagEnd::Item) => {
                container_depth = container_depth.saturating_sub(1);
            }
            Event::Start(Tag::Heading { level, .. }) if container_depth == 0 => {
                headings.push(MarkdownHeading {
                    level: heading_level_number(level),
                    source_line_index: source_line_index_for_offset(markdown, source_range.start),
                    source_range,
                });
            }
            _ => {}
        }
    }

    headings
}

/// Converts pulldown-cmark's source byte offset to the editor's line index.
fn source_line_index_for_offset(markdown: &str, source_offset: usize) -> usize {
    markdown[..source_offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
}

/// Keeps chapter comparisons independent of pulldown-cmark's enum ordering.
const fn heading_level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::source_range_for_chapter_containing_line;

    #[test]
    fn returns_the_innermost_heading_section_and_stops_at_its_peer() {
        let markdown = "# Document\n\n## Install\nsetup\n\n### Detail\ninner\n\n## Use\nrun\n";
        let range = source_range_for_chapter_containing_line(markdown, 6).unwrap();

        assert_eq!(&markdown[range], "### Detail\ninner\n\n");
    }

    #[test]
    fn returns_none_before_the_first_heading_or_without_a_heading() {
        assert_eq!(
            source_range_for_chapter_containing_line("preface\n# Chapter\nbody\n", 0),
            None
        );
        assert_eq!(
            source_range_for_chapter_containing_line("plain\ntext\n", 1),
            None
        );
    }

    #[test]
    fn ignores_heading_like_text_inside_fenced_code() {
        let markdown = "# Chapter\n\n```md\n# Not a chapter\n```\n\nbody\n";
        let range = source_range_for_chapter_containing_line(markdown, 3).unwrap();

        assert_eq!(&markdown[range], markdown);
    }

    #[test]
    fn ignores_a_heading_nested_in_a_list_item_on_its_own_line() {
        let markdown =
            "# Root\n\n## Chapter A\n\n- item\n\n  ## Nested\n\n  more\n\n## Chapter B\nb\n";
        let range = source_range_for_chapter_containing_line(markdown, 8).unwrap();

        assert_eq!(
            &markdown[range],
            "## Chapter A\n\n- item\n\n  ## Nested\n\n  more\n\n"
        );
    }

    #[test]
    fn ignores_headings_nested_inside_a_block_quote_or_list_item() {
        let markdown = "# Root\n\n## Chapter A\n\n> ## Not a real chapter\n> quoted\n\nbody of A\n\n## Chapter B\nbody of B\n";
        let range = source_range_for_chapter_containing_line(markdown, 5).unwrap();

        assert_eq!(
            &markdown[range],
            "## Chapter A\n\n> ## Not a real chapter\n> quoted\n\nbody of A\n\n"
        );
    }
}
