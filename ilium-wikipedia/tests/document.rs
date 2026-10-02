use ilium_wikipedia::{main_page_titles, parse_article, Block, ImageFloat};

fn parse(html: &str) -> ilium_wikipedia::Document {
    parse_article(
        "Example",
        "https://en.wikipedia.org/wiki/Example",
        "2026-10-02",
        html,
    )
    .unwrap()
}

#[test]
fn nested_styles_references_and_sections_survive() {
    let document = parse(
        r##"<html><head><meta property="mw:revisionId" content="123"/></head><body><section><p>A <b>bold <i>both</i></b> <a href="./Other">link</a><sup class="reference"><a href="#cite">[1]</a></sup>.</p><h2>History</h2><p>Full second section</p></section></body></html>"##,
    );
    assert_eq!(document.revision.as_deref(), Some("123"));
    let Block::Paragraph(spans) = &document.blocks[0] else {
        panic!("paragraph")
    };
    assert!(spans.iter().any(|s| s.text == "both" && s.bold && s.italic));
    assert!(spans
        .iter()
        .any(|s| s.text == "[1]" && s.link && s.superscript));
    assert!(matches!(
        &document.blocks[1],
        Block::Heading { level: 2, .. }
    ));
    assert_eq!(document.blocks.len(), 3);
}

#[test]
fn infobox_table_figures_lists_and_caption_are_structured() {
    let document = parse(
        r##"<body><table class="infobox"><tr><th>Name</th><td><b>Example</b></td></tr><tr><td colspan="2"><img src="//upload.wikimedia.org/wikipedia/commons/a/a0/Example.jpg" width="220" height="110"/></td></tr></table><figure class="mw-halign-right"><img src="//upload.wikimedia.org/wikipedia/commons/x/x0/Other.jpg" width="240" height="160"/><figcaption>An <i>image</i></figcaption></figure><ul><li>One <b>thing</b></li><li>Two</li></ul><table class="wikitable"><tr><td>A</td><td>B</td></tr></table></body>"##,
    );
    assert!(document
        .blocks
        .iter()
        .any(|b| matches!(b, Block::Table { infobox: true, rows, .. } if rows.len() == 2)));
    assert!(document
        .blocks
        .iter()
        .any(|b| matches!(b, Block::Table { infobox: false, .. })));
    assert!(document
        .blocks
        .iter()
        .any(|b| matches!(b, Block::List { ordered: false, items } if items.len() == 2)));
    let images: Vec<_> = document
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Image(i) => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1);
    let table_image = document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table {
                images,
                infobox: true,
                ..
            } => images.first(),
            _ => None,
        })
        .unwrap();
    assert_eq!(table_image.image.float, ImageFloat::Right);
    assert_eq!(images[0].float, ImageFloat::Right);
    assert!(images[0]
        .caption
        .iter()
        .any(|s| s.text == "image" && s.italic));
}

#[test]
fn main_page_uses_only_daily_panels_and_excludes_namespaces() {
    let html = r##"<body><a href="/wiki/Unrelated">outside</a><div id="mp-tfa"><a href="./Featured_article">x</a><a href="/wiki/Wikipedia:Help">no</a><a href="/wiki/Main_Page">no</a></div><div id="mp-dyk"><a href="/wiki/Caf%C3%A9#History">y</a><a href="/wiki/Featured_article">duplicate</a></div><div id="mp-itn"><a href="https://evil.invalid/wiki/No">no</a><a href="/wiki/Category:No">no</a><a href="/wiki/News_article">yes</a></div><div id="mp-otd"><a href="./October_2">yes</a></div></body>"##;
    assert_eq!(
        main_page_titles(html).unwrap(),
        vec!["Featured article", "Café", "News article", "October 2"]
    );
}

#[test]
fn malformed_input_is_recovered_but_empty_or_oversize_is_reported() {
    assert!(!parse("<body><p>recover <b>this").blocks.is_empty());
    assert!(parse_article("x", "url", "date", "<script>hidden</script>").is_err());
    assert!(parse_article("x", "url", "date", &"x".repeat(16 * 1024 * 1024 + 1)).is_err());
    assert!(main_page_titles("<p>not the Main Page</p>").is_err());
}

#[test]
fn loose_inline_content_and_definition_lists_keep_their_styles() {
    let document = parse("<body><div>Note <a href='./Link'><b>linked</b></a> end.</div><dl><dt>Term</dt><dd>Definition <i>italic</i></dd></dl><p>Last</p></body>");
    let Block::Paragraph(spans) = &document.blocks[0] else {
        panic!("paragraph")
    };
    assert!(spans.iter().any(|s| s.text == "linked" && s.link && s.bold));
    assert!(
        matches!(&document.blocks[1], Block::List { items, .. } if items.len() == 2 && items[0][0].bold)
    );
}

#[test]
fn explicit_center_images_and_infobox_flags_survive() {
    let document = parse(
        r##"<body><figure class="mw-halign-center"><img src="https://upload.wikimedia.org/wikipedia/commons/c/c0/C.png"/></figure><table class="infobox"><tr><td><p><img src="https://upload.wikimedia.org/wikipedia/commons/f/f0/Flag.png" width="20" height="10"/>Country</p></td></tr></table></body>"##,
    );
    let images: Vec<_> = document
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Image(i) => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].float, ImageFloat::None);
    let image = document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Table { images, .. } => images.first(),
            _ => None,
        })
        .unwrap();
    assert_eq!(image.image.width, 20);
}

#[test]
fn exceptional_nesting_and_text_are_reported_not_truncated() {
    let nested = format!(
        "{}<p>content</p>{}",
        "<div>".repeat(140),
        "</div>".repeat(140)
    );
    assert!(parse_article("x", "url", "date", &nested).is_err());
    let large_text = format!("<p>{}</p>", "a".repeat(4 * 1024 * 1024 + 1));
    assert!(parse_article("x", "url", "date", &large_text).is_err());
}

#[test]
fn parsoid_revision_is_read_from_document_root() {
    let document = parse(
        r##"<!doctype html><html about="//en.wikipedia.org/wiki/Special:Redirect/revision/1358980925"><body class="mw-parser-output"><section><p>Article.</p></section></body></html>"##,
    );
    assert_eq!(document.revision.as_deref(), Some("1358980925"));
}

#[test]
fn main_page_retains_colons_in_real_article_titles() {
    assert_eq!(main_page_titles(r##"<div id="mp-tfa"><a href="./Star_Trek:_Picard">article</a><a href="./File:Logo.png">file</a><a href="./Template:Foo">template</a></div>"##).unwrap(), vec!["Star Trek: Picard"]);
}

#[test]
fn repeated_asset_occurrences_keep_independent_captions_and_positions() {
    let document = parse(
        r##"<body><figure class="mw-halign-left"><img src="https://upload.wikimedia.org/wikipedia/commons/a/a0/Same.png"/><figcaption>First</figcaption></figure><p>Between</p><figure class="mw-halign-right"><img src="https://upload.wikimedia.org/wikipedia/commons/a/a0/Same.png"/><figcaption>Second</figcaption></figure></body>"##,
    );
    let images: Vec<_> = document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].caption[0].text, "First");
    assert_eq!(images[1].caption[0].text, "Second");
    assert_eq!(images[0].float, ImageFloat::Left);
    assert_eq!(images[1].float, ImageFloat::Right);
}

#[test]
fn table_grid_preserves_rowspan_and_colspan_geometry() {
    let document = parse(
        r##"<body><table><tr><th colspan="2">Header</th><th>Right</th></tr><tr><td rowspan="2">Tall</td><td>One</td><td>Two</td></tr><tr><td colspan="2">Wide</td></tr></table></body>"##,
    );
    let Block::Table { rows, spans, .. } = &document.blocks[0] else {
        panic!("table")
    };
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), vec![3, 3, 3]);
    assert!(rows[0][1].is_empty());
    assert!(rows[2][0].is_empty());
    assert!(spans
        .iter()
        .any(|span| span.row == 0 && span.column == 0 && span.column_span == 2));
    assert!(spans
        .iter()
        .any(|span| span.row == 1 && span.column == 0 && span.row_span == 2));
    assert_eq!(rows[2][1][0].text, "Wide");
}

#[test]
fn inline_wrapped_image_only_document_is_preserved() {
    let document = parse(
        r##"<div><span><a href="./File:X.png"><img src="https://upload.wikimedia.org/wikipedia/commons/x/x0/X.png"/></a></span></div>"##,
    );
    assert!(document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Image(_))));
}

#[test]
fn infobox_images_are_associated_with_the_original_cell() {
    let document = parse(
        r##"<table class="infobox"><tr><th>Portrait</th><td><img src="https://upload.wikimedia.org/wikipedia/commons/p/p0/P.png"/></td></tr><tr><td colspan="2"><img src="https://upload.wikimedia.org/wikipedia/commons/f/f0/F.png"/></td></tr></table>"##,
    );
    let Block::Table { images, .. } = &document.blocks[0] else {
        panic!("table")
    };
    assert_eq!(images.len(), 2);
    assert_eq!((images[0].row, images[0].column), (0, 1));
    assert_eq!((images[1].row, images[1].column), (1, 0));
    assert!(!document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Image(_))));
    assert!(document.warnings.is_empty());
}

#[test]
fn lone_images_do_not_move_later_images_before_intervening_content() {
    let document = parse(
        r##"<div><img src="https://upload.wikimedia.org/wikipedia/commons/a/a0/A.png"/><p>middle</p><img src="https://upload.wikimedia.org/wikipedia/commons/b/b0/B.png"/></div>"##,
    );
    assert!(matches!(&document.blocks[0], Block::Image(image) if image.url.ends_with("A.png")));
    assert!(matches!(&document.blocks[1], Block::Paragraph(spans) if spans[0].text == "middle"));
    assert!(matches!(&document.blocks[2], Block::Image(image) if image.url.ends_with("B.png")));
}

#[test]
fn lone_image_does_not_steal_a_later_table_image() {
    let document = parse(
        r##"<div><img src="https://upload.wikimedia.org/wikipedia/commons/a/a0/A.png"/><table><tr><td><img src="https://upload.wikimedia.org/wikipedia/commons/b/b0/B.png"/></td></tr></table></div>"##,
    );
    assert!(matches!(&document.blocks[0], Block::Image(image) if image.url.ends_with("A.png")));
    assert!(
        matches!(&document.blocks[1], Block::Table { images, .. } if images.len() == 1 && images[0].image.url.ends_with("B.png"))
    );
    assert_eq!(document.blocks.len(), 2);
}

#[test]
fn zero_rowspan_means_remaining_current_row_group() {
    let document = parse(
        r##"<table><tbody><tr><td rowspan="0">Tall</td><td>One</td></tr><tr><td>Two</td></tr><tr><td>Three</td></tr></tbody><tbody><tr><td>Separate</td><td>Group</td></tr></tbody></table>"##,
    );
    let Block::Table { rows, spans, .. } = &document.blocks[0] else {
        panic!("table")
    };
    assert_eq!(spans[0].row_span, 3);
    assert!(rows[1][0].is_empty());
    assert_eq!(rows[1][1][0].text, "Two");
    assert_eq!(rows[3][0][0].text, "Separate");
}

#[test]
fn nested_table_text_retains_cell_and_row_separators() {
    let document = parse(
        r##"<table class="infobox"><tr><td><table><tr><td>North</td><td>South</td></tr><tr><td>East</td><td>West</td></tr></table></td></tr></table>"##,
    );
    let Block::Table { rows, .. } = &document.blocks[0] else {
        panic!("table")
    };
    let text: String = rows[0][0].iter().map(|span| span.text.as_str()).collect();
    assert!(text.contains("North South"), "{text:?}");
    assert!(text.contains("\nEast West"), "{text:?}");
}

#[test]
fn current_wikimedia_thumbnail_host_is_preserved() {
    // Current REST HTML captured 2026-10-02 uses thumb.wikimedia.org.
    let document = parse(
        r##"<body><figure class="mw-halign-right"><img src="//thumb.wikimedia.org/wikipedia/commons/thumb/e/e8/Kaiser.jpg/330px-Kaiser.jpg?utm_source=en.wikipedia.org" width="300" height="227"/><figcaption>SMS Kaiser</figcaption></figure></body>"##,
    );
    assert!(
        matches!(&document.blocks[0], Block::Image(image) if image.url.starts_with("https://thumb.wikimedia.org/") && image.caption[0].text == "SMS Kaiser")
    );
}
