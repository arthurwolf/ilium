//! reference-sandbox.md "File scopes" (SEC-058 to SEC-064).
//!
//! Scope resolution is a host function; the helper forwards every file import with the path
//! unchanged and never opens a path itself. The manual text pins the exact rules.

use crate::support::*;

const PAGE: &str = "reference-sandbox";

// ------------------------------------------------------------------ SEC-058

/// REQ: SEC-058
#[test]
fn sec_058_dotdot_traversal_is_forwarded_unresolved_and_refused_by_the_host() {
    let outcome = hostile_call("fs-write", b"../../etc/passwd", Some("not-permitted"));
    assert!(outcome.forwarded("fs", b"../../etc/passwd"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-058
#[test]
fn sec_058_symlink_swap_race_ends_in_not_permitted() {
    let outcome = hostile_call("fs-symlink-swap-race", b"swap", Some("not-permitted"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-058
#[test]
fn sec_058_helper_has_no_directory_descriptor_outside_stdio_and_the_channel() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let links = helper.fd_links();
    assert!(links.len() >= 3, "stdio descriptors 0..=2 must be listed: {links:?}");
    assert!(links.iter().all(|(fd, target)| *fd <= 2 || !target.starts_with('/')), "{links:?}");
}

/// REQ: SEC-058
#[test]
fn doc_sec_058_openat2_flags_and_fallback() {
    assert_page_has(PAGE, "`openat2` with `RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV`");
    assert_page_has(PAGE, "`openat` walk that checks every component with `O_NOFOLLOW`");
}

// ------------------------------------------------------------------ SEC-059

/// REQ: SEC-059
#[test]
fn sec_059_write_through_a_hard_link_is_forwarded_and_refused_not_permitted() {
    let outcome = hostile_call("fs-write-hardlink", b"link", Some("not-permitted"));
    assert!(outcome.forwarded("fs", b"link"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-059
#[test]
fn doc_sec_059_link_count_rule_and_error() {
    assert_page_has(PAGE, "a write or a read-for-write open is refused when the device and inode differ");
    assert_page_has(PAGE, "The error is `not-permitted`");
}

// ------------------------------------------------------------------ SEC-060

const DENY_SET: [&str; 12] = [
    "`config.toml`", "the grant store", "the trust store", "the consent store", "the project trust store",
    "the secrets file", "the admin policy file", "the audit log", "the plugin install directory",
    "every other plugin's `data` and `cache`", "the runtime directory and the Ilium socket", "the HTTP API token file",
];

/// REQ: SEC-060
#[test]
fn doc_sec_060_deny_set_has_all_twelve_entries() {
    for entry in DENY_SET {
        assert_page_has(PAGE, entry);
    }
}

/// REQ: SEC-060
#[test]
fn sec_060_runtime_symlink_into_the_deny_set_is_refused_not_permitted() {
    let outcome = hostile_call("fs-read-symlink-into-deny-set", b"config.toml", Some("not-permitted"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-060
#[test]
fn sec_060_reading_another_plugins_data_is_forwarded_and_refused() {
    let outcome = hostile_call("fs-read-other-plugin-data", b"plugin-b", Some("not-permitted"));
    assert!(outcome.forwarded("fs", b"plugin-b"));
    assert_eq!(outcome.result, "not-permitted");
}

/// REQ: SEC-060
#[test]
fn doc_sec_060_install_time_and_run_time_codes() {
    assert_page_has(PAGE, "refused at install with `manifest.grant-scope`");
    assert_page_has(PAGE, "is refused with `not-permitted`");
}

// ------------------------------------------------------------------ SEC-061

const SENSITIVE_PATHS: [&str; 14] = [
    "`~/.ssh`", "`~/.gnupg`", "`~/.aws`", "`~/.config/gcloud`", "`~/.kube`", "`~/.netrc`", "`~/.git-credentials`",
    "`~/.docker/config.json`", "`~/.local/share/keyrings`", "`~/.mozilla`", "`~/.config/google-chrome`",
    "`~/.config/chromium`", "password-manager stores", "agent credential files",
];

/// REQ: SEC-061
#[test]
fn doc_sec_061_sensitive_path_set_is_complete() {
    for path in SENSITIVE_PATHS {
        assert_page_has(PAGE, path);
    }
}

/// REQ: SEC-061
#[test]
fn doc_sec_061_grant_below_a_sensitive_path_is_high_risk_and_shows_the_resolved_path() {
    assert_page_has(PAGE, "is, contains or lies below one of the following is high risk, and the review dialog shows the resolved path");
}

/// REQ: SEC-061
#[test]
fn sec_061_reading_ssh_keys_without_a_grant_is_not_mounted() {
    let wasm = require_fixture("hostile-escape");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("hostile", &wasm), Ok(()));
    let (result, _) = run_call(&mut helper, 1, "read-ssh-dir", b"", 2_000, 3_000, |_| (Vec::new(), None));
    assert_eq!(String::from_utf8_lossy(&result.expect("call result").result), "not-found");
}

// ------------------------------------------------------------------ SEC-062

/// REQ: SEC-062
#[test]
fn doc_sec_062_home_root_and_ancestors_are_high_risk() {
    assert_page_has(PAGE, "A template whose resolved root is `$HOME`, `/`, or an ancestor of `$HOME` is high risk");
}

/// REQ: SEC-062
#[test]
fn doc_sec_062_examples_home_is_high_pictures_is_normal() {
    assert_page_has(PAGE, "`fs.read:path:{home}` is therefore H");
    assert_page_has(PAGE, "`fs.read:path:{home}/Pictures/**` is normal");
}

// ------------------------------------------------------------------ SEC-063

const CREDENTIAL_EXCLUSIONS: [(&str, &[&str]); 4] = [
    ("claude", &["`.credentials.json`", "`credentials.json`"]),
    ("codex", &["`auth.json`"]),
    ("gemini", &["`oauth_creds.json`", "`google_accounts.json`"]),
    ("antigravity", &["`auth.json`", "`oauth_creds.json`"]),
];

/// REQ: SEC-063
#[test]
fn doc_sec_063_each_agent_has_its_credential_files_excluded() {
    let table = section(PAGE, "**[SEC-063]**");
    for (agent, files) in CREDENTIAL_EXCLUSIONS {
        assert!(table.contains(&format!("| `{agent}` |")), "{agent}");
        for file in files {
            assert!(table.contains(file), "{agent}: {file}");
        }
    }
}

/// REQ: SEC-063
#[test]
fn doc_sec_063_every_agent_pattern_rule() {
    for pattern in ["`*credentials*`", "`*token*`", "`*secret*`", "`*.pem`", "`*.key`"] {
        assert_page_has(PAGE, pattern);
    }
}

/// REQ: SEC-063
#[test]
fn sec_063_reading_a_credential_file_of_an_agent_config_scope_is_forwarded_and_refused() {
    let outcome = hostile_call("fs-read-agent-config", b"auth.json", Some("not-found"));
    assert!(outcome.forwarded("fs", b"auth.json"));
    assert_ne!(outcome.result, "");
}

// ------------------------------------------------------------------ SEC-064

/// REQ: SEC-064
#[test]
fn sec_064_error_returned_to_the_guest_has_no_absolute_path() {
    let outcome = hostile_call("fs-read", b"notes/missing.txt", Some("not-found"));
    assert!(!outcome.result.contains('/'), "result: {}", outcome.result);
    assert!(!outcome.result.contains("/home"));
}

/// REQ: SEC-064
#[test]
fn sec_064_trap_message_of_a_failed_call_does_not_contain_the_scope_root() {
    let outcome = hostile_call("fs-read", b"notes/missing.txt", Some("not-found"));
    if let Some(trap) = outcome.trap {
        assert!(!trap.starts_with('/'));
    }
}

/// REQ: SEC-064
#[test]
fn doc_sec_064_errors_carry_scope_and_relative_path_only() {
    assert_page_has(PAGE, "Errors never include the resolved absolute path");
    assert_page_has(PAGE, "returns the scope and the path relative to the scope root only");
}
