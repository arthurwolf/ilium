//! Parse saved, provenance-tracked source without making network requests.
use std::path::PathBuf;

use ilium_wikipedia::{main_page_titles, parse_article, Block};

fn main() {
    if let Err(error) = run() {
        println!("{}", serde_json::json!({"type": "error", "error": error}));
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut path = None;
    let mut title = None;
    let mut date = None;
    let mut main_page = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--html" => path = arguments.next().map(PathBuf::from),
            "--title" => title = arguments.next(),
            "--date" => date = arguments.next(),
            "--main-page" => main_page = true,
            _ => return Err(
                "usage: source_probe --html PATH [--main-page | --title TITLE --date YYYY-MM-DD]"
                    .into(),
            ),
        }
    }
    let path = path.ok_or("--html PATH is required")?;
    let html = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    if main_page {
        println!(
            "{}",
            serde_json::json!({"type": "result", "source_path": path, "titles": main_page_titles(&html)?})
        );
        return Ok(());
    }
    let title = title.ok_or("--title is required")?;
    let date = date.ok_or("--date is required")?;
    let document = parse_article(&title, "https://en.wikipedia.org/", &date, &html)?;
    let count = |predicate: fn(&Block) -> bool| {
        document
            .blocks
            .iter()
            .filter(|block| predicate(block))
            .count()
    };
    println!(
        "{}",
        serde_json::json!({
            "type": "result", "source_path": path, "title": document.title,
            "revision": document.revision, "date": document.date,
            "blocks": document.blocks.len(),
            "paragraphs": count(|b| matches!(b, Block::Paragraph(_))),
            "headings": count(|b| matches!(b, Block::Heading { .. })),
            "lists": count(|b| matches!(b, Block::List { .. })),
            "tables": count(|b| matches!(b, Block::Table { .. })),
            "infoboxes": count(|b| matches!(b, Block::Table { infobox: true, .. })),
            "images": document.blocks.iter().map(|block| match block { Block::Image(_) => 1, Block::Table { images, .. } => images.len(), _ => 0 }).sum::<usize>()
        })
    );
    Ok(())
}
