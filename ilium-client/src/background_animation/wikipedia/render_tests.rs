use super::*;
use image::{Rgba, RgbaImage};
use std::collections::{HashMap, HashSet};

fn span(text: &str) -> Span {
    Span {
        text: text.into(),
        bold: false,
        italic: false,
        link: false,
        superscript: false,
    }
}
fn document(blocks: Vec<Block>) -> Document {
    Document {
        title: "Native Wikipedia fixture".into(),
        url: "https://example.invalid/wiki/Fixture".into(),
        revision: Some("42".into()),
        date: "2026-10-02".into(),
        blocks,
        images: HashMap::new(),
        warnings: Vec::new(),
    }
}
fn text_settings() -> WikipediaSettings {
    WikipediaSettings {
        render_mode: RenderMode::Text,
        ..WikipediaSettings::default()
    }
}
fn all_text(layout: &Layout) -> String {
    layout
        .items
        .iter()
        .filter_map(|item| match &item.paint {
            Paint::Text(glyphs) => {
                Some(glyphs.iter().map(|g| g.symbol.as_str()).collect::<String>())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn no_space(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn image_ref(float: ImageFloat) -> ImageRef {
    ImageRef {
        url: "https://example.invalid/figure.png".into(),
        caption: vec![span("Actual caption")],
        width: 160,
        height: 80,
        float,
    }
}
fn with_image(float: ImageFloat, pixel: [u8; 4]) -> Document {
    let reference = image_ref(float);
    let mut doc = document(vec![Block::Image(reference)]);
    doc.images.insert(
        "https://example.invalid/figure.png".into(),
        RgbaImage::from_pixel(4, 2, Rgba(pixel)),
    );
    doc
}
fn builder(doc: &Document, columns: u16) -> Builder<'_> {
    Builder {
        document: doc,
        key: text_settings().geometry(columns),
        fonts: None,
        cancel: None,
        layout: Layout::default(),
    }
}

#[test]
fn defaults_and_normalization_match_the_packet() {
    let s = WikipediaSettings::default();
    assert_eq!(s.render_mode, RenderMode::Braille);
    assert!(s.greyscale);
    assert_eq!(
        (s.zoom_percent, s.scroll_tenths, s.dwell_seconds),
        (100, 2, 10)
    );
    let n = WikipediaSettings {
        zoom_percent: 0,
        scroll_tenths: 1000,
        dwell_seconds: 0,
        hue_degrees: 1000,
        saturation_percent: 1000,
        lightness_percent: 0,
        ..s
    }
    .normalized();
    assert_eq!(
        (n.zoom_percent, n.scroll_tenths, n.dwell_seconds),
        (50, 100, 2)
    );
    assert_eq!(
        (n.hue_degrees, n.saturation_percent, n.lightness_percent),
        (359, 200, 10)
    );
}
#[test]
fn safe_symbols_are_one_or_two_inked_terminal_columns() {
    for s in ["a", "é", "e\u{301}", "¹", "⣿", "界"] {
        assert!(is_safe_symbol(s), "{s:?}");
    }
    assert_eq!(safe_symbol_width("界"), Some(2));
    for s in [
        "",
        " ",
        "\u{a0}",
        "\u{2800}",
        "\u{301}",
        "ab",
        "\x1b",
        "\t",
        "\n",
        "a\u{202e}",
    ] {
        assert!(!is_safe_symbol(s), "{s:?}");
    }
}
#[test]
fn superscripts_preserve_recognized_references_and_mark_fallbacks() {
    assert_eq!(superscript_text("[12]"), "[¹²]");
    assert_eq!(superscript_text("n+1"), "ⁿ⁺¹");
    assert_eq!(superscript_text("citation"), "^{citation}");
}
#[test]
fn rich_text_preserves_adjacency_and_cross_span_combining_clusters() {
    let mut stats = LayoutStats::default();
    let mut accent = span("\u{301} words");
    accent.italic = true;
    let rich = Rich::from_spans(
        &[span("e"), accent, span(" with"), span("out gaps")],
        RenderMode::Text,
        Role::Body,
        &mut stats,
    );
    let joined: String = rich.parts.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(joined, "e\u{301} words without gaps");
    assert_eq!(stats.escaped_clusters, 0);
    let lines = text_lines(&rich, 80).unwrap();
    let Paint::Text(glyphs) = &lines[0].paint else {
        panic!("native text");
    };
    assert_eq!(glyphs[0].symbol, "e\u{301}");
    assert!(
        !glyphs[0].style.italic,
        "one cell uses the cluster-start style"
    );
}
#[test]
fn hostile_controls_escape_but_legitimate_wide_text_survives() {
    let mut stats = LayoutStats::default();
    let rich = Rich::from_spans(
        &[span("A\x1b[31m 界 \u{202e}B")],
        RenderMode::Text,
        Role::Body,
        &mut stats,
    );
    let joined: String = rich.parts.iter().map(|p| p.text.as_str()).collect();
    assert!(joined.contains("\\u{1b}[31m"));
    assert!(joined.contains("界"));
    assert!(joined.contains("\\u{202e}"));
    assert!(!joined.chars().any(forbidden));
    assert!(stats.escaped_clusters > 0);
}
#[test]
fn wide_text_wraps_before_boundary_and_reserves_following_cell() {
    let rich = Rich::from_spans(
        &[span("abcd界e")],
        RenderMode::Text,
        Role::Body,
        &mut LayoutStats::default(),
    );
    let lines = text_lines(&rich, 5).unwrap();
    let first = match &lines[0].paint {
        Paint::Text(glyphs) => glyphs,
        _ => panic!("text expected"),
    };
    let second = match &lines[1].paint {
        Paint::Text(glyphs) => glyphs,
        _ => panic!("text expected"),
    };
    assert_eq!(
        first.iter().map(|g| g.symbol.as_str()).collect::<String>(),
        "abcd"
    );
    assert_eq!(
        second.iter().map(|g| g.symbol.as_str()).collect::<String>(),
        "界e"
    );
    let mut renderer = WikipediaRenderer::new();
    renderer
        .prepare(
            Arc::new(document(vec![Block::Paragraph(vec![span("界")])])),
            40,
            &text_settings(),
        )
        .unwrap();
    let cells = renderer.render(40, 0.0).unwrap();
    let lead = cells.iter().position(|cell| cell.symbol() == "界").unwrap();
    assert!(cells[lead + 1].is_continuation());
    assert!(!cells[lead + 1].is_ink());
}
#[test]
fn word_wrap_does_not_invent_spaces_at_format_boundaries_or_lose_long_words() {
    let mut bold = span("ground");
    bold.bold = true;
    let mut stats = LayoutStats::default();
    let rich = Rich::from_spans(
        &[span("back"), bold, span(" supercalifragilistic")],
        RenderMode::Text,
        Role::Body,
        &mut stats,
    );
    let drafts = text_lines(&rich, 5).unwrap();
    let actual: String = drafts
        .iter()
        .filter_map(|d| match &d.paint {
            Paint::Text(g) => Some(g.iter().map(|g| g.symbol.as_str()).collect::<String>()),
            _ => None,
        })
        .collect();
    assert_eq!(no_space(&actual), "backgroundsupercalifragilistic");
    assert!(drafts
        .iter()
        .all(|d| matches!(&d.paint, Paint::Text(g) if g.len() <= 5)));
}
#[test]
fn explicit_source_line_break_survives_native_text_layout() {
    let doc = Arc::new(document(vec![Block::Paragraph(vec![span(
        "first line\nsecond line",
    )])]));
    let mut renderer = WikipediaRenderer::new();
    renderer.prepare(doc, 80, &text_settings()).unwrap();
    let text = all_text(renderer.layout.as_ref().unwrap());
    assert!(
        text.contains("first line\nsecond line"),
        "explicit <br> lost: {text}"
    );
}

#[test]
fn hard_break_font_offsets_consume_newline_and_preserve_empty_rows() {
    let mut fonts = Fonts::new().unwrap();
    let mut stats = LayoutStats::default();
    let rich = Rich::from_spans(
        &[span("first\n\nlast")],
        RenderMode::Braille,
        Role::Body,
        &mut stats,
    );
    let lines = fonts.lines(&rich, 200.0, 32.0).unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines.last().unwrap().end, 11);
    assert!(lines.windows(2).all(|pair| pair[0].end < pair[1].end));
}

#[test]
fn hard_break_at_exact_text_width_consumes_one_row() {
    let mut stats = LayoutStats::default();
    let rich = Rich::from_spans(
        &[span("ABCDE\nFGHIJ")],
        RenderMode::Text,
        Role::Body,
        &mut stats,
    );
    let lines = text_lines(&rich, 5).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].end, 6);
    assert_eq!(lines[1].end, 11);
}

#[test]
fn braille_float_reflow_after_hard_break_does_not_duplicate_source() {
    let doc = document(Vec::new());
    let mut fonts = Fonts::new().unwrap();
    let mut builder = Builder {
        document: &doc,
        key: WikipediaSettings::default().geometry(140),
        fonts: Some(&mut fonts),
        cancel: None,
        layout: Layout::default(),
    };
    let rich = builder.rich(&[span("A\nB\nC")], Role::Body);
    let mut flow = Flow {
        x: 0.0,
        width: 1120.0,
        y: 0.0,
        float: Some(Float {
            right: true,
            width: 440.0,
            bottom: 40.0,
        }),
    };
    builder.inline(rich, &mut flow, 1.0).unwrap();
    assert_eq!(builder.layout.stats.glyphs, 3);
    assert_eq!(builder.layout.items.len(), 3);
    assert_eq!(flow.y, 150.0);
}

#[test]
fn paragraphs_expand_below_right_and_left_floats_without_losing_text() {
    let doc = document(Vec::new());
    for right in [false, true] {
        let mut b = builder(&doc, 80);
        let source = "alpha beta gamma delta ".repeat(100);
        let rich = b.rich(&[span(&source)], Role::Body);
        let mut flow = Flow {
            x: 0.0,
            width: 80.0 * CW as f32,
            y: 0.0,
            float: Some(Float {
                right,
                width: 25.0 * CW as f32,
                bottom: 2.0 * CH as f32,
            }),
        };
        b.inline(rich, &mut flow, 1.0).unwrap();
        assert_eq!(no_space(&all_text(&b.layout)), no_space(&source));
        assert!(b.layout.items.iter().any(|i| i.rect.w < flow.width));
        assert!(b.layout.items.iter().any(|i| i.rect.w == flow.width));
        assert_eq!(b.layout.items[0].rect.x > 0.0, !right);
    }
}
#[test]
fn infobox_is_right_floated_when_readable_and_stacked_when_narrow() {
    let rows = vec![
        vec![
            vec![span("Name")],
            vec![span("A long value that must remain complete")],
        ],
        vec![vec![span("Last")], vec![span("FINAL-CELL")]],
    ];
    let doc = document(Vec::new());
    for (columns, floated) in [(96, true), (40, false)] {
        let mut b = builder(&doc, columns);
        let mut flow = Flow {
            x: 0.0,
            width: f32::from(columns) * CW as f32,
            y: 0.0,
            float: None,
        };
        b.table(&rows, &[], &[], &mut flow, true).unwrap();
        assert_eq!(flow.float.is_some(), floated);
        if let Some(f) = flow.float {
            assert!(f.right);
            assert_eq!(flow.y, 0.0);
            assert!(b
                .layout
                .items
                .iter()
                .all(|i| i.rect.x >= flow.width - f.width));
        } else {
            assert!(flow.y > 0.0);
        }
        assert!(no_space(&all_text(&b.layout)).contains("FINAL-CELL"));
        assert!(b
            .layout
            .items
            .iter()
            .all(|i| i.rect.x >= 0.0 && i.rect.x + i.rect.w <= flow.width + 0.01));
    }
}
#[test]
fn merged_table_geometry_and_positioned_image_keep_their_anchor() {
    let picture = image_ref(ImageFloat::Right);
    let doc = document(vec![Block::Table {
        rows: vec![
            vec![vec![span("Wide heading")], Vec::new()],
            vec![vec![span("Tall value")], vec![span("Other value")]],
            vec![Vec::new(), vec![span("Last value")]],
        ],
        spans: vec![
            TableSpan {
                row: 0,
                column: 0,
                row_span: 1,
                column_span: 2,
            },
            TableSpan {
                row: 1,
                column: 0,
                row_span: 2,
                column_span: 1,
            },
            TableSpan {
                row: 1,
                column: 1,
                row_span: 1,
                column_span: 1,
            },
            TableSpan {
                row: 2,
                column: 1,
                row_span: 1,
                column_span: 1,
            },
        ],
        images: vec![TableImage {
            row: 1,
            column: 0,
            image: picture.clone(),
        }],
        infobox: true,
    }]);
    for columns in [40, 100] {
        let mut doc = doc.clone();
        doc.images.insert(
            picture.url.clone(),
            RgbaImage::from_pixel(4, 2, Rgba([200, 40, 20, 255])),
        );
        let mut renderer = WikipediaRenderer::new();
        renderer
            .prepare(Arc::new(doc), columns, &text_settings())
            .unwrap();
        let layout = renderer.layout.as_ref().unwrap();
        let text = all_text(layout);
        for expected in [
            "Wide heading",
            "Tall value",
            "Other value",
            "Last value",
            "Actual caption",
        ] {
            assert_eq!(text.matches(expected).count(), 1, "{columns}: {expected}");
        }
        assert_eq!(
            layout
                .items
                .iter()
                .filter(|item| matches!(&item.paint, Paint::Image(url) if url == &picture.url))
                .count(),
            1
        );
        assert!(!text.contains("example.invalid"));
    }
}
#[test]
fn intermediate_table_rules_do_not_slice_row_spanning_positioned_images() {
    let mut picture = image_ref(ImageFloat::None);
    picture.width = 128;
    picture.height = 256;
    let mut doc = document(Vec::new());
    doc.images.insert(
        picture.url.clone(),
        RgbaImage::from_pixel(8, 16, Rgba([200, 60, 60, 255])),
    );
    let mut builder = builder(&doc, 80);
    let mut flow = Flow {
        x: 0.0,
        width: 80.0 * CW as f32,
        y: 0.0,
        float: None,
    };
    builder
        .table(
            &[
                vec![Vec::new(), vec![span("First row")]],
                vec![Vec::new(), vec![span("Second row")]],
            ],
            &[
                TableSpan {
                    row: 0,
                    column: 0,
                    row_span: 2,
                    column_span: 1,
                },
                TableSpan {
                    row: 0,
                    column: 1,
                    row_span: 1,
                    column_span: 1,
                },
                TableSpan {
                    row: 1,
                    column: 1,
                    row_span: 1,
                    column_span: 1,
                },
            ],
            &[TableImage {
                row: 0,
                column: 0,
                image: picture.clone(),
            }],
            &mut flow,
            false,
        )
        .unwrap();
    let image = builder
        .layout
        .items
        .iter()
        .find(|item| matches!(&item.paint, Paint::Image(url) if url == &picture.url))
        .unwrap()
        .rect;
    let crossing_rules: Vec<_> = builder
        .layout
        .items
        .iter()
        .filter(|item| matches!(&item.paint, Paint::Rule))
        .map(|item| item.rect)
        .filter(|rule| rule.h == 2.0 && rule.y < image.bottom() && rule.bottom() > image.y)
        .collect();
    assert!(
        !crossing_rules.is_empty(),
        "fixture must cross an intermediate row"
    );
    assert!(crossing_rules
        .iter()
        .all(|rule| rule.x + rule.w <= image.x || rule.x >= image.x + image.w));
}
#[test]
fn narrow_multicolumn_tables_keep_every_cell_and_row_order() {
    let doc = document(Vec::new());
    let mut b = builder(&doc, 20);
    let rows = vec![
        vec![vec![span("A1")], vec![span("A2")], vec![span("A3")]],
        vec![vec![span("B1")], vec![span("B2")], vec![span("B3")]],
    ];
    let mut flow = Flow {
        x: 0.0,
        width: 20.0 * CW as f32,
        y: 0.0,
        float: None,
    };
    b.table(&rows, &[], &[], &mut flow, false).unwrap();
    assert_eq!(no_space(&all_text(&b.layout)), "A1A2A3B1B2B3");
}
#[test]
fn full_document_keeps_last_block_and_provenance() {
    let mut blocks: Vec<Block> = (0..200)
        .map(|i| Block::Paragraph(vec![span(&format!("paragraph {i}"))]))
        .collect();
    blocks.push(Block::Paragraph(vec![span("FULL-ARTICLE-TAIL")]));
    let doc = Arc::new(document(blocks));
    let mut r = WikipediaRenderer::new();
    r.prepare(doc, 80, &text_settings()).unwrap();
    let text = all_text(r.layout.as_ref().unwrap());
    assert!(text.contains("FULL-ARTICLE-TAIL"));
    assert!(
        !text.contains("example.invalid"),
        "source metadata stays outside article body"
    );
    let total = r.total_rows();
    assert!(total > 200);
    let visible: String = r
        .render(12, f64::from(total))
        .unwrap()
        .iter()
        .map(RenderCell::symbol)
        .collect();
    assert!(visible.contains("FULL-ARTICLE-TAIL"));
}
#[test]
fn text_mode_preserves_actual_bold_italic_and_superscript_cells() {
    let mut bold = span("Bold");
    bold.bold = true;
    let mut italic = span("Italic");
    italic.italic = true;
    let mut reference = span("12");
    reference.superscript = true;
    let doc = Arc::new(document(vec![Block::Paragraph(vec![
        bold,
        span(" "),
        italic,
        reference,
    ])]));
    let mut r = WikipediaRenderer::new();
    r.prepare(doc, 80, &text_settings()).unwrap();
    let cells = r.render(40, 0.0).unwrap();
    assert!(cells.iter().any(|c| c.symbol() == "B" && c.bold()));
    assert!(cells.iter().any(|c| c.symbol() == "I" && c.italic()));
    assert!(cells.iter().any(|c| c.symbol() == "¹"));
    assert!(cells
        .iter()
        .filter(|c| c.is_ink())
        .all(|c| is_safe_symbol(c.symbol())));
}
#[test]
fn decoded_aspect_and_caption_survive_both_float_directions() {
    for float in [ImageFloat::Left, ImageFloat::Right, ImageFloat::None] {
        let doc = Arc::new(with_image(float, [220, 40, 100, 255]));
        let mut r = WikipediaRenderer::new();
        r.prepare(doc, 100, &text_settings()).unwrap();
        let l = r.layout.as_ref().unwrap();
        let rectangle = l
            .items
            .iter()
            .find(|i| matches!(&i.paint, Paint::Image(_)))
            .unwrap()
            .rect;
        assert!((rectangle.w / rectangle.h - 2.0).abs() < 0.001);
        let text = no_space(&all_text(l));
        assert!(text.contains("Actualcaption"));
        assert!(
            !text.contains("example.invalid"),
            "asset metadata stays outside captions"
        );
    }
}
#[test]
fn missing_image_is_not_a_fake_picture_or_a_lost_caption() {
    let doc = Arc::new(document(vec![Block::Image(image_ref(ImageFloat::None))]));
    let mut r = WikipediaRenderer::new();
    r.prepare(doc, 80, &text_settings()).unwrap();
    let l = r.layout.as_ref().unwrap();
    assert_eq!(l.stats.unavailable_images, 1);
    assert!(!l.items.iter().any(|i| matches!(&i.paint, Paint::Image(_))));
    assert!(all_text(l).contains("[Image unavailable]"));
    assert!(all_text(l).contains("Actual caption"));
}
#[test]
fn alpha_aware_image_sampling_has_no_black_transparency_fringe() {
    let image = RgbaImage::from_fn(2, 1, |x, _| {
        if x == 0 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([255, 0, 0, 255])
        }
    });
    let p = sample_image(&image, 0.5, 0.5);
    assert_eq!(&p[..3], &[255, 0, 0]);
    assert!((127..=128).contains(&p[3]));
}
#[test]
fn transparent_or_black_images_never_fill_a_page_rectangle() {
    for pixel in [[255, 255, 255, 0], [0, 0, 0, 255]] {
        let doc = with_image(ImageFloat::None, pixel);
        let mut layout = Layout::default();
        layout
            .push(Item {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 8.0,
                    h: 16.0,
                },
                paint: Paint::Image(image_ref(ImageFloat::None).url),
            })
            .unwrap();
        layout.finish().unwrap();
        let strip = raster_strip(&mut layout, &doc, &mut None, 1, text_settings(), 0).unwrap();
        assert!(strip.dots.iter().all(|d| d.alpha == 0));
    }
}
#[test]
fn opaque_black_diagram_mark_survives_white_paper_without_a_white_plane() {
    let mut doc = with_image(ImageFloat::None, [255, 255, 255, 255]);
    let image = doc
        .images
        .get_mut("https://example.invalid/figure.png")
        .unwrap();
    image.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
    image.put_pixel(1, 1, Rgba([0, 0, 0, 255]));
    let mut layout = Layout::default();
    layout
        .push(Item {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
            },
            paint: Paint::Image(image_ref(ImageFloat::None).url),
        })
        .unwrap();
    layout.finish().unwrap();
    let strip = raster_strip(&mut layout, &doc, &mut None, 2, text_settings(), 0).unwrap();
    assert!(strip.dots.iter().any(|dot| dot.alpha > 0));
    assert!(strip.dots.iter().any(|dot| dot.alpha == 0));
    assert!(strip
        .dots
        .iter()
        .filter(|dot| dot.alpha > 0)
        .all(|dot| dot.rgb.iter().any(|channel| *channel > 80)));
}
#[test]
fn dark_transparent_logo_marks_keep_ink_without_filling_alpha_gaps() {
    let mut doc = with_image(ImageFloat::None, [0, 0, 0, 0]);
    let image = doc
        .images
        .get_mut("https://example.invalid/figure.png")
        .unwrap();
    *image = RgbaImage::from_fn(16, 16, |x, y| {
        if (x / 4 + y / 4) % 2 == 0 {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let mut layout = Layout::default();
    layout
        .push(Item {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
            },
            paint: Paint::Image(image_ref(ImageFloat::None).url),
        })
        .unwrap();
    layout.finish().unwrap();
    let strip = raster_strip(&mut layout, &doc, &mut None, 2, text_settings(), 0).unwrap();
    assert!(strip.dots.iter().any(|dot| dot.alpha > 0));
    assert!(strip.dots.iter().any(|dot| dot.alpha == 0));
    assert!(strip
        .dots
        .iter()
        .filter(|dot| dot.alpha > 0)
        .all(|dot| dot.rgb.iter().any(|channel| *channel > 80)));
}
#[test]
fn one_pixel_black_transparent_checkerboard_cannot_alias_alpha_detection() {
    let mut doc = with_image(ImageFloat::None, [0, 0, 0, 0]);
    let image = doc
        .images
        .get_mut("https://example.invalid/figure.png")
        .unwrap();
    *image = RgbaImage::from_fn(32, 32, |x, y| {
        if (x + y) % 2 == 0 {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let contrast = analyze_image_contrast(image, None).unwrap();
    assert!(contrast.alpha_backed);
    let mut layout = Layout::default();
    layout
        .push(Item {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 32.0,
                h: 32.0,
            },
            paint: Paint::Image(image_ref(ImageFloat::None).url),
        })
        .unwrap();
    layout.finish().unwrap();
    let strip = raster_strip(&mut layout, &doc, &mut None, 4, text_settings(), 0).unwrap();
    assert!(strip.dots.iter().any(|dot| dot.alpha > 0));
    assert!(strip
        .dots
        .iter()
        .filter(|dot| dot.alpha > 0)
        .all(|dot| dot.rgb.iter().any(|channel| *channel > 80)));
    assert_eq!(layout.image_contrast.len(), 1);
    raster_strip(&mut layout, &doc, &mut None, 4, text_settings(), 0).unwrap();
    assert_eq!(layout.image_contrast.len(), 1);
}
#[test]
fn braille_dot_positions_match_the_native_bit_contract() {
    let doc = Arc::new(document(Vec::new()));
    for (dx, dy, expected) in [
        (0, 0, 1),
        (0, 1, 2),
        (0, 2, 4),
        (1, 0, 8),
        (1, 1, 16),
        (1, 2, 32),
        (0, 3, 64),
        (1, 3, 128),
    ] {
        let mut layout = Layout {
            bottom: 16.0,
            ..Layout::default()
        };
        layout.finish().unwrap();
        let mut dots = vec![DotInk::default(); 2 * STRIP_DOTS];
        dots[dy * 2 + dx] = DotInk {
            rgb: [100, 100, 100],
            alpha: 255,
        };
        let mut r = WikipediaRenderer {
            document: Some(Arc::clone(&doc)),
            layout: Some(layout),
            columns: 1,
            settings: text_settings(),
            ..WikipediaRenderer::default()
        };
        r.strips.push_back(Strip { band: 0, dots });
        let cells = r.render(1, 0.0).unwrap();
        assert_eq!(
            cells[0].symbol().chars().next().unwrap() as u32,
            0x2800 + expected
        );
    }
}
#[test]
fn greyscale_is_exact_after_every_palette_and_slider_combination() {
    for palette in [
        Palette::Wikipedia,
        Palette::Pastel,
        Palette::Sepia,
        Palette::Night,
    ] {
        for hue in [0, 120, 359] {
            let settings = WikipediaSettings {
                palette,
                hue_degrees: hue,
                saturation_percent: 200,
                greyscale: true,
                ..text_settings()
            };
            for rgb in [[255, 0, 40], [20, 190, 70], [0, 0, 0], [255, 255, 255]] {
                let c = transform(rgb, settings);
                assert_eq!(c[0], c[1]);
                assert_eq!(c[1], c[2]);
            }
            let mut r = WikipediaRenderer::new();
            r.prepare(
                Arc::new(with_image(ImageFloat::None, [240, 20, 80, 255])),
                80,
                &settings,
            )
            .unwrap();
            for cell in r.render(40, 0.0).unwrap().iter().filter(|c| c.is_ink()) {
                let [r, g, b] = cell.rgb();
                assert_eq!(r, g);
                assert_eq!(g, b);
            }
        }
    }
}
#[test]
fn palettes_and_each_color_slider_affect_both_text_and_images() {
    let s = WikipediaSettings {
        greyscale: false,
        ..text_settings()
    };
    let samples: Vec<_> = [
        Palette::Wikipedia,
        Palette::Pastel,
        Palette::Sepia,
        Palette::Night,
    ]
    .into_iter()
    .map(|palette| WikipediaSettings { palette, ..s })
    .collect();
    assert_eq!(
        samples
            .iter()
            .map(|s| ink(Role::Link, *s))
            .collect::<HashSet<_>>()
            .len(),
        4
    );
    assert_eq!(
        samples
            .iter()
            .map(|s| transform([180, 80, 35], *s))
            .collect::<HashSet<_>>()
            .len(),
        4
    );
    for changed in [
        WikipediaSettings {
            hue_degrees: 140,
            ..s
        },
        WikipediaSettings {
            saturation_percent: 0,
            ..s
        },
        WikipediaSettings {
            lightness_percent: 20,
            ..s
        },
    ] {
        assert_ne!(ink(Role::Link, s), ink(Role::Link, changed));
        assert_ne!(
            transform([180, 80, 35], s),
            transform([180, 80, 35], changed)
        );
    }
}
#[test]
fn cache_keys_separate_layout_ink_viewport_and_scroll_settings() {
    let doc = Arc::new(document(vec![Block::Paragraph(vec![span(
        &"cache words ".repeat(100),
    )])]));
    let mut s = text_settings();
    let mut r = WikipediaRenderer::new();
    assert!(r.prepare(Arc::clone(&doc), 80, &s).unwrap());
    r.render(20, 0.0).unwrap();
    let work = r.work_stats();
    assert!(!r.prepare(Arc::clone(&doc), 80, &s).unwrap());
    r.render(20, 0.9).unwrap();
    assert_eq!(r.work_stats(), work);
    s.scroll_tenths = 0;
    s.dwell_seconds = 60;
    s.zoom_percent = 300;
    assert!(!r.prepare(Arc::clone(&doc), 80, &s).unwrap());
    r.render(20, 0.0).unwrap();
    assert_eq!(r.work_stats(), work);
    s.hue_degrees = 180;
    assert!(!r.prepare(Arc::clone(&doc), 80, &s).unwrap());
    r.render(20, 0.0).unwrap();
    assert_eq!(r.work_stats().layouts, work.layouts);
    assert!(r.work_stats().raster_strips > work.raster_strips);
    let after = r.work_stats();
    r.render(21, 1.0).unwrap();
    assert_eq!(r.work_stats().layouts, after.layouts);
    assert!(r.prepare(doc, 60, &s).unwrap());
    assert_eq!(r.work_stats().layouts, work.layouts + 1);
}
#[test]
fn a_new_document_arc_invalidates_even_identical_url_and_revision() {
    let mut r = WikipediaRenderer::new();
    r.prepare(
        Arc::new(document(vec![Block::Paragraph(vec![span("old")])])),
        80,
        &text_settings(),
    )
    .unwrap();
    r.prepare(
        Arc::new(document(vec![Block::Paragraph(vec![span("NEW-DOCUMENT")])])),
        80,
        &text_settings(),
    )
    .unwrap();
    assert_eq!(r.work_stats().layouts, 2);
    assert!(all_text(r.layout.as_ref().unwrap()).contains("NEW-DOCUMENT"));
}
#[test]
fn failed_prepare_is_memoized_does_not_pin_images_or_keep_stale_output() {
    let mut r = WikipediaRenderer::new();
    r.prepare(Arc::new(document(Vec::new())), 80, &text_settings())
        .unwrap();
    r.render(12, 0.0).unwrap();
    let bad = Arc::new(document(vec![Block::Paragraph(vec![span(
        &"x".repeat(MAX_INLINE_BYTES + 1),
    )])]));
    assert!(matches!(
        r.prepare(Arc::clone(&bad), 80, &text_settings()),
        Err(RenderError::Limit(_))
    ));
    let work = r.work_stats();
    assert!(r.cells().is_empty());
    assert!(r.layout_stats().is_none());
    assert_eq!(Arc::strong_count(&bad), 1);
    assert!(r.prepare(Arc::clone(&bad), 80, &text_settings()).is_err());
    assert_eq!(r.work_stats(), work);
    assert!(matches!(r.render(12, 0.0), Err(RenderError::NotPrepared)));
}
#[test]
fn invalid_dimensions_offsets_and_release_never_expose_stale_cells() {
    let doc = Arc::new(document(Vec::new()));
    let mut r = WikipediaRenderer::new();
    assert!(matches!(
        r.prepare(Arc::clone(&doc), u16::MAX, &text_settings()),
        Err(RenderError::Limit(_))
    ));
    assert!(matches!(
        r.prepare(Arc::clone(&doc), 0, &text_settings()),
        Err(RenderError::TooNarrow)
    ));
    r.prepare(Arc::clone(&doc), 80, &text_settings()).unwrap();
    r.render(20, 0.0).unwrap();
    assert!(matches!(
        r.render(20, f64::NAN),
        Err(RenderError::InvalidOffset)
    ));
    assert!(r.cells().is_empty());
    assert!(r.render(u16::MAX, 0.0).is_err());
    assert!(r.cells().is_empty());
    assert!(r.render(0, 0.0).unwrap().is_empty());
    r.release();
    assert_eq!((r.width(), r.height(), r.total_rows()), (0, 0, 0));
    assert_eq!(Arc::strong_count(&doc), 1);
}
#[test]
fn strip_lru_is_bounded_over_a_complete_long_article() {
    let doc = Arc::new(document(
        (0..700)
            .map(|i| Block::Paragraph(vec![span(&format!("row {i}"))]))
            .collect(),
    ));
    let mut r = WikipediaRenderer::new();
    r.prepare(doc, 64, &text_settings()).unwrap();
    for offset in (0..r.total_rows()).step_by(16) {
        r.render(32, f64::from(offset)).unwrap();
        assert!(r.strips.len() <= MAX_STRIPS);
        assert!(r.cached_strip_bytes() <= MAX_STRIP_BYTES);
        assert_eq!(
            r.cached_strip_bytes(),
            r.strips
                .iter()
                .map(|s| s.dots.capacity() * std::mem::size_of::<DotInk>())
                .sum::<usize>()
        );
    }
}
#[test]
fn braille_uses_the_real_font_and_changes_with_zoom() {
    let doc = Arc::new(document(vec![Block::Paragraph(vec![span(
        "Real glyph rasterization",
    )])]));
    let mut r = WikipediaRenderer::new();
    let mut s = WikipediaSettings::default();
    r.prepare(Arc::clone(&doc), 140, &s).unwrap();
    let first = r.render(40, 0.0).unwrap().to_vec();
    assert!(first.iter().any(RenderCell::is_ink));
    assert!(first.iter().any(|c| !c.is_ink()));
    assert!(first.iter().filter(|c| c.is_ink()).all(|c| c
        .symbol()
        .chars()
        .all(|g| ('\u{2801}'..='\u{28ff}').contains(&g))));
    let work = r.work_stats();
    r.render(40, 0.10).unwrap();
    assert_eq!(r.work_stats(), work);
    s.zoom_percent = 150;
    assert!(r.prepare(doc, 140, &s).unwrap());
    assert_ne!(r.render(40, 0.0).unwrap(), first.as_slice());
}
#[test]
fn font_headings_and_references_have_distinct_sizes_and_raised_reference_baselines() {
    let mut fonts = Fonts::new().unwrap();
    let mut stats = LayoutStats::default();
    let mut reference = span("12");
    reference.superscript = true;
    let rich = Rich::from_spans(
        &[span("body"), reference],
        RenderMode::Braille,
        Role::Body,
        &mut stats,
    );
    let lines = fonts.lines(&rich, 500.0, 32.0).unwrap();
    let Paint::Font(g) = &lines[0].paint else {
        panic!("font");
    };
    let ordinary = g.iter().find(|g| !g.style.superscript).unwrap();
    let reference = g.iter().find(|g| g.style.superscript).unwrap();
    assert!(reference.glyph.font_size < ordinary.glyph.font_size);
    assert!(reference.baseline < ordinary.baseline);
    let heading = fonts.lines(&rich, 500.0, 48.0).unwrap();
    let Paint::Font(h) = &heading[0].paint else {
        panic!("font");
    };
    assert!(h[0].glyph.font_size > ordinary.glyph.font_size);
}
#[test]
fn braille_word_wrap_ignores_only_invisible_trailing_space_overhang() {
    let mut fonts = Fonts::new().unwrap();
    let rich = Rich::from_spans(
        &[
            span("Howaldtswerke"),
            span(" (1) "),
            span("\n"),
            span("Following line keeps its source characters."),
        ],
        RenderMode::Braille,
        Role::Body,
        &mut LayoutStats::default(),
    );
    let joined: String = rich.parts.iter().map(|part| part.text.as_str()).collect();
    let drafts = fonts.lines(&rich, 282.0, 27.2).unwrap();
    assert!(drafts.len() >= 2);
    assert_eq!(drafts.last().unwrap().end, joined.len());
    assert!(drafts.windows(2).all(|pair| pair[0].end <= pair[1].end));
}
#[test]
fn regular_bold_and_italic_font_rasters_are_distinct_without_extra_font_files() {
    let doc = document(Vec::new());
    let render = |bold, italic| {
        let mut fonts = Some(Fonts::new().unwrap());
        let mut stats = LayoutStats::default();
        let mut s = span("Native");
        s.bold = bold;
        s.italic = italic;
        let rich = Rich::from_spans(&[s], RenderMode::Braille, Role::Body, &mut stats);
        let draft = fonts
            .as_mut()
            .unwrap()
            .lines(&rich, 350.0, 32.0)
            .unwrap()
            .remove(0);
        let mut l = Layout::default();
        l.push(Item {
            rect: Rect {
                x: 16.0,
                y: 16.0,
                w: 360.0,
                h: draft.height,
            },
            paint: draft.paint,
        })
        .unwrap();
        l.finish().unwrap();
        raster_strip(
            &mut l,
            &doc,
            &mut fonts,
            48,
            WikipediaSettings::default(),
            0,
        )
        .unwrap()
        .dots
        .into_iter()
        .map(|d| d.alpha)
        .collect::<Vec<_>>()
    };
    let normal = render(false, false);
    assert_ne!(normal, render(true, false));
    assert_ne!(normal, render(false, true));
}
#[test]
fn installed_cjk_font_fallback_shapes_real_braille_glyphs() {
    let mut fonts = Fonts::new().unwrap();
    // Portable test: exercise the fallback when the host actually supplies a
    // CJK face. On the acceptance host this branch is required to run.
    if !fonts.system.db().faces().any(|face| {
        face.families
            .iter()
            .any(|(family, _)| family.contains("CJK"))
    }) {
        return;
    }
    let rich = Rich::from_spans(
        &[span("日本語中文")],
        RenderMode::Braille,
        Role::Body,
        &mut LayoutStats::default(),
    );
    let lines = fonts.lines(&rich, 500.0, 32.0).unwrap();
    let ids: Vec<_> = lines
        .iter()
        .flat_map(|line| match &line.paint {
            Paint::Font(glyphs) => glyphs
                .iter()
                .map(|glyph| glyph.glyph.glyph_id)
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    assert!(!ids.is_empty());
    assert!(
        ids.iter().all(|id| *id != 0),
        "system CJK fallback must shape real glyphs"
    );
}
#[test]
#[ignore = "renderer-only visual fixture; run with --ignored --nocapture on the primary"]
fn wikipedia_renderer_preview() {
    for mode in [RenderMode::Text, RenderMode::Braille] {
        for columns in [80, 140] {
            let mut doc = with_image(ImageFloat::Right, [200, 90, 30, 255]);
            doc.blocks.insert(
                0,
                Block::Table {
                    infobox: true,
                    spans: Vec::new(),
                    images: Vec::new(),
                    rows: vec![
                        vec![vec![span("Type")], vec![span("Semantic article")]],
                        vec![vec![span("Source")], vec![span("Frozen fixture")]],
                    ],
                },
            );
            let mut bold = span("Bold introduction");
            bold.bold = true;
            let mut italic = span(" with italic text");
            italic.italic = true;
            let mut reference = span("[12]");
            reference.superscript = true;
            reference.link = true;
            doc.blocks
                .push(Block::Paragraph(vec![bold, italic, reference]));
            doc.blocks.push(Block::Heading {
                level: 2,
                spans: vec![span("A real heading")],
            });
            for _ in 0..8 {
                doc.blocks.push(Block::Paragraph(vec![span(
                    &"Complete flowing article words. ".repeat(6),
                )]));
            }
            let doc = Arc::new(doc);
            for (grey, palette, zoom, offset) in [
                (true, Palette::Wikipedia, 100, 0.0),
                (false, Palette::Pastel, 150, 8.0),
            ] {
                let settings = WikipediaSettings {
                    render_mode: mode,
                    greyscale: grey,
                    palette,
                    zoom_percent: zoom,
                    ..WikipediaSettings::default()
                };
                let mut r = WikipediaRenderer::new();
                r.prepare(Arc::clone(&doc), columns, &settings).unwrap();
                println!("\n--- renderer only: {mode:?} {columns}x40 grey={grey} {palette:?} zoom={zoom} offset={offset} ---");
                let cells = r.render(40, offset).unwrap();
                for row in cells.chunks(usize::from(columns)) {
                    for cell in row {
                        let [r, g, b] = cell.rgb();
                        print!(
                            "\x1b[0;38;2;{r};{g};{b}m{}{}{}",
                            if cell.bold() { "\x1b[1m" } else { "" },
                            if cell.italic() { "\x1b[3m" } else { "" },
                            cell.symbol()
                        );
                    }
                    println!("\x1b[0m");
                }
            }
        }
    }
}

#[test]
fn a_cancelled_layout_is_blank_and_can_be_retried_without_reusing_failure() {
    let mut r = WikipediaRenderer::new();
    let doc = Arc::new(document(Vec::new()));
    let stop = AtomicBool::new(true);
    assert_eq!(
        r.prepare_cancellable(Arc::clone(&doc), 80, &text_settings(), &stop),
        Err(RenderError::Cancelled)
    );
    assert!(r.cells().is_empty());
    assert!(r.document.is_none());
    stop.store(false, Ordering::Relaxed);
    assert!(r
        .prepare_cancellable(doc, 80, &text_settings(), &stop)
        .unwrap());
}
#[test]
fn renderer_can_be_transferred_to_an_owned_worker() {
    fn assert_send<T: Send>() {}
    assert_send::<WikipediaRenderer>();
}
#[test]
fn settings_serde_defaults_round_trip_and_unknown_key_tolerance() {
    let default: WikipediaSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(default, WikipediaSettings::default());
    let selected = WikipediaSettings {
        render_mode: RenderMode::Text,
        palette: Palette::Pastel,
        greyscale: false,
        zoom_percent: 150,
        ..default
    };
    let restored: WikipediaSettings =
        serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
    assert_eq!(restored, selected);
    // Unknown keys are tolerated when reading; this type does not promise to round-trip them.
    let future: WikipediaSettings =
        serde_json::from_str(r#"{"render_mode":"text","future_field":true}"#).unwrap();
    assert_eq!(future.render_mode, RenderMode::Text);
}
