use super::*;
use ilium_wikipedia::{Block, ImageRef, Span};
use serde_json::json;

fn valid_text(text: &str, limit: usize) -> Result<()> {
    if text.trim().is_empty() || text.len() > limit || text.chars().any(char::is_control) {
        return types::fail("invalid document query");
    }
    Ok(())
}
fn featured<C: BrokerSourceClient>(client: &mut C, stop: &AtomicBool) -> Result<Value> {
    let mut event = None;
    let response = client.stream_lines(
        &http_options("https://lichess.org/api/tv/feed".into(), 256 * 1024, "text"),
        stop,
        16_384,
        128,
        &mut |line| {
            cancelled(stop)?;
            if line.iter().all(u8::is_ascii_whitespace) {
                return Ok(true);
            }
            let value: Value = serde_json::from_slice(line)?;
            if value["t"] != "featured" {
                return Ok(true);
            }
            let data = &value["d"];
            let id = data["id"]
                .as_str()
                .ok_or_else(|| AnimationError::Runtime("TV event missing game id".into()))?;
            if id.is_empty()
                || id.len() > 32
                || !id.bytes().all(|byte| byte.is_ascii_alphanumeric())
            {
                return types::fail("invalid TV game id");
            }
            let fen = data["fen"]
                .as_str()
                .ok_or_else(|| AnimationError::Runtime("TV event missing FEN".into()))?;
            ilium_ambient::live_chess::position::ChessPosition::from_fen(fen)
                .map_err(AnimationError::Runtime)?;
            event = Some(data.clone());
            Ok(false)
        },
    )?;
    if !(200..=299).contains(&response.status) {
        return types::fail("TV feed HTTP failure");
    }
    event.ok_or_else(|| AnimationError::Runtime("no featured game in bounded TV feed".into()))
}
pub fn discover_chess<C: BrokerSourceClient>(
    client: &mut C,
    max_games: usize,
    stop: &AtomicBool,
) -> Result<Value> {
    if !(1..=32).contains(&max_games) {
        return types::fail("chess discovery budget");
    }
    let event = featured(client, stop)?;
    Ok(
        json!([{"id":event["id"],"fen":event["fen"],"players":event["players"],"scope":"current_lichess_tv","attribution":"Lichess"}]),
    )
}
pub fn chess<C: BrokerSourceClient>(
    client: &mut C,
    options: &ChessOptions,
    now_ms: i64,
    revision: u64,
    stop: &AtomicBool,
) -> Result<ChessSnapshot> {
    let event = featured(client, stop)?;
    if options.game_id != "tv" && event["id"].as_str() != Some(options.game_id.as_str()) {
        return types::fail(
            "selected game is no longer the current native TV game; arbitrary-game service is unavailable",
        );
    }
    let name = |color: &str| {
        event["players"]
            .as_array()
            .and_then(|players| players.iter().find(|player| player["color"] == color))
            .and_then(|player| player["user"]["name"].as_str())
            .unwrap_or("")
            .chars()
            .take(128)
            .collect()
    };
    Ok(ChessSnapshot {
        metadata: captured(revision, now_ms, None),
        game_id: event["id"].as_str().unwrap_or_default().into(),
        fen: event["fen"].as_str().unwrap_or_default().into(),
        moves: Vec::new(),
        white: name("white"),
        black: name("black"),
        state: "current_tv_position_history_unavailable".into(),
        result: None,
        white_seconds: event["players"]
            .as_array()
            .and_then(|players| players.iter().find(|player| player["color"] == "white"))
            .and_then(|player| player["seconds"].as_u64())
            .and_then(|seconds| u32::try_from(seconds).ok()),
        black_seconds: event["players"]
            .as_array()
            .and_then(|players| players.iter().find(|player| player["color"] == "black"))
            .and_then(|player| player["seconds"].as_u64())
            .and_then(|seconds| u32::try_from(seconds).ok()),
    })
}
pub fn wiki_search<C: BrokerSourceClient>(
    client: &mut C,
    query: &str,
    max_results: usize,
    stop: &AtomicBool,
) -> Result<Value> {
    valid_text(query, 512)?;
    if !(1..=50).contains(&max_results) {
        return types::fail("Wikipedia search budget");
    }
    let mut url = url::Url::parse("https://en.wikipedia.org/w/api.php")
        .map_err(|error| AnimationError::Runtime(error.to_string()))?;
    url.query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("list", "search")
        .append_pair("srsearch", query)
        .append_pair("srlimit", &max_results.to_string())
        .append_pair("format", "json");
    let value: Value = serde_json::from_slice(&bytes(
        client,
        http_options(url.to_string(), 1024 * 1024, "text"),
        stop,
    )?)?;
    let results = value["query"]["search"]
        .as_array()
        .ok_or_else(|| AnimationError::Runtime("Wikipedia search response lacks results".into()))?;
    Ok(Value::Array(results.iter().take(max_results).filter_map(|item|item["title"].as_str().map(|title|json!({"title":title,"page_id":item["pageid"],"attribution":"Wikipedia contributors, CC BY-SA"}))).collect()))
}
fn spans(values: &[Span]) -> Value {
    Value::Array(values.iter().map(|span|json!({"text":span.text,"bold":span.bold,"italic":span.italic,"link":span.link,"superscript":span.superscript})).collect())
}
fn image(image: &ImageRef) -> Value {
    json!({"url":image.url,"caption":spans(&image.caption),"width":image.width,"height":image.height,"float":format!("{:?}",image.float).to_lowercase()})
}
fn block(block: &Block) -> Value {
    match block {
        Block::Paragraph(text) => json!({"kind":"paragraph","spans":spans(text)}),
        Block::Heading { level, spans: text } => {
            json!({"kind":"heading","level":level,"spans":spans(text)})
        }
        Block::List { ordered, items } => {
            json!({"kind":"list","ordered":ordered,"items":items.iter().map(|item|spans(item)).collect::<Vec<_>>()})
        }
        Block::Image(reference) => json!({"kind":"image","image":image(reference)}),
        Block::Table {
            rows,
            spans: geometry,
            images,
            infobox,
        } => {
            json!({"kind":"table","rows":rows.iter().map(|row|row.iter().map(|cell|spans(cell)).collect::<Vec<_>>()).collect::<Vec<_>>(),"spans":geometry.iter().map(|cell|json!({"row":cell.row,"column":cell.column,"row_span":cell.row_span,"column_span":cell.column_span})).collect::<Vec<_>>(),"images":images.iter().map(|entry|json!({"row":entry.row,"column":entry.column,"image":image(&entry.image)})).collect::<Vec<_>>(),"infobox":infobox})
        }
    }
}
// Guard JSON expansion before block()/spans() allocate per-field map nodes.
// Each span/geometry/image object reserves 8 KiB, exceeding the source JSON
// accountant's 1 KiB per-map-entry envelope for these fixed schemas. Leave
// 8 MiB of the operation's 32 MiB retained admission for outer metadata and
// the separately admitted native image-slot descriptions.
fn article_result_preflight(document: &ilium_wikipedia::Document) -> Result<()> {
    fn spans_charge(spans: &[Span]) -> usize {
        spans.iter().fold(0usize, |total, span| {
            total
                .saturating_add(8192)
                .saturating_add(span.text.capacity())
        })
    }
    fn image_charge(image: &ImageRef) -> usize {
        8192usize
            .saturating_add(image.url.capacity())
            .saturating_add(spans_charge(&image.caption))
    }
    let mut bytes = 8192usize
        .saturating_add(document.title.capacity())
        .saturating_add(document.url.capacity());
    for block in &document.blocks {
        bytes = bytes.saturating_add(8192);
        bytes = bytes.saturating_add(match block {
            Block::Paragraph(spans) | Block::Heading { spans, .. } => spans_charge(spans),
            Block::List { items, .. } => items.iter().fold(0usize, |total, spans| {
                total
                    .saturating_add(512)
                    .saturating_add(spans_charge(spans))
            }),
            Block::Image(image) => image_charge(image),
            Block::Table {
                rows,
                spans,
                images,
                ..
            } => {
                let cells = rows.iter().fold(0usize, |total, row| {
                    row.iter().fold(total.saturating_add(512), |total, cell| {
                        total.saturating_add(512).saturating_add(spans_charge(cell))
                    })
                });
                cells
                    .saturating_add(spans.len().saturating_mul(8192))
                    .saturating_add(images.iter().fold(0usize, |total, image| {
                        total
                            .saturating_add(8192)
                            .saturating_add(image_charge(&image.image))
                    }))
            }
        });
        if bytes > 24 * 1024 * 1024 {
            return Err(AnimationError::Budget(
                "Wikipedia article JSON expansion exceeds retained source budget".into(),
            ));
        }
    }
    Ok(())
}
pub fn wiki_article<C: BrokerSourceClient>(
    client: &mut C,
    title: &str,
    max_bytes: usize,
    max_images: usize,
    stop: &AtomicBool,
) -> Result<Value> {
    valid_text(title, 512)?;
    if !(1..=16 * 1024 * 1024).contains(&max_bytes) || max_images > 32 {
        return types::fail("Wikipedia article budget");
    }
    let mut url = url::Url::parse("https://en.wikipedia.org/w/rest.php/v1/page/")
        .map_err(|error| AnimationError::Runtime(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| AnimationError::Runtime("article base URL".into()))?
        .push(title)
        .push("html");
    let data = bytes(
        client,
        http_options(url.to_string(), max_bytes, "text"),
        stop,
    )?;
    let html = std::str::from_utf8(&data)
        .map_err(|_| AnimationError::Runtime("article is not UTF-8".into()))?;
    client.admit_process_baseline(
        "html5ever_atoms",
        ilium_wikipedia::ARTICLE_ATOM_BASELINE_BYTES,
    )?;
    let document = ilium_wikipedia::parse_article_bounded(
        title,
        url.as_str(),
        "",
        html,
        ilium_wikipedia::ArticleLimits::default(),
    )
    .map_err(AnimationError::Runtime)?;
    article_result_preflight(&document)?;
    // Semantic references retain placement; actual image decoding is capped
    // separately and always routes through the permission-bound host.
    let mut decoded_images = Vec::new();
    for reference in document
        .blocks
        .iter()
        .flat_map(|block| match block {
            Block::Image(reference) => vec![reference],
            Block::Table { images, .. } => images.iter().map(|entry| &entry.image).collect(),
            _ => Vec::new(),
        })
        .take(max_images)
    {
        cancelled(stop)?;
        let parsed = url::Url::parse(&reference.url)
            .map_err(|error| AnimationError::Runtime(error.to_string()))?;
        if parsed.scheme() != "https"
            || !matches!(
                parsed.host_str(),
                Some("upload.wikimedia.org" | "thumb.wikimedia.org" | "en.wikipedia.org")
            )
        {
            continue;
        }
        let data = bytes(
            client,
            http_options(parsed.to_string(), 4 * 1024 * 1024, "bytes"),
            stop,
        )?;
        let handle = client.decode_image(&data, 1024 * 1024, stop)?;
        decoded_images.push(json!({"url":reference.url,"image":handle}));
    }
    Ok(
        json!({"title":document.title,"url":document.url,"revision":document.revision,"blocks":document.blocks.iter().map(block).collect::<Vec<_>>(),"images":decoded_images,"warnings":document.warnings,"attribution":"Wikipedia contributors, CC BY-SA; individual image licenses remain attached to source URLs"}),
    )
}
