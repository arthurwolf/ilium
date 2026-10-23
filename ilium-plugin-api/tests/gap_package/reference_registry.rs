//! reference-registry.md: yank and unyank (PKG-800..802), publish limits and order (PKG-810..812),
//! client source rules (PKG-813, 814, 816, 817, 818), owners (PKG-819), takedown tombstones
//! (PKG-821), stable version order (PKG-822), yank dependents (PKG-823) and search limits (PKG-824).

use std::io::{self, Read};
use std::time::Duration;

use ilium_plugin_api::cli;
use ilium_plugin_api::client::{Client, InstallOptions};
use ilium_plugin_api::http::HttpResponse;
use ilium_plugin_api::mirror::{self, MirrorTarget};
use ilium_plugin_api::registry::{PublishRequest, Registry, YankRequest};
use serde_json::{json, Value};

use crate::support::*;

const REGISTRY_URL: &str = "https://registry.example";
const MIB: usize = 1024 * 1024;

/// A body reader that counts the bytes the registry pulled from it.
struct CountingReader {
    inner: io::Cursor<Vec<u8>>,
    bytes_read: usize,
}

impl CountingReader {
    fn new(bytes: Vec<u8>) -> CountingReader {
        CountingReader {
            inner: io::Cursor::new(bytes),
            bytes_read: 0,
        }
    }
}

impl Read for CountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.bytes_read += count;
        Ok(count)
    }
}

/// A body that never delivers a byte: every read reports that no data is ready.
struct StalledReader;

impl Read for StalledReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::WouldBlock))
    }
}

fn metadata_of(registry: &Registry, id: &str) -> Value {
    let text = registry
        .metadata_document(id)
        .expect("the plugin has a metadata document");
    serde_json::from_str(&text).expect("the metadata document is JSON")
}

fn published_with_yank(id: &str, version: &str, reason: &str) -> Registry {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, id, version);
    registry
        .yank(YankRequest::new(id, version, OWNER_KEY).with_reason(reason))
        .expect("the owner key may yank its own version");
    registry
}

// ---------------------------------------------------------------------------------------------
// PKG-800 .. PKG-802: yank, unyank, takedown
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-800
#[test]
fn pkg_800_yank_by_the_owner_key_is_accepted() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    registry
        .yank(YankRequest::new("demo.plugin", "1.0.0", OWNER_KEY).with_reason("broken"))
        .expect("a yank signed by a trusted key of the current publisher is accepted");
    assert!(
        metadata_of(&registry, "demo.plugin")["versions"]["1.0.0"]["yanked_at"].is_string(),
        "the owner's yank is recorded"
    );
}

/// REQ: PKG-800
#[test]
fn pkg_800_yank_by_a_stranger_is_refused_and_metadata_stays_unchanged() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let before = metadata_of(&registry, "demo.plugin");
    let error = refusal(
        registry.yank(YankRequest::new("demo.plugin", "1.0.0", STRANGER_KEY).with_reason("spite")),
        "a yank signed by a key that does not own the plugin must be refused",
    );
    assert_eq!(code_of(&error), "registry.not-owner");
    assert_eq!(
        metadata_of(&registry, "demo.plugin"),
        before,
        "the metadata must stay unchanged after the refusal"
    );
}

/// REQ: PKG-800
#[test]
fn pkg_800_unyank_by_a_stranger_is_refused_and_the_yank_stays() {
    let mut registry = published_with_yank("demo.plugin", "1.0.0", "broken");
    let error = refusal(
        registry.unyank(YankRequest::new("demo.plugin", "1.0.0", STRANGER_KEY)),
        "an unyank signed by a key that does not own the plugin must be refused",
    );
    assert_eq!(code_of(&error), "registry.not-owner");
    assert!(
        metadata_of(&registry, "demo.plugin")["versions"]["1.0.0"]["yanked_at"].is_string(),
        "the yank must still be recorded after the refused unyank"
    );
}

/// REQ: PKG-801
#[test]
fn pkg_801_yank_records_time_key_fingerprint_and_reason_in_the_metadata() {
    let registry = published_with_yank("demo.plugin", "1.0.0", "broken on load");
    let version = &metadata_of(&registry, "demo.plugin")["versions"]["1.0.0"];
    let yanked_at = version["yanked_at"]
        .as_str()
        .expect("yanked_at is recorded");
    assert_eq!(
        yanked_at.len(),
        20,
        "yanked_at is UTC with second precision, e.g. 2026-10-22T12:00:00Z"
    );
    assert!(yanked_at.ends_with('Z'), "yanked_at is UTC");
    assert_eq!(
        version["yanked_by"].as_str(),
        Some(OWNER_KEY),
        "yanked_by is the key fingerprint"
    );
    assert_eq!(version["reason"].as_str(), Some("broken on load"));
}

/// REQ: PKG-801
#[test]
fn pkg_801_cli_yank_without_reason_fails_with_usage_missing_argument() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let error = refusal(
        cli::run(
            &mut registry,
            OWNER_KEY,
            &["plugin", "yank", "demo.plugin@1.0.0"],
        ),
        "ilium plugin yank without --reason must fail",
    );
    assert_eq!(code_of(&error), "usage.missing-argument");
    assert!(
        metadata_of(&registry, "demo.plugin")["versions"]["1.0.0"]["yanked_at"].is_null(),
        "the refused yank writes nothing"
    );
}

/// REQ: PKG-801
#[test]
fn pkg_801_reason_must_be_between_1_and_500_scalar_values() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    assert!(
        registry
            .yank(YankRequest::new("demo.plugin", "1.0.0", OWNER_KEY).with_reason(""))
            .is_err(),
        "an empty reason is refused"
    );
    assert!(
        registry
            .yank(YankRequest::new("demo.plugin", "1.0.0", OWNER_KEY).with_reason(&"x".repeat(501)))
            .is_err(),
        "a 501-character reason is refused"
    );
    registry
        .yank(YankRequest::new("demo.plugin", "1.0.0", OWNER_KEY).with_reason(&"x".repeat(500)))
        .expect("a 500-character reason is accepted");
}

/// REQ: PKG-801
#[test]
fn pkg_801_reason_is_shown_by_info_and_by_the_install_refusal() {
    let registry = published_with_yank("demo.plugin", "1.0.0", "leaks secrets");
    let output = cli::run(
        &mut registry.clone(),
        OWNER_KEY,
        &["plugin", "info", "demo.plugin@1.0.0"],
    )
    .expect("info of a yanked version succeeds");
    assert!(
        output.text.contains("leaks secrets"),
        "info shows the yank reason"
    );

    let mut client = Client::new(vec![registry.clone()]);
    let error = refusal(
        client.install("demo.plugin", InstallOptions::new().exact_version("1.0.0")),
        "installing a yanked version is refused",
    );
    assert!(
        error.message.contains("leaks secrets"),
        "the install refusal shows the yank reason"
    );
}

/// REQ: PKG-802
#[test]
fn pkg_802_unyank_clears_yanked_fields_and_appends_a_log_entry() {
    let mut registry = published_with_yank("demo.plugin", "1.0.0", "broken");
    let log_before = registry.transparency_log().len();
    registry
        .unyank(YankRequest::new("demo.plugin", "1.0.0", OWNER_KEY))
        .expect("unyank of a yanked version by the owner succeeds");
    let version = metadata_of(&registry, "demo.plugin")["versions"]["1.0.0"].clone();
    assert!(version["yanked_at"].is_null(), "yanked_at is cleared");
    assert!(version["yanked_by"].is_null(), "yanked_by is cleared");
    assert!(version["reason"].is_null(), "reason is cleared");
    let log = registry.transparency_log();
    assert_eq!(
        log.len(),
        log_before + 1,
        "unyank appends one transparency entry"
    );
    assert_eq!(log.last().map(|entry| entry.action()), Some("unyank"));
}

/// REQ: PKG-802
#[test]
fn pkg_802_unyank_of_a_live_version_fails_with_not_yanked() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let error = refusal(
        registry.unyank(YankRequest::new("demo.plugin", "1.0.0", OWNER_KEY)),
        "unyank of a version that is not yanked must fail",
    );
    assert_eq!(code_of(&error), "registry.not-yanked");
}

/// REQ: PKG-802
#[test]
fn pkg_802_taken_down_version_cannot_be_unyanked() {
    let mut registry = published_with_yank("demo.plugin", "1.0.0", "broken");
    registry
        .takedown("demo.plugin", "1.0.0", "malware")
        .expect("the operator may take down a published version");
    let error = refusal(
        registry.unyank(YankRequest::new("demo.plugin", "1.0.0", OWNER_KEY)),
        "a taken-down version must not be unyankable",
    );
    assert_eq!(code_of(&error), "registry.taken-down");
}

// ---------------------------------------------------------------------------------------------
// PKG-810 .. PKG-812: publish limits, check order, dependencies
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-810
#[test]
fn pkg_810_publish_body_over_300_mib_is_refused_before_it_is_read() {
    let mut registry = Registry::at(REGISTRY_URL);
    let mut body = CountingReader::new(vec![0u8; 16]);
    let announced = 300 * MIB as u64 + 1;
    let error = refusal(
        registry.publish_body(announced, &mut body),
        "a publish body announced over 300 MiB must be refused",
    );
    assert_eq!(code_of(&error), "registry.size");
    assert_eq!(error.http_status(), 413, "the refusal is HTTP 413");
    assert_eq!(
        body.bytes_read, 0,
        "the registry must refuse before reading the body"
    );
}

/// REQ: PKG-810
#[test]
fn pkg_810_upload_without_progress_for_60_seconds_aborts_with_upload_timeout() {
    let registry = Registry::at(REGISTRY_URL);
    assert_eq!(
        registry.upload_idle_timeout(),
        Duration::from_secs(60),
        "the default idle limit for an upload is 60 s"
    );
    let mut registry = registry.with_upload_idle_timeout(Duration::from_millis(50));
    let error = refusal(
        registry.publish_body(1024, &mut StalledReader),
        "an upload that makes no progress must be aborted",
    );
    assert_eq!(code_of(&error), "registry.upload-timeout");
}

/// REQ: PKG-811
#[test]
fn pkg_811_size_is_reported_before_namespace_or_dependency_failures() {
    let mut registry = Registry::at(REGISTRY_URL);
    let mut body = CountingReader::new(vec![0u8; 16]);
    let error = refusal(
        registry.publish_body(300 * MIB as u64 + 1, &mut body),
        "an oversized publish must fail at the size stage",
    );
    assert_eq!(code_of(&error), "registry.size", "size is the first stage");
}

/// REQ: PKG-811
#[test]
fn pkg_811_namespace_ownership_is_reported_before_version_exists() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let error = refusal(
        registry.publish(
            PublishRequest::new("demo.plugin", "1.0.0", STRANGER_KEY)
                .with_package(sample_package()),
        ),
        "a stranger publishing an existing version must fail",
    );
    assert_eq!(
        code_of(&error),
        "registry.not-owner",
        "namespace ownership is checked before version-exists"
    );
}

/// REQ: PKG-811
#[test]
fn pkg_811_version_exists_is_reported_before_dependency_existence() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let error = refusal(
        registry.publish(
            PublishRequest::new("demo.plugin", "1.0.0", OWNER_KEY)
                .with_package(sample_package())
                .with_requires_plugin("missing.dep", "^1.0.0", false),
        ),
        "a duplicate version with a missing dependency must fail",
    );
    assert_eq!(
        code_of(&error),
        "registry.version-exists",
        "version-exists is checked before dependency existence"
    );
}

/// REQ: PKG-811
#[test]
fn pkg_811_failed_publish_leaves_no_blob_and_no_metadata_change() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let blobs_before = registry.blob_count();
    let revision_before = registry.metadata_revision();
    refusal(
        registry.publish(
            PublishRequest::new("new.plugin", "1.0.0", OWNER_KEY)
                .with_package(b"other-package".to_vec())
                .with_requires_plugin("missing.dep", "^1.0.0", false),
        ),
        "a publish with a missing dependency must fail",
    );
    assert_eq!(
        registry.blob_count(),
        blobs_before,
        "no blob is stored by a failed publish"
    );
    assert_eq!(
        registry.metadata_revision(),
        revision_before,
        "no metadata changes after a failed publish"
    );
    assert!(
        registry.metadata_document("new.plugin").is_none(),
        "no metadata for the new id"
    );
}

/// REQ: PKG-812
#[test]
fn pkg_812_unknown_required_plugin_is_refused_with_dependency_unknown() {
    let mut registry = Registry::at(REGISTRY_URL);
    let error = refusal(
        registry.publish(
            PublishRequest::new("demo.plugin", "1.0.0", OWNER_KEY)
                .with_package(sample_package())
                .with_requires_plugin("missing.dep", "^1.0.0", false),
        ),
        "a required plugin that exists nowhere must refuse the publish",
    );
    assert_eq!(code_of(&error), "registry.dependency-unknown");
}

/// REQ: PKG-812
#[test]
fn pkg_812_optional_unknown_requirement_is_accepted() {
    let mut registry = Registry::at(REGISTRY_URL);
    registry
        .publish(
            PublishRequest::new("demo.plugin", "1.0.0", OWNER_KEY)
                .with_package(sample_package())
                .with_requires_plugin("missing.dep", "^1.0.0", true),
        )
        .expect("an optional requirement on an unknown id does not refuse the publish");
}

/// REQ: PKG-812
#[test]
fn pkg_812_requirement_served_by_a_configured_upstream_is_accepted() {
    let mut upstream = Registry::at("https://upstream.example");
    publish_owned(&mut upstream, "upstream.dep", "1.0.0");
    let mut registry = Registry::at(REGISTRY_URL).with_upstream(upstream);
    registry
        .publish(
            PublishRequest::new("demo.plugin", "1.0.0", OWNER_KEY)
                .with_package(sample_package())
                .with_requires_plugin("upstream.dep", "^1.0.0", false),
        )
        .expect("a requirement served by a configured upstream is not unknown");
}

// ---------------------------------------------------------------------------------------------
// PKG-813, PKG-814: client source rules
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-813
#[test]
fn pkg_813_second_registry_source_is_refused_unless_allow_source_change_is_passed() {
    let mut first = Registry::at("https://first.example");
    publish_owned(&mut first, "demo.plugin", "1.0.0");
    let mut second = Registry::at("https://second.example");
    publish_owned(&mut second, "demo.plugin", "1.0.0");

    let mut client = Client::new(vec![first.clone()]);
    client
        .install("demo.plugin", InstallOptions::new())
        .expect("the first install comes from the first registry");
    client.set_sources(vec![second.clone()]);

    let error = refusal(
        client.install("demo.plugin", InstallOptions::new()),
        "a different source for an installed id must be refused",
    );
    assert_eq!(code_of(&error), "registry.source-changed");

    let allowed = client
        .install(
            "demo.plugin",
            InstallOptions::new().allow_source_change(true),
        )
        .expect("--allow-source-change accepts the different source");
    assert_eq!(allowed.source_url(), second.url());
}

/// REQ: PKG-814
#[test]
fn pkg_814_routed_id_is_requested_only_from_its_route() {
    let mut acme = Registry::at("https://acme.example");
    publish_owned(&mut acme, "acme.widget", "1.0.0");
    let mut public = Registry::at("https://public.example");
    publish_owned(&mut public, "acme.widget", "1.0.0");

    let mut client = Client::new(vec![public.clone()]).with_route("acme.", acme.clone());
    let installed = client
        .install("acme.widget", InstallOptions::new())
        .expect("the routed id installs from its route");
    assert_eq!(installed.source_url(), acme.url());
    assert_eq!(
        public.requests_received(),
        0,
        "a routed id must never be requested from another registry"
    );
}

/// REQ: PKG-814
#[test]
fn pkg_814_route_miss_is_plugin_not_found_and_never_falls_through() {
    let acme = Registry::at("https://acme.example");
    let mut public = Registry::at("https://public.example");
    publish_owned(&mut public, "acme.gizmo", "1.0.0");

    let mut client = Client::new(vec![public.clone()]).with_route("acme.", acme.clone());
    let error = refusal(
        client.install("acme.gizmo", InstallOptions::new()),
        "a miss on a routed namespace must not fall through to another registry",
    );
    assert_eq!(code_of(&error), "plugin.not-found");
    assert_eq!(
        public.requests_received(),
        0,
        "the fall-through registry is never asked"
    );
}

// ---------------------------------------------------------------------------------------------
// PKG-816, PKG-817: rollback and mirror closure
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-816
#[test]
fn pkg_816_snapshot_older_than_the_stored_version_is_refused_as_rollback() {
    let mut client = Client::new(vec![]);
    client
        .accept_snapshot(&json!({ "version": 5 }).to_string())
        .expect("the first snapshot is accepted");
    let error = refusal(
        client.accept_snapshot(&json!({ "version": 4 }).to_string()),
        "a snapshot below the stored version must be refused",
    );
    assert_eq!(code_of(&error), "registry.rollback");
    assert_eq!(
        client.stored_snapshot_version(),
        Some(5),
        "the stored version is unchanged"
    );
}

/// REQ: PKG-816
#[test]
fn pkg_816_timestamp_older_than_the_stored_version_is_refused_as_rollback() {
    let mut client = Client::new(vec![]);
    client
        .accept_timestamp(&json!({ "version": 7 }).to_string())
        .expect("the first timestamp is accepted");
    let error = refusal(
        client.accept_timestamp(&json!({ "version": 6 }).to_string()),
        "a timestamp below the stored version must be refused",
    );
    assert_eq!(code_of(&error), "registry.rollback");
    assert_eq!(
        client.stored_timestamp_version(),
        Some(7),
        "the stored version is unchanged"
    );
}

/// REQ: PKG-817
#[test]
fn pkg_817_mirror_with_allow_refuses_an_incomplete_closure_and_writes_nothing() {
    let mut upstream = Registry::at("https://upstream.example");
    publish_owned(&mut upstream, "base.lib", "1.0.0");
    let mut source = Registry::at("https://source.example").with_upstream(upstream);
    publish_requiring(&mut source, "app.main", "1.0.0", &[("base.lib", "^1.0.0")]);
    let source = source.without_upstreams();

    let mut target = MirrorTarget::empty();
    let error = refusal(
        mirror::mirror(&source, &["app.main"], &mut target),
        "a closure that names an id the source does not serve must fail",
    );
    assert_eq!(code_of(&error), "mirror.closure-incomplete");
    assert!(
        target.is_empty(),
        "the mirror writes nothing when the closure is incomplete"
    );
}

// ---------------------------------------------------------------------------------------------
// PKG-818: content types and gzip for metadata
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-818
#[test]
fn pkg_818_metadata_is_served_as_application_json() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let response = registry.serve("/plugins/demo.plugin.json");
    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), "application/json");
}

/// REQ: PKG-818
#[test]
fn pkg_818_blobs_are_served_as_application_octet_stream() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let sha256 = registry.sha256_of("demo.plugin", "1.0.0");
    let response = registry.serve(&format!("/blobs/{sha256}"));
    assert_eq!(response.status(), 200);
    assert_eq!(response.content_type(), "application/octet-stream");
}

/// REQ: PKG-818
#[test]
fn pkg_818_client_ignores_the_content_type_of_metadata() {
    let client = Client::new(vec![]);
    let body = json!({ "id": "demo.plugin", "versions": {} }).to_string();
    let response = HttpResponse::new(200, "text/plain", None, body.into_bytes());
    let parsed = client
        .decode_metadata(&response)
        .expect("metadata is accepted whatever its content type");
    assert_eq!(parsed["id"], "demo.plugin");
}

/// REQ: PKG-818
#[test]
fn pkg_818_client_accepts_gzip_content_encoding_for_metadata() {
    let client = Client::new(vec![]);
    let body = json!({ "id": "demo.plugin", "versions": {} }).to_string();
    let response = HttpResponse::with_gzip_body(200, "application/json", body.as_bytes());
    let parsed = client
        .decode_metadata(&response)
        .expect("gzip-encoded metadata is accepted");
    assert_eq!(parsed["id"], "demo.plugin");
}

/// REQ: PKG-818
#[test]
fn pkg_818_metadata_size_limit_applies_to_the_decoded_bytes() {
    let client = Client::new(vec![]);
    let limit = client.metadata_size_limit();
    let padding = "x".repeat(limit + 1);
    let decoded = json!({ "id": "demo.plugin", "pad": padding }).to_string();
    let response = HttpResponse::with_gzip_body(200, "application/json", decoded.as_bytes());
    assert!(
        response.body().len() < MIB,
        "the compressed body is small, so only the decoded size can refuse it"
    );
    assert!(
        client.decode_metadata(&response).is_err(),
        "decoded metadata over the size limit must be refused"
    );
}

// ---------------------------------------------------------------------------------------------
// PKG-819: owners
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-819
#[test]
fn pkg_819_owner_add_signed_by_an_owner_is_recorded_in_the_log() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    registry
        .add_owner("demo.plugin", "second.publisher", OWNER_KEY)
        .expect("an owner key may add an owner");
    assert!(
        registry
            .transparency_log()
            .iter()
            .any(|entry| entry.action() == "owner-add"),
        "the owner change is recorded in the transparency log"
    );
}

/// REQ: PKG-819
#[test]
fn pkg_819_owner_change_signed_by_a_non_owner_is_refused() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let error = refusal(
        registry.add_owner("demo.plugin", "stranger.publisher", STRANGER_KEY),
        "an owner change signed by a key of no owner must be refused",
    );
    assert_eq!(code_of(&error), "registry.not-owner");
}

/// REQ: PKG-819
#[test]
fn pkg_819_removing_the_last_owner_is_refused_with_last_owner() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let error = refusal(
        registry.remove_owner("demo.plugin", OWNER_PUBLISHER, OWNER_KEY),
        "removing the only owner must be refused",
    );
    assert_eq!(code_of(&error), "registry.last-owner");
}

// ---------------------------------------------------------------------------------------------
// PKG-821: takedown tombstones
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-821
#[test]
fn pkg_821_publish_of_a_taken_down_version_is_refused_as_version_exists() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    registry
        .takedown("demo.plugin", "1.0.0", "malware")
        .expect("the operator may take down a published version");
    let error = refusal(
        registry.publish(
            PublishRequest::new("demo.plugin", "1.0.0", OWNER_KEY).with_package(sample_package()),
        ),
        "the same id@version must not be published again after a takedown",
    );
    assert_eq!(code_of(&error), "registry.version-exists");
}

/// REQ: PKG-821
#[test]
fn pkg_821_tombstone_keeps_id_version_sha256_reason_and_time() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let sha256 = registry.sha256_of("demo.plugin", "1.0.0");
    registry
        .takedown("demo.plugin", "1.0.0", "malware")
        .expect("the operator may take down a published version");
    let document = metadata_of(&registry, "demo.plugin");
    let tombstone = &document["tombstones"][0];
    assert_eq!(tombstone["id"], "demo.plugin");
    assert_eq!(tombstone["version"], "1.0.0");
    assert_eq!(tombstone["sha256"], sha256.as_str());
    assert_eq!(tombstone["reason_code"], "malware");
    assert!(
        tombstone["time"]
            .as_str()
            .is_some_and(|time| !time.is_empty()),
        "the tombstone records the time of the takedown"
    );
}

// ---------------------------------------------------------------------------------------------
// PKG-822: stable version order
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-822
#[test]
fn pkg_822_lower_stable_version_of_a_different_major_line_is_accepted() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "2.0.0");
    registry
        .publish(
            PublishRequest::new("demo.plugin", "1.5.0", OWNER_KEY).with_package(sample_package()),
        )
        .expect("a lower stable version on a different MAJOR line is accepted");
}

/// REQ: PKG-822
#[test]
fn pkg_822_lower_stable_version_of_the_same_major_line_is_refused_with_version_order() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "2.0.0");
    let error = refusal(
        registry.publish(
            PublishRequest::new("demo.plugin", "2.0.0-rc.1", OWNER_KEY)
                .with_package(sample_package())
                .with_channel("stable"),
        ),
        "a lower stable version on the same MAJOR line must be refused",
    );
    assert_eq!(code_of(&error), "registry.version-order");
}

/// REQ: PKG-822
#[test]
fn pkg_822_the_version_order_rule_applies_only_to_the_stable_channel() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "2.0.0");
    registry
        .publish(
            PublishRequest::new("demo.plugin", "2.0.0-rc.1", OWNER_KEY)
                .with_package(sample_package())
                .with_channel("beta"),
        )
        .expect("the beta channel is not ordered against the stable highest version");
}

// ---------------------------------------------------------------------------------------------
// PKG-823: yank dependents
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-823
#[test]
fn pkg_823_yank_with_dependents_requires_confirm_dependents() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    publish_owned(&mut registry, "demo.plugin", "1.1.0");
    publish_requiring(
        &mut registry,
        "dep.one",
        "1.0.0",
        &[("demo.plugin", "=1.0.0")],
    );
    let error = refusal(
        cli::run(
            &mut registry,
            OWNER_KEY,
            &["plugin", "yank", "demo.plugin@1.0.0", "--reason", "broken"],
        ),
        "a yank that breaks one dependent must require --confirm-dependents",
    );
    assert!(
        error.message.contains("confirm"),
        "the refusal asks for --confirm-dependents"
    );
    assert!(
        metadata_of(&registry, "demo.plugin")["versions"]["1.0.0"]["yanked_at"].is_null(),
        "the refused yank writes nothing"
    );
}

/// REQ: PKG-823
#[test]
fn pkg_823_yank_prints_the_number_of_plugins_that_only_the_yanked_version_satisfies() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    publish_owned(&mut registry, "demo.plugin", "1.1.0");
    publish_requiring(
        &mut registry,
        "dep.one",
        "1.0.0",
        &[("demo.plugin", "=1.0.0")],
    );
    let output = cli::run(
        &mut registry,
        OWNER_KEY,
        &[
            "plugin",
            "yank",
            "demo.plugin@1.0.0",
            "--reason",
            "broken",
            "--confirm-dependents",
        ],
    )
    .expect("the yank succeeds once dependents are confirmed");
    assert!(
        output.text.contains("1 plugin"),
        "yank prints the number of plugins whose latest release only the yanked version satisfies"
    );
}

/// REQ: PKG-823
#[test]
fn pkg_823_yank_without_dependents_needs_no_confirmation() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    publish_owned(&mut registry, "demo.plugin", "1.1.0");
    cli::run(
        &mut registry,
        OWNER_KEY,
        &["plugin", "yank", "demo.plugin@1.1.0", "--reason", "broken"],
    )
    .expect("a yank with zero dependents does not need --confirm-dependents");
}

// ---------------------------------------------------------------------------------------------
// PKG-824: search limits
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-824
#[test]
fn pkg_824_search_limit_or_offset_out_of_range_fails_before_any_request() {
    let mut registry = Registry::at(REGISTRY_URL);
    publish_owned(&mut registry, "demo.plugin", "1.0.0");
    let requests_before = registry.requests_received();
    for arguments in [
        vec!["plugin", "search", "demo", "--limit", "0"],
        vec!["plugin", "search", "demo", "--limit", "101"],
        vec!["plugin", "search", "demo", "--offset", "10001"],
    ] {
        let error = refusal(
            cli::run(&mut registry, OWNER_KEY, &arguments),
            "an out-of-range search option must fail",
        );
        assert_eq!(code_of(&error), "usage.invalid-value");
    }
    assert_eq!(
        registry.requests_received(),
        requests_before,
        "no network request is made for an invalid search"
    );
}

/// REQ: PKG-824
#[test]
fn pkg_824_search_page_holds_at_most_the_requested_limit() {
    let mut registry = Registry::at(REGISTRY_URL);
    for index in 0..5 {
        publish_owned(&mut registry, &format!("demo.n{index}"), "1.0.0");
    }
    let output = cli::run(
        &mut registry,
        OWNER_KEY,
        &["plugin", "search", "demo", "--limit", "2"],
    )
    .expect("a search with a valid limit succeeds");
    let rows = output
        .text
        .lines()
        .filter(|line| line.contains("demo.n"))
        .count();
    assert!(
        rows <= 2,
        "a result page holds at most the requested limit, got {rows}"
    );
}

/// REQ: PKG-809
#[test]
fn pkg_809_successful_publish_answers_201_fields_and_index_within_60_seconds() {
    let mut registry = Registry::at(REGISTRY_URL);
    let receipt = registry
        .publish(
            PublishRequest::new("demo.plugin", "1.0.0", OWNER_KEY).with_package(sample_package()),
        )
        .expect("the owner's first publish of a version must succeed");
    let body: Value =
        serde_json::from_str(&receipt.response_body()).expect("the 201 body of a publish is JSON");
    for field in [
        "id",
        "version",
        "sha256",
        "length",
        "url",
        "channel",
        "published_at",
    ] {
        assert!(
            body.get(field).is_some(),
            "the 201 body must carry `{field}`"
        );
    }
    assert_eq!(
        body["id"], "demo.plugin",
        "the body names the published plugin"
    );
    assert!(
        registry.index_delay_of("demo.plugin", "1.0.0") <= Duration::from_secs(60),
        "the release must appear in the signed index within 60 s",
    );
}
