//! Requirements SEC-958, SEC-959, SEC-963 and SEC-964 (reference-secrets.md): plugin data is stored as
//! plain bytes, a file-store secret moves to the keyring when the keyring returns, and a secret lost
//! from the store reads as unset and is re-entered one row at a time.

use serde_json::json;

use crate::support::*;

/// Whether any file below `dir` holds `needle` as a contiguous run of bytes.
fn tree_contains(dir: &std::path::Path, needle: &[u8]) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return false,
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            return tree_contains(&path, needle);
        }
        match std::fs::read(&path) {
            Ok(bytes) => bytes.windows(needle.len()).any(|window| window == needle),
            Err(_) => false,
        }
    })
}

/// REQ: SEC-958
#[test]
fn sec_958_plugin_data_is_written_as_plain_bytes_readable_in_the_state_directory() {
    let rig = setup(&[]).start();
    rig.host
        .write_plugin_data("example.demo", b"plain-marker-958");
    rig.host.persist_all_stores();
    assert!(
        tree_contains(rig.state.path(), b"plain-marker-958"),
        "the plugin data must be stored verbatim, without encryption at rest"
    );
}

/// REQ: SEC-958
#[test]
fn sec_958_the_storage_page_states_no_host_encryption_and_relies_on_owner_only_modes() {
    let page = manual_page("reference-secrets.md");
    assert!(
        page.contains("the host does not encrypt it at rest"),
        "the page must state that the host does not encrypt plugin data at rest"
    );
    assert!(
        page.contains("The only protection the host gives these files is the owner-only access of [USR-206]"),
        "the page must name owner-only access as the only host protection"
    );
}

/// REQ: SEC-959
#[test]
fn sec_959_a_file_store_value_moves_into_the_keyring_at_the_next_start_and_leaves_the_file() {
    let mut rig = setup(&[]).no_keyring().start();
    rig.host.set_secret_now("token", "file-value");
    rig.host.persist_all_stores();
    rig.setup = setup(&[]).keyring();
    let rig = rig.restart();
    assert_eq!(
        rig.host.keyring_value("example.demo", "token").as_deref(),
        Some("file-value"),
        "the keyring must hold the file-store value after the keyring returns"
    );
    assert!(
        !rig.host.stored_secret_text().contains("file-value"),
        "the file copy must be removed once the keyring readback succeeds"
    );
}

/// REQ: SEC-959
#[test]
fn sec_959_the_newer_file_value_overwrites_an_older_keyring_value() {
    unimplemented!("ilium_plugin_host::secrets::file_store_to_keyring_newer_value_overwrites")
}

/// REQ: SEC-959
#[test]
fn sec_959_the_file_copy_is_removed_only_after_the_keyring_readback_succeeds() {
    unimplemented!("ilium_plugin_host::secrets::file_copy_removed_only_after_keyring_readback")
}

/// REQ: SEC-963
#[test]
fn sec_963_a_secret_missing_from_the_store_reads_as_unset_in_settings_get_json() {
    let rig = setup(&[]).keyring().secret("token", "present-value").start();
    rig.host.forget_secrets(Some("example.demo"));
    assert!(
        rig.host.secret_value("token").is_none(),
        "a secret lost from the store must read as unset"
    );
    let read = rig
        .value("settings.get-json", json!({ "key": "token" }))
        .expect("settings.get-json answers for a declared secret row");
    assert!(read.is_null(), "the key must read as unset, got {read}");
}

/// REQ: SEC-963
#[test]
fn sec_963_a_settings_changed_event_arrives_once_per_missing_key_at_the_next_start() {
    let rig = setup(&[]).keyring().secret("token", "present-value").start();
    rig.host.forget_secrets(Some("example.demo"));
    let rig = rig.restart();
    let delivered = rig.host.delivered_events_text();
    assert_eq!(
        delivered.matches("settings-changed").count(),
        1,
        "one settings-changed event per missing key at the next start: {delivered}"
    );
}

/// REQ: SEC-964
#[test]
fn sec_964_the_secrets_page_re_enters_a_lost_secret_one_row_at_a_time_on_its_plugin_tab() {
    let page = manual_page("reference-secrets.md");
    assert!(
        page.contains("is re-entered by the user, one secret row at a time, on the settings tab of the plugin that owns the row"),
        "the page must state one-row-at-a-time re-entry on the owning plugin's tab"
    );
}

/// REQ: SEC-964
#[test]
fn sec_964_the_host_offers_no_bulk_import_and_exports_no_secret_for_re_entry() {
    let page = manual_page("reference-secrets.md");
    assert!(
        page.contains("The host MUST NOT offer a bulk import of secrets, and it MUST NOT export a secret to make re-entry easier"),
        "the page must forbid bulk import and export of secrets for re-entry"
    );
}

/// REQ: SEC-964
#[test]
fn sec_964_a_plugin_with_an_unset_secret_names_the_row_and_never_retries_with_an_empty_credential() {
    unimplemented!("ilium_plugin_host::secrets::unset_secret_message_names_row_and_no_empty_retry")
}
