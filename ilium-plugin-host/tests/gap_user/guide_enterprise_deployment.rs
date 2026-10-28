//! Inventory, notice contact and administrator limits of the guide `guide-enterprise-deployment`
//! (USR-341, USR-343, USR-344).

use std::collections::BTreeSet;

use serde_json::Value;

use crate::common::*;

/// REQ: USR-341
#[test]
fn usr_341_inventory_lists_one_sorted_object_per_plugin_without_home_paths() {
    let rig = rig();
    install_ok(&rig, &pkg(OTHER, "1.0.0").grant("fs.read"));
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let output = rig.cli(&["list", "--json", "--inventory"]);
    assert_eq!(output.exit_code, 0, "{}", output.stderr);
    let objects: Vec<Value> = output
        .stdout_raw
        .lines()
        .map(|line| serde_json::from_str(line).expect("one JSON object per line"))
        .collect();
    assert_eq!(
        objects.len(),
        2,
        "one object per plugin: {}",
        output.stdout_raw
    );
    for object in &objects {
        for key in [
            "id",
            "version",
            "tier",
            "content_hash",
            "key_id",
            "state",
            "granted_permissions",
        ] {
            assert!(object.get(key).is_some(), "object lacks {key}: {object}");
        }
    }
    let ids: Vec<&str> = objects
        .iter()
        .map(|object| object["id"].as_str().expect("id"))
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "objects must be sorted by id");
    let home = rig.home_dir().to_string_lossy().into_owned();
    assert!(
        !output.stdout_raw.contains(&home),
        "paths under the user's home leaked"
    );
}

/// REQ: USR-343
#[test]
fn usr_343_blocked_by_administrator_messages_end_with_the_policy_notice_contact() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_policy(&format!(
        "[notice]\ncontact = \"it-help@example.org\"\n\n[plugins]\ndeny_ids = [\"{NOTES}\"]\n"
    ));
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let row = server
        .render_plugins_screen()
        .row_for(NOTES)
        .expect("row")
        .text
        .clone();
    assert!(row.contains("Blocked by your administrator"), "{row}");
    assert!(
        row.contains("it-help@example.org"),
        "the policy contact must be appended: {row}"
    );
}

/// REQ: USR-344
#[test]
fn usr_344_policy_gives_no_access_to_plugin_data_and_audit_shows_only_ids_events_and_reasons() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_settings(NOTES, "api_token", "s3cret-value");
    rig.write_policy(&format!("[plugins]\ndeny_ids = [\"{NOTES}\"]\n"));
    let _server = rig.start_server("s1");
    let audit = rig.cli(&["audit"]);
    assert_eq!(audit.exit_code, 0, "{}", audit.stderr);
    assert!(
        !audit.stdout_raw.contains("s3cret-value"),
        "settings values leaked into audit"
    );
    let allowed: BTreeSet<&str> = ["seq", "time", "event", "plugin", "reason"]
        .into_iter()
        .collect();
    for record in &audit.stdout_jsonl {
        for key in record.as_object().expect("record object").keys() {
            assert!(
                allowed.contains(key.as_str()),
                "audit record carries {key}: {record}"
            );
        }
    }

    let page = manual_page("guide-enterprise-deployment");
    assert!(
        page.lines().any(|line| line.contains("settings values")
            && line.contains("consent records")
            && line.contains("administrator")),
        "the administration guide must list this limit"
    );
}

/// REQ: USR-341
#[test]
fn usr_341_inventory_of_a_machine_with_no_plugins_is_empty_and_exits_zero() {
    assert_manual_says(
        "guide-enterprise-deployment",
        "`ilium plugin list --json --inventory` MUST output id, version, tier, content hash, key id, state and granted permissions",
    );
    let rig = rig();
    let output = rig.cli(&["list", "--json", "--inventory"]);
    assert_eq!(output.exit_code, 0, "{}", output.stderr);
    assert!(
        output.stdout_raw.trim().is_empty(),
        "no plugins means no inventory lines: {}",
        output.stdout_raw
    );
}

/// REQ: USR-343
#[test]
fn usr_343_row_of_an_allowed_plugin_carries_no_blocked_message_or_contact() {
    assert_manual_says(
        "guide-enterprise-deployment",
        "the host MUST append it to every message that says `Blocked by your administrator`",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_policy(
        "[notice]\ncontact = \"it-help@example.org\"\n\n[plugins]\ndeny_ids = [\"com.example.other\"]\n",
    );
    let server = rig.start_server("s1");
    rig.advance(secs(3));
    let row = server
        .render_plugins_screen()
        .row_for(NOTES)
        .expect("row")
        .text
        .clone();
    assert!(
        !row.contains("Blocked by your administrator"),
        "an allowed plugin is not blocked: {row}"
    );
    assert!(
        !row.contains("it-help@example.org"),
        "the contact is only added to blocked messages: {row}"
    );
}

/// REQ: USR-344
#[test]
fn usr_344_applying_a_policy_leaves_the_plugin_setting_value_unchanged() {
    assert_manual_says(
        "guide-enterprise-deployment",
        "Applying a policy MUST NOT give an administrator access to a user's plugin data, settings values or consent records",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.plant_settings(NOTES, "api_token", "s3cret-value");
    rig.write_policy(&format!("[plugins]\ndeny_ids = [\"{NOTES}\"]\n"));
    rig.reload_policy();
    assert_eq!(
        rig.setting(NOTES, "api_token").as_deref(),
        Some("s3cret-value"),
        "the policy does not rewrite or expose the user's setting"
    );
}
