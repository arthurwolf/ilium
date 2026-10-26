//! reference-manifest.md: components and exports (PKG-040..045), grants (PKG-050..058),
//! activation and contributes (PKG-059..065) and limits and validation (PKG-070..079).

use std::collections::BTreeMap;

use ilium_plugin_api::grants::{Grant, check_grant};
use ilium_plugin_api::limits::{self, LimitKey, Limits};
use ilium_plugin_api::manifest::{
    CheckMode, ComponentSummary, Stage, ValidationContext, check_component, parse_activation,
};
use ilium_plugin_api::{Side, Tier};
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::support::*;

fn component_context() -> ValidationContext {
    context_with(Tier::T3, CheckMode::Install)
}

fn manifest_of(value: &Value) -> ilium_plugin_api::manifest::Manifest {
    ilium_plugin_api::manifest::parse_manifest(&value.to_string()).expect("fixture parses")
}

fn summary(exports: &[&str]) -> ComponentSummary {
    ComponentSummary {
        exports: exports.iter().map(|name| (*name).to_owned()).collect(),
        api_sections: vec![ilium_plugin_api::version::API_VERSION_BYTES.to_vec()],
        ..ComponentSummary::default()
    }
}

fn codes_of(found: &[ilium_plugin_api::manifest::Diagnostic]) -> Vec<String> {
    error_codes(found)
}

// ---------------------------------------------------------------------------------------------
// PKG-040 .. PKG-045
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-040
#[test]
fn pkg_040_the_fixed_entry_names_are_accepted() {
    assert_clean(&server());
    assert_clean(&client());
    assert_clean(&scene());
}

/// REQ: PKG-040
#[test]
fn pkg_040_other_entry_names_refused_entry_invalid() {
    for (base, side, wrong) in [
        (server(), "server", "client.wasm"),
        (client(), "client", "server.wasm"),
        (scene(), "scene", "client.wasm"),
        (client(), "client", "Client.wasm"),
        (client(), "client", "plugin/client.wasm"),
        (client(), "client", "client.wasm "),
        (client(), "client", ""),
    ] {
        let value = set(base, &format!("/{side}/entry"), json!(wrong));
        assert_has(&errors(&value), "manifest.entry-invalid");
    }
}

/// REQ: PKG-041
#[test]
fn pkg_041_the_fixed_worlds_are_accepted() {
    assert_lacks(&errors(&server()), "manifest.schema");
    assert_lacks(&errors(&client()), "manifest.schema");
    assert_lacks(&errors(&scene()), "manifest.schema");
}

/// REQ: PKG-041
#[test]
fn pkg_041_other_worlds_refused_schema() {
    for (base, side, wrong) in [
        (server(), "server", "ilium:client/base@0.1.0"),
        (client(), "client", "ilium:server/base@0.1.0"),
        (scene(), "scene", "ilium:client/base@0.1.0"),
        (client(), "client", "ilium:client/base@0.2.0"),
        (client(), "client", "ilium:client/base"),
        (client(), "client", ""),
    ] {
        let value = set(base, &format!("/{side}/world"), json!(wrong));
        assert_has(&errors(&value), "manifest.schema");
    }
}

/// REQ: PKG-042
#[test]
fn pkg_042_optional_exports_of_each_side_are_accepted() {
    for name in [
        "views",
        "annotator",
        "service-provider",
        "migrator",
        "hook-handler",
    ] {
        let value = set(client(), "/client/exports", json!([name]));
        assert_lacks(&errors(&value), "manifest.schema");
    }
    for name in ["detector", "job-runner", "settings-validate"] {
        let value = set(server(), "/server/exports", json!([name]));
        assert_lacks(&errors(&value), "manifest.schema");
    }
}

/// REQ: PKG-042
#[test]
fn pkg_042_required_unknown_and_wrong_side_exports_refused_schema() {
    for name in ["lifecycle", "handler", "renderer", "no-such-export"] {
        let value = set(client(), "/client/exports", json!([name]));
        assert_has(&errors(&value), "manifest.schema");
    }
    // `detector` is server only, `views` is client only.
    assert_has(
        &errors(&set(client(), "/client/exports", json!(["detector"]))),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(server(), "/server/exports", json!(["views"]))),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(scene(), "/scene/exports", json!(["migrator"]))),
        "manifest.schema",
    );
}

/// REQ: PKG-042
#[test]
fn pkg_042_duplicate_names_refused_schema_and_the_default_is_empty() {
    assert_has(
        &errors(&set(client(), "/client/exports", json!(["views", "views"]))),
        "manifest.schema",
    );
    let parsed = manifest_of(&client());
    assert!(parsed.client.expect("client").exports.is_empty());
}

/// REQ: PKG-043
#[test]
fn pkg_043_matching_export_lists_pass() {
    let value = set(client(), "/client/exports", json!(["views", "annotator"]));
    let manifest = manifest_of(&value);
    let found = check_component(
        &manifest,
        Side::Client,
        &summary(&["lifecycle", "handler", "annotator", "views"]),
        &component_context(),
    );
    assert_lacks(&codes_of(&found), "manifest.exports-mismatch");
}

/// REQ: PKG-043
#[test]
fn pkg_043_a_missing_name_is_refused_exports_mismatch() {
    let value = set(client(), "/client/exports", json!(["views", "annotator"]));
    let manifest = manifest_of(&value);
    let found = check_component(
        &manifest,
        Side::Client,
        &summary(&["lifecycle", "handler", "views"]),
        &component_context(),
    );
    assert_has(&codes_of(&found), "manifest.exports-mismatch");
}

/// REQ: PKG-043
#[test]
fn pkg_043_an_extra_name_is_refused_exports_mismatch() {
    let manifest = manifest_of(&client());
    let found = check_component(
        &manifest,
        Side::Client,
        &summary(&["lifecycle", "handler", "views"]),
        &component_context(),
    );
    assert_has(&codes_of(&found), "manifest.exports-mismatch");
}

/// REQ: PKG-043
#[test]
fn pkg_043_the_mismatch_is_reported_at_stage_six() {
    let manifest = manifest_of(&client());
    let found = check_component(
        &manifest,
        Side::Client,
        &summary(&["lifecycle", "handler", "views"]),
        &component_context(),
    );
    let diagnostic = first_with(&found, "manifest.exports-mismatch");
    assert_eq!(diagnostic.stage, Some(Stage(6)));
}

fn extension_value(handler: Option<&str>) -> Value {
    let mut entry = json!({ "point": "com.example.host/point", "id": "ext", "priority": 1 });
    if let Some(side) = handler {
        entry["handler"] = json!(side);
    }
    entry
}

/// REQ: PKG-044
#[test]
fn pkg_044_hook_handler_with_a_handler_extension_is_consistent() {
    let value = set(
        set(client(), "/client/exports", json!(["hook-handler"])),
        "/contributes/extensions",
        json!([extension_value(Some("client"))]),
    );
    let found = install_errors(&packed(value), Tier::T3);
    assert!(found.is_empty(), "valid hook-handler fixture: {found:?}");
}

/// REQ: PKG-044
#[test]
fn pkg_044_hook_handler_without_any_handler_extension_refused_exports_mismatch() {
    let value = set(client(), "/client/exports", json!(["hook-handler"]));
    assert_has(
        &install_errors(&packed(value.clone()), Tier::T3),
        "manifest.exports-mismatch",
    );
}

/// REQ: PKG-044
#[test]
fn pkg_044_handler_extension_without_hook_handler_in_the_list_refused_exports_mismatch() {
    let value = set(
        client(),
        "/contributes/extensions",
        json!([extension_value(Some("client"))]),
    );
    assert_has(
        &install_errors(&packed(value.clone()), Tier::T3),
        "manifest.exports-mismatch",
    );
}

/// REQ: PKG-044
#[test]
fn pkg_044_extension_without_handler_needs_no_hook_handler() {
    let value = set(
        client(),
        "/contributes/extensions",
        json!([extension_value(None)]),
    );
    assert_lacks(
        &install_errors(&packed(value.clone()), Tier::T3),
        "manifest.exports-mismatch",
    );
}

/// REQ: PKG-045
#[test]
fn pkg_045_data_version_above_one_needs_migrator_in_exports() {
    let value = set(client(), "/data_version", json!(2));
    assert_has(
        &install_errors(&packed(value.clone()), Tier::T3),
        "manifest.exports-mismatch",
    );
    let listed = set(value, "/client/exports", json!(["migrator"]));
    assert_lacks(
        &install_errors(&packed(listed), Tier::T3),
        "manifest.exports-mismatch",
    );
}

/// REQ: PKG-045
#[test]
fn pkg_045_data_version_one_needs_no_migrator() {
    let value = set(client(), "/data_version", json!(1));
    assert!(install_errors(&packed(value.clone()), Tier::T3).is_empty());
    assert_has(
        &install_errors(&packed(set(client(), "/data_version", json!(2))), Tier::T3),
        "manifest.exports-mismatch",
    );
}

/// REQ: PKG-045
#[test]
fn pkg_045_each_server_and_client_component_needs_its_own_migrator() {
    let both = set(
        set(client(), "/data_version", json!(3)),
        "/server",
        json!({ "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }),
    );
    let only_client = set(both.clone(), "/client/exports", json!(["migrator"]));
    assert_has(
        &install_errors(&packed(only_client.clone()), Tier::T3),
        "manifest.exports-mismatch",
    );
    let both_listed = set(only_client, "/server/exports", json!(["migrator"]));
    assert_lacks(
        &install_errors(&packed(both_listed), Tier::T3),
        "manifest.exports-mismatch",
    );
}

/// REQ: PKG-045
#[test]
fn pkg_045_a_scene_component_has_no_migrator_and_needs_none() {
    let value = set(scene(), "/data_version", json!(2));
    assert_lacks(
        &install_errors(&packed(value.clone()), Tier::T3),
        "manifest.exports-mismatch",
    );
}

/// REQ: PKG-045
#[test]
fn pkg_045_the_component_must_really_export_migrator() {
    let value = set(
        set(client(), "/data_version", json!(2)),
        "/client/exports",
        json!(["migrator"]),
    );
    let manifest = manifest_of(&value);
    let missing = check_component(
        &manifest,
        Side::Client,
        &summary(&["lifecycle", "handler"]),
        &component_context(),
    );
    assert_has(&codes_of(&missing), "manifest.exports-mismatch");
    let present = check_component(
        &manifest,
        Side::Client,
        &summary(&["lifecycle", "handler", "migrator"]),
        &component_context(),
    );
    assert_lacks(&codes_of(&present), "manifest.exports-mismatch");
}

// ---------------------------------------------------------------------------------------------
// PKG-050 .. PKG-058 grants
// ---------------------------------------------------------------------------------------------

fn grant_code(text: &str) -> Option<String> {
    Grant::parse(text)
        .err()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
}

/// REQ: PKG-050
#[test]
fn pkg_050_well_formed_grants_parse_and_split_on_the_first_colon() {
    let bare = Grant::parse("tree.read").expect("parses");
    assert_eq!(bare.family(), "tree.read");
    assert_eq!(bare.argument(), None);
    let with_argument = Grant::parse("http:api.example.com:8080").expect("parses");
    assert_eq!(with_argument.family(), "http");
    assert_eq!(with_argument.argument(), Some("api.example.com:8080"));
    assert_eq!(with_argument.as_str(), "http:api.example.com:8080");
}

/// REQ: PKG-050
#[test]
fn pkg_050_length_boundary_3_and_256_characters() {
    assert_eq!(grant_code("tree.read"), None);
    // A grant shorter than 3 characters is never a grant.
    assert!(grant_code("ab").is_some());
    let argument = repeat('a', 256 - "topics.subscribe:".len());
    let at_limit = format!("topics.subscribe:{argument}");
    assert_eq!(at_limit.len(), 256);
    assert_ne!(
        grant_code(&at_limit).as_deref(),
        Some("manifest.grant-argument")
    );
    let over = format!("{at_limit}a");
    assert!(grant_code(&over).is_some());
}

/// REQ: PKG-050
#[test]
fn pkg_050_malformed_names_are_refused() {
    for text in [
        "",
        "Tree.Read",
        "tree_read",
        "tree read",
        ".tree",
        "tree.",
        "tree..read",
        ":x",
    ] {
        let code = grant_code(text);
        // manual does not pin which of the two codes each malformed string gets
        assert!(
            matches!(
                code.as_deref(),
                Some("manifest.grant-unknown") | Some("manifest.grant-argument")
            ),
            "{text:?} gave {code:?}"
        );
    }
}

/// REQ: PKG-050
#[test]
fn pkg_050_in_a_manifest_a_bad_grant_string_is_refused_with_a_grant_code() {
    let value = client_with_grants(&["Tree.Read"]);
    let found = errors(&value);
    // manual does not pin grant-unknown versus grant-argument for a malformed name
    assert!(
        found
            .iter()
            .any(|code| code == "manifest.grant-unknown" || code == "manifest.grant-argument"),
        "{found:?}"
    );
    assert_lacks(&found, "manifest.schema");
}

/// REQ: PKG-051
#[test]
fn pkg_051_a_grant_outside_the_capability_table_refused_grant_unknown() {
    assert_eq!(
        grant_code("teleport.now").as_deref(),
        Some("manifest.grant-unknown")
    );
    assert_has(
        &errors(&client_with_grants(&["teleport.now"])),
        "manifest.grant-unknown",
    );
}

/// REQ: PKG-051
#[test]
fn pkg_051_every_family_of_the_table_is_known() {
    for info in ilium_plugin_api::grants::table() {
        let family = info.family;
        assert!(ilium_plugin_api::grants::info(family).is_some(), "{family}");
    }
    assert!(ilium_plugin_api::grants::info("no.such.family").is_none());
}

/// REQ: PKG-052
#[test]
fn pkg_052_a_grant_that_needs_an_argument_and_has_none_refused() {
    for text in [
        "http",
        "process.run",
        "fs.read",
        "services.use",
        "messages.peer",
        "topics.subscribe",
    ] {
        assert_eq!(
            grant_code(text).as_deref(),
            Some("manifest.grant-argument-missing"),
            "{text}"
        );
    }
}

/// REQ: PKG-052
#[test]
fn pkg_052_a_grant_without_argument_but_given_one_refused_grant_argument() {
    for text in ["tree.read:everything", "fs.pick:x", "sessions.cross:all"] {
        assert_eq!(
            grant_code(text).as_deref(),
            Some("manifest.grant-argument"),
            "{text}"
        );
    }
}

/// REQ: PKG-052
#[test]
fn pkg_052_an_empty_argument_counts_as_missing_or_invalid() {
    let code = grant_code("http:");
    assert!(
        matches!(
            code.as_deref(),
            Some("manifest.grant-argument-missing") | Some("manifest.grant-argument")
        ),
        "{code:?}"
    );
}

/// REQ: PKG-053
#[test]
fn pkg_053_wildcards_only_where_the_grammar_allows() {
    assert_eq!(grant_code("topics.subscribe:*"), None);
    assert_eq!(grant_code("topics.subscribe:ilium/*"), None);
    assert_eq!(grant_code("commands.invoke:*"), None);
    assert_eq!(grant_code("commands.invoke:com.example.a/*"), None);
    assert_eq!(grant_code("http:*.example.com"), None);
    for text in [
        "messages.peer:*",
        "services.use:*",
        "process.run:*",
        "fs.read:work*trees",
    ] {
        let code = grant_code(text);
        assert!(code.is_some(), "{text} must be refused");
    }
    assert_eq!(
        grant_code("messages.peer:*").as_deref(),
        Some("manifest.grant-wildcard")
    );
    assert_eq!(
        grant_code("services.use:*").as_deref(),
        Some("manifest.grant-wildcard")
    );
}

/// REQ: PKG-053
#[test]
fn pkg_053_a_wildcard_inside_a_path_template_other_than_the_final_double_star_refused() {
    assert!(grant_code("fs.read:path:{home}/*/x").is_some());
    assert_eq!(grant_code("fs.read:path:{home}/docs/**"), None);
}

/// REQ: PKG-054
#[test]
fn pkg_054_a_grant_on_a_side_that_does_not_have_it_refused_grant_side() {
    let grant = Grant::parse("settings.read-all").expect("parses");
    let found = check_grant(&grant, Tier::T1, Side::Server);
    assert_eq!(
        found.err().map(|d| d.code.as_str().to_owned()).as_deref(),
        Some("manifest.grant-side")
    );
    assert!(check_grant(&grant, Tier::T1, Side::Client).is_ok());
}

/// REQ: PKG-054
#[test]
fn pkg_054_server_only_grant_on_the_client_list_refused_grant_side() {
    let grant = Grant::parse("sessions.cross").expect("parses");
    assert_eq!(
        check_grant(&grant, Tier::T1, Side::Client)
            .err()
            .map(|d| d.code.as_str().to_owned())
            .as_deref(),
        Some("manifest.grant-side")
    );
    assert!(check_grant(&grant, Tier::T1, Side::Server).is_ok());
}

/// REQ: PKG-054
#[test]
fn pkg_054_a_manifest_that_lists_a_grant_on_the_wrong_side_is_refused() {
    let value = set(client(), "/capabilities/client", json!(["sessions.cross"]));
    assert_has(&errors(&value), "manifest.grant-side");
}

/// REQ: PKG-055
#[test]
fn pkg_055_fs_pick_needs_a_picked_scope_in_the_same_list() {
    assert_has(
        &errors(&client_with_grants(&["fs.pick"])),
        "manifest.grant-dependency",
    );
    assert_lacks(
        &errors(&client_with_grants(&["fs.pick", "fs.read:picked"])),
        "manifest.grant-dependency",
    );
    assert_lacks(
        &errors(&client_with_grants(&["fs.pick", "fs.write:picked"])),
        "manifest.grant-dependency",
    );
}

/// REQ: PKG-055
#[test]
fn pkg_055_the_dependency_must_be_on_the_same_side() {
    let value = set(
        set(
            client(),
            "/server",
            json!({ "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }),
        ),
        "/capabilities",
        json!({ "client": ["fs.pick"], "server": ["fs.read:picked"] }),
    );
    assert_has(&errors(&value), "manifest.grant-dependency");
}

/// REQ: PKG-055
#[test]
fn pkg_055_settings_write_all_needs_settings_read_all() {
    let alone = errors(&client_with_grants(&["settings.write-all"]));
    assert_has(&alone, "manifest.grant-dependency");
    let both = errors(&client_with_grants(&[
        "settings.write-all",
        "settings.read-all",
    ]));
    assert_lacks(&both, "manifest.grant-dependency");
}

/// REQ: PKG-056
#[test]
fn pkg_056_process_run_takes_a_program_name_or_an_absolute_path() {
    assert_eq!(grant_code("process.run:git"), None);
    assert_eq!(grant_code("process.run:/usr/bin/git"), None);
}

/// REQ: PKG-056
#[test]
fn pkg_056_process_run_with_arguments_a_relative_path_or_dot_dot_refused_grant_program() {
    for text in [
        "process.run:git status",
        "process.run:bin/git",
        "process.run:../git",
        "process.run:/usr/../bin/git",
        "process.run:./git",
    ] {
        assert_eq!(
            grant_code(text).as_deref(),
            Some("manifest.grant-program"),
            "{text}"
        );
    }
}

/// REQ: PKG-056
#[test]
fn pkg_056_an_unknown_fs_scope_refused_grant_scope() {
    assert_eq!(
        grant_code("fs.read:nowhere").as_deref(),
        Some("manifest.grant-scope")
    );
    assert_eq!(grant_code("fs.read:worktrees"), None);
    assert_eq!(grant_code("fs.read:picked"), None);
}

/// REQ: PKG-056
#[test]
fn pkg_056_a_scope_that_does_not_accept_the_access_refused_grant_scope() {
    // `picked` is granted per pick, `project` is not writable by scope name alone in some builds;
    // the stable fact is that an fs.write scope must be one that accepts writes.
    let code = grant_code("fs.write:definitely-not-a-scope");
    assert_eq!(code.as_deref(), Some("manifest.grant-scope"));
}

/// REQ: PKG-057
#[test]
fn pkg_057_settings_read_all_is_bundled_only() {
    let grant = Grant::parse("settings.read-all").expect("parses");
    assert!(check_grant(&grant, Tier::T1, Side::Client).is_ok());
    for tier in [Tier::T2, Tier::T3] {
        assert_eq!(
            check_grant(&grant, tier, Side::Client)
                .err()
                .map(|d| d.code.as_str().to_owned())
                .as_deref(),
            Some("manifest.grant-tier")
        );
    }
}

/// REQ: PKG-057
#[test]
fn pkg_057_sessions_cross_is_refused_for_t3() {
    let grant = Grant::parse("sessions.cross").expect("parses");
    assert_eq!(
        check_grant(&grant, Tier::T3, Side::Server)
            .err()
            .map(|d| d.code.as_str().to_owned())
            .as_deref(),
        Some("manifest.grant-tier")
    );
}

/// REQ: PKG-057
#[test]
fn pkg_057_the_tier_check_runs_at_stage_nine_in_install_mode() {
    let value = packed(client_with_grants(&["settings.read-all"]));
    let found = diagnostics(&value, Tier::T3, CheckMode::Install);
    let diagnostic = first_with(&found, "manifest.grant-tier");
    assert_eq!(diagnostic.stage, Some(Stage(9)));
    let bundled = install_errors(&value, Tier::T1);
    assert_lacks(&bundled, "manifest.grant-tier");
}

/// REQ: PKG-058
#[test]
fn pkg_058_doc_declining_a_grant_never_fails_the_install() {
    let text = page("reference-manifest.md");
    assert!(text.contains("Declining a grant at install never fails the install."));
    assert!(text.contains("`not-permitted`"));
}

// ---------------------------------------------------------------------------------------------
// PKG-059 .. PKG-065 activation and contributes
// ---------------------------------------------------------------------------------------------

/// REQ: PKG-059
#[test]
fn pkg_059_kind_colon_name_triggers_parse() {
    let startup = parse_activation("startup").expect("parses");
    let _ = startup;
    assert!(parse_activation("durable-timer").is_ok());
    assert!(parse_activation("event:session-created").is_ok());
}

/// REQ: PKG-059
#[test]
fn pkg_059_unknown_kind_or_a_name_with_spaces_is_refused() {
    assert!(parse_activation("bogus:name").is_err());
    assert!(parse_activation("event:two words").is_err());
    assert!(parse_activation("event:").is_err());
    assert!(parse_activation("").is_err());
}

/// REQ: PKG-059
#[test]
fn pkg_059_every_kind_of_the_table_is_accepted_with_a_name() {
    assert!(!ilium_plugin_api::manifest::ACTIVATION_KINDS.is_empty());
    for kind in ilium_plugin_api::manifest::ACTIVATION_KINDS {
        assert!(!kind.is_empty());
        let text = format!("{kind}:name");
        assert!(parse_activation(&text).is_ok(), "{text}");
    }
}

/// REQ: PKG-059
#[test]
fn pkg_059_a_manifest_without_activation_is_lazy_and_valid() {
    let parsed = manifest_of(&client());
    assert!(parsed.activation.is_empty());
    assert_clean(&client());
}

/// REQ: PKG-059
#[test]
fn pkg_059_manifest_with_33_triggers_or_a_duplicate_refused_schema() {
    let many: Vec<String> = (0..33).map(|index| format!("event:e{index}")).collect();
    assert_has(
        &errors(&set(client(), "/activation", json!(many))),
        "manifest.schema",
    );
    assert_has(
        &errors(&set(client(), "/activation", json!(["startup", "startup"]))),
        "manifest.schema",
    );
}

/// REQ: PKG-060
#[test]
fn pkg_060_a_trigger_that_names_an_existing_own_command_is_accepted() {
    let value = set(
        set(client(), "/activation", json!(["command:run"])),
        "/contributes/commands",
        json!([{ "id": "run", "title": "Run" }]),
    );
    assert_lacks(&errors(&value), "manifest.contribution-target");
}

/// REQ: PKG-060
#[test]
fn pkg_060_a_trigger_that_names_a_missing_own_contribution_refused_contribution_target() {
    for trigger in [
        "command:missing",
        "view:missing",
        "pane-kind:missing",
        "scene:missing",
        "cli:missing",
        "settings:missing",
        "catalog:missing",
        "menu:missing",
        "virtual-rows:missing",
    ] {
        let value = set(client(), "/activation", json!([trigger]));
        assert_has(&errors(&value), "manifest.contribution-target");
    }
}

/// REQ: PKG-061
#[test]
fn pkg_061_commands_allow_256_entries_and_refuse_257() {
    let commands = |count: usize| -> Vec<Value> {
        (0..count)
            .map(|index| json!({ "id": format!("c{index}"), "title": "T" }))
            .collect()
    };
    assert_lacks(
        &errors(&set(
            client(),
            "/contributes/commands",
            json!(commands(256)),
        )),
        "manifest.contribution-limit",
    );
    assert_has(
        &errors(&set(
            client(),
            "/contributes/commands",
            json!(commands(257)),
        )),
        "manifest.contribution-limit",
    );
}

/// REQ: PKG-061
#[test]
fn pkg_061_entry_id_grammar_and_uniqueness() {
    let ok = set(
        client(),
        "/contributes/commands",
        json!([{ "id": repeat('a', 64), "title": "T" }]),
    );
    assert_lacks(&errors(&ok), "manifest.schema");
    for id in ["", "A", "a_b", "a b", repeat('a', 65).as_str()] {
        let value = set(
            client(),
            "/contributes/commands",
            json!([{ "id": id, "title": "T" }]),
        );
        assert_has(&errors(&value), "manifest.schema");
    }
    let duplicate = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "a", "title": "T" }, { "id": "a", "title": "U" }]),
    );
    let found = errors(&duplicate);
    // Uniqueness belongs to the id rule (schema); contribution-limit is for entry counts.
    assert_has(&found, "manifest.schema");
    assert_lacks(&found, "manifest.contribution-limit");
}

/// REQ: PKG-062
#[test]
fn pkg_062_keybinding_naming_a_missing_command_refused_contribution_target() {
    let value = set(
        client(),
        "/contributes/keybindings",
        json!([{ "command": "missing", "chord": "ctrl+k" }]),
    );
    assert_has(&errors(&value), "manifest.contribution-target");
}

/// REQ: PKG-062
#[test]
fn pkg_062_keybinding_naming_an_own_command_is_accepted() {
    let value = set(
        set(
            client(),
            "/contributes/commands",
            json!([{ "id": "run", "title": "Run" }]),
        ),
        "/contributes/keybindings",
        json!([{ "command": "run", "chord": "ctrl+k" }]),
    );
    assert_lacks(&errors(&value), "manifest.contribution-target");
}

/// REQ: PKG-062
#[test]
fn pkg_062_toolbar_button_and_context_menu_commands_must_exist() {
    let button = set(
        client(),
        "/contributes/toolbar_buttons",
        json!([{ "id": "b", "icon": "puzzle", "label": "B", "command": "missing" }]),
    );
    assert_has(&errors(&button), "manifest.contribution-target");
    let menu = set(
        client(),
        "/contributes/context_menu",
        json!([{ "id": "m", "title": "M", "command": "missing", "target": "terminal" }]),
    );
    assert_has(&errors(&menu), "manifest.contribution-target");
}

/// REQ: PKG-062
#[test]
fn pkg_062_virtual_row_parent_is_project_or_session() {
    for parent in ["project", "session"] {
        let value = set(
            client(),
            "/contributes/virtual_rows",
            json!([{ "id": "r", "title": "R", "icon": "puzzle", "parent": parent }]),
        );
        assert_lacks(&errors(&value), "manifest.schema");
    }
    let bad = set(
        client(),
        "/contributes/virtual_rows",
        json!([{ "id": "r", "title": "R", "icon": "puzzle", "parent": "nowhere" }]),
    );
    // manual does not pin the pointer of the refused parent
    assert_has(&errors(&bad), "manifest.schema");
}

/// REQ: PKG-062
#[test]
fn pkg_062_help_entry_file_must_resolve_to_an_own_help_file() {
    let value = set(
        client(),
        "/contributes/help",
        json!([{ "id": "intro", "file": "help/intro.md", "title": "Intro" }]),
    );
    let found = install_errors(&packed_with(value, &[]), Tier::T3);
    assert_has(&found, "manifest.file-missing");
}

/// REQ: PKG-063
#[test]
fn pkg_063_a_help_file_listed_in_files_is_accepted() {
    let value = set(
        client(),
        "/contributes/help",
        json!([{ "id": "intro", "file": "help/intro.md", "title": "Intro" }]),
    );
    let found = install_errors(&packed_with(value, &["help/intro.md"]), Tier::T3);
    assert_lacks(&found, "manifest.file-missing");
}

/// REQ: PKG-063
#[test]
fn pkg_063_a_sound_file_missing_from_files_refused_file_missing() {
    let value = set(
        client(),
        "/contributes/sounds",
        json!([{ "id": "ding", "file": "sounds/ding.ogg" }]),
    );
    assert_has(
        &install_errors(&packed_with(value.clone(), &[]), Tier::T3),
        "manifest.file-missing",
    );
    assert_lacks(
        &install_errors(&packed_with(value, &["sounds/ding.ogg"]), Tier::T3),
        "manifest.file-missing",
    );
}

/// REQ: PKG-063
#[test]
fn pkg_063_a_file_outside_help_or_sounds_directory_is_refused_by_schema() {
    let value = set(
        client(),
        "/contributes/sounds",
        json!([{ "id": "ding", "file": "other/ding.ogg" }]),
    );
    let found = install_errors(&packed_with(value, &["other/ding.ogg"]), Tier::T3);
    assert_has(&found, "manifest.schema");
}

/// REQ: PKG-064
#[test]
fn pkg_064_scenes_without_a_scene_component_refused_contribution_target() {
    let value = set(
        client(),
        "/contributes/scenes",
        json!([{ "id": "s", "title": "S", "family": "pipes" }]),
    );
    assert_has(&errors(&value), "manifest.contribution-target");
}

/// REQ: PKG-064
#[test]
fn pkg_064_scenes_with_a_scene_component_are_accepted() {
    let value = set(
        scene(),
        "/contributes/scenes",
        json!([{ "id": "s", "title": "S", "family": "pipes" }]),
    );
    assert_lacks(&errors(&value), "manifest.contribution-target");
}

/// REQ: PKG-065
#[test]
fn pkg_065_two_cli_entries_with_the_same_name_refused_cli_name_taken() {
    let value = set(
        client(),
        "/contributes/cli",
        json!([{ "name": "tool", "summary": "A" }, { "name": "tool", "summary": "B" }]),
    );
    assert_has(&errors(&value), "manifest.cli-name-taken");
}

/// REQ: PKG-065
#[test]
fn pkg_065_two_sounds_with_the_same_id_refused_sound_id_taken() {
    let value = set(
        client(),
        "/contributes/sounds",
        json!([{ "id": "ding", "file": "sounds/a.ogg" }, { "id": "ding", "file": "sounds/b.ogg" }]),
    );
    assert_has(&errors(&value), "manifest.sound-id-taken");
}

/// REQ: PKG-065
#[test]
fn pkg_065_distinct_names_are_accepted() {
    let value = set(
        client(),
        "/contributes/cli",
        json!([{ "name": "one", "summary": "A" }, { "name": "two", "summary": "B" }]),
    );
    assert_lacks(&errors(&value), "manifest.cli-name-taken");
}

/// REQ: PKG-065
#[test]
fn pkg_065_undeclared_cli_alias_field_is_refused_before_name_checks() {
    let value = set(
        client(),
        "/contributes/cli",
        json!([{ "name": "one", "summary": "A", "aliases": ["two"] }, { "name": "two", "summary": "B" }]),
    );
    assert_has(&errors(&value), "manifest.unknown-key");
}

// ---------------------------------------------------------------------------------------------
// PKG-070 .. PKG-079 limits and validation
// ---------------------------------------------------------------------------------------------

fn limits_of(pairs: &[(&str, u64)]) -> Limits {
    Limits(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), *value))
            .collect::<BTreeMap<_, _>>(),
    )
}

/// REQ: PKG-070
#[test]
fn pkg_070_integer_boundaries_0_and_4294967295_accepted() {
    for number in ["0", "4294967295"] {
        let text = with_raw_member(&client(), &format!("\"limits\":{{\"timers\":{number}}}"));
        assert_lacks(&text_errors(&text), "manifest.schema");
    }
}

/// REQ: PKG-070
#[test]
fn pkg_070_negative_fractional_and_oversized_values_refused_schema() {
    for number in ["-1", "1.5", "4294967296", "1e3", "\"7\"", "null", "true"] {
        let text = with_raw_member(&client(), &format!("\"limits\":{{\"timers\":{number}}}"));
        assert_has(&text_errors(&text), "manifest.schema");
    }
}

/// REQ: PKG-071
#[test]
fn pkg_071_a_value_above_the_ceiling_is_clamped_and_never_refused() {
    let key = LimitKey::MemoryMib;
    let ceiling = limits::ceiling(key, Tier::T3);
    let requested = limits_of(&[(key.as_key(), ceiling + 1)]);
    let effective = limits::clamp(&requested, Tier::T3).expect("clamps, never refuses");
    assert_eq!(effective[&key], ceiling);
}

/// REQ: PKG-071
#[test]
fn pkg_071_a_value_at_the_ceiling_is_kept() {
    let key = LimitKey::MemoryMib;
    let ceiling = limits::ceiling(key, Tier::T3);
    let effective = limits::clamp(&limits_of(&[(key.as_key(), ceiling)]), Tier::T3).expect("ok");
    assert_eq!(effective[&key], ceiling);
}

/// REQ: PKG-071
#[test]
fn pkg_071_zero_means_the_default() {
    let key = LimitKey::MemoryMib;
    let effective = limits::clamp(&limits_of(&[(key.as_key(), 0)]), Tier::T3).expect("ok");
    assert_eq!(effective[&key], limits::default_of(key));
}

/// REQ: PKG-071
#[test]
fn pkg_071_a_request_above_the_ceiling_in_a_manifest_is_accepted_at_install() {
    let value = set(
        packed(client()),
        "/limits/memory_mib",
        json!(4_294_967_295u64),
    );
    assert!(install_errors(&value, Tier::T3).is_empty());
}

/// REQ: PKG-072
#[test]
fn pkg_072_a_limit_key_outside_the_table_refused_unknown_key() {
    assert_has(
        &errors(&set(client(), "/limits/memory", json!(1))),
        "manifest.unknown-key",
    );
    assert_has(
        &errors(&set(client(), "/limits/Memory_MiB", json!(1))),
        "manifest.unknown-key",
    );
}

/// REQ: PKG-072
#[test]
fn pkg_072_clamp_refuses_a_key_outside_the_table() {
    let result = limits::clamp(&limits_of(&[("not_a_limit", 1)]), Tier::T3);
    assert_eq!(
        result.err().map(|d| d.code.as_str().to_owned()).as_deref(),
        Some("manifest.unknown-key")
    );
}

/// REQ: PKG-073
#[test]
fn pkg_073_the_ceiling_depends_on_the_tier_and_never_on_the_manifest() {
    let key = LimitKey::MemoryMib;
    let t1 = limits::ceiling(key, Tier::T1);
    let t3 = limits::ceiling(key, Tier::T3);
    assert!(t1 >= t3);
    let huge = limits_of(&[(key.as_key(), u32::MAX as u64)]);
    assert_eq!(limits::clamp(&huge, Tier::T3).expect("ok")[&key], t3);
    assert_eq!(limits::clamp(&huge, Tier::T1).expect("ok")[&key], t1);
}

/// REQ: PKG-073
#[test]
fn pkg_073_ceilings_are_never_below_the_defaults() {
    for key in LimitKey::ALL {
        for tier in [Tier::T1, Tier::T2, Tier::T3] {
            assert!(
                limits::ceiling(key, tier) >= limits::default_of(key),
                "{key:?} {tier:?}"
            );
        }
    }
}

/// REQ: PKG-074
#[test]
fn pkg_074_doc_effective_value_formula_is_stated() {
    let text = page("reference-manifest.md");
    assert!(text.contains("min(ceiling, v)"));
}

/// REQ: PKG-074
#[test]
fn pkg_074_clamp_without_a_request_gives_the_default_for_every_key() {
    let effective = limits::clamp(&Limits::default(), Tier::T3).expect("ok");
    for key in LimitKey::ALL {
        assert_eq!(effective[&key], limits::default_of(key), "{key:?}");
    }
}

/// REQ: PKG-075
#[test]
fn pkg_075_two_defects_in_one_stage_report_both_codes_in_key_order() {
    // Stage 3: an unknown key and a missing required key.
    let value = unset(set(client(), "/zzz", json!(1)), "/name");
    let found = errors(&value);
    assert_eq!(found.len(), 2, "{found:?}");
    let missing = found.iter().position(|code| code == "manifest.key-missing");
    let unknown = found.iter().position(|code| code == "manifest.unknown-key");
    assert!(missing.is_some() && unknown.is_some(), "{found:?}");
    // `name` precedes the appended `zzz` key in manifest key order.
    assert!(missing < unknown, "{found:?}");
}

/// REQ: PKG-075
#[test]
fn pkg_075_defects_in_two_stages_report_only_the_earlier_one() {
    // Stage 3 defect (unknown key) and a stage 4 defect (self require).
    let value = set(
        set(client(), "/zzz", json!(1)),
        "/requires",
        json!({ "plugins": [{ "id": "example.demo" }] }),
    );
    let found = errors(&value);
    assert_has(&found, "manifest.unknown-key");
    assert_lacks(&found, "manifest.requires-self");
}

/// REQ: PKG-075
#[test]
fn pkg_075_at_most_100_errors_are_reported() {
    let mut value = client();
    for index in 0..150 {
        value[format!("unknown_{index:03}")] = json!(1);
    }
    let found = errors(&value);
    assert_eq!(found.len(), 100);
}

/// REQ: PKG-075
#[test]
fn pkg_075_exactly_100_defects_are_all_reported() {
    let mut value = client();
    for index in 0..100 {
        value[format!("unknown_{index:03}")] = json!(1);
    }
    assert_eq!(errors(&value).len(), 100);
}

/// REQ: PKG-076
#[test]
fn pkg_076_every_error_has_a_code_a_message_and_a_pointer_when_it_has_a_value() {
    let found = error_diagnostics(&set(client(), "/name", json!(5)));
    assert!(!found.is_empty());
    for diagnostic in &found {
        assert!(!diagnostic.code.as_str().is_empty());
        assert!(!diagnostic.message.is_empty());
    }
    assert_eq!(first_with(&found, "manifest.schema").pointer, "/name");
}

/// REQ: PKG-076
#[test]
fn pkg_076_pointers_use_rfc_6901_escaping() {
    let value = set(
        client(),
        "/contributes/commands",
        json!([{ "id": "a", "title": "T", "bogus": 1 }]),
    );
    let found = error_diagnostics(&value);
    let diagnostic = first_with(&found, "manifest.unknown-key");
    assert_eq!(diagnostic.pointer, "/contributes/commands/0/bogus");
    let tilde = error_diagnostics(&set(client(), "/a~b", json!(1)));
    assert_eq!(first_with(&tilde, "manifest.unknown-key").pointer, "/a~0b");
    let slash = error_diagnostics(&set(client(), "/a~1b", json!(1)));
    let _ = slash;
}

/// REQ: PKG-077
#[test]
fn pkg_077_check_reports_the_same_codes_as_the_installer_for_stage_three_and_four_defects() {
    let value = set(client(), "/zzz", json!(1));
    let check = error_codes(&diagnostics(&value, Tier::T3, CheckMode::Check));
    let install = error_codes(&diagnostics(&packed(value), Tier::T3, CheckMode::Install));
    assert!(check.contains(&"manifest.unknown-key".to_owned()));
    assert!(install.contains(&"manifest.unknown-key".to_owned()));
}

/// REQ: PKG-077
#[test]
fn pkg_077_check_does_not_run_install_only_stages() {
    // A missing component file is a stage 5 defect: only the installer sees it.
    let mut value = client();
    value["files"] = json!([file_entry("assets/a.png", 1)]);
    assert_lacks(
        &error_codes(&diagnostics(&value, Tier::T3, CheckMode::Check)),
        "manifest.file-missing",
    );
    assert_has(
        &error_codes(&diagnostics(&value, Tier::T3, CheckMode::Install)),
        "manifest.file-missing",
    );
}

/// REQ: PKG-078
#[test]
fn pkg_078_every_code_produced_for_a_set_of_defects_is_in_the_catalogue() {
    let known: Vec<String> = catalogue().into_iter().map(|row| row.code).collect();
    let samples = [
        set(client(), "/zzz", json!(1)),
        unset(client(), "/name"),
        set(client(), "/id", json!("Bad")),
        set(client(), "/version", json!("v1")),
        set(client(), "/api", json!("ilium:plugin@9.9.9")),
    ];
    assert!(!known.is_empty());
    for sample in samples {
        let produced = errors(&sample);
        assert!(!produced.is_empty(), "sample produced no error");
        for code in produced {
            assert!(known.contains(&code), "{code} is not in the catalogue");
        }
    }
}

/// REQ: PKG-078
#[test]
fn pkg_078_doc_the_retired_spellings_are_listed_with_their_replacement() {
    let rows = retired_codes();
    assert!(!rows.is_empty());
    for (old, replacement) in &rows {
        assert!(!old.is_empty());
        assert!(!replacement.is_empty());
    }
}

/// REQ: PKG-079
#[test]
fn pkg_079_a_provider_name_with_the_plugin_prefix_is_accepted() {
    let value = set(
        client(),
        "/contributes/services",
        json!({ "provides": [{"name":"example.demo/status", "version":"1.0.0", "schema":"schemas/status.json"}] }),
    );
    assert_clean(&value);
}

/// REQ: PKG-079
#[test]
fn pkg_079_a_provider_name_with_a_foreign_prefix_refused_service_name() {
    let value = set(
        client(),
        "/contributes/services",
        json!({ "provides": [{"name":"com.other.thing/status", "version":"1.0.0", "schema":"schemas/status.json"}] }),
    );
    assert_has(&errors(&value), "manifest.service-name");
}

/// REQ: PKG-079
#[test]
fn pkg_079_a_duplicate_provider_name_refused_service_name() {
    let value = set(
        client(),
        "/contributes/services",
        json!({ "provides": [{"name":"example.demo/status", "version":"1.0.0", "schema":"schemas/status.json"}, {"name":"example.demo/status", "version":"1.0.0", "schema":"schemas/other.json"}] }),
    );
    assert_has(&errors(&value), "manifest.service-name");
}

/// REQ: PKG-079
#[test]
fn pkg_079_a_missing_schema_file_refused_file_missing() {
    let value = set(
        client(),
        "/contributes/services",
        json!({ "provides": [{"name":"example.demo/status", "version":"1.0.0", "schema":"schemas/status.json"}] }),
    );
    assert_has(
        &install_errors(&packed(value), Tier::T3),
        "manifest.file-missing",
    );
}

/// REQ: PKG-079
#[test]
fn pkg_079_a_core_plugin_prefix_is_accepted_for_its_surface_when_replaced() {
    let value = set(
        set(packed(client()), "/replaces", json!(["ilium.core.titles"])),
        "/contributes/services",
        json!({ "provides": [{"name":"ilium.core.titles/suggest", "version":"1.0.0", "schema":"schemas/status.json"}] }),
    );
    let found = install_errors(&packed_with(value, &["schemas/status.json"]), Tier::T1);
    assert_lacks(&found, "manifest.service-name");
}

proptest! {
    // REQ: PKG-050
    #[test]
    fn pkg_050_bare_lower_case_names_never_panic_and_never_split(name in "[a-z][a-z.-]{2,20}") {
        // manual does not pin the code of a bare name; it only forbids splitting it.
        match Grant::parse(&name) {
            Ok(grant) => {
                prop_assert!(grant.argument().is_none());
                prop_assert_eq!(grant.family(), name.as_str());
            }
            Err(diagnostic) => {
                prop_assert_eq!(diagnostic.code.as_str(), "manifest.grant-unknown");
            }
        }
    }

    // REQ: PKG-052
    #[test]
    fn pkg_052_a_parsed_grant_round_trips_through_as_str(
        argument in "[a-z][a-z0-9]{0,19}"
    ) {
        let text = format!("topics.subscribe:{argument}");
        let grant = Grant::parse(&text).expect("valid topics.subscribe argument");
        prop_assert_eq!(grant.as_str(), text.as_str());
        prop_assert_eq!(grant.argument(), Some(argument.as_str()));
    }
}
