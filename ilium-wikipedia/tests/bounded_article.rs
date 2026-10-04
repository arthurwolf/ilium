use ilium_wikipedia::{parse_article, parse_article_bounded, ArticleLimits};
fn bounded(html: &str, limits: ArticleLimits) -> Result<ilium_wikipedia::Document, String> {
    parse_article_bounded(
        "Fixture",
        "https://en.wikipedia.org/wiki/Fixture",
        "2026-10-03",
        html,
        limits,
    )
}
#[test]
fn dense_empty_elements_reject_without_truncated_article_publication() {
    let html = format!("<body><p>Kept</p>{}</body>", "<div></div>".repeat(1000));
    assert!(parse_article("Fixture", "url", "date", &html).is_ok());
    let error = bounded(
        &html,
        ArticleLimits {
            dom_nodes: 64,
            ..ArticleLimits::default()
        },
    )
    .unwrap_err();
    assert!(error.contains("DOM"));
}
#[test]
fn oversized_single_tag_is_rejected_before_attribute_materialization() {
    let attrs = (0..1000)
        .map(|index| format!(" a{index}='value' "))
        .collect::<String>();
    let html = format!("<body><p{attrs}>Text</p></body>");
    assert!(parse_article("Fixture", "url", "date", &html).is_ok());
    assert!(bounded(
        &html,
        ArticleLimits {
            tag_bytes: 512,
            ..ArticleLimits::default()
        }
    )
    .unwrap_err()
    .contains("tokenizer"));
}
#[test]
fn total_normalized_cells_are_capped_across_multiple_tables_before_padding() {
    let table = format!(
        "<table><tr><td colspan=5>X</td></tr>{}</table>",
        "<tr><td>Y</td></tr>".repeat(9)
    );
    let html = format!("<body>{table}{table}</body>");
    assert!(parse_article("Fixture", "url", "date", &html).is_ok());
    assert!(bounded(
        &html,
        ArticleLimits {
            table_cells: 50,
            ..ArticleLimits::default()
        }
    )
    .unwrap_err()
    .contains("normalized table"));
}
#[test]
fn reconstructed_formatting_fostered_tables_entities_and_templates_keep_native_semantics() {
    for html in [
        "<body><p><b>one<i>two</b>three</i></p></body>",
        "<body><table>Outside<tr><td>&amp; &#128512; text</table><p>After</p></body>",
        "<body><template><p>Hidden</p></template><!-- comment --><p>Visible</p></body>",
        "<body><svg><title>Shape</title></svg><p class='mw-parser-output'>Ångström &#x1f600;</p></body>",
    ] {
        let native=parse_article("Fixture","url","date",html).unwrap();
        let limited=bounded(html,ArticleLimits::default()).unwrap();
        assert_eq!(limited.blocks,native.blocks);
        assert_eq!(limited.revision,native.revision);
    }
}
#[test]
fn chunked_text_coalesces_and_budget_errors_do_not_publish_partial_documents() {
    let html = format!("<body><p>{}</p></body>", "word ".repeat(400));
    let native = parse_article("Fixture", "url", "date", &html).unwrap();
    assert_eq!(
        bounded(&html, ArticleLimits::default()).unwrap().blocks,
        native.blocks
    );
    assert!(bounded(
        &html,
        ArticleLimits {
            dom_bytes: 100,
            ..ArticleLimits::default()
        }
    )
    .is_err());
}

#[test]
fn shared_caption_amplification_is_rejected_before_reference_copies() {
    let caption = "x".repeat(65536);
    let images = "<img src='https://upload.wikimedia.org/fixture.png'>".repeat(65);
    let html = format!("<body><figure>{images}<figcaption>{caption}</figcaption></figure></body>");
    assert!(bounded(&html, ArticleLimits::default())
        .unwrap_err()
        .contains("repeated caption"));
}
