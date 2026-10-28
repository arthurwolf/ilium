//! Requirements SEC-966 .. SEC-968 (reference-security-response.md): the security-fix window of each
//! host minor, the start-time advisory check with its warn line and `ilium version --json` field, and
//! the status bar badge for a High or Critical advisory.

use serde_json::json;

use crate::support::*;

/// REQ: SEC-966
#[test]
fn sec_966_each_host_minor_gets_security_fixes_for_at_least_twelve_months_after_release() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("Each host minor receives security fixes for the window of [DEV-964]: at least 12 months (proposed, owner to confirm) after its release date."),
        "the page must state the 12-month minimum window after release"
    );
}

/// REQ: SEC-966
#[test]
fn sec_966_the_latest_minor_and_the_one_before_keep_fixes_even_after_their_end_date() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("The latest minor and the one before it receive security fixes in every case, even after their end date (proposed, owner to confirm)."),
        "the page must keep the latest two minors supported past their end date"
    );
}

/// REQ: SEC-966
#[test]
fn sec_966_a_minor_past_its_end_date_reports_end_of_life_and_keeps_loading_plugins() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("A minor past its end date reports `support: \"end-of-life\"`"),
        "the page must name the end-of-life support state"
    );
    assert!(
        page.contains("keeps loading its installed plugins unchanged."),
        "the page must keep installed plugins loading past the end date"
    );
}

/// REQ: SEC-966
#[test]
fn sec_966_a_host_past_its_end_date_still_loads_installed_plugins_unchanged() {
    let rig = setup(&[])
        .option("host-version", json!("0.1.0"))
        .option("host-support", json!("end-of-life"))
        .option("installed-plugins", json!(["example.demo"]))
        .start();
    assert!(
        !rig.host.plugin_disabled("example.demo"),
        "an end-of-life host must not disable an installed plugin"
    );
    let restarted = rig.restart();
    assert!(
        !restarted.host.plugin_disabled("example.demo"),
        "the plugin loads unchanged at the next start of the end-of-life host"
    );
    let root = scratch();
    let mut env = cli_env(root.path());
    env.host_release = "v0.1.0".to_owned();
    env.now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(4_000_000_000);
    let output = ilium_plugin_host::cli::run(&args(&["version", "--json"]), &env);
    assert_eq!(output.exit_code, 0, "{}", output.stderr);
    let text = format!("{:?}", output.stdout_jsonl);
    assert!(text.contains("end-of-life"), "version --json reports support end-of-life: {text}");
}

/// REQ: SEC-967
#[test]
fn sec_967_an_affected_host_writes_one_host_advisory_affected_warn_line_at_start() {
    let advisory = json!({ "id": "ILIUM-2026-0001", "severity": "High", "fixed": "1.0.0" });
    let affected = setup(&[])
        .option("runtime-advisory", advisory.clone())
        .option("host-version", json!("0.9.0"))
        .start();
    let lines: Vec<String> = affected
        .log_messages()
        .into_iter()
        .filter(|message| message.contains("host.advisory-affected"))
        .collect();
    assert_eq!(lines.len(), 1, "exactly one host.advisory-affected line per start: {lines:?}");
    assert!(lines[0].contains("ILIUM-2026-0001"), "the line names the advisory id: {lines:?}");
    assert!(lines[0].contains("1.0.0"), "the line names the fixed version: {lines:?}");
    let fixed = setup(&[])
        .option("runtime-advisory", advisory)
        .option("host-version", json!("1.0.0"))
        .start();
    assert!(
        fixed
            .log_messages()
            .iter()
            .all(|message| !message.contains("host.advisory-affected")),
        "a host at the fixed version writes no advisory line"
    );
}

/// REQ: SEC-967
#[test]
fn sec_967_ilium_version_json_lists_the_affected_advisory_ids_and_a_fixed_host_lists_none() {
    let root = scratch();
    let mut env = cli_env(root.path());
    env.host_release = "v0.1.0".to_owned();
    let output = ilium_plugin_host::cli::run(&args(&["version", "--json"]), &env);
    assert_eq!(output.exit_code, 0, "{}", output.stderr);
    let record = output
        .stdout_jsonl
        .iter()
        .find(|record| record.get("advisories").is_some())
        .unwrap_or_else(|| panic!("version --json must carry an advisories field: {:?}", output.stdout_jsonl));
    let affected = record["advisories"].as_array().expect("advisories is a list");
    for entry in affected {
        let id = entry.as_str().or_else(|| entry["id"].as_str()).expect("an advisory id");
        assert!(id.starts_with("ILIUM-"), "advisory ids look like ILIUM-YYYY-NNNN, got {id}");
    }
    env.host_release = "v99.0.0".to_owned();
    let fixed = ilium_plugin_host::cli::run(&args(&["version", "--json"]), &env);
    let fixed_record = fixed
        .stdout_jsonl
        .iter()
        .find(|record| record.get("advisories").is_some())
        .unwrap_or_else(|| panic!("a fixed host still reports the field: {:?}", fixed.stdout_jsonl));
    assert_eq!(fixed_record["advisories"], json!([]), "a fixed host lists no advisory");
}

/// REQ: SEC-968
#[test]
fn sec_968_a_high_advisory_shows_the_update_badge_next_to_the_safe_badge() {
    let rig = setup(&[])
        .option(
            "runtime-advisory",
            json!({ "id": "ILIUM-2026-0001", "severity": "High", "fixed": "1.0.0" }),
        )
        .option("host-version", json!("0.9.0"))
        .start();
    let bar = rig.host.status_bar_text();
    assert!(
        bar.contains("Update: ILIUM-2026-0001"),
        "the status bar must show the advisory id: {bar}"
    );
    assert!(bar.contains("SAFE"), "the badge is shown next to SAFE: {bar}");
}

/// REQ: SEC-968
#[test]
fn sec_968_the_badge_disappears_only_when_the_host_reaches_the_fixed_version() {
    let rig = setup(&[])
        .option(
            "runtime-advisory",
            json!({ "id": "ILIUM-2026-0001", "severity": "High", "fixed": "1.0.0" }),
        )
        .option("host-version", json!("1.0.0"))
        .start();
    assert!(
        !rig.host.status_bar_text().contains("Update:"),
        "a host at the fixed version must show no update badge"
    );
}

/// REQ: SEC-968
#[test]
fn sec_968_the_advisory_page_states_the_badge_and_its_disappearance_rule() {
    let page = manual_page("reference-security-response.md");
    assert!(
        page.contains("the status bar MUST show `Update: <advisory id>` (proposed, owner to confirm) next to the `SAFE` badge of [SEC-329]"),
        "the page must state the badge text and its place next to SAFE"
    );
    assert!(
        page.contains("The badge MUST disappear only when the host reaches the `fixed` version of every such advisory."),
        "the page must state that the badge disappears only at the fixed version"
    );
}
