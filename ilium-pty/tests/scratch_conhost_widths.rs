//! SCRATCH DIAGNOSTIC (never on the release branch): measure conhost's cell
//! width for every glyph the Ilium client draws, and compare it with the
//! width the client (unicode-width) and the vt100 parser assume.
#![cfg(windows)]

use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn census_of_conhost_glyph_widths() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let glyphs = manifest.join("tests/scratch/glyphs.txt");
    let script = manifest.join("tests/scratch/census.ps1");
    let out = std::env::temp_dir().join("ilium-conhost-census.tsv");
    let _ = std::fs::remove_file(&out);
    let command = ilium_pty::PtyCommand::new("pwsh.exe", manifest, 10, 80)
        .arg("-NoProfile")
        .arg("-File")
        .arg(script.to_string_lossy().to_string())
        .arg("-Glyphs")
        .arg(glyphs.to_string_lossy().to_string())
        .arg("-Out")
        .arg(out.to_string_lossy().to_string());
    let mut session = ilium_pty::PtySession::spawn(command).expect("spawn pwsh under ConPTY");
    let deadline = Instant::now() + Duration::from_secs(120);
    while !session.has_exited() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let report = std::fs::read_to_string(&out).expect("census output");
    println!("=== conhost width census (codepoints, conhost, unicode-width) ===");
    for line in report.lines() {
        let (codes, column) = line.split_once('\t').unwrap_or((line, "?"));
        let text: String = codes
            .split(' ')
            .filter_map(|code| u32::from_str_radix(code.trim_start_matches("U+"), 16).ok())
            .filter_map(char::from_u32)
            .collect();
        let client = unicode_width::UnicodeWidthStr::width(text.as_str());
        let parser: usize = text
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        let marker = if column != client.to_string() || column != parser.to_string() {
            "MISMATCH"
        } else {
            ""
        };
        println!("{codes}\t{text}\tconhost={column}\tclient={client}\tvt100={parser}\t{marker}");
    }
}
