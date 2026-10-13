//! The wit/ tree against the manual and the generated code: ABI-001, ABI-003, ABI-005, ABI-016,
//! plus drift checks of the generated files. Doc lints: pure data, they pass against the skeleton.

use std::collections::BTreeMap;

use ilium_plugin_api::generated;

use crate::support::{
    crate_dir, definitions, manual_page, manual_pages, read, sha256_hex, strip_comments,
    wit_blocks, wit_dir, wit_files, wit_tree_sha256, Definition,
};

const GENERATED_FILES: [&str; 6] = [
    "mod.rs",
    "types.rs",
    "host.rs",
    "primitives.rs",
    "anim.rs",
    "exports.rs",
];

fn extracted_from() -> BTreeMap<String, String> {
    read(&wit_dir().join("EXTRACTED_FROM"))
        .lines()
        .filter_map(|line| line.split_once(": "))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// REQ: ABI-001
#[test]
fn doc_abi_001_wit_tree_was_extracted_from_the_current_reference_wit() {
    let recorded = extracted_from();
    let page = read(&crate::support::manual_dir().join("reference-wit.md"));
    assert_eq!(
        recorded.get("sha256").map(String::as_str),
        Some(sha256_hex(page.as_bytes()).as_str()),
        "wit/ is stale: rerun python3 tools/extract_wit.py"
    );
    assert_eq!(recorded.get("blocks").map(String::as_str), Some("83"));
}

/// REQ: ABI-001
#[test]
fn doc_abi_001_every_wit_line_is_in_reference_wit() {
    let page = manual_page("reference-wit.md");
    let page_lines: std::collections::HashSet<&str> = page.lines().map(str::trim).collect();
    let mut missing = Vec::new();
    for (relative, text) in wit_files() {
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with("package ") {
                continue;
            }
            if !page_lines.contains(line) {
                missing.push(format!("{relative}: {line}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "lines not in reference-wit.md: {missing:#?}"
    );
}

/// REQ: ABI-001
#[test]
fn doc_abi_001_wit_tree_has_the_expected_files() {
    let files = wit_files();
    let count = |prefix: &str| {
        files
            .iter()
            .filter(|(relative, _)| relative.starts_with(prefix))
            .count()
    };
    assert_eq!(count("types/"), 1);
    assert_eq!(count("host/"), 39);
    assert_eq!(count("primitives/"), 4);
    assert_eq!(count("anim/"), 3);
    assert_eq!(count("exports/"), 16);
    assert_eq!(count("worlds/"), 3);
}

/// REQ: ABI-001
#[test]
fn doc_abi_001_generated_headers_match_the_wit_tree() {
    let tree = wit_tree_sha256();
    assert_eq!(
        generated::WIT_SHA256,
        tree,
        "rerun python3 tools/gen_plugin_api.py"
    );
    for file in GENERATED_FILES {
        let text = read(&crate_dir().join("src/generated").join(file));
        let first = text.lines().next().unwrap_or("");
        assert_eq!(
            first,
            format!("// @generated from wit/ sha256={tree}"),
            "src/generated/{file} header"
        );
    }
}

/// REQ: ABI-001
#[test]
fn doc_abi_001_generated_files_match_the_golden_hashes() {
    let golden = read(&crate_dir().join("tests/abi/golden/generated.sha256"));
    let mut checked = 0;
    for line in golden.lines().filter(|line| !line.trim().is_empty()) {
        let (hash, relative) = line
            .split_once("  ")
            .expect("golden line: <sha256>  <path>");
        let bytes = std::fs::read(crate_dir().join(relative)).expect("generated file");
        assert_eq!(sha256_hex(&bytes), hash, "{relative} was edited by hand");
        checked += 1;
    }
    assert_eq!(checked, GENERATED_FILES.len());
}

/// REQ: ABI-001
#[test]
fn doc_abi_001_sdk_core_generated_copy_matches_the_api_copy() {
    let sdk_directory = crate_dir().join("../ilium-plugin-sdk-core/src/generated");
    for file in GENERATED_FILES {
        let api_text = read(&crate_dir().join("src/generated").join(file));
        let sdk_text = read(&sdk_directory.join(file));
        // Line 2 names the output path; it is the only permitted difference.
        let without_output_line = |text: &str| -> Vec<String> {
            text.lines()
                .enumerate()
                .filter(|(index, _)| *index != 1)
                .map(|(_, line)| line.to_owned())
                .collect()
        };
        assert_eq!(
            without_output_line(&api_text),
            without_output_line(&sdk_text),
            "{file} differs between the crates"
        );
    }
}

/// REQ: ABI-003
#[test]
fn doc_abi_003_every_package_line_is_version_0_1_0() {
    let mut packages = std::collections::BTreeSet::new();
    for (relative, text) in wit_files() {
        let line = text
            .lines()
            .find(|line| line.starts_with("package "))
            .unwrap_or_else(|| panic!("{relative} has no package line"));
        assert!(line.ends_with("@0.1.0;"), "{relative}: {line}");
        packages.insert(
            line.trim_start_matches("package ")
                .trim_end_matches(';')
                .to_owned(),
        );
    }
    assert_eq!(packages.len(), 7, "{packages:?}");
}

/// REQ: ABI-003
#[test]
fn doc_abi_003_every_ilium_reference_names_0_1_0() {
    let mut wrong = Vec::new();
    for (relative, text) in wit_files() {
        let text = strip_comments(&text);
        for (offset, _) in text.match_indices("ilium:") {
            let reference: String = text[offset..]
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ';' && *c != ',' && *c != '{')
                .collect();
            if let Some((_, version)) = reference.split_once('@') {
                let version = version.trim_end_matches('.');
                if !version.starts_with("0.1.0") {
                    wrong.push(format!("{relative}: {reference}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// REQ: ABI-003
#[test]
fn doc_abi_003_generated_packages_list_seven_at_0_1_0() {
    assert_eq!(generated::PACKAGES.len(), 7);
    assert!(generated::PACKAGES
        .iter()
        .all(|package| package.ends_with("@0.1.0")));
}

fn canonical_definitions() -> BTreeMap<(String, String), Vec<Definition>> {
    let mut found: BTreeMap<(String, String), Vec<Definition>> = BTreeMap::new();
    for (_, text) in wit_files() {
        for definition in definitions(&text) {
            found
                .entry((definition.kind.clone(), definition.name.clone()))
                .or_default()
                .push(definition);
        }
    }
    found
}

/// REQ: ABI-005
#[test]
fn doc_abi_005_wit_excerpts_on_other_pages_match_reference_wit() {
    let canonical = canonical_definitions();
    let mut drift = Vec::new();
    let mut checked = 0;
    for (page_name, page) in manual_pages() {
        if page_name == "reference-wit.md" {
            continue;
        }
        for block in wit_blocks(&page) {
            for excerpt in definitions(&block) {
                checked += 1;
                let key = (excerpt.kind.clone(), excerpt.name.clone());
                let Some(candidates) = canonical.get(&key) else {
                    drift.push(format!(
                        "{page_name}: {} {} is not declared with that kind in reference-wit.md",
                        excerpt.kind, excerpt.name
                    ));
                    continue;
                };
                let matches = candidates.iter().any(|candidate| {
                    excerpt
                        .members
                        .iter()
                        .all(|member| candidate.members.contains(member))
                });
                if !matches {
                    drift.push(format!(
                        "{page_name}: {} {} has members {:?}, reference-wit.md has {:?}",
                        excerpt.kind, excerpt.name, excerpt.members, candidates[0].members
                    ));
                }
            }
        }
    }
    assert!(checked > 200, "only {checked} excerpt definitions found");
    assert!(drift.is_empty(), "excerpt drift: {drift:#?}");
}

/// REQ: ABI-016
#[test]
fn doc_abi_016_wit_names_are_kebab_case() {
    let is_kebab = |word: &str| {
        let mut previous_dash = true;
        word.chars().enumerate().all(|(index, c)| {
            let ok = match c {
                'a'..='z' => true,
                '0'..='9' => index > 0,
                '-' => !previous_dash,
                _ => false,
            };
            previous_dash = c == '-';
            ok
        }) && !word.ends_with('-')
    };
    let mut wrong = Vec::new();
    for (relative, text) in wit_files() {
        let text = strip_comments(&text);
        let words =
            text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '%'));
        for word in words {
            let word = word.trim_start_matches('%');
            if word.is_empty() || word == "_" || word.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if !is_kebab(word) {
                wrong.push(format!("{relative}: {word}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
