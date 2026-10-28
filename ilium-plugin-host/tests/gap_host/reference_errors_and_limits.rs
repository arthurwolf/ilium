//! Manual page `reference-errors-and-limits`: the host limits table and the `ilium plugin limits`
//! command (HST-682, 683).

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-682
#[test]
fn hst_682_manual_limits_table_lists_every_named_limit_with_its_compiled_default() {
    let rig = rig();
    let table = rig.manual_limits_table();
    for column in [
        "name",
        "unit",
        "default",
        "minimum",
        "ceiling",
        "config key",
        "error code",
        "requirement",
    ] {
        assert!(
            table
                .header
                .iter()
                .any(|head| head.eq_ignore_ascii_case(column)),
            "table has column {column}"
        );
    }
    for limit in rig.limits_named_in_requirements() {
        assert!(
            table.rows.iter().any(|row| row.name == limit.name),
            "limit {} listed in the table",
            limit.name
        );
        assert_eq!(
            table.default_for(&limit.name),
            limit.compiled_default,
            "default of {} matches the compiled value",
            limit.name
        );
    }
}

/// REQ: HST-683
#[test]
fn hst_683_limits_command_shows_source_usage_and_requested_versus_applied_for_clamped_limits() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_config_value("compile_cache_mib", "99999");
    let output = rig.cli(&["plugin", "limits", NOTES, "--json"]);
    assert_eq!(output.exit_code, 0, "limits command succeeds");
    let clamped = output
        .stdout_jsonl
        .iter()
        .find(|row| row["name"] == "compile_cache_mib")
        .expect("the clamped limit listed");
    assert!(
        clamped["source"].is_string() && clamped["usage"].is_number(),
        "source and current usage shown"
    );
    assert_eq!(clamped["requested"], 99_999, "requested value shown");
    assert_eq!(clamped["applied"], 1024, "applied value shown");
}

/// REQ: HST-682
#[test]
fn hst_682_manual_table_has_at_least_one_row_per_named_limit() {
    let rig = rig();
    let table = rig.manual_limits_table();
    let named = rig.limits_named_in_requirements().into_iter().count();
    assert!(
        table.rows.len() >= named,
        "{} table rows for {named} named limits",
        table.rows.len()
    );
}

/// REQ: HST-683
#[test]
fn hst_683_limits_command_without_a_plugin_id_lists_the_host_limits() {
    let rig = rig();
    let output = rig.cli(&["plugin", "limits", "--json"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert!(
        !output.stdout_jsonl.is_empty(),
        "host limits are printed without a plugin id"
    );
}
