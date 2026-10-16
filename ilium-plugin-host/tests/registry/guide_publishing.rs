//! guide-publishing.md: ids, versions, keys, reproducible builds, SBOM, publish (PKG-490..508).
//! Lane `registry-publish`, part 2.

use crate::common::*;
use crate::pages_p2::*;
use proptest::prelude::*;

const PAGE: &str = "guide-publishing.md";
const SLUG: &str = "guide-publishing";
const NOTES: &str = "com.acme.notes";
const MIB4: usize = 4 * 1024 * 1024;

fn publish_to(sandbox: &Sandbox, bytes: Vec<u8>, extra: &[&str]) -> ilium_plugin_host::cli::CliOutput {
    let path = sandbox.project_file("upload.iliumpkg", &bytes);
    let mut args = vec!["publish", path.to_str().expect("utf-8 path"), "--registry", BASE];
    args.extend_from_slice(extra);
    sandbox.run_with_vars(&[("ILIUM_REGISTRY_TOKEN", "tok-guide-1")], &args)
}

// ---- ids and versions (PKG-490..492) --------------------------------------------------------

/// REQ: PKG-490
#[test]
fn doc_pkg_490_plugin_id_must_start_with_the_publisher_id() {
    assert_row(PAGE, "PKG-490", &["MUST match the publisher id", "`manifest.id-publisher`"]);
}

/// REQ: PKG-490
#[test]
fn pkg_490_publisher_that_does_not_own_the_id_prefix_is_refused_id_publisher() {
    let sandbox = Sandbox::new();
    let bytes = package_signed_by(NOTES, "1.0.0", PUBLISHER_SEED, "com.other");
    let output = verify_bytes(&sandbox, &bytes);
    assert_code(&output, "manifest.id-publisher");
}

/// REQ: PKG-490
#[test]
fn pkg_490_a_prefix_without_the_dot_boundary_is_refused_id_publisher() {
    let sandbox = Sandbox::new();
    let bytes = package_signed_by(NOTES, "1.0.0", PUBLISHER_SEED, "com.acm");
    assert_code(&verify_bytes(&sandbox, &bytes), "manifest.id-publisher");
}

/// REQ: PKG-490
#[test]
fn pkg_490_the_matching_publisher_verifies() {
    let sandbox = Sandbox::new();
    let bytes = package_signed_by(NOTES, "1.0.0", PUBLISHER_SEED, PUBLISHER_ID);
    assert!(!has_error(&verify_bytes(&sandbox, &bytes), "manifest.id-publisher"));
}

/// REQ: PKG-491 WARN
#[test]
fn doc_pkg_491_semver_and_major_bump_on_removal() {
    assert_row(PAGE, "PKG-491", &["SemVer", "major number", "remove a command"]);
}

/// REQ: PKG-491 WARN
#[test]
fn pkg_491_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-491", SLUG, "SHOULD");
}

/// REQ: PKG-492 WARN
#[test]
fn doc_pkg_492_a_published_version_is_immutable() {
    assert_row(PAGE, "PKG-492", &["immutable", "new version", "PKG-394"]);
}

/// REQ: PKG-492 WARN
#[test]
fn pkg_492_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-492", SLUG, "SHOULD");
}

/// REQ: PKG-492 WARN
#[test]
fn pkg_492_republishing_an_existing_version_is_refused_version_exists() {
    let sandbox = Sandbox::standard();
    let output = publish_to(&sandbox, signed_package(NOTES, "1.0.0"), &[]);
    assert_code(&output, "registry.version-exists");
}

/// REQ: PKG-492 WARN
#[test]
fn pkg_492_a_new_version_number_is_accepted_by_the_client() {
    let sandbox = Sandbox::standard();
    let output = publish_to(&sandbox, signed_package(NOTES, "1.2.0"), &[]);
    assert!(!has_error(&output, "registry.version-exists"), "{output:?}");
}

proptest! {
    /// REQ: PKG-492 WARN
    #[test]
    fn pkg_492_any_existing_version_is_refused_and_any_new_one_is_not(major in 2u32..9, minor in 0u32..9) {
        let sandbox = Sandbox::standard();
        let fresh = publish_to(&sandbox, signed_package(NOTES, &format!("{major}.{minor}.0"), ), &[]);
        prop_assert!(!has_error(&fresh, "registry.version-exists"));
        let existing = publish_to(&sandbox, signed_package(NOTES, "1.1.0"), &[]);
        prop_assert!(has_error(&existing, "registry.version-exists"));
    }
}

// ---- keys (PKG-493, 494) ----------------------------------------------------------------------

/// REQ: PKG-493
#[test]
fn doc_pkg_493_private_key_stays_out_of_the_repository_and_ci_uses_the_variable() {
    assert_row(PAGE, "PKG-493", &["MUST NOT be stored in the plugin repository", "`ILIUM_PUBLISHER_KEY`"]);
}

fn keygen(sandbox: &Sandbox) -> String {
    let dir = sandbox.dir.path().join("keys");
    std::fs::create_dir_all(&dir).expect("keys folder");
    assert_ok(&sandbox.run(&["keygen", "--publisher-id", "com.acme", "--publisher", "Acme Labs", "--out", dir.to_str().expect("utf-8 path")]));
    std::fs::read_to_string(dir.join("publisher.key")).expect("generated key")
}

/// REQ: PKG-493, PKG-446
#[test]
fn pkg_493_signing_with_the_key_variable_never_writes_the_key_into_the_project_or_state() {
    let sandbox = Sandbox::new();
    let pem = keygen(&sandbox);
    let package = sandbox.project_file("plugin.iliumpkg", &unsigned_package(NOTES, "1.0.0"));
    let before = sandbox.snapshot();
    let output = sandbox.run_with_vars(&[("ILIUM_PUBLISHER_KEY", pem.as_str())], &["sign", package.to_str().expect("utf-8 path")]);
    assert!(!output_contains(&output, pem.trim()), "{output:?}");
    for (path, bytes) in sandbox.snapshot() {
        if path.ends_with("publisher.key") || before.get(&path) == Some(&bytes) {
            continue;
        }
        assert!(!String::from_utf8_lossy(&bytes).contains(pem.trim()), "key text leaked into {path}");
    }
}

/// REQ: PKG-493, PKG-446
#[test]
fn pkg_493_an_unreadable_key_variable_is_refused_without_echoing_it() {
    let sandbox = Sandbox::new();
    let package = sandbox.project_file("plugin.iliumpkg", &unsigned_package(NOTES, "1.0.0"));
    let secret = "NOT-A-KEY-secret-ZXCV-4455";
    let output = sandbox.run_with_vars(&[("ILIUM_PUBLISHER_KEY", secret)], &["sign", package.to_str().expect("utf-8 path")]);
    assert_code(&output, "sign.key-unreadable");
    assert!(!output_contains(&output, secret), "{output:?}");
}

/// REQ: PKG-494 WARN
#[test]
fn doc_pkg_494_passphrase_comes_from_the_variable_not_the_command_line() {
    assert_row(PAGE, "PKG-494", &["Encrypt a key", "`ILIUM_KEY_PASSPHRASE`", "never on the command line"]);
}

/// REQ: PKG-494 WARN
#[test]
fn pkg_494_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-494", SLUG, "SHOULD");
}

/// REQ: PKG-494 WARN, PKG-446
#[test]
fn pkg_494_a_passphrase_flag_is_not_a_command_option_and_its_value_is_not_echoed() {
    let sandbox = Sandbox::new();
    let package = sandbox.project_file("plugin.iliumpkg", &unsigned_package(NOTES, "1.0.0"));
    let secret = "hunter2-passphrase-9981";
    let output = sandbox.run(&["sign", package.to_str().expect("utf-8 path"), "--passphrase", secret]);
    assert_eq!(output.exit_code, 2, "{output:?}");
    assert!(!output_contains(&output, secret), "{output:?}");
}

// ---- reproducible builds (PKG-495..499) -------------------------------------------------------

/// REQ: PKG-495 WARN, PKG-496 WARN, PKG-497 WARN
#[test]
fn doc_pkg_495_to_497_build_hygiene_rows_name_the_files_and_flags() {
    assert_row(PAGE, "PKG-495", &["`rust-toolchain.toml`", "`package-lock.json`"]);
    assert_row(PAGE, "PKG-496", &["`cargo build --locked`", "`npm ci`"]);
    assert_row(PAGE, "PKG-497", &["build time", "user name", "absolute path", "`--remap-path-prefix`"]);
}

/// REQ: PKG-495 WARN
#[test]
fn pkg_495_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-495", SLUG, "SHOULD");
}

/// REQ: PKG-496 WARN
#[test]
fn pkg_496_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-496", SLUG, "SHOULD");
}

/// REQ: PKG-497 WARN
#[test]
fn pkg_497_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-497", SLUG, "SHOULD");
}

/// REQ: PKG-498 WARN
#[test]
fn doc_pkg_498_build_info_names_toolchain_and_commit_and_pack_includes_it() {
    assert_row(PAGE, "PKG-498", &["`build-info.json`", "toolchain", "source commit", "`pack` includes it"]);
    assert!(page_text("reference-package-format.md").contains("build-info.json"));
}

/// REQ: PKG-498 WARN
#[test]
fn pkg_498_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-498", SLUG, "SHOULD");
}

/// REQ: PKG-498 WARN
#[test]
fn pkg_498_a_package_that_carries_build_info_verifies() {
    let sandbox = Sandbox::new();
    let info = br#"{"toolchain":"rustc","version":"1.90.0","commit":"0123456789abcdef0123456789abcdef01234567"}"#.to_vec();
    let bytes = package_with_entries(NOTES, "1.0.0", &[("build-info.json", info)], PUBLISHER_SEED, PUBLISHER_ID);
    let output = verify_bytes(&sandbox, &bytes);
    assert_eq!(error_codes(&output), Vec::<String>::new(), "{output:?}");
}

/// REQ: PKG-499
#[test]
fn doc_pkg_499_two_clean_builds_must_have_equal_sha256() {
    assert_row(PAGE, "PKG-499", &["build twice", "`sha256sum`", "MUST be equal"]);
}

/// REQ: PKG-499
#[test]
fn pkg_499_packing_the_same_inputs_twice_gives_the_same_bytes() {
    let first = signed_package(NOTES, "1.0.0");
    let second = signed_package(NOTES, "1.0.0");
    assert_eq!(sha256_hex(&first), sha256_hex(&second));
    assert_eq!(first, second);
}

/// REQ: PKG-499
#[test]
fn pkg_499_a_different_version_changes_the_hash() {
    assert_ne!(sha256_hex(&signed_package(NOTES, "1.0.0")), sha256_hex(&signed_package(NOTES, "1.0.1")));
}

proptest! {
    /// REQ: PKG-499
    #[test]
    fn pkg_499_packing_is_deterministic_for_any_version(major in 0u32..50, minor in 0u32..50, patch in 0u32..50) {
        let version = format!("{major}.{minor}.{patch}");
        prop_assert_eq!(signed_package(NOTES, &version), signed_package(NOTES, &version));
    }
}

// ---- SBOM (PKG-500..504) ------------------------------------------------------------------------

/// REQ: PKG-500 WARN, PKG-501 WARN, PKG-503 WARN
#[test]
fn doc_pkg_500_501_503_sbom_rows_name_the_file_the_formats_and_the_generators() {
    assert_row(PAGE, "PKG-500", &["`sbom.json`", "project root"]);
    assert_row(PAGE, "PKG-501", &["CycloneDX 1.5", "SPDX 2.3", "opaque"]);
    assert_row(PAGE, "PKG-503", &["lock file", "`cargo cyclonedx`", "`npx @cyclonedx/cyclonedx-npm`"]);
}

/// REQ: PKG-500 WARN
#[test]
fn pkg_500_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-500", SLUG, "SHOULD");
}

/// REQ: PKG-501 WARN
#[test]
fn pkg_501_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-501", SLUG, "SHOULD");
}

/// REQ: PKG-503 WARN
#[test]
fn pkg_503_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-503", SLUG, "SHOULD");
}

/// REQ: PKG-500 WARN
#[test]
fn pkg_500_a_package_with_sbom_json_verifies() {
    let sandbox = Sandbox::new();
    let bytes = package_with_entries(NOTES, "1.0.0", &[("sbom.json", br#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#.to_vec())], PUBLISHER_SEED, PUBLISHER_ID);
    assert_eq!(error_codes(&verify_bytes(&sandbox, &bytes)), Vec::<String>::new());
}

/// REQ: PKG-501 WARN
#[test]
fn pkg_501_the_host_does_not_validate_sbom_content() {
    let sandbox = Sandbox::new();
    for content in [b"not json at all".to_vec(), b"{}".to_vec(), br#"{"spdxVersion":"SPDX-2.3"}"#.to_vec()] {
        let bytes = package_with_entries(NOTES, "1.0.0", &[("sbom.json", content)], PUBLISHER_SEED, PUBLISHER_ID);
        assert_eq!(error_codes(&verify_bytes(&sandbox, &bytes)), Vec::<String>::new());
    }
}

/// REQ: PKG-502
#[test]
fn doc_pkg_502_sbom_is_at_most_4_mib() {
    assert_row(PAGE, "PKG-502", &["MUST be at most 4 MiB", "`package.size-limit`"]);
}

/// REQ: PKG-502
#[test]
fn pkg_502_an_sbom_of_exactly_4_mib_is_accepted() {
    let sandbox = Sandbox::new();
    let bytes = package_with_entries(NOTES, "1.0.0", &[("sbom.json", vec![b' '; MIB4])], PUBLISHER_SEED, PUBLISHER_ID);
    assert!(!has_error(&verify_bytes(&sandbox, &bytes), "package.size-limit"));
}

/// REQ: PKG-502
#[test]
fn pkg_502_an_sbom_one_byte_over_4_mib_is_refused_size_limit() {
    let sandbox = Sandbox::new();
    let bytes = package_with_entries(NOTES, "1.0.0", &[("sbom.json", vec![b' '; MIB4 + 1])], PUBLISHER_SEED, PUBLISHER_ID);
    assert_code(&verify_bytes(&sandbox, &bytes), "package.size-limit");
}

/// REQ: PKG-502
#[test]
fn pkg_502_an_sbom_one_byte_under_4_mib_is_accepted() {
    let sandbox = Sandbox::new();
    let bytes = package_with_entries(NOTES, "1.0.0", &[("sbom.json", vec![b' '; MIB4 - 1])], PUBLISHER_SEED, PUBLISHER_ID);
    assert!(!has_error(&verify_bytes(&sandbox, &bytes), "package.size-limit"));
}

/// REQ: PKG-504 MAY
#[test]
fn doc_pkg_504_a_review_level_b_registry_may_require_an_sbom() {
    assert_row(PAGE, "PKG-504", &["review level B", "MAY require an SBOM", "PKG-401"]);
}

/// REQ: PKG-504 MAY
#[test]
fn pkg_504_a_registry_that_does_not_require_an_sbom_accepts_a_package_without_one() {
    let sandbox = Sandbox::standard();
    let output = publish_to(&sandbox, signed_package(NOTES, "1.2.0"), &[]);
    assert!(!has_error(&output, "package.sbom-missing"), "{output:?}");
}

/// REQ: PKG-504 MAY
#[test]
fn pkg_504_a_package_with_an_sbom_is_never_refused_for_having_one() {
    let sandbox = Sandbox::standard();
    let bytes = package_with_entries(NOTES, "1.2.0", &[("sbom.json", b"{}".to_vec())], PUBLISHER_SEED, PUBLISHER_ID);
    let output = publish_to(&sandbox, bytes, &[]);
    assert!(!has_error(&output, "package.size-limit"), "{output:?}");
}

// ---- signing, verify, publish (PKG-505..508) ----------------------------------------------------

/// REQ: PKG-505 WARN
#[test]
fn doc_pkg_505_sign_only_after_the_final_pack() {
    assert_row(PAGE, "PKG-505", &["Sign only after the final `pack`", "`manifest.file-hash`", "`manifest.signature-invalid`"]);
}

/// REQ: PKG-505 WARN
#[test]
fn pkg_505_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-505", SLUG, "SHOULD");
}

fn rebuilt_after_signing(change_manifest: bool) -> Vec<u8> {
    let manifest = manifest_text(NOTES, "1.0.0");
    let signature = signature_entry(manifest.as_bytes(), PUBLISHER_SEED, PUBLISHER_ID);
    let (manifest, component) = if change_manifest {
        (manifest.replace("1.0.0", "1.0.1").into_bytes(), unhex(COMPONENT_HEX))
    } else {
        let mut component = unhex(COMPONENT_HEX);
        component.push(0);
        (manifest.into_bytes(), component)
    };
    write_package(&[("manifest.json".to_owned(), manifest), ("server.wasm".to_owned(), component), ("signature".to_owned(), signature.into_bytes())]).expect("package")
}
use ilium_plugin_host::package::write_package;

/// REQ: PKG-505 WARN
#[test]
fn pkg_505_a_file_changed_after_signing_is_refused_file_hash() {
    let sandbox = Sandbox::new();
    let output = verify_bytes(&sandbox, &rebuilt_after_signing(false));
    assert!(has_error(&output, "manifest.file-hash") || has_error(&output, "manifest.signature-invalid"), "{output:?}");
}

/// REQ: PKG-505 WARN
#[test]
fn pkg_505_a_manifest_changed_after_signing_is_refused_signature_invalid() {
    let sandbox = Sandbox::new();
    let output = verify_bytes(&sandbox, &rebuilt_after_signing(true));
    assert!(has_error(&output, "manifest.signature-invalid") || has_error(&output, "manifest.file-hash"), "{output:?}");
}

/// REQ: PKG-505 WARN
#[test]
fn pkg_505_the_untouched_final_pack_verifies() {
    let sandbox = Sandbox::new();
    assert_eq!(error_codes(&verify_bytes(&sandbox, &signed_package(NOTES, "1.0.0"))), Vec::<String>::new());
}

/// REQ: PKG-506 WARN
#[test]
fn doc_pkg_506_verify_and_install_on_a_clean_machine_before_upload() {
    assert_row(PAGE, "PKG-506", &["`verify`", "clean machine", "before you upload"]);
}

/// REQ: PKG-506 WARN
#[test]
fn pkg_506_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-506", SLUG, "SHOULD");
}

/// REQ: PKG-506 WARN
#[test]
fn pkg_506_a_fresh_sandbox_verifies_and_installs_the_file() {
    let sandbox = Sandbox::new();
    let bytes = signed_package(NOTES, "1.0.0");
    assert_eq!(error_codes(&verify_bytes(&sandbox, &bytes)), Vec::<String>::new());
    let path = sandbox.project_file("candidate.iliumpkg", &bytes);
    let key = publisher_key_id();
    assert_ok(&sandbox.run(&["install", path.to_str().expect("utf-8 path"), "--yes", "--trust", key.as_str()]));
}

/// REQ: PKG-507 WARN
#[test]
fn doc_pkg_507_publish_limit_is_20_per_hour_per_publisher() {
    assert_row(PAGE, "PKG-507", &["20 per hour per publisher", "PKG-400"]);
}

/// REQ: PKG-507 WARN
#[test]
fn pkg_507_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-507", SLUG, "SHOULD");
}

/// REQ: PKG-508 WARN
#[test]
fn doc_pkg_508_new_plugins_start_on_beta() {
    assert_row(PAGE, "PKG-508", &["`beta` channel", "`--channel beta`", "`stable`"]);
}

/// REQ: PKG-508 WARN
#[test]
fn pkg_508_traced_in_the_requirement_table_as_should() {
    assert_traced("PKG-508", SLUG, "SHOULD");
}

/// REQ: PKG-508 WARN
#[test]
fn pkg_508_publish_accepts_the_beta_and_stable_channels() {
    for channel in ["beta", "stable"] {
        let sandbox = Sandbox::standard();
        let output = publish_to(&sandbox, signed_package(NOTES, "1.2.0"), &["--channel", channel]);
        assert_ne!(output.exit_code, 2, "{channel}: {output:?}");
    }
}

/// REQ: PKG-508 WARN
#[test]
fn pkg_508_an_unknown_channel_is_refused_invalid_value() {
    let sandbox = Sandbox::standard();
    let output = publish_to(&sandbox, signed_package(NOTES, "1.2.0"), &["--channel", "dev"]);
    assert_failed(&output, "usage.invalid-value", 2);
}
