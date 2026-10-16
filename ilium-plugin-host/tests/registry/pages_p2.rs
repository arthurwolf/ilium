//! Helpers of part 2 of lane `registry-publish`: manual rows, requirement-table rows, policy
//! files and packages with extra signed entries. Shares `common` with part 1.

use crate::common::*;
use ilium_plugin_host::cli::CliOutput;
use ilium_plugin_host::package::write_package;
use std::path::Path;

/// The first line of a manual page that carries the marker `[ID]`.
pub fn row(page: &str, id: &str) -> String {
    let text = page_text(page);
    let tag = format!("[{id}]");
    text.lines().find(|line| line.contains(&tag)).unwrap_or_else(|| panic!("row {id} is missing on {page}")).to_owned()
}

/// Asserts that the page row of `id` contains every part.
pub fn assert_row(page: &str, id: &str, parts: &[&str]) {
    let line = row(page, id);
    for part in parts {
        assert!(line.contains(part), "missing {part:?} in {line}");
    }
}

/// The requirement-table row `| ID | ...` from any `reference-requirements-*.md` page.
pub fn traced_row(id: &str) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("manual pages folder")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("reference-requirements-") && name.ends_with(".md"))
        .collect();
    names.sort();
    let prefix = format!("| {id} |");
    for name in names {
        let text = page_text(&name);
        if let Some(line) = text.lines().find(|line| line.starts_with(&prefix)) {
            return line.to_owned();
        }
    }
    panic!("requirement row {id} is missing in the requirement tables");
}

/// Asserts that the requirement table cites the page `slug` and the keyword for `id`.
pub fn assert_traced(id: &str, slug: &str, keyword: &str) {
    let line = traced_row(id);
    assert!(line.contains(&format!("/pages/{slug}/")), "{id} must cite {slug}: {line}");
    assert!(line.contains(keyword), "{id} must use {keyword}: {line}");
}

/// Writes the user policy file of the sandbox.
pub fn write_policy(sandbox: &Sandbox, text: &str) {
    std::fs::write(sandbox.config_dir().join("policy.toml"), text).expect("write policy.toml");
}

/// A policy refusal: exit 7 and the detail code in the records (policy page, "Errors").
pub fn assert_policy_block(output: &CliOutput, detail: &str) {
    assert_eq!(output.exit_code, 7, "policy refusals exit 7: {output:?}");
    assert!(output_contains(output, detail), "expected detail {detail}: {output:?}");
}

/// `install com.acme.notes --yes --trust <publisher key> <extra...>`.
pub fn install_notes(sandbox: &Sandbox, extra: &[&str]) -> CliOutput {
    let key = publisher_key_id();
    let mut args = vec!["install", "com.acme.notes", "--yes", "--trust", key.as_str()];
    args.extend_from_slice(extra);
    sandbox.run(&args)
}

/// A package signed by `seed` as `publisher_id` that lists `extra` entries in `files`.
pub fn package_with_entries(plugin_id: &str, version: &str, extra: &[(&str, Vec<u8>)], seed: u8, publisher_id: &str) -> Vec<u8> {
    let mut manifest = manifest_text(plugin_id, version);
    let tail = "\n  ]\n}\n";
    let cut = manifest.rfind(tail).expect("files array tail of the fixture manifest");
    let mut entries = String::new();
    for (path, bytes) in extra {
        entries.push_str(&format!(
            ",\n    {{\n      \"path\": \"{path}\",\n      \"length\": {},\n      \"sha256\": \"{}\"\n    }}",
            bytes.len(),
            sha256_hex(bytes)
        ));
    }
    manifest.insert_str(cut, &entries);
    let manifest = manifest.into_bytes();
    let signature = signature_entry(&manifest, seed, publisher_id).into_bytes();
    let mut files = vec![
        ("manifest.json".to_owned(), manifest),
        ("server.wasm".to_owned(), unhex(COMPONENT_HEX)),
        ("signature".to_owned(), signature),
    ];
    for (path, bytes) in extra {
        files.push(((*path).to_owned(), bytes.clone()));
    }
    write_package(&files).expect("write package with extra entries")
}

/// `verify <file>` on package bytes written into the project folder.
pub fn verify_bytes(sandbox: &Sandbox, bytes: &[u8]) -> CliOutput {
    let path = sandbox.project_file("candidate.iliumpkg", bytes);
    sandbox.run(&["verify", path.to_str().expect("utf-8 path")])
}

/// True when the error codes of the output contain `code`.
pub fn has_error(output: &CliOutput, code: &str) -> bool {
    error_codes(output).iter().any(|found| found == code)
}

/// The three policy files the administration tests need, as TOML text.
pub fn policy_with(packages: &str, extra: &str) -> String {
    format!("version = 1\n[packages]\n{packages}\n{extra}\n")
}
