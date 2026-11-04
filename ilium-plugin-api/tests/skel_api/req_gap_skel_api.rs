//! Gap tests for the skeleton API lane: ABI-000, ABI-084, ABI-085, ABI-086, ABI-087, ABI-088 and
//! CMP-000 (reference-abi-evolution.md, reference-requirements-wit-abi.md and
//! compat-deprecation-timeline.md). Red by design: the install-stage WIT checks, the release gates
//! and the host-side delivery filter are not implemented yet, so each call reaches the closest
//! existing public entry point and asserts the outcome the requirement names.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ilium_plugin_api::abi::QueueModel;
use ilium_plugin_api::client::{Client, InstallOptions};
use ilium_plugin_api::compat::event_deliverable;
use ilium_plugin_api::deprecation::{deprecation_records, notice_is_long_enough, MarkedFeature};
use ilium_plugin_api::error::Code;
use ilium_plugin_api::generated::types::{ErrorCode, PluginError};
use ilium_plugin_api::registry::{sha256_hex, PublishRequest, Registry};
use ilium_plugin_api::version::ApiVersion;

const REGISTRY_URL: &str = "https://registry.example";
const OWNER_KEY: &str = "SHA256:owner-fixture-key";
const PLUGIN_ID: &str = "demo.plugin";
const PLUGIN_VERSION: &str = "1.0.0";
const RELEASE_NOTES_PAGE: &str = "release-notes";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn manual_page(name: &str) -> String {
    let path = repo_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{name}.md"));
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()))
}

fn wit_root() -> PathBuf {
    repo_root().join("wit")
}

/// Every `.wit` file below `dir`, recursively. The directory must exist.
fn wit_files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{} must be a readable folder: {error}", dir.display()));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.is_dir() {
            files.extend(wit_files_under(&path));
        } else if path.extension().is_some_and(|extension| extension == "wit") {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// The versions named by every `@since(version = X)` marker in `text`.
fn since_markers(text: &str) -> Vec<String> {
    text.split("@since(version = ")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .map(|version| version.trim().to_owned())
        .collect()
}

/// The minor number of a `0.N.0` text; unparsable text sorts as the largest minor.
fn minor_of(version: &str) -> u32 {
    version
        .split('.')
        .nth(1)
        .and_then(|minor| minor.parse().ok())
        .unwrap_or(u32::MAX)
}

/// The text of one `## <version>` section of the release notes, up to the next `## ` heading.
fn release_notes_section<'a>(notes: &'a str, version: &str) -> Option<&'a str> {
    let start = notes.find(&format!("## {version}"))?;
    let rest = &notes[start..];
    let end = rest[1..].find("\n## ").map_or(rest.len(), |index| index + 1);
    Some(&rest[..end])
}

/// A 64-character lowercase or uppercase hex run, as printed for a SHA-256 digest.
fn hex_digests(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_ascii_hexdigit())
        .filter(|word| word.len() == 64)
        .map(str::to_owned)
        .collect()
}

fn api_0_1_0() -> ApiVersion {
    ApiVersion {
        major: 0,
        minor: 1,
        patch: 0,
    }
}

/// A registry holding one fixture plugin whose package bytes are `package`.
fn registry_with_package(package: &[u8]) -> Registry {
    let mut registry = Registry::at(REGISTRY_URL);
    registry
        .publish(
            PublishRequest::new(PLUGIN_ID, PLUGIN_VERSION, OWNER_KEY)
                .with_package(package.to_vec()),
        )
        .expect("fixture publish by the owner key succeeds");
    registry
}

/// Installs the fixture plugin and returns the refusal. Panics when the install succeeds.
fn install_refusal(package: &[u8]) -> PluginError {
    let mut client = Client::new(vec![registry_with_package(package)]);
    match client.install(PLUGIN_ID, InstallOptions::new()) {
        Ok(_) => panic!("the install must be refused before any code loads"),
        Err(error) => error,
    }
}

// ---------------------------------------------------------------------------------------------
// ABI-000: newer cases never reach a plugin built for an older minor
// ---------------------------------------------------------------------------------------------

/// REQ: ABI-000
#[test]
fn req_abi_000_known_ui_action_still_delivered_a() {
    let plugin_api = api_0_1_0();
    assert!(
        event_deliverable("ui-action", plugin_api),
        "a ui-action known to the plugin's api minor must still be delivered"
    );
    let mut queue = QueueModel::new(8);
    queue.admit("ui-action", 1, None);
    assert_eq!(
        queue.len(),
        1,
        "the known ui-action must be queued for the plugin"
    );
}

/// REQ: ABI-000
#[test]
fn req_abi_000_newer_action_dropped_and_error_mapped_to_internal_b() {
    let plugin_api = api_0_1_0();
    let newer_action = "ui-action:future-action-kind";
    assert!(
        !event_deliverable(newer_action, plugin_api),
        "a ui-action case newer than the plugin's api minor must not be delivered"
    );
    let mut queue = QueueModel::new(8);
    queue.admit(newer_action, 1, None);
    assert_eq!(
        queue.len(),
        0,
        "the newer action must be dropped, never queued for on-events"
    );
    assert_eq!(
        queue.dropped_total(),
        0,
        "the newer action must not be counted as an events-dropped loss"
    );
    assert!(
        queue.events_dropped_records().is_empty(),
        "events-dropped must stay empty for a newer action"
    );

    let original = "ui.future-error-code";
    let mapped_base = Code::from_static(original).base();
    assert!(
        matches!(mapped_base, ErrorCode::Internal),
        "an error-code newer than the plugin's api minor must map to internal"
    );
    let mapped = PluginError::new(
        mapped_base,
        format!("{original}: unknown error code for this api minor"),
    );
    assert!(
        mapped.message.contains(original),
        "the mapped message must name the original code {original}"
    );
}

// ---------------------------------------------------------------------------------------------
// ABI-084: WIT hash comparison at install stage 3
// ---------------------------------------------------------------------------------------------

/// REQ: ABI-084
#[test]
fn req_abi_084_matching_wit_hashes_install_a() {
    let package = b"component built against ilium:host@0.1.0 with the published wit".to_vec();
    let registry = registry_with_package(&package);
    let expected = registry.sha256_of(PLUGIN_ID, PLUGIN_VERSION);
    let mut client = Client::new(vec![registry.clone()]);
    let installed = client
        .install(PLUGIN_ID, InstallOptions::new())
        .expect("a component whose embedded wit hashes match the published ones installs");
    assert_eq!(
        installed.sha256(),
        expected.as_str(),
        "the installed package must be the one whose wit hashes matched"
    );
}

/// REQ: ABI-084
#[test]
fn req_abi_084_wit_hash_difference_refused_before_load_b() {
    let package = b"component built against a wit text with one appended case".to_vec();
    let error = install_refusal(&package);
    assert!(
        error.message.starts_with("manifest.wit-mismatch: "),
        "a differing wit hash must be refused with manifest.wit-mismatch: {}",
        error.message
    );
    assert!(
        error.message.contains("ilium:host"),
        "the refusal must name the package: {}",
        error.message
    );
    assert!(
        hex_digests(&error.message).len() >= 2,
        "the refusal must name both sha256 hashes: {}",
        error.message
    );
}

// ---------------------------------------------------------------------------------------------
// ABI-085: deprecated log records for marked features
// ---------------------------------------------------------------------------------------------

fn marked(name: &str) -> MarkedFeature {
    MarkedFeature {
        name: name.to_owned(),
        marked_in_minor: 1,
        removal_minor: 3,
    }
}

/// REQ: ABI-085
#[test]
fn req_abi_085_deprecated_record_per_marked_feature_a() {
    let marked_features = [marked("host.legacy-clipboard"), marked("host.unstable-widget")];
    let records = deprecation_records(
        PLUGIN_ID,
        &["host.legacy-clipboard", "host.unstable-widget"],
        &marked_features,
        2,
    );
    assert_eq!(
        records.len(),
        2,
        "a marked feature and an unstable feature each give one deprecated record"
    );
    for record in &records {
        assert_eq!(record.plugin_id, PLUGIN_ID, "each record names the plugin");
        assert_eq!(
            record.removal_minor, 3,
            "each record carries the removal minor of its feature"
        );
    }
}

/// REQ: ABI-085
#[test]
fn req_abi_085_one_record_per_plugin_kind_and_name_b() {
    let marked_features = [marked("host.legacy-clipboard")];
    let records = deprecation_records(
        PLUGIN_ID,
        &["host.legacy-clipboard", "host.legacy-clipboard"],
        &marked_features,
        2,
    );
    assert_eq!(
        records.len(),
        1,
        "the host must not write two deprecated records for the same plugin, kind and name in one load"
    );
    assert_eq!(records[0].feature, "host.legacy-clipboard");
}

// ---------------------------------------------------------------------------------------------
// ABI-086: @since markers and the release notes of each minor
// ---------------------------------------------------------------------------------------------

/// REQ: ABI-086
#[test]
fn req_abi_086_since_markers_listed_in_release_notes_a() {
    let notes = manual_page(RELEASE_NOTES_PAGE);
    let mut versions = BTreeSet::new();
    for file in wit_files_under(&wit_root()) {
        let text = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", file.display()));
        versions.extend(since_markers(&text));
    }
    versions.extend(since_markers(&manual_page("reference-wit")));
    for version in versions.iter().filter(|version| minor_of(version) > 1) {
        let section = release_notes_section(&notes, version).unwrap_or_else(|| {
            panic!("the release notes must have a section for {version}")
        });
        assert!(
            section.contains("Added to the contract"),
            "the {version} release notes must list its items under `Added to the contract`"
        );
    }
}

/// REQ: ABI-086
#[test]
fn req_abi_086_frozen_snapshot_has_no_later_since_marker_b() {
    let frozen = wit_root().join("since_v0_2");
    let mut later_markers = Vec::new();
    for file in wit_files_under(&frozen) {
        let text = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", file.display()));
        for version in since_markers(&text) {
            if minor_of(&version) > 2 {
                later_markers.push(format!("{} marks {version}", file.display()));
            }
        }
    }
    assert!(
        later_markers.is_empty(),
        "the 0.2 frozen snapshot must not carry a later @since version: {later_markers:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// ABI-087: frozen WIT copies hash-equal the published values
// ---------------------------------------------------------------------------------------------

/// Live WIT files outside the frozen `since_v*` folders, with their paths relative to `wit/`.
fn live_wit_files() -> Vec<PathBuf> {
    wit_files_under(&wit_root())
        .into_iter()
        .filter_map(|file| {
            let relative = file
                .strip_prefix(wit_root())
                .expect("a file under wit/ has a relative path")
                .to_path_buf();
            let frozen_folder = relative
                .components()
                .next()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_default();
            (!frozen_folder.starts_with("since_v")).then_some(relative)
        })
        .collect()
}

/// REQ: ABI-087
#[test]
fn req_abi_087_frozen_copy_hash_equals_published_a() {
    let frozen_root = wit_root().join("since_v0_1");
    for relative in live_wit_files() {
        let live = fs::read(wit_root().join(&relative))
            .unwrap_or_else(|error| panic!("{} must be readable: {error}", relative.display()));
        let frozen_path = frozen_root.join(&relative);
        let frozen = fs::read(&frozen_path).unwrap_or_else(|error| {
            panic!("the frozen copy {} must exist: {error}", frozen_path.display())
        });
        assert_eq!(
            sha256_hex(&frozen),
            sha256_hex(&live),
            "the frozen copy of {} must hash-equal its published value",
            relative.display()
        );
    }
}

/// REQ: ABI-087
#[test]
fn req_abi_087_frozen_copy_has_no_missing_or_extra_package_b() {
    let frozen_root = wit_root().join("since_v0_1");
    let frozen_files: Vec<PathBuf> = wit_files_under(&frozen_root)
        .into_iter()
        .map(|file| {
            file.strip_prefix(&frozen_root)
                .expect("a frozen file has a relative path")
                .to_path_buf()
        })
        .collect();
    let live_files = live_wit_files();
    assert_eq!(
        frozen_files.len(),
        live_files.len(),
        "the frozen copy and the published 0.1 contract must hold the same packages"
    );
}

// ---------------------------------------------------------------------------------------------
// ABI-088: import versions checked at install stage 3
// ---------------------------------------------------------------------------------------------

/// REQ: ABI-088
#[test]
fn req_abi_088_matching_import_version_installs_a() {
    let package = b"component importing ilium:host@0.1.0 with manifest api 0.1.0".to_vec();
    let registry = registry_with_package(&package);
    let mut client = Client::new(vec![registry]);
    let installed = client
        .install(PLUGIN_ID, InstallOptions::new())
        .expect("an import that matches the manifest api installs");
    assert_eq!(
        installed.id(),
        PLUGIN_ID,
        "the matching component installs under its id"
    );
}

/// REQ: ABI-088
#[test]
fn req_abi_088_mismatched_import_version_refused_at_install_b() {
    let package = b"component importing ilium:host@0.2.0 with manifest api 0.1.0".to_vec();
    let error = install_refusal(&package);
    assert!(
        error.message.starts_with("manifest.api-version: "),
        "an import version that differs from the manifest api must be refused at install: {}",
        error.message
    );
    assert!(
        error.message.contains("ilium:host"),
        "the refusal must name the import: {}",
        error.message
    );
    assert!(
        error.message.contains("0.2.0") && error.message.contains("0.1.0"),
        "the refusal must name both versions: {}",
        error.message
    );
}

// ---------------------------------------------------------------------------------------------
// CMP-000: the removal minor waits 90 days after the marking release
// ---------------------------------------------------------------------------------------------

/// REQ: CMP-000
#[test]
fn req_cmp_000_removal_waits_ninety_days_after_marking_a() {
    let page = manual_page("compat-deprecation-timeline");
    let rule = page
        .lines()
        .find(|line| line.contains("[CMP-000]"))
        .expect("the timeline page must state CMP-000");
    assert!(
        rule.contains("90 days"),
        "CMP-000 must state the 90-day window after the marking release: {rule}"
    );
    assert!(
        rule.contains("[ABI-052]"),
        "CMP-000 must keep the two-minor rule of ABI-052 in addition: {rule}"
    );
}

/// REQ: CMP-000
#[test]
fn req_cmp_000_release_gate_blocks_young_marking_b() {
    let feature = MarkedFeature {
        name: "host.legacy-clipboard".to_owned(),
        marked_in_minor: 2,
        removal_minor: 4,
    };
    assert!(
        notice_is_long_enough(&feature),
        "the two-minor rule of ABI-052 holds for this removal"
    );
    let page = manual_page("compat-deprecation-timeline");
    assert!(
        page.contains("block a removal whose marking release is younger than that"),
        "the release gate must block a removal whose marking release is younger than 90 days"
    );
}
