//! Shared helpers of the api grants tests: manual readers, the parsed capability table of
//! reference-capabilities.md, and a one-side manifest builder run through `validate_json`.


use std::path::PathBuf;

use ilium_plugin_api::manifest::{validate_json, CheckMode, Diagnostic, ValidationContext};
use ilium_plugin_api::{Platform, Side, Tier};

/// The canonical manual pages: `ILIUM_PLUGIN_MANUAL_DIR`, else `<repo>/docs/plugins-manual/pages`.
pub fn manual_dir() -> PathBuf {
    if let Some(directory) = std::env::var_os("ILIUM_PLUGIN_MANUAL_DIR") {
        return PathBuf::from(directory);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages")
}

/// Reads one manual page by file name.
pub fn manual_page(name: &str) -> String {
    let path = manual_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// A validation context for `ilium plugin check` on Linux with nothing installed.
pub fn context(tier: Tier) -> ValidationContext {
    ValidationContext {
        tier,
        mode: CheckMode::Check,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: Vec::new(),
    }
}

/// The world a minimal component of this side targets (reference-manifest.md).
pub fn world_of(side: Side) -> &'static str {
    match side {
        Side::Server => "ilium:server/base@0.1.0",
        Side::Client => "ilium:client/base@0.1.0",
        Side::Scene => "ilium:anim/base@0.1.0",
    }
}

/// `manifest.json` text with one component on `side` and the given grants on that side.
pub fn manifest_json(side: Side, grants: &[&str]) -> String {
    let key = side.as_str();
    let mut manifest = serde_json::json!({
        "api": "ilium:plugin@0.1.0",
        "id": "example.grants",
        "name": "Grants test",
        "version": "0.1.0",
        "publisher": "Example",
    });
    manifest[key] = serde_json::json!({ "entry": format!("{key}.wasm"), "world": world_of(side) });
    manifest["capabilities"] = serde_json::json!({ key: grants });
    manifest.to_string()
}

/// The diagnostics the install check gives for a manifest with these grants on `side`.
pub fn diagnostics_for(tier: Tier, side: Side, grants: &[&str]) -> Vec<Diagnostic> {
    validate_json(&manifest_json(side, grants), &context(tier))
}

/// The codes of `diagnostics_for`, in order.
pub fn codes_for(tier: Tier, side: Side, grants: &[&str]) -> Vec<String> {
    diagnostics_for(tier, side, grants)
        .iter()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
        .collect()
}

/// Whether the check reports `code` for these grants.
pub fn reports(tier: Tier, side: Side, grants: &[&str], code: &str) -> bool {
    codes_for(tier, side, grants)
        .iter()
        .any(|found| found == code)
}

/// Whether the check reports any `manifest.grant-*` code for these grants.
pub fn reports_any_grant_code(tier: Tier, side: Side, grants: &[&str]) -> bool {
    codes_for(tier, side, grants)
        .iter()
        .any(|found| found.starts_with("manifest.grant-"))
}

/// One row of the capability table of reference-capabilities.md.
#[derive(Debug, Clone)]
pub struct TableRow {
    /// Grant families of the row (text before the first `:` of each backticked form).
    pub families: Vec<String>,
    /// The backticked forms as written, for example `fs.read:<scope>`.
    pub forms: Vec<String>,
    /// Sides in the `Sides` column.
    pub sides: Vec<Side>,
    /// The `Risk` column with `**` removed.
    pub risk_text: String,
}

/// The rows between `## Table` and `### Newer grants`.
pub fn table_rows() -> Vec<TableRow> {
    let page = manual_page("reference-capabilities.md");
    let section = page
        .split("\n## Table\n")
        .nth(1)
        .and_then(|rest| rest.split("### Newer grants").next())
        .expect("the capability table section");
    let mut rows = Vec::new();
    for line in section.lines().filter(|line| line.starts_with("| `")) {
        let columns: Vec<&str> = line.split('|').map(str::trim).collect();
        let forms: Vec<String> = columns[1]
            .split('`')
            .enumerate()
            .filter(|(index, _)| index % 2 == 1)
            .map(|(_, form)| form.to_owned())
            .collect();
        let families = forms
            .iter()
            .map(|form| form.split(':').next().unwrap_or("").to_owned())
            .collect();
        let sides = columns[3]
            .split_whitespace()
            .filter_map(|letter| match letter {
                "S" => Some(Side::Server),
                "C" => Some(Side::Client),
                "A" => Some(Side::Scene),
                _ => None,
            })
            .collect();
        rows.push(TableRow {
            families,
            forms,
            sides,
            risk_text: columns[4].replace("**", ""),
        });
    }
    rows
}

/// A grant string with a valid-looking argument for a family, for `check_grant` matrix tests.
pub fn sample_grant(family: &str) -> String {
    let argument = match family {
        "commands.invoke" => "ilium.git/status",
        "fs.read" | "fs.write" | "fs.watch" => "project",
        "http" => "api.example.com",
        "process.run" | "panes.create-command" => "git",
        "topics.subscribe" => "ilium.progress/*",
        "services.use" | "services.provide" => "com.example.git/status",
        "messages.peer" => "com.example.logger",
        "http-api.route" => "/notes/{id}",
        "settings.contribute" | "settings.read-kernel" | "settings.write-kernel" => "api",
        "extensions.provide" | "extensions.contribute" => "com.example.notes/status",
        _ => return family.to_owned(),
    };
    format!("{family}:{argument}")
}

/// Families that only a bundled (T1) plugin may request (reference-capabilities.md).
pub const T1_ONLY: [&str; 4] = [
    "settings.read-all",
    "settings.write-all",
    "settings.write-kernel",
    "plugins.manage",
];

/// Families outside the tier/side matrix: refused or gated for reasons other than tier and side.
pub const MATRIX_EXCLUDED: [&str; 2] = ["editor.provide", "gpu.kernels"];
