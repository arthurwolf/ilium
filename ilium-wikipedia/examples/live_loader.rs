//! Bounded opt-in live source proof. Normal unit tests never use the network.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ilium_wikipedia::{LoaderEvent, PageLoader};

fn main() {
    if let Err(error) = run() {
        println!("{}", serde_json::json!({"type": "error", "error": error}));
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut cache_dir = None;
    let mut deadline_seconds = 90_u64;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--cache-dir" => cache_dir = arguments.next().map(PathBuf::from),
            "--deadline-seconds" => {
                deadline_seconds = arguments
                    .next()
                    .ok_or("missing deadline")?
                    .parse()
                    .map_err(|_| "invalid deadline")?
            }
            _ => {
                return Err(
                    "usage: live_loader --cache-dir PATH [--deadline-seconds 1..300]".into(),
                )
            }
        }
    }
    if !(1..=300).contains(&deadline_seconds) {
        return Err("deadline must be 1..300 seconds".into());
    }
    let loader = PageLoader::start(cache_dir.ok_or("--cache-dir is required")?)?;
    if !loader.request_next() {
        return Err("Wikipedia request was not admitted".into());
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(deadline_seconds) {
        match loader.try_recv() {
            Some(LoaderEvent::Loaded(document)) => {
                println!(
                    "{}",
                    serde_json::json!({"type":"result", "title":document.title, "url":document.url, "revision":document.revision, "date":document.date, "blocks":document.blocks.len(), "images":document.images.len(), "warnings":document.warnings})
                );
                return Ok(());
            }
            Some(LoaderEvent::Status(message)) => println!(
                "{}",
                serde_json::json!({"type":"progress", "message":message})
            ),
            Some(LoaderEvent::Failed(error)) => return Err(error),
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    Err("live Wikipedia loader deadline exceeded; owned worker cancellation requested".into())
}
