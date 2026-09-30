//! JSONL probe for exact render and standalone artifact verification.
use std::io::{BufRead, BufReader};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || args[1] != "--input" {
        return Err("usage: prompt_probe --input /absolute/path/to/cases.jsonl".into());
    }
    let file = std::fs::File::open(&args[2])?;
    for line in BufReader::new(file).lines() {
        let case: Value = serde_json::from_str(&line?)?;
        let name = case["name"].as_str().ok_or("case requires name")?;
        let context = &case["context"];
        let rendered = if let Some(source) = case["baseline_source"].as_str() {
            ilium_prompts::render_source(name, source, context)?
        } else {
            ilium_prompts::render(name, context)?
        };
        let digest = format!("{:x}", Sha256::digest(rendered.as_bytes()));
        println!(
            "{}",
            json!({"type":"result", "name":name,"bytes":rendered.len(),"sha256":digest})
        );
    }
    Ok(())
}
