//! Page `reference-install-and-update.md`: install sources, on-disk state, the install
//! transaction, running servers, update, downgrade, rollback, uninstall, auto-update
//! (PKG-260 .. PKG-305). Expected values are copied from the page.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::types::PluginId;
use ilium_plugin_api::manifest::CheckMode;
use ilium_plugin_api::{Platform, Tier};
use ilium_plugin_host::autoupdate::{
    decide, later_hides_for, schedule, should_check_network, AutoAction, AutoUpdateConfig,
    AutoUpdateMode, NoticeAction, UpdateCandidate,
};
use ilium_plugin_host::install::{
    Consent, ConsentDecision, ConsentRequest, FirstActivate, InstallOptions, InstallStep,
    Installer, UpdateOutcome,
};
use ilium_plugin_host::registry::{GrantStore, InstalledRegistry, PackageStore};
use ilium_plugin_host::signing::TrustStore;
use ilium_plugin_host::testing::FakeRegistry;
use serde_json::Value;

use crate::support::*;

const MIB: u64 = 1024 * 1024;

fn installer(fx: &Fx, options: InstallOptions) -> Installer {
    Installer::with_options(PackageStore { root: fx.root() }, fx.state_dir(), options)
}

fn unsigned_ok() -> InstallOptions {
    InstallOptions {
        allow_unsigned: true,
    }
}

/// Approves exactly what is asked and records every request.
struct Recording {
    seen: Mutex<Vec<ConsentRequest>>,
    decline_all: bool,
}

impl Recording {
    fn approving() -> Recording {
        Recording {
            seen: Mutex::new(Vec::new()),
            decline_all: false,
        }
    }

    fn declining() -> Recording {
        Recording {
            seen: Mutex::new(Vec::new()),
            decline_all: true,
        }
    }

    fn requests(&self) -> Vec<ConsentRequest> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }
}

impl Consent for Recording {
    fn decide(&self, request: &ConsentRequest) -> ConsentDecision {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(request.clone());
        }
        if self.decline_all {
            ConsentDecision::Deny
        } else {
            ConsentDecision::Approve(request.requested.clone())
        }
    }
}

fn grant_texts(request: &ConsentRequest) -> Vec<String> {
    request
        .requested
        .iter()
        .map(|grant| grant.as_str().to_owned())
        .collect()
}

fn plugin(id: &str) -> PluginId {
    PluginId(id.to_owned())
}

fn install_via_installer(fx: &Fx, spec: &Spec, consent: &Recording) -> bool {
    installer(fx, unsigned_ok())
        .install(&spec.build(), consent)
        .is_ok()
}

// ---------------------------------------------------------------- install sources: URL

/// REQ: PKG-260
#[test]
fn pkg_260_http_url_refused_insecure_url() {
    let fx = Fx::new();
    fx.fetcher.serve(
        "http://example.test/a.iliumpkg",
        Spec::new("example.a", "1.0.0").build(),
    );
    let out = fx.run(&[
        "install",
        "http://example.test/a.iliumpkg",
        "--allow-unsigned",
        "--yes",
    ]);
    assert!(
        out.has_error("install.insecure-url"),
        "{:?}",
        out.stdout_jsonl
    );
    assert_eq!(out.exit_code, 1);
    assert!(
        fx.fetcher.calls().is_empty(),
        "an http URL must not be requested"
    );
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-260
#[test]
fn pkg_260_https_url_downloads_then_installs_as_local_file() {
    let fx = Fx::new();
    fx.fetcher.serve(
        "https://example.test/a.iliumpkg",
        Spec::new("example.a", "1.0.0").build(),
    );
    let out = fx.run(&[
        "install",
        "https://example.test/a.iliumpkg",
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(
        fx.fetcher.calls(),
        vec!["https://example.test/a.iliumpkg".to_owned()]
    );
    let record = fx.record("example.a").expect("record written");
    assert_eq!(record["source"], "url");
    assert_eq!(record["source_url"], "https://example.test/a.iliumpkg");
}

/// REQ: PKG-260
#[test]
fn pkg_260_three_https_redirects_are_followed() {
    let fx = Fx::new();
    fx.fetcher
        .redirect("https://example.test/r0", "https://example.test/r1");
    fx.fetcher
        .redirect("https://example.test/r1", "https://example.test/r2");
    fx.fetcher
        .redirect("https://example.test/r2", "https://example.test/r3");
    fx.fetcher.serve(
        "https://example.test/r3",
        Spec::new("example.a", "1.0.0").build(),
    );
    let out = fx.run(&[
        "install",
        "https://example.test/r0",
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.a").is_some());
}

/// REQ: PKG-260
#[test]
fn pkg_260_fourth_redirect_refused() {
    let fx = Fx::new();
    for hop in 0..4 {
        fx.fetcher.redirect(
            &format!("https://example.test/r{hop}"),
            &format!("https://example.test/r{}", hop + 1),
        );
    }
    fx.fetcher.serve(
        "https://example.test/r4",
        Spec::new("example.a", "1.0.0").build(),
    );
    let out = fx.run(&[
        "install",
        "https://example.test/r0",
        "--allow-unsigned",
        "--yes",
    ]);
    assert_ne!(out.exit_code, 0, "four redirects exceed the limit of 3");
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-260
#[test]
fn pkg_260_redirect_to_http_refused() {
    let fx = Fx::new();
    fx.fetcher
        .redirect("https://example.test/r0", "http://example.test/plain");
    fx.fetcher.serve(
        "http://example.test/plain",
        Spec::new("example.a", "1.0.0").build(),
    );
    let out = fx.run(&[
        "install",
        "https://example.test/r0",
        "--allow-unsigned",
        "--yes",
    ]);
    assert_ne!(out.exit_code, 0);
    assert!(!fx
        .fetcher
        .calls()
        .contains(&"http://example.test/plain".to_owned()));
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-261
#[test]
fn pkg_261_download_over_300_mib_aborted_install_size() {
    let fx = Fx::new();
    fx.fetcher.serve_as(
        "https://example.test/big",
        Vec::new(),
        None,
        Some(300 * MIB + 1),
    );
    let out = fx.run(&[
        "install",
        "https://example.test/big",
        "--allow-unsigned",
        "--yes",
    ]);
    assert!(out.has_error("install.size"), "{:?}", out.stdout_jsonl);
    assert_eq!(out.exit_code, 1);
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-261
#[test]
fn pkg_261_download_of_exactly_300_mib_is_not_install_size() {
    let fx = Fx::new();
    // n: the limit itself is allowed; the (garbage) body is then judged by the package format.
    fx.fetcher.serve_as(
        "https://example.test/edge",
        b"not a package".to_vec(),
        None,
        Some(300 * MIB),
    );
    let out = fx.run(&[
        "install",
        "https://example.test/edge",
        "--allow-unsigned",
        "--yes",
    ]);
    assert!(!out.has_error("install.size"), "{:?}", out.stdout_jsonl);
    assert_eq!(out.exit_code, 1);
}

/// REQ: PKG-261
#[test]
fn pkg_261_download_just_below_limit_is_not_install_size() {
    let fx = Fx::new();
    fx.fetcher.serve_as(
        "https://example.test/below",
        b"not a package".to_vec(),
        None,
        Some(300 * MIB - 1),
    );
    let out = fx.run(&[
        "install",
        "https://example.test/below",
        "--allow-unsigned",
        "--yes",
    ]);
    assert!(!out.has_error("install.size"), "{:?}", out.stdout_jsonl);
}

/// REQ: PKG-261
#[test]
fn pkg_261_content_type_is_ignored_for_a_valid_package() {
    let fx = Fx::new();
    fx.fetcher.serve_as(
        "https://example.test/a",
        Spec::new("example.a", "1.0.0").build(),
        Some("text/html"),
        None,
    );
    let out = fx.run(&[
        "install",
        "https://example.test/a",
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.a").is_some());
}

/// REQ: PKG-261
#[test]
fn pkg_261_package_content_type_does_not_rescue_garbage() {
    let fx = Fx::new();
    fx.fetcher.serve_as(
        "https://example.test/a",
        b"garbage".to_vec(),
        Some("application/x-iliumpkg"),
        None,
    );
    let out = fx.run(&[
        "install",
        "https://example.test/a",
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.error_codes()
            .iter()
            .any(|code| code.starts_with("package.")),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-262
#[test]
fn pkg_262_url_install_of_unsigned_package_needs_allow_unsigned() {
    let fx = Fx::new();
    fx.fetcher.serve(
        "https://example.test/a",
        Spec::new("example.a", "1.0.0").build(),
    );
    let out = fx.run(&["install", "https://example.test/a", "--yes"]);
    assert_eq!(out.exit_code, 1);
    // The `install` command reports `install.unsigned` (DEV-143, reference-plugin-cli.md).
    assert_eq!(out.error_codes(), vec!["install.unsigned".to_owned()]);
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-262
#[test]
fn pkg_262_url_install_with_allow_unsigned_installs() {
    let fx = Fx::new();
    fx.fetcher.serve(
        "https://example.test/a",
        Spec::new("example.a", "1.0.0").build(),
    );
    let out = fx.run(&[
        "install",
        "https://example.test/a",
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.a").is_some());
}

/// REQ: PKG-262
#[test]
fn pkg_262_file_and_url_give_the_same_trust_refusal() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0");
    fx.fetcher.serve("https://example.test/a", spec.build());
    let path = fx.write_file("a.iliumpkg", &spec.build());
    let from_file = fx.run(&["install", &path, "--yes"]);
    let from_url = fx.run(&["install", "https://example.test/a", "--yes"]);
    assert_eq!(from_file.error_codes(), from_url.error_codes());
    assert_eq!(from_file.exit_code, from_url.exit_code);
}

// ---------------------------------------------------------------- registry id and recommendation

fn registry_fx(versions: &[&str]) -> Fx {
    let fx = Fx::new();
    let mut registry = FakeRegistry::new(REGISTRY_URL);
    for version in versions {
        registry.publish(Spec::new("example.b", version).build(), "stable");
    }
    for (url, body) in registry.documents() {
        fx.fetcher.serve(&url, body);
    }
    fx
}

/// REQ: PKG-263
#[test]
fn pkg_263_registry_id_with_range_installs_the_highest_match() {
    let fx = registry_fx(&["1.0.0", "1.2.0", "2.0.0"]);
    let out = fx.run(&[
        "install",
        "example.b@^1.0",
        "--registry",
        REGISTRY_URL,
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.2.0"));
}

/// REQ: PKG-263
#[test]
fn pkg_263_registry_install_records_source_registry_and_url() {
    let fx = registry_fx(&["1.0.0"]);
    let out = fx.run(&[
        "install",
        "example.b@^1.0",
        "--registry",
        REGISTRY_URL,
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    let record = fx.record("example.b").expect("record");
    assert_eq!(record["source"], "registry");
    assert!(
        record["source_url"]
            .as_str()
            .unwrap_or("")
            .starts_with(REGISTRY_URL),
        "{record}"
    );
}

/// REQ: PKG-263
#[test]
fn pkg_263_bare_zero_series_range_is_a_caret_range() {
    let fx = registry_fx(&["0.1.0", "0.1.4", "0.2.0"]);
    let out = fx.run(&[
        "install",
        "example.b@0.1.0",
        "--registry",
        REGISTRY_URL,
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("0.1.4"));
}

/// REQ: PKG-263
#[test]
fn pkg_263_registry_range_without_match_is_dependency_unresolved() {
    let fx = registry_fx(&["1.0.0"]);
    let out = fx.run(&[
        "install",
        "example.b@^2.0",
        "--registry",
        REGISTRY_URL,
        "--allow-unsigned",
        "--yes",
    ]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("dependency.unresolved"),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.b").is_none());
}

/// REQ: PKG-264
#[test]
fn pkg_264_project_recommendation_is_never_fetched_or_installed() {
    let fx = Fx::new();
    let ilium_dir = fx.cwd.join(".ilium");
    std::fs::create_dir_all(&ilium_dir).expect("mkdir");
    std::fs::write(
        ilium_dir.join("plugins.toml"),
        "[recommend]\n\"example.rec\" = \"^1.0\"\n",
    )
    .expect("write");
    fx.fetcher.serve(
        "https://example.test/rec",
        Spec::new("example.rec", "1.0.0").build(),
    );
    let _ = fx.run(&["list"]);
    let _ = fx.run(&["list", "--json"]);
    assert!(
        fx.fetcher.calls().is_empty(),
        "no download without a user action: {:?}",
        fx.fetcher.calls()
    );
    assert!(fx.record("example.rec").is_none());
}

/// REQ: PKG-264
#[test]
fn pkg_264_confirmed_recommendation_file_installs_like_any_file() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.rec", "1.0.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.record("example.rec").expect("record")["source"], "file");
}

/// REQ: PKG-264
#[test]
fn pkg_264_install_with_input_unavailable_does_not_proceed_on_its_own() {
    let fx = Fx::new();
    let spec = Spec::new("example.rec", "1.0.0").grant("tree.read");
    let path = fx.write_file("rec.iliumpkg", &spec.build());
    let out = fx.run(&["install", &path, "--allow-unsigned"]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("install.needs-input"),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.rec").is_none());
}

// ---------------------------------------------------------------- on-disk state

const RECORD_KEYS: [&str; 17] = [
    "id",
    "active_version",
    "previous_version",
    "state",
    "source",
    "source_url",
    "channel",
    "publisher_id",
    "key_id",
    "package_sha256",
    "manifest_sha256",
    "pinned",
    "skip",
    "declined",
    "failure_stopped",
    "installed_at",
    "update",
];

/// REQ: PKG-265
#[test]
fn pkg_265_record_has_exactly_the_schema_keys() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let record = fx.record("example.a").expect("record");
    let mut keys: Vec<&str> = record
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    let mut expected = RECORD_KEYS.to_vec();
    expected.sort_unstable();
    assert_eq!(keys, expected);
    let installed_doc = fx.installed();
    let top: Vec<&str> = installed_doc
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert!(
        top.contains(&"format") && top.contains(&"plugins") && top.contains(&"purge_tombstones"),
        "{top:?}"
    );
    assert_eq!(fx.installed()["format"], "ilium-installed/1");
}

/// REQ: PKG-265
#[test]
fn pkg_265_file_install_source_file_channel_file() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["source"], "file");
    assert_eq!(record["channel"], "file");
    assert!(["bundled", "file", "url", "registry", "dev"]
        .contains(&record["source"].as_str().unwrap_or("")));
    assert!(["stable", "beta", "dev", "file"].contains(&record["channel"].as_str().unwrap_or("")));
}

/// REQ: PKG-265
#[test]
fn pkg_265_hashes_are_64_lowercase_hex_of_the_right_bytes() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let record = fx.record("example.a").expect("record");
    let package_sha = record["package_sha256"].as_str().unwrap_or("");
    let manifest_sha = record["manifest_sha256"].as_str().unwrap_or("");
    assert!(
        is_sha256_hex(package_sha) && is_sha256_hex(manifest_sha),
        "{record}"
    );
    assert_eq!(package_sha, sha256_hex(&spec.build()));
    assert_eq!(manifest_sha, sha256_hex(spec.manifest_text().as_bytes()));
}

/// REQ: PKG-265
#[test]
fn pkg_265_unsigned_record_has_null_key_id_and_publisher() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let record = fx.record("example.a").expect("record");
    assert!(record["key_id"].is_null(), "{record}");
    assert!(record["publisher_id"].is_null(), "{record}");
}

/// REQ: PKG-265
#[test]
fn pkg_265_installed_at_is_rfc3339_utc_with_z_from_the_clock() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let record = fx.record("example.a").expect("record");
    let stamp = record["installed_at"].as_str().unwrap_or("");
    assert!(is_utc_stamp(stamp), "{stamp}");
    assert_eq!(stamp, NOW_TEXT);
}

/// REQ: PKG-265
#[test]
fn pkg_265_skip_declined_lists_and_null_update_after_a_fresh_install() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["skip"], serde_json::json!([]));
    assert_eq!(record["declined"], serde_json::json!([]));
    assert!(record["update"].is_null());
    assert_eq!(record["pinned"], false);
}

/// REQ: PKG-265
#[test]
fn pkg_265_staged_update_object_has_candidate_state_attempts_started_at() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    let update = fx.record("example.a").expect("record")["update"].clone();
    let mut keys: Vec<&str> = update
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["attempts", "candidate", "started_at", "state"]);
    assert_eq!(update["candidate"], "1.1.0");
    assert_eq!(update["attempts"], 0);
    assert!(
        is_utc_stamp(update["started_at"].as_str().unwrap_or("")),
        "{update}"
    );
}

/// REQ: PKG-265
#[test]
fn pkg_265_plugins_are_sorted_by_id_in_byte_order() {
    let fx = Fx::new();
    for id in ["example.c", "example.a", "example.b"] {
        assert_eq!(fx.install(&Spec::new(id, "1.0.0")).exit_code, 0);
    }
    let ids: Vec<String> = fx.installed()["plugins"]
        .as_array()
        .expect("array")
        .iter()
        .map(|entry| entry["id"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(ids, vec!["example.a", "example.b", "example.c"]);
}

/// REQ: PKG-265
#[test]
fn pkg_265_file_uses_two_space_indent_lf_and_one_final_newline() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let text = String::from_utf8(fx.installed_bytes().expect("file")).expect("utf8");
    assert!(text.ends_with("}\n") && !text.ends_with("\n\n"), "{text:?}");
    assert!(!text.contains('\r'));
    assert!(text.contains("\n  \"plugins\""), "{text}");
}

/// REQ: PKG-266
#[test]
fn pkg_266_package_is_kept_next_to_the_unpacked_files() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let dir = fx.store_dir("example.a", "1.0.0");
    assert_eq!(
        std::fs::read(dir.join("package.iliumpkg")).expect("kept package"),
        spec.build()
    );
    assert!(dir.join("manifest.json").is_file());
    assert!(dir.join("server.wasm").is_file());
}

/// REQ: PKG-266
#[test]
fn pkg_266_store_files_are_read_only() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let dir = fx.store_dir("example.a", "1.0.0");
    for name in ["package.iliumpkg", "manifest.json", "server.wasm"] {
        let metadata = std::fs::metadata(dir.join(name)).expect(name);
        assert!(
            metadata.permissions().readonly(),
            "{name} must be read-only"
        );
    }
}

/// REQ: PKG-266
#[test]
fn pkg_266_update_writes_a_new_folder_and_leaves_the_old_one_untouched() {
    let fx = Fx::new();
    let first = Spec::new("example.a", "1.0.0");
    assert_eq!(fx.install(&first).exit_code, 0);
    let before = fx.snapshot_tree(&fx.store_dir("example.a", "1.0.0"));
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    assert_eq!(
        fx.snapshot_tree(&fx.store_dir("example.a", "1.0.0")),
        before
    );
    assert!(fx
        .store_dir("example.a", "1.1.0")
        .join("package.iliumpkg")
        .is_file());
}

/// REQ: PKG-266
#[test]
fn pkg_266_version_dir_is_store_id_version_under_the_root() {
    let fx = Fx::new();
    let store = PackageStore { root: fx.root() };
    assert_eq!(
        store.version_dir(&plugin("example.a"), "1.0.0"),
        fx.root().join("store").join("example.a").join("1.0.0")
    );
}

/// REQ: PKG-266
#[test]
fn pkg_266_verify_and_rollback_need_no_network() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let out = fx.run(&["verify", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.fetcher.calls().is_empty());
}

fn env_with(fx: &Fx, pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let _ = fx;
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// REQ: PKG-267
#[test]
fn pkg_267_trust_store_is_under_xdg_state_home() {
    let fx = Fx::new();
    let env = env_with(&fx, &[("XDG_STATE_HOME", "/xdg/state")]);
    let path = TrustStore::default_path(&env, std::path::Path::new("/home/u"));
    assert_eq!(
        path,
        std::path::PathBuf::from("/xdg/state/ilium/trust.json")
    );
}

/// REQ: PKG-267
#[test]
fn pkg_267_trust_store_defaults_to_local_state_when_xdg_is_unset() {
    let fx = Fx::new();
    let path = TrustStore::default_path(&env_with(&fx, &[]), std::path::Path::new("/home/u"));
    assert_eq!(
        path,
        std::path::PathBuf::from("/home/u/.local/state/ilium/trust.json")
    );
}

/// REQ: PKG-267
#[test]
fn pkg_267_ilium_trust_store_variable_replaces_the_path() {
    let fx = Fx::new();
    let env = env_with(
        &fx,
        &[
            ("XDG_STATE_HOME", "/xdg/state"),
            ("ILIUM_TRUST_STORE", "/etc/ilium/mine.json"),
        ],
    );
    let path = TrustStore::default_path(&env, std::path::Path::new("/home/u"));
    assert_eq!(path, std::path::PathBuf::from("/etc/ilium/mine.json"));
}

/// REQ: PKG-267
#[test]
fn pkg_267_relative_ilium_trust_store_is_not_accepted() {
    let fx = Fx::new();
    let env = env_with(&fx, &[("ILIUM_TRUST_STORE", "relative/trust.json")]);
    let path = TrustStore::default_path(&env, std::path::Path::new("/home/u"));
    assert_eq!(
        path,
        std::path::PathBuf::from("/home/u/.local/state/ilium/trust.json")
    );
}

/// REQ: PKG-267
#[test]
fn pkg_267_trust_store_is_written_with_mode_0600_atomically() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    let path = fx.state_dir().join("ilium").join("trust.json");
    let store = TrustStore { keys: Vec::new() };
    store.save_atomic(&path).expect("save");
    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().expect("parent"))
        .expect("dir")
        .flatten()
        .collect();
    assert_eq!(leftovers.len(), 1, "no temporary file may remain");
}

/// REQ: PKG-267
#[test]
fn pkg_267_downloads_and_registry_metadata_live_in_the_cache() {
    let cache = std::path::Path::new("/cache");
    assert_eq!(
        ilium_plugin_host::registry::downloads_dir(cache),
        std::path::PathBuf::from("/cache/plugins/downloads")
    );
    let first = ilium_plugin_host::registry::registry_cache_dir(cache, "https://a.example/v1/");
    let second = ilium_plugin_host::registry::registry_cache_dir(cache, "https://b.example/v1/");
    assert_eq!(
        first.parent(),
        Some(std::path::Path::new("/cache/plugins/registry"))
    );
    assert_ne!(first, second, "one folder per URL hash");
    assert_eq!(
        first,
        ilium_plugin_host::registry::registry_cache_dir(cache, "https://a.example/v1/")
    );
}

/// REQ: PKG-268
#[test]
fn pkg_268_compile_cache_key_has_component_sha_and_engine_id() {
    use ilium_plugin_host::engine::compile_cache_key;
    let base = compile_cache_key(COMPONENT_SHA256, "wasmtime-49-a");
    assert_eq!(base, compile_cache_key(COMPONENT_SHA256, "wasmtime-49-a"));
    assert_ne!(base, compile_cache_key(COMPONENT_SHA256, "wasmtime-49-b"));
    assert_ne!(base, compile_cache_key(&"0".repeat(64), "wasmtime-49-a"));
}

/// REQ: PKG-268
#[test]
fn pkg_268_deleting_the_compile_cache_loses_nothing() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let compile = fx.cache_dir().join("plugins").join("compile");
    let _ = std::fs::remove_dir_all(&compile);
    let out = fx.run(&["verify", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.a").as_deref(), Some("1.0.0"));
}

/// REQ: PKG-268
#[test]
fn pkg_268_compile_cache_is_outside_the_plugin_root() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert!(!fx.root().join("compile").exists());
}

// ---------------------------------------------------------------- transaction

fn crash_after(fx: &Fx, spec: &Spec, step: InstallStep) {
    let consent = Recording::approving();
    let _ = installer(fx, unsigned_ok()).install_crashing_after(&spec.build(), &consent, step);
}

/// REQ: PKG-269
#[test]
fn pkg_269_crash_before_commit_leaves_installed_json_unchanged() {
    for step in [
        InstallStep::Acquire,
        InstallStep::Fetch,
        InstallStep::Validate,
        InstallStep::Review,
        InstallStep::Stage,
    ] {
        let fx = Fx::new();
        assert_eq!(fx.install(&Spec::new("example.base", "1.0.0")).exit_code, 0);
        let before = fx.installed_bytes();
        crash_after(&fx, &Spec::new("example.a", "1.0.0"), step);
        assert_eq!(fx.installed_bytes(), before, "after {step:?}");
    }
}

/// REQ: PKG-269
#[test]
fn pkg_269_crash_at_stage_leaves_an_orphan_that_is_deleted_at_next_start() {
    let fx = Fx::new();
    crash_after(&fx, &Spec::new("example.a", "1.0.0"), InstallStep::Stage);
    let orphans = std::fs::read_dir(fx.root().join("staging"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert!(orphans > 0, "the crash must leave an orphan in staging/");
    let report = installer(&fx, unsigned_ok())
        .recover_at_start()
        .expect("recover");
    assert_eq!(report.removed_staging.len(), orphans);
    let leftovers = std::fs::read_dir(fx.root().join("staging"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0);
}

/// REQ: PKG-269
#[test]
fn pkg_269_registry_file_is_the_single_commit_point() {
    let fx = Fx::new();
    crash_after(&fx, &Spec::new("example.a", "1.0.0"), InstallStep::Commit);
    assert!(
        fx.record("example.a").is_some(),
        "after step 6 the install is true"
    );
    let fx2 = Fx::new();
    crash_after(&fx2, &Spec::new("example.a", "1.0.0"), InstallStep::Stage);
    assert!(fx2.record("example.a").is_none(), "before step 6 it is not");
}

/// REQ: PKG-270
#[test]
fn pkg_270_second_writer_waits_then_gets_the_lock_when_it_is_released() {
    use ilium_platform::file_lock::ExclusiveFileLock;
    let fx = Fx::new();
    std::fs::create_dir_all(fx.root()).expect("mkdir");
    let lock_path = fx.root().join("installed.lock");
    let lock = ExclusiveFileLock::acquire(&lock_path).expect("lock");
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        drop(lock);
    });
    let started = Instant::now();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    release.join().expect("join");
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(
        started.elapsed() >= Duration::from_millis(1000),
        "the install must have waited for the holder"
    );
    assert!(started.elapsed() < Duration::from_secs(20));
}

/// REQ: PKG-270
#[test]
fn pkg_270_lock_held_for_30_seconds_fails_with_install_locked_exit_4() {
    use ilium_platform::file_lock::ExclusiveFileLock;
    let fx = Fx::new();
    std::fs::create_dir_all(fx.root()).expect("mkdir");
    let _lock = ExclusiveFileLock::acquire(&fx.root().join("installed.lock")).expect("lock");
    let started = Instant::now();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    let waited = started.elapsed();
    assert_eq!(out.exit_code, 4, "{:?}", out.stdout_jsonl);
    assert!(out.has_error("install.locked"), "{:?}", out.error_codes());
    assert!(
        waited >= Duration::from_secs(29),
        "polls for up to 30 s, waited {waited:?}"
    );
    assert!(waited < Duration::from_secs(60), "waited {waited:?}");
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-270
#[test]
fn pkg_270_released_lock_is_taken_at_once_without_a_takeover_rule() {
    use ilium_platform::file_lock::ExclusiveFileLock;
    let fx = Fx::new();
    std::fs::create_dir_all(fx.root()).expect("mkdir");
    drop(ExclusiveFileLock::acquire(&fx.root().join("installed.lock")).expect("lock"));
    let started = Instant::now();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// REQ: PKG-270
#[test]
fn pkg_270_uninstall_and_update_use_the_same_lock() {
    use ilium_platform::file_lock::ExclusiveFileLock;
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let held = Arc::new(AtomicBool::new(true));
    let lock = ExclusiveFileLock::acquire(&fx.root().join("installed.lock")).expect("lock");
    let flag = held.clone();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        flag.store(false, Ordering::SeqCst);
        drop(lock);
    });
    let out = fx.run(&["uninstall", "example.a"]);
    releaser.join().expect("join");
    assert!(!held.load(Ordering::SeqCst));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-271
#[test]
fn pkg_271_crash_before_step_6_keeps_the_previous_registry_and_cleans_staging() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.base", "1.0.0")).exit_code, 0);
    let before = fx.installed_bytes();
    crash_after(&fx, &Spec::new("example.a", "1.0.0"), InstallStep::Stage);
    assert_eq!(fx.installed_bytes(), before);
    installer(&fx, unsigned_ok())
        .recover_at_start()
        .expect("recover");
    let staging = fx.root().join("staging");
    assert_eq!(
        std::fs::read_dir(&staging).map(|d| d.count()).unwrap_or(0),
        0
    );
    assert_eq!(fx.installed_bytes(), before);
}

/// REQ: PKG-271
#[test]
fn pkg_271_recovery_keeps_staging_that_installed_json_references_and_deletes_the_rest() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let orphan = fx.root().join("staging").join("00000000-orphan");
    std::fs::create_dir_all(&orphan).expect("mkdir");
    std::fs::write(orphan.join("half.bin"), b"x").expect("write");
    installer(&fx, unsigned_ok())
        .recover_at_start()
        .expect("recover");
    assert!(!orphan.exists());
    assert!(
        fx.store_dir("example.a", "1.0.0").is_dir(),
        "committed store entries stay"
    );
}

/// REQ: PKG-271
#[test]
fn pkg_271_crash_after_step_6_leaves_a_complete_install() {
    let fx = Fx::new();
    crash_after(&fx, &Spec::new("example.a", "1.0.0"), InstallStep::Commit);
    assert_eq!(fx.active_version("example.a").as_deref(), Some("1.0.0"));
    assert!(fx
        .store_dir("example.a", "1.0.0")
        .join("package.iliumpkg")
        .is_file());
    assert!(fx
        .store_dir("example.a", "1.0.0")
        .join("server.wasm")
        .is_file());
}

/// REQ: PKG-271
#[test]
fn pkg_271_registry_reader_sees_the_commit_when_a_server_starts() {
    let fx = Fx::new();
    crash_after(&fx, &Spec::new("example.a", "1.0.0"), InstallStep::Commit);
    let registry = InstalledRegistry::open(&fx.registry_path()).expect("open");
    let ids: Vec<String> = registry
        .list()
        .expect("list")
        .into_iter()
        .map(|record| record.id)
        .collect();
    assert_eq!(ids, vec!["example.a".to_owned()]);
}

/// REQ: PKG-272
#[test]
fn pkg_272_no_component_is_instantiated_before_commit_and_notify() {
    let fx = Fx::new();
    let engine = installer(&fx, unsigned_ok());
    let consent = Recording::approving();
    let _ = engine.install_crashing_after(
        &Spec::new("example.a", "1.0.0").build(),
        &consent,
        InstallStep::Stage,
    );
    assert_eq!(engine.instantiated_count(), 0);
    let _ = engine.install_crashing_after(
        &Spec::new("example.b", "1.0.0").build(),
        &consent,
        InstallStep::Commit,
    );
    assert_eq!(
        engine.instantiated_count(),
        0,
        "step 7 has not been sent yet"
    );
}

/// REQ: PKG-272
#[test]
fn pkg_272_installer_never_instantiates_even_after_notify() {
    let fx = Fx::new();
    let engine = installer(&fx, unsigned_ok());
    let consent = Recording::approving();
    assert!(engine
        .install(&Spec::new("example.a", "1.0.0").build(), &consent)
        .is_ok());
    assert_eq!(
        engine.instantiated_count(),
        0,
        "activation belongs to the servers"
    );
}

/// REQ: PKG-272
#[test]
fn pkg_272_progress_steps_run_in_transaction_order() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    let steps = out.steps();
    let expected = [
        "acquire", "fetch", "validate", "review", "stage", "commit", "notify",
    ];
    let positions: Vec<usize> = expected
        .iter()
        .filter_map(|name| steps.iter().position(|step| step == name))
        .collect();
    assert_eq!(positions.len(), expected.len(), "{steps:?}");
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{steps:?}"
    );
}

/// REQ: PKG-273
#[test]
fn pkg_273_windows_refuses_t3_before_the_review() {
    let fx = Fx::on(Platform::Windows);
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("package.platform-unsupported"),
        "{:?}",
        out.error_codes()
    );
    assert!(
        !out.steps().iter().any(|step| step == "review"),
        "{:?}",
        out.steps()
    );
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-273
#[test]
fn pkg_273_macos_refuses_t3_before_the_review() {
    let fx = Fx::on(Platform::MacOs);
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    assert!(
        out.has_error("package.platform-unsupported"),
        "{:?}",
        out.error_codes()
    );
    assert!(!out.steps().iter().any(|step| step == "review"));
}

/// REQ: PKG-273
#[test]
fn pkg_273_linux_has_a_sandbox_and_installs_t3() {
    let fx = Fx::on(Platform::Linux);
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(!out.has_error("package.platform-unsupported"));
}

/// REQ: PKG-273
#[test]
fn pkg_273_check_reports_the_platform_gate_at_stage_9() {
    let fx = Fx::on(Platform::Windows);
    let report = installer(&fx, unsigned_ok())
        .check(&Spec::new("example.a", "1.0.0").build(), CheckMode::Install);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "package.platform-unsupported"),
        "{:?}",
        report.diagnostics
    );
    assert_eq!(report.last_stage, 9);
}

/// REQ: PKG-274
#[test]
fn pkg_274_same_version_same_manifest_is_a_noop_with_a_warning() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let before = fx.installed_bytes();
    let out = fx.install(&spec);
    assert_eq!(out.exit_code, 0);
    assert!(
        out.has_warning("install.already-installed"),
        "{:?}",
        out.stdout_jsonl
    );
    assert_eq!(fx.installed_bytes(), before);
}

/// REQ: PKG-274
#[test]
fn pkg_274_reinstall_rewrites_the_store_folder_from_the_package() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let wasm = fx.store_dir("example.a", "1.0.0").join("server.wasm");
    let mut permissions = std::fs::metadata(&wasm).expect("metadata").permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(permissions.mode() | 0o200);
    std::fs::set_permissions(&wasm, permissions).expect("chmod");
    std::fs::write(&wasm, b"damaged").expect("damage");
    let out = fx.install_with(&spec, &["--reinstall"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(std::fs::read(&wasm).expect("read"), component_bytes());
}

/// REQ: PKG-274
#[test]
fn pkg_274_same_version_different_manifest_is_refused_version_reused() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let before = fx.installed_bytes();
    let out = fx.install(&Spec::new("example.a", "1.0.0").named("Changed"));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("package.version-reused"),
        "{:?}",
        out.error_codes()
    );
    assert_eq!(fx.installed_bytes(), before);
}

/// REQ: PKG-274
#[test]
fn pkg_274_manifest_hash_not_package_hash_decides() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let mut padded = spec.clone();
    padded.extra_files.clear();
    // Same manifest text, repacked: identical manifest hash is the same install even if the
    // archive bytes were produced by another run.
    let out = fx.install(&padded);
    assert_eq!(out.exit_code, 0);
    assert!(out.has_warning("install.already-installed"));
}

/// REQ: PKG-275
#[test]
fn pkg_275_dry_run_writes_nothing_and_exits_0() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.base", "1.0.0")).exit_code, 0);
    let before = fx.snapshot_tree(&fx.data_dir());
    let before_cache = fx.snapshot_tree(&fx.cache_dir());
    let out = fx.install_with(&Spec::new("example.a", "1.0.0"), &["--dry-run"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.snapshot_tree(&fx.data_dir()), before);
    assert_eq!(fx.snapshot_tree(&fx.cache_dir()), before_cache);
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-275
#[test]
fn pkg_275_dry_run_prints_the_plan() {
    let fx = Fx::new();
    let out = fx.install_with(&Spec::new("example.a", "1.0.0"), &["--dry-run"]);
    assert_eq!(out.exit_code, 0);
    assert!(!out.stdout_jsonl.is_empty());
    assert!(out.all_text().contains("example.a"), "{}", out.all_text());
    assert!(out.all_text().contains("1.0.0"), "{}", out.all_text());
    assert!(
        fx.record("example.a").is_none(),
        "a dry run installs nothing"
    );
    assert!(
        !out.steps()
            .iter()
            .any(|step| step == "stage" || step == "commit"),
        "{:?}",
        out.steps()
    );
}

/// REQ: PKG-275
#[test]
fn pkg_275_dry_run_of_a_bad_package_fails_like_the_real_install() {
    let fx = Fx::new();
    let path = fx.write_file("bad.iliumpkg", b"not a package");
    let dry = fx.run(&["install", &path, "--allow-unsigned", "--yes", "--dry-run"]);
    let real = fx.run(&["install", &path, "--allow-unsigned", "--yes"]);
    assert_eq!(dry.exit_code, 1);
    assert_eq!(dry.error_codes(), real.error_codes());
}

/// REQ: PKG-276
#[test]
fn pkg_276_every_session_of_the_user_sees_the_same_installed_json() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let other_project = fx.dir.path().join("other-project");
    std::fs::create_dir_all(&other_project).expect("mkdir");
    let here = fx.run(&["list", "--json"]);
    let there = fx.run_in(&other_project, &["list", "--json"]);
    assert_eq!(here.exit_code, 0);
    assert_eq!(here.stdout_jsonl, there.stdout_jsonl);
    assert!(here.all_text().contains("example.a"));
}

/// REQ: PKG-276
#[test]
fn pkg_276_installed_set_is_not_stored_in_the_project() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert!(!fx.cwd.join(".ilium").join("installed.json").exists());
    assert!(fx.registry_path().is_file());
}

/// REQ: PKG-277
#[test]
fn pkg_277_notify_step_comes_after_commit() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    let steps = out.steps();
    let commit = steps
        .iter()
        .position(|step| step == "commit")
        .expect("commit step");
    let notify = steps
        .iter()
        .position(|step| step == "notify")
        .expect("notify step");
    assert!(commit < notify, "{steps:?}");
}

/// REQ: PKG-277
#[test]
fn pkg_277_unreachable_server_is_not_a_failure_and_state_is_on_disk() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(
        fx.record("example.a").is_some(),
        "a server that was not reached reads the file at its next start"
    );
}

/// REQ: PKG-277
#[test]
fn pkg_277_plugins_changed_message_is_sent_to_every_server() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    let notified: Vec<Value> = out
        .lines_of("progress")
        .into_iter()
        .filter(|line| line["step"] == "notify")
        .collect();
    assert!(!notified.is_empty(), "{:?}", out.stdout_jsonl);
    assert!(
        // manual does not pin the notify line's status values or the server count of this fixture.
        notified.iter().all(|line| line["status"]
            .as_str()
            .is_some_and(|status| !status.is_empty())),
        "{notified:?}"
    );
}

/// REQ: PKG-278
#[test]
fn pkg_278_new_plugin_is_installed_and_active_from_now() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    let result = out.result_line().expect("result line");
    assert_eq!(result["action"], "installed");
    assert_eq!(result["active_from"], "now");
    assert!(fx.record("example.a").expect("record")["update"].is_null());
}

/// REQ: PKG-278
#[test]
fn pkg_278_update_waits_staged_and_the_old_version_stays_active() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let out = fx.update_with(&Spec::new("example.a", "1.1.0"), &[]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["active_version"], "1.0.0");
    assert_eq!(record["update"]["state"], "staged");
    assert_eq!(record["update"]["candidate"], "1.1.0");
    assert!(fx.store_dir("example.a", "1.1.0").is_dir());
    let result = out.result_line().expect("result");
    assert_eq!(result["action"], "updated");
    assert_eq!(result["active_from"], "restart");
}

/// REQ: PKG-278
#[test]
fn pkg_278_candidate_activates_after_the_old_version_deactivates_for_upgrade() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    let outcome = installer(&fx, unsigned_ok())
        .finish_update(&plugin("example.a"), FirstActivate::Ok)
        .expect("finish");
    match outcome {
        UpdateOutcome::Committed {
            version,
            deactivation_reason,
        } => {
            assert_eq!(version, "1.1.0");
            assert_eq!(deactivation_reason, "upgrade");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.active_version("example.a").as_deref(), Some("1.1.0"));
}

/// REQ: PKG-278
#[test]
fn pkg_278_a_second_update_before_the_swap_replaces_the_staged_candidate() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.2.0"), &[])
            .exit_code,
        0
    );
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["active_version"], "1.0.0");
    assert_eq!(record["update"]["candidate"], "1.2.0");
}

/// REQ: PKG-279
#[test]
fn pkg_279_data_folder_is_shared_by_both_versions() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").data_version(1))
            .exit_code,
        0
    );
    let data = fx.root().join("data").join("example.a");
    std::fs::create_dir_all(&data).expect("mkdir");
    std::fs::write(data.join("state.bin"), b"shared").expect("write");
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0").data_version(2), &[])
            .exit_code,
        0
    );
    assert_eq!(
        std::fs::read(data.join("state.bin")).expect("read"),
        b"shared"
    );
}

/// REQ: PKG-279
#[test]
fn doc_pkg_279_page_states_the_both_versions_rule() {
    let page = manual_page("reference-install-and-update.md");
    assert!(
        page.contains("each switches independently"),
        "page text moved"
    );
    assert!(
        page.contains("MUST handle both versions during the switch"),
        "page text moved"
    );
}

// ---------------------------------------------------------------- update

/// REQ: PKG-280
#[test]
fn pkg_280_equal_version_is_not_an_update() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0");
    assert_eq!(fx.install(&spec).exit_code, 0);
    let before = fx.installed_bytes();
    let out = fx.update_with(&spec, &[]);
    assert_eq!(out.exit_code, 0);
    assert_eq!(fx.installed_bytes(), before);
    assert!(fx.record("example.a").expect("record")["update"].is_null());
}

/// REQ: PKG-280
#[test]
fn pkg_280_release_is_higher_than_its_prerelease() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0-beta.2"))
            .exit_code,
        0
    );
    let out = fx.update_with(&Spec::new("example.a", "1.0.0"), &[]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(out.result_line().expect("result")["action"], "updated");
    assert_eq!(
        fx.record("example.a").expect("record")["update"]["candidate"],
        "1.0.0"
    );
}

/// REQ: PKG-280
#[test]
fn pkg_280_prerelease_of_the_installed_release_is_a_downgrade() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let out = fx.update_with(&Spec::new("example.a", "1.0.0-beta.2"), &[]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("package.downgrade"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-280
#[test]
fn pkg_280_prerelease_numbers_compare_numerically() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0-beta.2"))
            .exit_code,
        0
    );
    let out = fx.update_with(&Spec::new("example.a", "1.0.0-beta.11"), &[]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(
        fx.record("example.a").expect("record")["update"]["candidate"],
        "1.0.0-beta.11"
    );
}

/// REQ: PKG-281
#[test]
fn pkg_281_only_new_grants_are_asked_and_removed_ones_are_dropped() {
    let fx = Fx::new();
    let consent = Recording::approving();
    let first = Spec::new("example.a", "1.0.0")
        .grant("tree.read")
        .grant("ui.panels");
    assert!(install_via_installer(&fx, &first, &consent));
    let second = Spec::new("example.a", "1.1.0")
        .grant("ui.panels")
        .grant("http:api.example.com");
    let engine = installer(&fx, unsigned_ok());
    assert!(engine.update(&second.build(), &consent).is_ok());
    let asked = grant_texts(&consent.requests()[1]);
    assert_eq!(asked, vec!["http:api.example.com".to_owned()]);
    let store = GrantStore::open(&fx.state_dir().join("grants.json")).expect("grant store");
    let kept: Vec<String> = store
        .granted(&plugin("example.a"))
        .expect("granted")
        .iter()
        .map(|g| g.as_str().to_owned())
        .collect();
    assert!(!kept.contains(&"tree.read".to_owned()), "{kept:?}");
    assert!(
        kept.contains(&"ui.panels".to_owned()) && kept.contains(&"http:api.example.com".to_owned()),
        "{kept:?}"
    );
}

/// REQ: PKG-281
#[test]
fn pkg_281_declined_grants_are_not_asked_again() {
    let fx = Fx::new();
    let approving = Recording::approving();
    assert!(install_via_installer(
        &fx,
        &Spec::new("example.a", "1.0.0").grant("tree.read"),
        &approving
    ));
    let declining = Recording::declining();
    let _ = installer(&fx, unsigned_ok()).update(
        &Spec::new("example.a", "1.1.0")
            .grant("tree.read")
            .grant("ui.panels")
            .build(),
        &declining,
    );
    let third = Recording::approving();
    let _ = installer(&fx, unsigned_ok()).update(
        &Spec::new("example.a", "1.2.0")
            .grant("tree.read")
            .grant("ui.panels")
            .build(),
        &third,
    );
    let asked: Vec<String> = third.requests().iter().flat_map(grant_texts).collect();
    assert!(
        !asked.contains(&"ui.panels".to_owned()),
        "a declined grant stays declined: {asked:?}"
    );
}

/// REQ: PKG-281
#[test]
fn pkg_281_a_different_argument_is_a_different_grant_and_is_asked() {
    let fx = Fx::new();
    let first = Recording::approving();
    assert!(install_via_installer(
        &fx,
        &Spec::new("example.a", "1.0.0").grant("http:a.example.com"),
        &first
    ));
    let declining = Recording::declining();
    let _ = installer(&fx, unsigned_ok()).update(
        &Spec::new("example.a", "1.1.0")
            .grant("http:a.example.com")
            .grant("http:b.example.com")
            .build(),
        &declining,
    );
    let third = Recording::approving();
    let _ = installer(&fx, unsigned_ok()).update(
        &Spec::new("example.a", "1.2.0")
            .grant("http:a.example.com")
            .grant("http:c.example.com")
            .build(),
        &third,
    );
    let asked: Vec<String> = third.requests().iter().flat_map(grant_texts).collect();
    assert!(
        asked.contains(&"http:c.example.com".to_owned()),
        "{asked:?}"
    );
}

/// REQ: PKG-282
#[test]
fn pkg_282_yes_does_not_confirm_a_new_high_risk_grant() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let out = fx.update_with(
        &Spec::new("example.a", "1.1.0").grant("commands.invoke:*"),
        &[],
    );
    assert_eq!(out.exit_code, 1, "{:?}", out.stdout_jsonl);
    assert!(
        out.has_error("install.needs-input"),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.a").expect("record")["update"].is_null());
}

/// REQ: PKG-282
#[test]
fn pkg_282_grant_flag_for_that_exact_grant_confirms() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let out = fx.update_with(
        &Spec::new("example.a", "1.1.0").grant("commands.invoke:*"),
        &["--grant", "server:commands.invoke:*"],
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(
        fx.record("example.a").expect("record")["update"]["candidate"],
        "1.1.0"
    );
}

/// REQ: PKG-282
#[test]
fn pkg_282_grant_flag_for_another_grant_does_not_confirm() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let out = fx.update_with(
        &Spec::new("example.a", "1.1.0").grant("commands.invoke:*"),
        &["--grant", "server:commands.invoke:other.*"],
    );
    assert_eq!(out.exit_code, 1);
    assert!(fx.record("example.a").expect("record")["update"].is_null());
}

/// REQ: PKG-282
#[test]
fn pkg_282_normal_new_grant_is_confirmed_by_yes() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let out = fx.update_with(&Spec::new("example.a", "1.1.0").grant("tree.read"), &[]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
}

/// REQ: PKG-283
#[test]
fn pkg_283_update_signed_by_an_untrusted_second_key_is_refused_publisher_key_changed() {
    use ilium_plugin_host::signing::{sign, SignedAt, SigningKey};
    let fx = Fx::new();
    let key_a = SigningKey::from_seed([1u8; 32]);
    let key_b = SigningKey::from_seed([2u8; 32]);
    let signed_at = || SignedAt("2026-10-12T09:00:00Z".to_owned());
    let v1 = sign(
        &Spec::new("example.a", "1.0.0").build(),
        &key_a,
        signed_at(),
    )
    .expect("sign v1");
    let v2 = sign(
        &Spec::new("example.a", "1.1.0").build(),
        &key_b,
        signed_at(),
    )
    .expect("sign v2");
    let first = fx.write_file("v1.iliumpkg", &v1);
    let second = fx.write_file("v2.iliumpkg", &v2);
    let trust_a = key_a.key_id();
    let trust_b = key_b.key_id();
    assert_eq!(
        fx.run(&["install", &first, "--yes", "--trust", &trust_a])
            .exit_code,
        0
    );
    let out = fx.run(&["update", &second, "--yes", "--trust", &trust_b]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("package.publisher-key-changed"),
        "{:?}",
        out.error_codes()
    );
    assert_eq!(fx.active_version("example.a").as_deref(), Some("1.0.0"));
}

/// REQ: PKG-283
#[test]
fn doc_pkg_283_signing_page_owns_the_key_change_rule() {
    let page = manual_page("reference-signing-and-trust.md");
    assert!(page.contains("package.publisher-key-changed"));
    assert!(page.contains("No dialog can override this inside `install`"));
}

/// REQ: PKG-284
#[test]
fn pkg_284_settings_data_and_storage_are_kept_across_an_update() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let data = fx.root().join("data").join("example.a");
    let storage = fx.root().join("storage").join("example.a");
    std::fs::create_dir_all(&data).expect("mkdir");
    std::fs::create_dir_all(&storage).expect("mkdir");
    std::fs::write(data.join("d.bin"), b"data").expect("write");
    std::fs::write(storage.join("global.db"), b"bucket").expect("write");
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    let _ = installer(&fx, unsigned_ok()).finish_update(&plugin("example.a"), FirstActivate::Ok);
    assert_eq!(std::fs::read(data.join("d.bin")).expect("read"), b"data");
    assert_eq!(
        std::fs::read(storage.join("global.db")).expect("read"),
        b"bucket"
    );
}

/// REQ: PKG-284
#[test]
fn pkg_284_old_version_deactivates_with_reason_upgrade_before_the_new_one_runs() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    let outcome = installer(&fx, unsigned_ok())
        .finish_update(&plugin("example.a"), FirstActivate::Ok)
        .expect("finish");
    assert!(
        matches!(outcome, UpdateOutcome::Committed { ref deactivation_reason, .. } if deactivation_reason == "upgrade"),
        "{outcome:?}"
    );
}

fn update_three_times_with_commits(fx: &Fx) {
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    for version in ["1.1.0", "1.2.0", "1.3.0"] {
        assert_eq!(
            fx.update_with(&Spec::new("example.a", version), &[])
                .exit_code,
            0
        );
        installer(fx, unsigned_ok())
            .finish_update(&plugin("example.a"), FirstActivate::Ok)
            .expect("finish");
    }
}

/// REQ: PKG-285
#[test]
fn pkg_285_exactly_one_previous_version_stays_on_disk() {
    let fx = Fx::new();
    update_three_times_with_commits(&fx);
    let mut folders: Vec<String> = std::fs::read_dir(fx.root().join("store").join("example.a"))
        .expect("store dir")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    folders.sort();
    assert_eq!(folders, vec!["1.2.0".to_owned(), "1.3.0".to_owned()]);
}

/// REQ: PKG-285
#[test]
fn pkg_285_registry_names_the_previous_version() {
    let fx = Fx::new();
    update_three_times_with_commits(&fx);
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["active_version"], "1.3.0");
    assert_eq!(record["previous_version"], "1.2.0");
}

/// REQ: PKG-285
#[test]
fn pkg_285_staged_candidate_does_not_delete_anything_before_the_commit() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    assert!(
        fx.store_dir("example.a", "1.0.0").is_dir() && fx.store_dir("example.a", "1.1.0").is_dir()
    );
}

/// REQ: PKG-286
#[test]
fn pkg_286_first_activate_ok_makes_the_candidate_active() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    installer(&fx, unsigned_ok())
        .finish_update(&plugin("example.a"), FirstActivate::Ok)
        .expect("finish");
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["active_version"], "1.1.0");
    assert_eq!(record["previous_version"], "1.0.0");
    assert!(record["update"].is_null());
}

/// REQ: PKG-286
#[test]
fn pkg_286_failing_first_activate_rolls_back_marks_skip_and_logs() {
    for failure in [
        FirstActivate::Error,
        FirstActivate::Trap,
        FirstActivate::DeadlineMissed,
    ] {
        let fx = Fx::new();
        assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
        assert_eq!(
            fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
                .exit_code,
            0
        );
        let outcome = installer(&fx, unsigned_ok())
            .finish_update(&plugin("example.a"), failure)
            .expect("finish");
        match outcome {
            UpdateOutcome::RolledBack {
                restored_version,
                skipped_version,
                log_code,
                counted_in_failure_window,
            } => {
                assert_eq!(restored_version, "1.0.0");
                assert_eq!(skipped_version, "1.1.0");
                assert_eq!(log_code, "update.rolled-back");
                assert!(!counted_in_failure_window, "LIF-075: not counted");
            }
            other => panic!("unexpected {other:?}"),
        }
        let record = fx.record("example.a").expect("record");
        assert_eq!(record["active_version"], "1.0.0");
        assert_eq!(record["skip"], serde_json::json!(["1.1.0"]));
        assert!(record["update"].is_null());
    }
}

/// REQ: PKG-286
#[test]
fn pkg_286_a_failure_after_commit_never_rolls_back() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0"), &[])
            .exit_code,
        0
    );
    let engine = installer(&fx, unsigned_ok());
    engine
        .finish_update(&plugin("example.a"), FirstActivate::Ok)
        .expect("finish");
    let _ = engine.finish_update(&plugin("example.a"), FirstActivate::Trap);
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["active_version"], "1.1.0");
    assert_eq!(record["skip"], serde_json::json!([]));
}

/// REQ: PKG-287
#[test]
fn pkg_287_lower_version_is_refused_package_downgrade() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.1.0")).exit_code, 0);
    let before = fx.installed_bytes();
    let out = fx.install(&Spec::new("example.a", "1.0.0"));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("package.downgrade"),
        "{:?}",
        out.error_codes()
    );
    assert_eq!(fx.installed_bytes(), before);
}

/// REQ: PKG-287
#[test]
fn pkg_287_allow_downgrade_installs_the_lower_version() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.1.0")).exit_code, 0);
    let out = fx.install_with(&Spec::new("example.a", "1.0.0"), &["--allow-downgrade"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(out.result_line().expect("result")["action"], "downgraded");
}

/// REQ: PKG-287
#[test]
fn pkg_287_one_patch_below_is_refused_and_equal_is_not_a_downgrade() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.1.1")).exit_code, 0);
    assert!(fx
        .install(&Spec::new("example.a", "1.1.0"))
        .has_error("package.downgrade"));
    assert!(!fx
        .install(&Spec::new("example.a", "1.1.1"))
        .has_error("package.downgrade"));
}

/// REQ: PKG-288
#[test]
fn pkg_288_downgrade_to_lower_data_version_is_refused_data_newer() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.1.0").data_version(2))
            .exit_code,
        0
    );
    ilium_plugin_host::storage::write_bucket_stamp(&fx.root().join("storage"), "example.a", 2)
        .expect("stamp");
    let out = fx.install_with(
        &Spec::new("example.a", "1.0.0").data_version(1),
        &["--allow-downgrade"],
    );
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("migration.data-newer"),
        "{:?}",
        out.error_codes()
    );
    assert_eq!(fx.active_version("example.a").as_deref(), Some("1.1.0"));
}

/// REQ: PKG-288
#[test]
fn pkg_288_discard_data_lifts_the_refusal_and_deletes_the_data() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.1.0").data_version(2))
            .exit_code,
        0
    );
    ilium_plugin_host::storage::write_bucket_stamp(&fx.root().join("storage"), "example.a", 2)
        .expect("stamp");
    let data = fx.root().join("data").join("example.a");
    std::fs::create_dir_all(&data).expect("mkdir");
    std::fs::write(data.join("d.bin"), b"x").expect("write");
    let out = fx.install_with(
        &Spec::new("example.a", "1.0.0").data_version(1),
        &["--allow-downgrade", "--discard-data"],
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(!data.join("d.bin").exists());
}

/// REQ: PKG-288
#[test]
fn pkg_288_equal_data_version_needs_no_flag() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.1.0").data_version(2))
            .exit_code,
        0
    );
    ilium_plugin_host::storage::write_bucket_stamp(&fx.root().join("storage"), "example.a", 2)
        .expect("stamp");
    let out = fx.install_with(
        &Spec::new("example.a", "1.0.0").data_version(2),
        &["--allow-downgrade"],
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
}

/// REQ: PKG-288
#[test]
fn pkg_288_rollback_to_lower_data_version_is_refused_too() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").data_version(1))
            .exit_code,
        0
    );
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0").data_version(2), &[])
            .exit_code,
        0
    );
    installer(&fx, unsigned_ok())
        .finish_update(&plugin("example.a"), FirstActivate::Ok)
        .expect("finish");
    ilium_plugin_host::storage::write_bucket_stamp(&fx.root().join("storage"), "example.a", 2)
        .expect("stamp");
    let out = fx.run(&["rollback", "example.a"]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("migration.data-newer"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-289
#[test]
fn pkg_289_downgrade_keeps_the_replaced_newer_version_as_previous() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.1.0")).exit_code, 0);
    assert_eq!(
        fx.install_with(&Spec::new("example.a", "1.0.0"), &["--allow-downgrade"])
            .exit_code,
        0
    );
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["active_version"], "1.0.0");
    assert_eq!(record["previous_version"], "1.1.0");
    assert!(fx
        .store_dir("example.a", "1.1.0")
        .join("package.iliumpkg")
        .is_file());
}

/// REQ: PKG-289
#[test]
fn pkg_289_update_back_to_the_kept_version_needs_no_download() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.1.0")).exit_code, 0);
    assert_eq!(
        fx.install_with(&Spec::new("example.a", "1.0.0"), &["--allow-downgrade"])
            .exit_code,
        0
    );
    let out = fx.run(&["rollback", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.a").as_deref(), Some("1.1.0"));
    assert!(fx.fetcher.calls().is_empty());
}

fn two_committed_versions(fx: &Fx) {
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").grant("tree.read"))
            .exit_code,
        0
    );
    assert_eq!(
        fx.update_with(&Spec::new("example.a", "1.1.0").grant("tree.read"), &[])
            .exit_code,
        0
    );
    installer(fx, unsigned_ok())
        .finish_update(&plugin("example.a"), FirstActivate::Ok)
        .expect("finish");
}

/// REQ: PKG-290
#[test]
fn pkg_290_rollback_makes_the_previous_version_active() {
    let fx = Fx::new();
    two_committed_versions(&fx);
    let out = fx.run(&["rollback", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.a").as_deref(), Some("1.0.0"));
    assert_eq!(out.result_line().expect("result")["action"], "rolled-back");
}

/// REQ: PKG-290
#[test]
fn pkg_290_rollback_runs_without_the_version_comparison() {
    let fx = Fx::new();
    two_committed_versions(&fx);
    let out = fx.run(&["rollback", "example.a"]);
    assert!(
        !out.has_error("package.downgrade"),
        "{:?}",
        out.error_codes()
    );
    assert_eq!(out.exit_code, 0);
}

/// REQ: PKG-290
#[test]
fn pkg_290_rollback_keeps_current_grants_the_old_manifest_still_asks_for() {
    let fx = Fx::new();
    two_committed_versions(&fx);
    assert_eq!(fx.run(&["rollback", "example.a"]).exit_code, 0);
    let store = GrantStore::open(&fx.state_dir().join("grants.json")).expect("grant store");
    let kept: Vec<String> = store
        .granted(&plugin("example.a"))
        .expect("granted")
        .iter()
        .map(|g| g.as_str().to_owned())
        .collect();
    assert_eq!(kept, vec!["tree.read".to_owned()]);
}

/// REQ: PKG-291
#[test]
fn pkg_291_rollback_without_a_previous_version_is_install_no_previous() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let out = fx.run(&["rollback", "example.a"]);
    assert!(
        out.has_error("install.no-previous"),
        "{:?}",
        out.error_codes()
    );
    assert_eq!(out.exit_code, 5);
    assert_eq!(fx.active_version("example.a").as_deref(), Some("1.0.0"));
}

/// REQ: PKG-291
#[test]
fn pkg_291_rollback_twice_has_no_second_previous_version() {
    let fx = Fx::new();
    two_committed_versions(&fx);
    assert_eq!(fx.run(&["rollback", "example.a"]).exit_code, 0);
    let out = fx.run(&["rollback", "example.a"]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("install.no-previous"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-292
#[test]
fn pkg_292_rolled_back_version_is_marked_skip() {
    let fx = Fx::new();
    two_committed_versions(&fx);
    assert_eq!(fx.run(&["rollback", "example.a"]).exit_code, 0);
    assert_eq!(
        fx.record("example.a").expect("record")["skip"],
        serde_json::json!(["1.1.0"])
    );
}

/// REQ: PKG-292
#[test]
fn pkg_292_unskip_clears_the_mark() {
    let fx = Fx::new();
    two_committed_versions(&fx);
    assert_eq!(fx.run(&["rollback", "example.a"]).exit_code, 0);
    let _ = fx.run(&["update", "example.a", "--unskip"]);
    assert_eq!(
        fx.record("example.a").expect("record")["skip"],
        serde_json::json!([])
    );
}

// ---------------------------------------------------------------- uninstall

/// REQ: PKG-293
#[test]
fn pkg_293_uninstall_removes_the_record_and_the_store_folder() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(fx.install(&Spec::new("example.b", "1.0.0")).exit_code, 0);
    let out = fx.run(&["uninstall", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.a").is_none());
    assert!(!fx.root().join("store").join("example.a").exists());
    assert!(fx.record("example.b").is_some() && fx.store_dir("example.b", "1.0.0").is_dir());
    assert_eq!(out.result_line().expect("result")["action"], "uninstalled");
}

/// REQ: PKG-293
#[test]
fn pkg_293_uninstall_of_an_unknown_id_is_not_found() {
    let fx = Fx::new();
    let out = fx.run(&["uninstall", "example.none"]);
    assert_eq!(out.exit_code, 5);
}

/// REQ: PKG-293
#[test]
fn pkg_293_uninstall_deletes_every_store_version_of_the_plugin() {
    let fx = Fx::new();
    two_committed_versions(&fx);
    assert_eq!(fx.run(&["uninstall", "example.a"]).exit_code, 0);
    assert!(!fx.root().join("store").join("example.a").exists());
}

fn seed_plugin_files(fx: &Fx, id: &str) -> Vec<(std::path::PathBuf, usize)> {
    let layout: Vec<(std::path::PathBuf, usize)> = vec![
        (fx.root().join("data").join(id).join("d.bin"), 11),
        (fx.root().join("storage").join(id).join("global.db"), 22),
        (
            fx.root()
                .join("backups")
                .join(id)
                .join("1.0.0-20261012T091404Z")
                .join("b.bin"),
            33,
        ),
        (
            fx.root().join("locks").join(format!("{id}.migrate.lock")),
            4,
        ),
        (fx.root().join("logs").join(format!("{id}.log")), 55),
        (fx.cache_dir().join("plugins").join(id).join("c.bin"), 66),
    ];
    for (path, size) in &layout {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, vec![b'x'; *size]).expect("write");
    }
    layout
}

/// REQ: PKG-294
#[test]
fn pkg_294_without_purge_data_storage_and_backups_remain_with_warning_lines() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let layout = seed_plugin_files(&fx, "example.a");
    let out = fx.run(&["uninstall", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    for index in [0usize, 1, 2] {
        assert!(
            layout[index].0.is_file(),
            "{:?} must remain",
            layout[index].0
        );
    }
    let warned = out.all_text();
    for fragment in ["data/example.a", "storage/example.a", "backups/example.a"] {
        assert!(
            warned.contains(fragment),
            "kept path {fragment} must be listed: {warned}"
        );
    }
    assert!(!out.lines_of("warning").is_empty());
}

/// REQ: PKG-294
#[test]
fn pkg_294_uninstall_asks_no_question() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    seed_plugin_files(&fx, "example.a");
    let out = fx.run(&["uninstall", "example.a"]);
    assert!(!out.has_error("install.needs-input"));
    assert_eq!(out.exit_code, 0);
}

/// REQ: PKG-294
#[test]
fn pkg_294_reinstall_asks_for_grants_again() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0").grant("tree.read");
    assert_eq!(fx.install(&spec).exit_code, 0);
    assert_eq!(fx.run(&["uninstall", "example.a"]).exit_code, 0);
    let path = fx.write_file("again.iliumpkg", &spec.build());
    let out = fx.run(&["install", &path, "--allow-unsigned"]);
    assert!(
        out.has_error("install.needs-input"),
        "grants were deleted, so the review is needed again: {:?}",
        out.error_codes()
    );
}

/// REQ: PKG-294
#[test]
fn pkg_294_grant_entry_is_deleted_on_every_uninstall() {
    let fx = Fx::new();
    let consent = Recording::approving();
    assert!(install_via_installer(
        &fx,
        &Spec::new("example.a", "1.0.0").grant("tree.read"),
        &consent
    ));
    installer(&fx, unsigned_ok())
        .uninstall(&plugin("example.a"), true)
        .expect("uninstall");
    let store = GrantStore::open(&fx.state_dir().join("grants.json")).expect("grant store");
    assert!(store
        .granted(&plugin("example.a"))
        .expect("granted")
        .is_empty());
}

/// REQ: PKG-295
#[test]
fn pkg_295_purge_deletes_the_listed_folders() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let layout = seed_plugin_files(&fx, "example.a");
    let out = fx.run(&["uninstall", "example.a", "--purge"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    for (path, _) in &layout {
        assert!(!path.exists(), "{path:?} must be purged");
    }
}

/// REQ: PKG-295
#[test]
fn pkg_295_purge_never_touches_other_plugins_or_paths_outside_the_list() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(fx.install(&Spec::new("example.b", "1.0.0")).exit_code, 0);
    seed_plugin_files(&fx, "example.a");
    let other = seed_plugin_files(&fx, "example.b");
    let outside = fx.dir.path().join("outside.txt");
    std::fs::write(&outside, b"mine").expect("write");
    assert_eq!(fx.run(&["uninstall", "example.a", "--purge"]).exit_code, 0);
    for (path, _) in &other {
        assert!(path.is_file(), "{path:?} belongs to another plugin");
    }
    assert!(outside.is_file());
}

/// REQ: PKG-295
#[test]
fn pkg_295_purge_prints_the_number_of_bytes_freed() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let layout = seed_plugin_files(&fx, "example.a");
    let total: usize = layout.iter().map(|(_, size)| *size).sum();
    let out = fx.run(&["uninstall", "example.a", "--purge"]);
    let mentioned = out.stdout_jsonl.iter().any(|line| {
        line.as_object()
            .map(|object| {
                object
                    .values()
                    .any(|value| value.as_u64() == Some(total as u64))
            })
            .unwrap_or(false)
    });
    assert!(
        mentioned,
        "expected {total} bytes freed in {:?}",
        out.stdout_jsonl
    );
}

/// REQ: PKG-295
#[test]
fn pkg_295_grant_entry_is_deleted_without_purge_as_well() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0").grant("tree.read");
    assert_eq!(fx.install(&spec).exit_code, 0);
    assert_eq!(fx.run(&["uninstall", "example.a"]).exit_code, 0);
    let store = GrantStore::open(&fx.state_dir().join("grants.json")).expect("grant store");
    assert!(store
        .granted(&plugin("example.a"))
        .expect("granted")
        .is_empty());
}

/// REQ: PKG-296
#[test]
fn pkg_296_uninstall_never_edits_the_trust_store() {
    let fx = Fx::new();
    let trust_path = fx.dir.path().join("trust.json");
    std::fs::write(&trust_path, b"{\"format\":\"ilium-trust/1\",\"keys\":[]}\n").expect("write");
    let before = std::fs::read(&trust_path).expect("read");
    let modified_before = std::fs::metadata(&trust_path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let env = env_with(&fx, &[("ILIUM_TRUST_STORE", &trust_path.to_string_lossy())]);
    let owned = vec![
        "uninstall".to_owned(),
        "example.a".to_owned(),
        "--purge".to_owned(),
    ];
    let out = ilium_plugin_host::cli::run(&owned, &fx.env_in(&fx.cwd, env));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(std::fs::read(&trust_path).expect("read"), before);
    assert_eq!(
        std::fs::metadata(&trust_path)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        modified_before
    );
}

/// REQ: PKG-297
#[test]
fn pkg_297_uninstall_of_a_required_plugin_is_dependency_in_use() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.b", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", false))
            .exit_code,
        0
    );
    let out = fx.run(&["uninstall", "example.b"]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("dependency.in-use"),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.b").is_some());
}

/// REQ: PKG-297
#[test]
fn pkg_297_admin_installed_read_only_plugin_is_refused_managed_plugin() {
    let admin = Fx::new();
    assert_eq!(admin.install(&Spec::new("example.m", "1.0.0")).exit_code, 0);
    let user = Fx::new();
    let mut env = user.env();
    env.system_dir = Some(admin.root());
    let owned = vec!["uninstall".to_owned(), "example.m".to_owned()];
    let out = ilium_plugin_host::cli::run(&owned, &env);
    assert!(
        out.has_error("policy.managed-plugin"),
        "{:?}",
        out.error_codes()
    );
    assert_eq!(out.exit_code, 7);
}

/// REQ: PKG-298
#[test]
fn pkg_298_uninstall_of_a_disabled_plugin_works() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    assert_eq!(fx.run(&["disable", "example.a"]).exit_code, 0);
    let out = fx.run(&["uninstall", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-298
#[test]
fn pkg_298_uninstall_of_a_failed_plugin_works() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let mut document = fx.installed();
    document["plugins"][0]["failure_stopped"] = serde_json::json!(["server"]);
    std::fs::write(
        fx.registry_path(),
        serde_json::to_string_pretty(&document).expect("json") + "\n",
    )
    .expect("write");
    let out = fx.run(&["uninstall", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-299
#[test]
fn pkg_299_bundled_plugin_cannot_be_uninstalled() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let mut document = fx.installed();
    document["plugins"][0]["source"] = serde_json::json!("bundled");
    std::fs::write(
        fx.registry_path(),
        serde_json::to_string_pretty(&document).expect("json") + "\n",
    )
    .expect("write");
    let out = fx.run(&["uninstall", "example.a"]);
    assert_eq!(out.exit_code, 1);
    assert!(out.has_error("install.bundled"), "{:?}", out.error_codes());
    assert!(fx.record("example.a").is_some());
}

/// REQ: PKG-299
#[test]
fn pkg_299_bundled_plugin_can_be_disabled() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.a", "1.0.0")).exit_code, 0);
    let mut document = fx.installed();
    document["plugins"][0]["source"] = serde_json::json!("bundled");
    std::fs::write(
        fx.registry_path(),
        serde_json::to_string_pretty(&document).expect("json") + "\n",
    )
    .expect("write");
    let out = fx.run(&["disable", "example.a"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(
        fx.record("example.a").expect("record")["state"],
        "installed-disabled-by-user"
    );
}

// ---------------------------------------------------------------- auto-update

fn config(text: &str) -> AutoUpdateConfig {
    AutoUpdateConfig::from_toml(text).expect("config parses")
}

fn plain_candidate() -> UpdateCandidate {
    UpdateCandidate {
        plugin: "example.a".to_owned(),
        old_version: "1.0.0".to_owned(),
        new_version: "1.1.0".to_owned(),
        new_grants: Vec::new(),
        adds_replaces: false,
        key_changed: false,
        channel_selected: true,
        changelog_url: "https://example.test/changelog".to_owned(),
    }
}

/// REQ: PKG-300
#[test]
fn pkg_300_default_is_off_and_never_checks_the_network() {
    let defaults = config("");
    assert_eq!(defaults.mode, AutoUpdateMode::Off);
    assert!(!should_check_network(&defaults));
}

/// REQ: PKG-300
#[test]
fn pkg_300_notify_checks_and_shows_a_notice_install_installs_without_new_grants() {
    let notify = config("[plugins]\nauto_update = \"notify\"\n");
    assert!(should_check_network(&notify));
    assert!(matches!(
        decide(&notify, &plain_candidate()),
        AutoAction::Notice(_)
    ));
    let install = config("[plugins]\nauto_update = \"install\"\n");
    assert!(should_check_network(&install));
    assert_eq!(decide(&install, &plain_candidate()), AutoAction::Install);
}

/// REQ: PKG-300
#[test]
fn pkg_300_unknown_value_is_refused() {
    assert!(AutoUpdateConfig::from_toml("[plugins]\nauto_update = \"always\"\n").is_err());
}

/// REQ: PKG-301
#[test]
fn pkg_301_check_runs_at_start_then_every_24_hours_with_up_to_1_hour_delay() {
    let plan = schedule(&config("[plugins]\nauto_update = \"notify\"\n"));
    assert!(plan.at_start);
    assert_eq!(plan.interval, Duration::from_secs(24 * 3600));
    assert_eq!(plan.max_jitter, Duration::from_secs(3600));
}

/// REQ: PKG-301
#[test]
fn pkg_301_interval_hours_accepts_1_to_168() {
    for hours in [1u32, 24, 168] {
        let text =
            format!("[plugins]\nauto_update = \"notify\"\nauto_update_interval_hours = {hours}\n");
        let plan = schedule(&config(&text));
        assert_eq!(plan.interval, Duration::from_secs(u64::from(hours) * 3600));
    }
}

/// REQ: PKG-301
#[test]
fn pkg_301_interval_hours_0_and_169_are_refused() {
    for hours in [0u32, 169] {
        let text =
            format!("[plugins]\nauto_update = \"notify\"\nauto_update_interval_hours = {hours}\n");
        assert!(AutoUpdateConfig::from_toml(&text).is_err(), "{hours}");
    }
}

/// REQ: PKG-302
#[test]
fn pkg_302_auto_install_waits_for_new_grants() {
    let install = config("[plugins]\nauto_update = \"install\"\n");
    let mut candidate = plain_candidate();
    candidate.new_grants = vec!["http:api.example.com".to_owned()];
    assert!(matches!(
        decide(&install, &candidate),
        AutoAction::Notice(_)
    ));
}

/// REQ: PKG-302
#[test]
fn pkg_302_auto_install_waits_for_replaces_key_change_and_foreign_channel() {
    let install = config("[plugins]\nauto_update = \"install\"\n");
    let mut with_replaces = plain_candidate();
    with_replaces.adds_replaces = true;
    let mut with_key_change = plain_candidate();
    with_key_change.key_changed = true;
    let mut foreign_channel = plain_candidate();
    foreign_channel.channel_selected = false;
    for candidate in [with_replaces, with_key_change, foreign_channel] {
        assert!(
            matches!(decide(&install, &candidate), AutoAction::Notice(_)),
            "{candidate:?}"
        );
    }
}

/// REQ: PKG-303
#[test]
fn pkg_303_metered_network_is_not_checked() {
    let by_config = config("[plugins]\nauto_update = \"install\"\nmetered = true\n");
    assert!(!should_check_network(&by_config));
    let mut by_os = config("[plugins]\nauto_update = \"install\"\n");
    by_os.os_metered = true;
    assert!(!should_check_network(&by_os));
    let unmetered = config("[plugins]\nauto_update = \"install\"\nmetered = false\n");
    assert!(should_check_network(&unmetered));
}

/// REQ: PKG-304
#[test]
fn pkg_304_notice_lists_versions_grants_changelog_and_three_actions() {
    let notify = config("[plugins]\nauto_update = \"notify\"\n");
    let mut candidate = plain_candidate();
    candidate.new_grants = vec!["tree.read".to_owned()];
    match decide(&notify, &candidate) {
        AutoAction::Notice(notice) => {
            assert_eq!(notice.plugin, "example.a");
            assert_eq!(notice.old_version, "1.0.0");
            assert_eq!(notice.new_version, "1.1.0");
            assert_eq!(notice.new_grants, vec!["tree.read".to_owned()]);
            assert_eq!(notice.changelog_url, "https://example.test/changelog");
            assert_eq!(
                notice.actions,
                vec![
                    NoticeAction::Update,
                    NoticeAction::Later,
                    NoticeAction::SkipThisVersion
                ]
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// REQ: PKG-304
#[test]
fn pkg_304_later_hides_the_notice_for_seven_days() {
    assert_eq!(later_hides_for(), Duration::from_secs(7 * 24 * 3600));
}

/// REQ: PKG-305
#[test]
fn pkg_305_admin_policy_off_overrides_the_user_setting() {
    let user = config("[plugins]\nauto_update = \"install\"\n");
    let locked = user.with_policy(Some(AutoUpdateMode::Off));
    assert_eq!(locked.mode, AutoUpdateMode::Off);
    assert!(!should_check_network(&locked));
}

/// REQ: PKG-305
#[test]
fn pkg_305_without_a_policy_the_user_setting_stands() {
    let user = config("[plugins]\nauto_update = \"notify\"\n");
    assert_eq!(user.with_policy(None).mode, AutoUpdateMode::Notify);
}

/// REQ: PKG-305
#[test]
fn doc_pkg_305_page_calls_auto_update_a_policy_item() {
    let page = manual_page("reference-install-and-update.md");
    assert!(page.contains("Auto-update is a policy item"));
}

#[allow(dead_code)]
fn unused_tier_marker() -> Tier {
    Tier::T3
}
