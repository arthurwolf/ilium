//! Unit tests for the documented behaviour of the public `ilium-plugin-api` items that the
//! inventory found without tests. Each test follows the manual page named in its REQ comment or
//! in the item it covers; red tests against `unimplemented!()` stubs are intentional.

// ---------------------------------------------------------------- grants

fn grant_or_panic(text: &str) -> ilium_plugin_api::grants::Grant {
    match ilium_plugin_api::grants::Grant::parse(text) {
        Ok(parsed_grant) => parsed_grant,
        Err(diagnostic) => panic!("`{text}` should parse, but it was refused: {diagnostic}"),
    }
}

fn grant_refusal_code(text: &str) -> Option<String> {
    ilium_plugin_api::grants::Grant::parse(text)
        .err()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
}

fn grant_check_refusal(
    text: &str,
    tier: ilium_plugin_api::Tier,
    side: ilium_plugin_api::Side,
) -> Option<String> {
    let grant = grant_or_panic(text);
    ilium_plugin_api::grants::check_grant(&grant, tier, side)
        .err()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
}

#[test]
fn grant_parse_valid_string_parses_and_keeps_its_text() {
    let parsed_grant = grant_or_panic("tree.read");
    assert_eq!(
        parsed_grant.as_str(),
        "tree.read",
        "parsed grant must keep its manifest text"
    );
    assert_eq!(
        grant_or_panic("fs.read:project").as_str(),
        "fs.read:project"
    );
}

#[test]
fn grant_parse_unknown_family_is_refused_with_grant_unknown() {
    assert_eq!(
        grant_refusal_code("files.read"),
        Some(ilium_plugin_api::codes::manifest::GRANT_UNKNOWN.to_owned()),
        "an unknown family must be refused with manifest.grant-unknown"
    );
}

#[test]
fn grant_parse_missing_argument_is_refused_with_grant_argument_missing() {
    assert_eq!(
        grant_refusal_code("process.run"),
        Some(ilium_plugin_api::codes::manifest::GRANT_ARGUMENT_MISSING.to_owned()),
        "a grant that needs an argument must be refused when none is written"
    );
}

#[test]
fn grant_parse_empty_string_is_refused_with_grant_unknown() {
    assert_eq!(
        grant_refusal_code(""),
        Some(ilium_plugin_api::codes::manifest::GRANT_UNKNOWN.to_owned()),
        "an empty grant string names no family"
    );
}

#[test]
fn grant_parse_argument_containing_separators_keeps_them_after_first_colon() {
    let parsed_grant = grant_or_panic("fs.read:path:{home}/Pictures/**");
    assert_eq!(
        parsed_grant.family(),
        "fs.read",
        "family ends at the first colon"
    );
    assert_eq!(
        parsed_grant.argument(),
        Some("path:{home}/Pictures/**"),
        "colons, braces and slashes after the first colon belong to the argument"
    );
}

#[test]
fn grant_family_maps_each_family_row_to_its_family_name() {
    for row in ilium_plugin_api::grants::table() {
        if row.takes_argument {
            continue;
        }
        let parsed_grant = grant_or_panic(row.family);
        assert_eq!(
            parsed_grant.family(),
            row.family,
            "argument-less family `{}` must be its own family",
            row.family
        );
    }
    assert_eq!(grant_or_panic("fs.read:project").family(), "fs.read");
    assert_eq!(
        grant_or_panic("services.use:ilium:inference/provider").family(),
        "services.use"
    );
}

#[test]
fn grant_argument_returns_the_text_after_the_first_colon() {
    assert_eq!(
        grant_or_panic("fs.read:project").argument(),
        Some("project")
    );
    assert_eq!(
        grant_or_panic("services.use:ilium:inference/provider").argument(),
        Some("ilium:inference/provider")
    );
}

#[test]
fn grant_argument_is_none_for_argument_less_grant() {
    assert_eq!(grant_or_panic("tree.read").argument(), None);
    assert_eq!(grant_or_panic("clipboard.read").argument(), None);
}

#[test]
fn grant_argument_empty_argument_is_refused_at_parse_time() {
    // An empty argument never reaches the accessor: parse refuses it (empty program name).
    assert_eq!(
        grant_refusal_code("process.run:"),
        Some(ilium_plugin_api::codes::manifest::GRANT_PROGRAM.to_owned()),
        "an empty argument must be refused, so argument() never yields Some(\"\")"
    );
}

#[test]
fn grant_covers_exact_match_covers_itself() {
    let requested = grant_or_panic("http:api.example.com");
    assert!(
        grant_or_panic("http:api.example.com").covers(&requested),
        "an exact grant must cover the identical request"
    );
}

#[test]
fn grant_covers_wider_wildcard_covers_narrower_host() {
    let wide = grant_or_panic("http:*.openstreetmap.org");
    let narrow = grant_or_panic("http:tiles.openstreetmap.org");
    assert!(
        wide.covers(&narrow),
        "a wildcard grant must cover a narrower host it matches"
    );
    assert!(
        !narrow.covers(&wide),
        "a narrow grant must not cover a wider request"
    );
}

#[test]
fn grant_covers_disjoint_grants_do_not_cover_each_other() {
    let first = grant_or_panic("http:api.example.com");
    let second = grant_or_panic("fs.read:project");
    assert!(
        !first.covers(&second),
        "different families never cover each other"
    );
    assert!(
        !second.covers(&first),
        "different families never cover each other, both ways"
    );
}

#[test]
fn grant_covers_argument_less_grant_covers_only_its_own_family() {
    let tree = grant_or_panic("tree.read");
    assert!(
        tree.covers(&grant_or_panic("tree.read")),
        "an argument-less grant covers itself"
    );
    assert!(
        !tree.covers(&grant_or_panic("tree.write")),
        "tree.read must not cover tree.write"
    );
}

#[test]
fn grant_table_lists_each_family_once() {
    let families: Vec<&str> = ilium_plugin_api::grants::table()
        .iter()
        .map(|row| row.family)
        .collect();
    let unique: std::collections::HashSet<&str> = families.iter().copied().collect();
    assert_eq!(
        unique.len(),
        families.len(),
        "every grant family needs exactly one row"
    );
    assert!(!families.is_empty(), "the grant table must not be empty");
}

#[test]
fn grant_table_rows_have_family_and_risk_filled() {
    for row in ilium_plugin_api::grants::table() {
        assert!(
            !row.family.is_empty(),
            "every grant row needs a family name"
        );
        assert!(
            ilium_plugin_api::grants::Risk::ALL.contains(&row.risk),
            "row `{}` must carry a known risk class",
            row.family
        );
        assert!(
            !row.tiers.is_empty(),
            "row `{}` must name at least one tier",
            row.family
        );
    }
}

#[test]
fn grant_info_returns_the_row_for_each_table_family() {
    for row in ilium_plugin_api::grants::table() {
        let found = ilium_plugin_api::grants::info(row.family);
        assert!(
            found.is_some(),
            "info must return the row for family `{}`",
            row.family
        );
    }
}

#[test]
fn grant_info_agrees_with_table_row() {
    for row in ilium_plugin_api::grants::table() {
        let found = ilium_plugin_api::grants::info(row.family)
            .unwrap_or_else(|| panic!("info missing for `{}`", row.family));
        assert_eq!(found.family, row.family);
        assert_eq!(
            found.takes_argument, row.takes_argument,
            "argument flag of `{}`",
            row.family
        );
        assert_eq!(found.risk, row.risk, "risk of `{}`", row.family);
        assert_eq!(found.sides, row.sides, "sides of `{}`", row.family);
    }
}

#[test]
fn grant_check_grant_granted_capability_is_allowed() {
    assert_eq!(
        grant_check_refusal(
            "tree.read",
            ilium_plugin_api::Tier::T1,
            ilium_plugin_api::Side::Server
        ),
        None
    );
}

#[test]
fn grant_check_grant_ungranted_capability_is_refused_with_a_reason() {
    let grant = grant_or_panic("settings.read-all");
    let diagnostic = ilium_plugin_api::grants::check_grant(
        &grant,
        ilium_plugin_api::Tier::T2,
        ilium_plugin_api::Side::Client,
    )
    .expect_err("a bundled-only grant must be refused for a T2 package");
    assert_eq!(
        diagnostic.code.as_str(),
        ilium_plugin_api::codes::manifest::GRANT_TIER
    );
    assert!(
        !diagnostic.message.is_empty(),
        "the refusal must carry a human-readable reason"
    );
}

#[test]
fn grant_check_grant_high_risk_grant_is_accepted_at_check_time_for_bundled_tier() {
    // Consent is a runtime prompt, not a manifest check: check_grant only validates tier and side.
    let grant = grant_or_panic("process.run:git");
    assert!(ilium_plugin_api::grants::is_high_risk("process.run:git"));
    assert!(
        ilium_plugin_api::grants::check_grant(
            &grant,
            ilium_plugin_api::Tier::T1,
            ilium_plugin_api::Side::Server
        )
        .is_ok(),
        "a high-risk grant on a permitted tier and side passes the manifest check"
    );
}

#[test]
fn grant_check_grant_empty_grant_set_has_nothing_to_refuse() {
    assert_eq!(
        ilium_plugin_api::grants::risk_of(&[]),
        ilium_plugin_api::grants::Risk::Low
    );
}

#[test]
fn grant_risk_of_each_grant_maps_to_its_table_risk() {
    for row in ilium_plugin_api::grants::table() {
        if row.takes_argument {
            continue;
        }
        let parsed_grant = grant_or_panic(row.family);
        assert_eq!(
            ilium_plugin_api::grants::risk_of(&[parsed_grant]),
            row.risk,
            "risk_of must equal the table risk of `{}`",
            row.family
        );
    }
}

#[test]
fn grant_risk_of_unknown_argument_falls_back_to_the_family_risk() {
    let plain = grant_or_panic("commands.invoke:ilium.git/status");
    assert_eq!(
        ilium_plugin_api::grants::risk_of(&[plain]),
        ilium_plugin_api::grants::Risk::Low,
        "a plugin-scoped command grant is not high risk"
    );
    let bare = grant_or_panic("commands.invoke:*");
    assert_eq!(
        ilium_plugin_api::grants::risk_of(&[bare]),
        ilium_plugin_api::grants::Risk::High
    );
}

#[test]
fn grant_is_high_risk_classifies_low_and_high_grants() {
    assert!(ilium_plugin_api::grants::is_high_risk("process.run:git"));
    assert!(!ilium_plugin_api::grants::is_high_risk("tree.read"));
}

#[test]
fn grant_is_high_risk_matches_the_table_at_each_risk_level() {
    for risk in ilium_plugin_api::grants::Risk::ALL {
        let found = ilium_plugin_api::grants::table()
            .iter()
            .find(|row| row.risk == risk);
        if let Some(row) = found {
            if !row.takes_argument {
                assert_eq!(
                    ilium_plugin_api::grants::is_high_risk(row.family),
                    risk == ilium_plugin_api::grants::Risk::High,
                    "is_high_risk of `{}` must agree with its {} risk",
                    row.family,
                    risk
                );
            }
        }
    }
}

// ---------------------------------------------------------------- lifecycle

fn lifecycle_code(text: &'static str) -> ilium_plugin_api::error::Code {
    ilium_plugin_api::error::Code::from_static(text)
}

#[test]
fn lifecycle_instance_each_valid_transition_is_taken() {
    use ilium_plugin_api::lifecycle::InstanceState as State;
    use ilium_plugin_api::lifecycle::{next_instance_state, InstanceInput};
    let cases = [
        (State::Inactive, InstanceInput::Activate, State::Activating),
        (
            State::Inactive,
            InstanceInput::DependencyMissing,
            State::WaitingDependency,
        ),
        (
            State::WaitingDependency,
            InstanceInput::DependencyReady,
            State::Activating,
        ),
        (State::Activating, InstanceInput::ActivateOk, State::Active),
        (
            State::RestartScheduled,
            InstanceInput::RestartTimerFired,
            State::Activating,
        ),
        (
            State::StoppedByFailure,
            InstanceInput::Enable,
            State::Inactive,
        ),
    ];
    for (current, input, expected) in cases {
        assert_eq!(
            next_instance_state(current, &input),
            Ok(expected),
            "{current:?} + {input:?} must move to the documented next state"
        );
    }
}

#[test]
fn lifecycle_instance_invalid_transition_is_refused() {
    use ilium_plugin_api::lifecycle::InstanceState as State;
    use ilium_plugin_api::lifecycle::{next_instance_state, InstanceInput};
    let result = next_instance_state(State::Inactive, &InstanceInput::ActivateOk);
    assert!(
        result.is_err(),
        "activate-ok on an inactive instance is not a legal transition"
    );
}

#[test]
fn lifecycle_instance_terminal_stopped_has_no_successor_for_trap() {
    use ilium_plugin_api::lifecycle::InstanceState as State;
    use ilium_plugin_api::lifecycle::{next_instance_state, InstanceInput};
    assert!(
        next_instance_state(
            State::Stopped,
            &InstanceInput::Trap(lifecycle_code("trap.deadline"))
        )
        .is_err(),
        "a stopped instance has no successor for a trap"
    );
    assert!(
        next_instance_state(State::Stopped, &InstanceInput::RestartTimerFired).is_err(),
        "a stopped instance has no successor for a restart timer"
    );
}

#[test]
fn lifecycle_instance_repeated_same_state_event_is_refused_not_absorbed() {
    use ilium_plugin_api::lifecycle::InstanceState as State;
    use ilium_plugin_api::lifecycle::{next_instance_state, InstanceInput};
    assert!(
        next_instance_state(State::Active, &InstanceInput::ActivateOk).is_err(),
        "a repeated activate-ok on an active instance is not in the table"
    );
    assert!(
        next_instance_state(State::Activating, &InstanceInput::Activate).is_err(),
        "a repeated activate on an activating instance is not in the table"
    );
}

#[test]
fn lifecycle_package_each_valid_transition_is_taken() {
    use ilium_plugin_api::lifecycle::PackageState as State;
    use ilium_plugin_api::lifecycle::{next_package_state, PackageInput};
    let cases = [
        (
            State::NotInstalled,
            PackageInput::Install,
            State::InstalledEnabled,
        ),
        (
            State::InstalledEnabled,
            PackageInput::Disable,
            State::InstalledDisabledByUser,
        ),
        (
            State::InstalledDisabledByUser,
            PackageInput::Enable,
            State::InstalledEnabled,
        ),
        (
            State::InstalledEnabled,
            PackageInput::TrustRemoved,
            State::DisabledUntrusted,
        ),
        (
            State::InstalledEnabled,
            PackageInput::Revoked,
            State::DisabledBlocked,
        ),
        (
            State::InstalledEnabled,
            PackageInput::PolicyDeny,
            State::DisabledByPolicy,
        ),
        (
            State::InstalledEnabled,
            PackageInput::RequestUninstall,
            State::UninstallPending,
        ),
    ];
    for (current, input, expected) in cases {
        assert_eq!(
            next_package_state(current, &input),
            Ok(expected),
            "{current:?} + {input:?} must move to the documented next state"
        );
    }
}

#[test]
fn lifecycle_package_invalid_transition_is_refused() {
    use ilium_plugin_api::lifecycle::PackageState as State;
    use ilium_plugin_api::lifecycle::{next_package_state, PackageInput};
    assert!(
        next_package_state(State::NotInstalled, &PackageInput::Enable).is_err(),
        "enable of a plugin that is not installed is an illegal transition"
    );
    assert!(
        next_package_state(State::InstalledEnabled, &PackageInput::Install).is_err(),
        "installing an already installed plugin is an illegal transition"
    );
}

#[test]
fn lifecycle_package_uninstall_pending_has_no_enable_or_disable_successor() {
    use ilium_plugin_api::lifecycle::PackageState as State;
    use ilium_plugin_api::lifecycle::{next_package_state, PackageInput};
    for input in [PackageInput::Enable, PackageInput::Disable] {
        assert_eq!(
            next_package_state(State::UninstallPending, &input),
            Err(lifecycle_code("conflict")),
            "uninstall-pending is the end of the user path and refuses {input:?} with conflict"
        );
    }
}

#[test]
fn lifecycle_package_repeated_same_state_set_enabled_changes_nothing() {
    use ilium_plugin_api::lifecycle::PackageState as State;
    use ilium_plugin_api::lifecycle::{next_package_state, PackageInput};
    assert_eq!(
        next_package_state(State::InstalledEnabled, &PackageInput::Enable),
        Ok(State::InstalledEnabled),
        "set-enabled(true) on an enabled plugin keeps it enabled"
    );
    assert_eq!(
        next_package_state(State::InstalledDisabledByUser, &PackageInput::Disable),
        Ok(State::InstalledDisabledByUser),
        "set-enabled(false) on a disabled plugin keeps it disabled"
    );
}

#[test]
fn lifecycle_restart_failure_under_limit_restarts_after_delay() {
    use ilium_plugin_api::lifecycle::{RestartDecision, RestartPolicy};
    let mut policy = RestartPolicy::default();
    assert_eq!(policy.on_failure(0), RestartDecision::RestartAfterMs(1_000));
    assert_eq!(
        policy.on_failure(60_000),
        RestartDecision::RestartAfterMs(5_000),
        "a second failure inside the window still restarts"
    );
}

#[test]
fn lifecycle_restart_failure_at_limit_disables() {
    use ilium_plugin_api::lifecycle::{RestartDecision, RestartPolicy};
    let mut policy = RestartPolicy::default();
    policy.on_failure(0);
    policy.on_failure(60_000);
    assert_eq!(
        policy.on_failure(120_000),
        RestartDecision::Disable,
        "the third failure inside ten minutes reaches the limit and stops the plugin"
    );
}

#[test]
fn lifecycle_restart_window_counts_failures_inside_and_expires_outside() {
    use ilium_plugin_api::lifecycle::RestartPolicy;
    let mut policy = RestartPolicy::default();
    policy.on_failure(0);
    policy.on_failure(10_000);
    assert_eq!(
        policy.failures_in_window(10_000),
        2,
        "both failures are inside the window"
    );
    assert_eq!(
        policy.failures_in_window(600_000),
        0,
        "the window has expired at ten minutes"
    );
}

#[test]
fn lifecycle_restart_empty_history_gives_zero() {
    use ilium_plugin_api::lifecycle::RestartPolicy;
    assert_eq!(RestartPolicy::default().failures_in_window(0), 0);
}

#[test]
fn lifecycle_restart_window_boundary_is_exclusive_at_ten_minutes() {
    use ilium_plugin_api::lifecycle::RestartPolicy;
    let mut policy = RestartPolicy::default();
    policy.on_failure(0);
    assert_eq!(
        policy.failures_in_window(599_999),
        1,
        "age just under ten minutes counts"
    );
    assert_eq!(
        policy.failures_in_window(600_000),
        0,
        "age exactly ten minutes does not count"
    );
}

// ---------------------------------------------------------------- compat

fn compat_version(major: u16, minor: u16, patch: u16) -> ilium_plugin_api::version::ApiVersion {
    ilium_plugin_api::version::ApiVersion {
        major,
        minor,
        patch,
    }
}

#[test]
fn compat_case_since_at_boundary_reports_0_1_0_for_stable_cases() {
    use ilium_plugin_api::compat::{case_since, CaseFamily};
    let host = compat_version(0, 1, 0);
    assert_eq!(
        case_since(CaseFamily::Event, "tree-changed"),
        Some(host),
        "stable event at 0.1.0"
    );
    assert_eq!(
        case_since(CaseFamily::Outcome, "ok-epoch"),
        Some(host),
        "ok-epoch at 0.1.0"
    );
    assert_eq!(
        case_since(CaseFamily::DeactivationReason, "crash-restart"),
        Some(host),
        "reserved crash-restart exists from 0.1.0"
    );
}

#[test]
fn compat_case_since_before_boundary_is_older_than_predating_plugin() {
    use ilium_plugin_api::compat::{case_since, CaseFamily};
    let since = case_since(CaseFamily::Event, "tree-changed").expect("tree-changed is known");
    let predating = compat_version(0, 0, 9);
    assert!(
        predating < since,
        "a plugin built before 0.1.0 is older than the case, so ABI-029 applies"
    );
}

#[test]
fn compat_case_since_unknown_version_name_is_none() {
    use ilium_plugin_api::compat::{case_since, CaseFamily};
    assert_eq!(case_since(CaseFamily::Event, "no-such-event"), None);
    assert_eq!(case_since(CaseFamily::Intent, ""), None);
    assert_eq!(
        case_since(CaseFamily::Outcome, "tree-changed"),
        None,
        "a name from another family is unknown in this family"
    );
}

#[test]
fn compat_map_outcome_each_outcome_is_mapped_for_current_api() {
    use ilium_plugin_api::compat::map_outcome_for;
    use ilium_plugin_api::generated::types::Outcome;
    let host = compat_version(0, 1, 0);
    assert_eq!(map_outcome_for(&Outcome::Ok, host), Outcome::Ok);
    assert_eq!(
        map_outcome_for(&Outcome::OkEpoch(7), host),
        Outcome::OkEpoch(7)
    );
    assert_eq!(
        map_outcome_for(&Outcome::Cancelled, host),
        Outcome::Cancelled
    );
}

#[test]
fn compat_map_outcome_newer_ok_epoch_maps_to_ok_for_older_plugin() {
    use ilium_plugin_api::compat::map_outcome_for;
    use ilium_plugin_api::generated::types::Outcome;
    assert_eq!(
        map_outcome_for(&Outcome::OkEpoch(5), compat_version(0, 0, 9)),
        Outcome::Ok,
        "ok-epoch reaches an earlier API as ok"
    );
}

#[test]
fn compat_map_outcome_unmapped_outcome_passes_through_for_older_plugin() {
    use ilium_plugin_api::compat::map_outcome_for;
    use ilium_plugin_api::generated::types::Outcome;
    let older = compat_version(0, 0, 9);
    assert_eq!(
        map_outcome_for(&Outcome::Cancelled, older),
        Outcome::Cancelled
    );
    assert_eq!(map_outcome_for(&Outcome::Ok, older), Outcome::Ok);
}

#[test]
fn compat_map_reason_crash_restart_is_never_delivered() {
    use ilium_plugin_api::compat::map_reason_for;
    use ilium_plugin_api::generated::types::DeactivationReason;
    assert_eq!(
        map_reason_for(DeactivationReason::CrashRestart, compat_version(0, 0, 9)),
        None,
        "crash-restart is never delivered to an older plugin"
    );
    assert_eq!(
        map_reason_for(DeactivationReason::CrashRestart, compat_version(0, 1, 0)),
        None,
        "crash-restart is never delivered to the current API either"
    );
}

#[test]
fn compat_map_reason_each_reason_is_mapped_for_current_api() {
    use ilium_plugin_api::compat::map_reason_for;
    use ilium_plugin_api::generated::types::DeactivationReason;
    let host = compat_version(0, 1, 0);
    assert_eq!(
        map_reason_for(DeactivationReason::Shutdown, host),
        Some(DeactivationReason::Shutdown)
    );
    assert_eq!(
        map_reason_for(DeactivationReason::Revoked, host),
        Some(DeactivationReason::Revoked)
    );
    assert_eq!(
        map_reason_for(DeactivationReason::Disabled, host),
        Some(DeactivationReason::Disabled)
    );
    assert_eq!(
        map_reason_for(DeactivationReason::Upgrade, host),
        Some(DeactivationReason::Upgrade)
    );
}

#[test]
fn compat_map_reason_newer_reasons_become_shutdown_for_older_plugin() {
    use ilium_plugin_api::compat::map_reason_for;
    use ilium_plugin_api::generated::types::DeactivationReason;
    let older = compat_version(0, 0, 9);
    assert_eq!(
        map_reason_for(DeactivationReason::Revoked, older),
        Some(DeactivationReason::Shutdown)
    );
    assert_eq!(
        map_reason_for(DeactivationReason::Granted, older),
        Some(DeactivationReason::Shutdown)
    );
}

#[test]
fn compat_event_deliverable_known_event_reaches_current_api() {
    use ilium_plugin_api::compat::event_deliverable;
    let host = compat_version(0, 1, 0);
    assert!(
        event_deliverable("tree-changed", host),
        "a known event reaches the current API"
    );
    assert!(
        event_deliverable("screen-changed", host),
        "an appended event reaches the current API"
    );
}

#[test]
fn compat_event_deliverable_newer_event_is_refused_for_older_plugin() {
    use ilium_plugin_api::compat::event_deliverable;
    assert!(
        !event_deliverable("tree-changed", compat_version(0, 0, 9)),
        "an event added in 0.1.0 is not delivered to a 0.0.9 plugin"
    );
}

#[test]
fn compat_event_deliverable_unknown_and_empty_events_are_refused() {
    use ilium_plugin_api::compat::event_deliverable;
    let host = compat_version(0, 1, 0);
    assert!(!event_deliverable("no-such-event", host));
    assert!(!event_deliverable("", host));
}

#[test]
fn compat_event_deliverable_version_boundary_is_at_0_1_0() {
    use ilium_plugin_api::compat::event_deliverable;
    assert!(
        event_deliverable("tree-changed", compat_version(0, 1, 0)),
        "at 0.1.0 it is delivered"
    );
    assert!(
        !event_deliverable("tree-changed", compat_version(0, 0, 9)),
        "below 0.1.0 it is not"
    );
}

// ---------------------------------------------------------------- dedup

#[test]
fn dedup_parse_valid_key_parses_and_keeps_its_text() {
    let key = ilium_plugin_api::dedup::DedupKey::parse("send-input_01.v2:pane/7")
        .expect("letters, digits and ._:/- are all valid");
    assert_eq!(key.as_str(), "send-input_01.v2:pane/7");
}

#[test]
fn dedup_parse_single_character_key_is_valid() {
    let key = ilium_plugin_api::dedup::DedupKey::parse("a").expect("a one-character key is valid");
    assert_eq!(key.as_str(), "a");
}

#[test]
fn dedup_parse_empty_string_is_invalid_argument() {
    let error = ilium_plugin_api::dedup::DedupKey::parse("")
        .expect_err("an empty dedup key must be refused");
    assert_eq!(
        error.code,
        ilium_plugin_api::generated::types::ErrorCode::InvalidArgument
    );
}

#[test]
fn dedup_parse_missing_separator_is_still_valid_when_characters_are_legal() {
    // The rule is the character set and length only; a key without any separator is valid.
    let key = ilium_plugin_api::dedup::DedupKey::parse("finish")
        .expect("a key with no separator is valid when every character is allowed");
    assert_eq!(key.as_str(), "finish");
}

#[test]
fn dedup_parse_repeated_and_leading_separators_are_valid() {
    let key = ilium_plugin_api::dedup::DedupKey::parse("feed::day//-.x")
        .expect("extra separators are within the allowed character set");
    assert_eq!(key.as_str(), "feed::day//-.x");
}

#[test]
fn dedup_parse_key_of_exactly_128_characters_is_accepted() {
    let text = "k".repeat(ilium_plugin_api::dedup::DEDUP_KEY_MAX_CHARS);
    let key = ilium_plugin_api::dedup::DedupKey::parse(&text).expect("128 characters is the limit");
    assert_eq!(key.as_str().len(), 128);
}

#[test]
fn dedup_parse_key_of_129_characters_is_invalid_argument() {
    let text = "k".repeat(ilium_plugin_api::dedup::DEDUP_KEY_MAX_CHARS + 1);
    let error = ilium_plugin_api::dedup::DedupKey::parse(&text)
        .expect_err("129 characters exceeds the limit");
    assert_eq!(
        error.code,
        ilium_plugin_api::generated::types::ErrorCode::InvalidArgument
    );
}

#[test]
fn dedup_parse_disallowed_characters_are_invalid_argument() {
    for text in ["a b", "a+b", "a@b", "a,b", "a\nb", "a\u{e9}b"] {
        let error = ilium_plugin_api::dedup::DedupKey::parse(text)
            .expect_err("a character outside the allowed set must be refused");
        assert_eq!(
            error.code,
            ilium_plugin_api::generated::types::ErrorCode::InvalidArgument,
            "input {text:?}"
        );
    }
}

#[test]
fn dedup_is_reserved_immediately_and_inside_the_window() {
    assert!(ilium_plugin_api::dedup::is_reserved(1_000, 1_000));
    assert!(ilium_plugin_api::dedup::is_reserved(1_000, 1_000 + 599_999));
}

#[test]
fn dedup_is_reserved_is_free_at_exactly_the_window_end() {
    assert!(!ilium_plugin_api::dedup::is_reserved(
        1_000,
        1_000 + 600_000
    ));
    assert!(!ilium_plugin_api::dedup::is_reserved(0, 600_001));
}

#[test]
fn dedup_is_reserved_backwards_clock_keeps_key_reserved() {
    assert!(ilium_plugin_api::dedup::is_reserved(700_000, 699_999));
    assert!(ilium_plugin_api::dedup::is_reserved(1_000_000, 0));
}

#[test]
fn dedup_is_reserved_extreme_timestamps_do_not_overflow() {
    assert!(ilium_plugin_api::dedup::is_reserved(u64::MAX, u64::MAX));
    assert!(ilium_plugin_api::dedup::is_reserved(u64::MAX - 1, u64::MAX));
    assert!(!ilium_plugin_api::dedup::is_reserved(0, u64::MAX));
}

// ---------------------------------------------------------------- ids

fn plugin_id_refusal_code(text: &str) -> Option<String> {
    ilium_plugin_api::ids::parse_plugin_id(text)
        .err()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
}

#[test]
fn ids_valid_plugin_id_is_accepted_and_returned_unchanged() {
    let plugin_id = ilium_plugin_api::ids::parse_plugin_id("com.acme.git-tools")
        .expect("a documented valid plugin id is accepted");
    assert_eq!(plugin_id.0, "com.acme.git-tools");
}

#[test]
fn ids_uppercase_letters_are_refused_as_manifest_id_invalid() {
    assert_eq!(
        plugin_id_refusal_code("Example.Demo"),
        Some("manifest.id-invalid".to_owned()),
        "uppercase letters are outside the plugin id alphabet"
    );
}

#[test]
fn ids_too_long_id_is_refused_as_manifest_id_invalid() {
    let text = format!(
        "a.{}",
        "b".repeat(ilium_plugin_api::ids::PLUGIN_ID_MAX_CHARS)
    );
    assert!(text.chars().count() > ilium_plugin_api::ids::PLUGIN_ID_MAX_CHARS);
    assert_eq!(
        plugin_id_refusal_code(&text),
        Some("manifest.id-invalid".to_owned())
    );
}

#[test]
fn ids_empty_text_is_refused_as_manifest_id_invalid() {
    assert_eq!(
        plugin_id_refusal_code(""),
        Some("manifest.id-invalid".to_owned())
    );
}

#[test]
fn ids_reserved_prefix_is_refused_as_manifest_id_reserved() {
    assert_eq!(
        plugin_id_refusal_code("ilium.demo"),
        Some("manifest.id-reserved".to_owned()),
        "the ilium prefix is reserved for the host"
    );
}

#[test]
fn ids_invalid_character_is_refused_as_manifest_id_invalid() {
    assert_eq!(
        plugin_id_refusal_code("example.dem_o"),
        Some("manifest.id-invalid".to_owned())
    );
    assert_eq!(
        plugin_id_refusal_code("example.de mo"),
        Some("manifest.id-invalid".to_owned())
    );
}

#[test]
fn ids_segment_count_boundaries_are_two_and_eight() {
    assert!(
        ilium_plugin_api::ids::parse_plugin_id("a.b").is_ok(),
        "two segments is the minimum"
    );
    assert!(
        ilium_plugin_api::ids::parse_plugin_id("a.b.c.d.e.f.g.h").is_ok(),
        "eight segments is the maximum"
    );
    assert_eq!(
        plugin_id_refusal_code("a.b.c.d.e.f.g.h.i"),
        Some("manifest.id-invalid".to_owned()),
        "nine segments is refused"
    );
    assert_eq!(
        plugin_id_refusal_code("demo"),
        Some("manifest.id-invalid".to_owned())
    );
}

#[test]
fn ids_segment_length_boundary_is_thirty_two_characters() {
    let accepted = "a".repeat(32);
    assert!(
        ilium_plugin_api::ids::parse_plugin_id(&format!("example.{accepted}")).is_ok(),
        "a 32-character segment is accepted"
    );
    let refused = "a".repeat(33);
    assert_eq!(
        plugin_id_refusal_code(&format!("example.{refused}")),
        Some("manifest.id-invalid".to_owned()),
        "a 33-character segment is refused"
    );
}

#[test]
fn ids_whole_id_length_boundary_is_128_characters() {
    let max_text = format!("ab.{}", "c".repeat(125));
    assert_eq!(
        max_text.chars().count(),
        ilium_plugin_api::ids::PLUGIN_ID_MAX_CHARS
    );
    assert!(ilium_plugin_api::ids::parse_plugin_id(&max_text).is_ok());
}

#[test]
fn ids_documented_limit_constants_match_the_manual() {
    assert_eq!(ilium_plugin_api::ids::PLUGIN_ID_MAX_CHARS, 128);
    assert_eq!(ilium_plugin_api::ids::PLUGIN_ID_SEGMENTS, 2..=8);
}

// ---------------------------------------------------------------- naming

fn naming_request_function(name: &'static str) -> ilium_plugin_api::generated::WitFunction {
    ilium_plugin_api::generated::WitFunction {
        package: "ilium:host",
        interface: "http",
        name,
        result: "result<request-id, plugin-error>",
        unstable: None,
    }
}

#[test]
fn naming_rust_function_plain_identifier_is_snake_case() {
    assert_eq!(
        ilium_plugin_api::naming::rust_function_name("fetch"),
        "fetch"
    );
    assert_eq!(
        ilium_plugin_api::naming::rust_function_name("read-cells"),
        "read_cells"
    );
}

#[test]
fn naming_rust_function_acronym_keeps_json_as_lowercase_word() {
    assert_eq!(
        ilium_plugin_api::naming::rust_function_name("get-json"),
        "get_json"
    );
}

#[test]
fn naming_rust_function_digits_stay_inside_the_word() {
    assert_eq!(
        ilium_plugin_api::naming::rust_function_name("read-v2"),
        "read_v2"
    );
}

#[test]
fn naming_rust_function_empty_identifier_gives_empty_name() {
    assert_eq!(ilium_plugin_api::naming::rust_function_name(""), "");
}

#[test]
fn naming_rust_function_reserved_word_is_escaped_as_raw_identifier() {
    assert_eq!(
        ilium_plugin_api::naming::rust_function_name("%type"),
        "r#type"
    );
    assert_eq!(
        ilium_plugin_api::naming::rust_function_name("%loop"),
        "r#loop"
    );
}

#[test]
fn naming_rust_type_plain_identifier_is_pascal_case() {
    assert_eq!(ilium_plugin_api::naming::rust_type_name("tone"), "Tone");
    assert_eq!(
        ilium_plugin_api::naming::rust_type_name("catalog-item"),
        "CatalogItem"
    );
}

#[test]
fn naming_rust_type_acronym_is_title_cased() {
    assert_eq!(
        ilium_plugin_api::naming::rust_type_name("http-request"),
        "HttpRequest"
    );
    assert_eq!(
        ilium_plugin_api::naming::rust_type_name("json-value"),
        "JsonValue"
    );
}

#[test]
fn naming_rust_type_empty_identifier_gives_empty_name() {
    assert_eq!(ilium_plugin_api::naming::rust_type_name(""), "");
}

#[test]
fn naming_typescript_function_plain_identifier_is_camel_case() {
    assert_eq!(
        ilium_plugin_api::naming::typescript_function_name("fetch"),
        "fetch"
    );
    assert_eq!(
        ilium_plugin_api::naming::typescript_function_name("read-cells"),
        "readCells"
    );
}

#[test]
fn naming_typescript_function_acronym_json_suffix_becomes_json() {
    assert_eq!(
        ilium_plugin_api::naming::typescript_function_name("get-json"),
        "getJson"
    );
    assert_eq!(
        ilium_plugin_api::naming::typescript_function_name("set-plugin-json"),
        "setPluginJson"
    );
}

#[test]
fn naming_typescript_function_empty_identifier_gives_empty_name() {
    assert_eq!(ilium_plugin_api::naming::typescript_function_name(""), "");
}

#[test]
fn naming_typescript_type_plain_and_acronym_identifiers_are_pascal_case() {
    assert_eq!(
        ilium_plugin_api::naming::typescript_type_name("tone"),
        "Tone"
    );
    assert_eq!(
        ilium_plugin_api::naming::typescript_type_name("catalog-item"),
        "CatalogItem"
    );
    assert_eq!(
        ilium_plugin_api::naming::typescript_type_name("json-value"),
        "JsonValue"
    );
}

#[test]
fn naming_typescript_type_empty_identifier_gives_empty_name() {
    assert_eq!(ilium_plugin_api::naming::typescript_type_name(""), "");
}

#[test]
fn naming_wrapper_names_request_function_gets_plain_and_request_wrapper_per_language() {
    use ilium_plugin_api::naming::{wrapper_names, Language};
    let function = naming_request_function("fetch");
    assert_eq!(
        wrapper_names(&function, Language::Rust),
        vec!["fetch".to_string(), "fetch_request".to_string()]
    );
    assert_eq!(
        wrapper_names(&function, Language::TypeScript),
        vec!["fetch".to_string(), "fetchRequest".to_string()]
    );
}

#[test]
fn naming_wrapper_names_non_request_function_has_only_plain_wrapper() {
    use ilium_plugin_api::naming::{wrapper_names, Language};
    let function = ilium_plugin_api::generated::WitFunction {
        package: "ilium:host",
        interface: "tree",
        name: "read-cells",
        result: "result<list<cell>, plugin-error>",
        unstable: None,
    };
    assert_eq!(
        wrapper_names(&function, Language::Rust),
        vec!["read_cells".to_string()]
    );
    assert_eq!(
        wrapper_names(&function, Language::TypeScript),
        vec!["readCells".to_string()]
    );
}

#[test]
fn naming_wrapper_names_function_without_result_has_only_plain_wrapper() {
    use ilium_plugin_api::naming::{wrapper_names, Language};
    let function = ilium_plugin_api::generated::WitFunction {
        package: "ilium:host",
        interface: "tree",
        name: "get-json",
        result: "",
        unstable: None,
    };
    assert_eq!(
        wrapper_names(&function, Language::Rust),
        vec!["get_json".to_string()]
    );
    assert_eq!(
        wrapper_names(&function, Language::TypeScript),
        vec!["getJson".to_string()]
    );
}

#[test]
fn naming_wrapper_names_json_request_function_keeps_json_before_request_suffix() {
    use ilium_plugin_api::naming::{wrapper_names, Language};
    let function = naming_request_function("fetch-json");
    assert_eq!(
        wrapper_names(&function, Language::Rust),
        vec!["fetch_json".to_string(), "fetch_json_request".to_string()]
    );
    assert_eq!(
        wrapper_names(&function, Language::TypeScript),
        vec!["fetchJson".to_string(), "fetchJsonRequest".to_string()]
    );
}

#[test]
fn naming_wrapper_names_list_of_request_ids_is_not_a_request_function() {
    use ilium_plugin_api::naming::{wrapper_names, Language};
    let function = ilium_plugin_api::generated::WitFunction {
        package: "ilium:host",
        interface: "http",
        name: "collect",
        result: "result<list<request-id>, plugin-error>",
        unstable: None,
    };
    assert_eq!(
        wrapper_names(&function, Language::Rust),
        vec!["collect".to_string()]
    );
}

// ---------------------------------------------------------------- deprecation

fn marked_feature(
    name: &str,
    marked_in_minor: u32,
    removal_minor: u32,
) -> ilium_plugin_api::deprecation::MarkedFeature {
    ilium_plugin_api::deprecation::MarkedFeature {
        name: name.to_owned(),
        marked_in_minor,
        removal_minor,
    }
}

#[test]
fn deprecation_notice_exactly_two_minors_is_long_enough() {
    let feature = marked_feature("host.legacy-clipboard", 4, 6);
    assert!(
        ilium_plugin_api::deprecation::notice_is_long_enough(&feature),
        "a removal two minors after the marking release meets the ABI-052 threshold"
    );
}

#[test]
fn deprecation_notice_shorter_than_two_minors_is_refused() {
    let feature = marked_feature("host.legacy-clipboard", 4, 5);
    assert!(
        !ilium_plugin_api::deprecation::notice_is_long_enough(&feature),
        "a removal one minor after the marking release is too short"
    );
}

#[test]
fn deprecation_feature_is_available_before_its_removal_minor() {
    let feature = marked_feature("host.legacy-clipboard", 4, 6);
    assert!(
        ilium_plugin_api::deprecation::feature_available(&feature, 4),
        "marking minor still works"
    );
    assert!(
        ilium_plugin_api::deprecation::feature_available(&feature, 5),
        "notice period still works"
    );
}

#[test]
fn deprecation_feature_boundary_minor_is_already_unavailable() {
    let feature = marked_feature("host.legacy-clipboard", 4, 6);
    assert!(
        !ilium_plugin_api::deprecation::feature_available(&feature, 6),
        "at the removal minor the feature no longer works"
    );
}

#[test]
fn deprecation_feature_is_unavailable_after_its_removal_minor() {
    let feature = marked_feature("host.legacy-clipboard", 4, 6);
    assert!(!ilium_plugin_api::deprecation::feature_available(
        &feature, 7
    ));
}

#[test]
fn deprecation_records_one_per_distinct_marked_feature_used() {
    let marked = [marked_feature("host.legacy-clipboard", 4, 6)];
    let records = ilium_plugin_api::deprecation::deprecation_records(
        "com.acme.clip",
        &["host.legacy-clipboard", "host.legacy-clipboard"],
        &marked,
        5,
    );
    assert_eq!(
        records.len(),
        1,
        "ABI-053 writes one record per plugin and feature"
    );
    assert_eq!(records[0].plugin_id, "com.acme.clip");
    assert_eq!(records[0].feature, "host.legacy-clipboard");
    assert_eq!(
        records[0].removal_minor, 6,
        "the record names the removal minor"
    );
}

#[test]
fn deprecation_records_unmarked_feature_gives_no_record() {
    let marked = [marked_feature("host.legacy-clipboard", 4, 6)];
    let records = ilium_plugin_api::deprecation::deprecation_records(
        "com.acme.clip",
        &["tree.read"],
        &marked,
        5,
    );
    assert!(
        records.is_empty(),
        "a feature that is not marked produces no record"
    );
}

#[test]
fn deprecation_records_no_used_features_gives_no_record() {
    let marked = [marked_feature("host.legacy-clipboard", 4, 6)];
    let records =
        ilium_plugin_api::deprecation::deprecation_records("com.acme.clip", &[], &marked, 5);
    assert!(records.is_empty());
}

#[test]
fn deprecation_refusal_is_none_while_feature_still_works() {
    let feature = marked_feature("host.legacy-clipboard", 4, 6);
    assert_eq!(
        ilium_plugin_api::deprecation::refusal_for_feature(&feature, 5),
        None
    );
}

#[test]
fn deprecation_refusal_at_removal_minor_is_a_section_four_code() {
    let feature = marked_feature("host.legacy-clipboard", 4, 6);
    let code = ilium_plugin_api::deprecation::refusal_for_feature(&feature, 6)
        .expect("a removed feature must yield a refusal code");
    let section_four_codes = [
        "package.api-unsupported",
        "manifest.api-version",
        "manifest.import-not-requested",
        "manifest.grant-unknown",
        "manifest.schema",
    ];
    assert!(
        section_four_codes.contains(&code),
        "removal refusal `{code}` must be one of the section 4 refusal codes"
    );
}

// ---------------------------------------------------------------- abi

#[test]
fn abi_check_submission_count_empty_batch_is_accepted() {
    assert_eq!(ilium_plugin_api::abi::check_submission_count(0), Ok(()));
}

#[test]
fn abi_check_submission_count_exactly_the_limit_is_accepted() {
    let limit = ilium_plugin_api::abi::SUBMISSION_BATCH_LIMIT;
    assert_eq!(ilium_plugin_api::abi::check_submission_count(limit), Ok(()));
}

#[test]
fn abi_check_submission_count_over_the_limit_traps() {
    let limit = ilium_plugin_api::abi::SUBMISSION_BATCH_LIMIT;
    let trap = ilium_plugin_api::abi::check_submission_count(limit + 1)
        .expect_err("one submission over the limit must trap the call");
    assert_eq!(trap.as_str(), "trap.submission-batch");
}

#[test]
fn abi_batch_verdicts_empty_batch_has_no_verdicts() {
    assert!(ilium_plugin_api::abi::batch_verdicts(&[]).is_empty());
}

#[test]
fn abi_batch_verdicts_all_accepted_submissions_proceed() {
    use ilium_plugin_api::abi::{batch_verdicts, BatchCheck, BatchVerdict};
    let valid = BatchCheck {
        shape_ok: true,
        grant_ok: true,
    };
    assert_eq!(
        batch_verdicts(&[valid, valid]),
        vec![BatchVerdict::Proceed, BatchVerdict::Proceed]
    );
}

#[test]
fn abi_batch_verdicts_mixed_verdicts_keep_submission_order() {
    use ilium_plugin_api::abi::{batch_verdicts, BatchCheck, BatchVerdict};
    use ilium_plugin_api::generated::types::ErrorCode;
    let checks = [
        BatchCheck {
            shape_ok: true,
            grant_ok: true,
        },
        BatchCheck {
            shape_ok: false,
            grant_ok: true,
        },
        BatchCheck {
            shape_ok: true,
            grant_ok: true,
        },
    ];
    assert_eq!(
        batch_verdicts(&checks),
        vec![
            BatchVerdict::Cancelled("batch refused: submission 1".to_string()),
            BatchVerdict::Refused(ErrorCode::InvalidArgument),
            BatchVerdict::Cancelled("batch refused: submission 1".to_string()),
        ],
        "the offender is refused, the rest are cancelled, and the order is kept"
    );
}

#[test]
fn abi_is_sdk_correlation_present_sdk_number_belongs_to_sdk() {
    assert!(ilium_plugin_api::abi::is_sdk_correlation(
        ilium_plugin_api::abi::SDK_CORRELATION_FLOOR
    ));
    assert!(ilium_plugin_api::abi::is_sdk_correlation(u64::MAX));
}

#[test]
fn abi_is_sdk_correlation_absent_number_belongs_to_plugin() {
    assert!(!ilium_plugin_api::abi::is_sdk_correlation(0));
    assert!(!ilium_plugin_api::abi::is_sdk_correlation(
        ilium_plugin_api::abi::SDK_CORRELATION_FLOOR - 1
    ));
}

#[test]
fn abi_deadline_ticks_zero_deadline_is_one_tick() {
    assert_eq!(ilium_plugin_api::abi::deadline_ticks(0), 1);
}

#[test]
fn abi_deadline_ticks_boundary_tick_rounds_up() {
    assert_eq!(
        ilium_plugin_api::abi::deadline_ticks(10),
        2,
        "exactly one tick plus the margin"
    );
    assert_eq!(
        ilium_plugin_api::abi::deadline_ticks(11),
        3,
        "a partial tick rounds up"
    );
}

#[test]
fn abi_deadline_ticks_overflow_is_handled_without_wrapping() {
    assert_eq!(
        ilium_plugin_api::abi::deadline_ticks(u64::MAX),
        1_844_674_407_370_955_163
    );
}

#[test]
fn abi_batch_sizes_minimum_size_is_one_batch() {
    assert_eq!(ilium_plugin_api::abi::batch_sizes(1), vec![1]);
}

#[test]
fn abi_batch_sizes_maximum_size_is_one_full_batch() {
    assert_eq!(ilium_plugin_api::abi::batch_sizes(256), vec![256]);
}

#[test]
fn abi_batch_sizes_above_maximum_splits_into_batches_of_at_most_256() {
    assert_eq!(ilium_plugin_api::abi::batch_sizes(257), vec![256, 1]);
    assert_eq!(ilium_plugin_api::abi::batch_sizes(0), Vec::<usize>::new());
}

#[test]
fn abi_restore_bytes_empty_and_exactly_max_are_accepted() {
    assert!(ilium_plugin_api::abi::restore_bytes_accepted(0));
    assert!(ilium_plugin_api::abi::restore_bytes_accepted(
        ilium_plugin_api::abi::RESTORE_BYTES_MAX
    ));
}

#[test]
fn abi_restore_bytes_over_max_is_refused() {
    assert!(!ilium_plugin_api::abi::restore_bytes_accepted(
        ilium_plugin_api::abi::RESTORE_BYTES_MAX + 1
    ));
}

#[test]
fn abi_is_pure_export_true_for_pure_calls() {
    assert!(ilium_plugin_api::abi::is_pure_export("views.render"));
    assert!(ilium_plugin_api::abi::is_pure_export("annotate"));
}

#[test]
fn abi_is_pure_export_false_for_effectful_export() {
    assert!(!ilium_plugin_api::abi::is_pure_export("intents.submit"));
    assert!(!ilium_plugin_api::abi::is_pure_export("on-events"));
    assert!(!ilium_plugin_api::abi::is_pure_export(""));
}

#[test]
fn abi_effectful_import_error_is_not_permitted_with_documented_message() {
    let error = ilium_plugin_api::abi::effectful_import_error();
    assert_eq!(
        error.code,
        ilium_plugin_api::generated::types::ErrorCode::NotPermitted
    );
    assert_eq!(error.message, "effectful import in a pure call");
}

#[test]
fn abi_delivery_policy_each_class_has_its_documented_policy() {
    use ilium_plugin_api::abi::{delivery_policy, DeliveryPolicy};
    for case in [
        "screen-changed",
        "pane-focused",
        "tree-changed",
        "events-dropped",
    ] {
        assert_eq!(
            delivery_policy(case),
            Some(DeliveryPolicy::Coalesced),
            "{case}"
        );
    }
    for case in ["output-matched", "topic-published", "audio-frames"] {
        assert_eq!(
            delivery_policy(case),
            Some(DeliveryPolicy::DropOldestCounted),
            "{case}"
        );
    }
    for case in [
        "agent-activity-changed",
        "message",
        "pane-exited",
        "request-completed",
    ] {
        assert_eq!(
            delivery_policy(case),
            Some(DeliveryPolicy::NeverDropped),
            "{case}"
        );
    }
}

#[test]
fn abi_delivery_policy_unknown_class_is_none() {
    assert_eq!(
        ilium_plugin_api::abi::delivery_policy("no-such-event"),
        None
    );
    assert_eq!(ilium_plugin_api::abi::delivery_policy(""), None);
}

#[test]
fn abi_counted_in_events_dropped_true_for_counted_classes_only() {
    assert!(ilium_plugin_api::abi::counted_in_events_dropped(
        "output-matched"
    ));
    assert!(ilium_plugin_api::abi::counted_in_events_dropped(
        "topic-published"
    ));
    assert!(!ilium_plugin_api::abi::counted_in_events_dropped(
        "pane-focused"
    ));
    assert!(!ilium_plugin_api::abi::counted_in_events_dropped(
        "events-dropped"
    ));
}

fn abi_push_times(
    queue: &mut ilium_plugin_api::abi::QueueModel,
    event_case: &str,
    key: u64,
    times: usize,
) {
    for _ in 0..times {
        queue.push(event_case, key);
    }
}

#[test]
fn abi_queue_new_queue_is_empty_with_no_drops() {
    use ilium_plugin_api::abi::QueueModel;
    let queue = QueueModel::new(8);
    assert_eq!(queue.capacity(), 8);
    assert!(queue.is_empty());
    assert_eq!(queue.len(), 0, "empty is zero");
    assert_eq!(queue.dropped_total(), 0);
    assert!(queue.events_dropped_records().is_empty());
}

#[test]
fn abi_queue_push_below_capacity_queues_without_drop() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(4);
    queue.push("output-matched", 0);
    assert_eq!(queue.len(), 1, "one event below capacity is queued");
    assert_eq!(queue.dropped_total(), 0);
    assert!(queue.events_dropped_records().is_empty());
}

#[test]
fn abi_queue_push_at_capacity_drops_oldest_and_counts_it() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(4);
    abi_push_times(&mut queue, "output-matched", 0, 5);
    assert_eq!(
        queue.dropped_total(),
        1,
        "the fifth event evicts one older event and counts it"
    );
    assert_eq!(
        queue.events_dropped_records(),
        vec![("output-matched".to_string(), 1)]
    );
}

#[test]
fn abi_queue_push_on_empty_queue_makes_it_non_empty() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(2);
    assert!(queue.is_empty());
    queue.push("pane-focused", 1);
    assert!(!queue.is_empty());
    assert_eq!(queue.len(), 1);
}

#[test]
fn abi_queue_len_after_push_and_drop_reflects_events_queued() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(2);
    abi_push_times(&mut queue, "output-matched", 0, 3);
    assert!(queue.len() <= 3, "len never exceeds the events pushed");
    assert_eq!(queue.dropped_total(), 1, "one event was dropped");
    assert!(!queue.is_empty());
}

#[test]
fn abi_queue_dropped_total_is_zero_when_nothing_dropped_and_grows_per_drop() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(1);
    queue.push("output-matched", 0);
    assert_eq!(queue.dropped_total(), 0, "a single event fits the queue");
    queue.push("output-matched", 0);
    assert_eq!(
        queue.dropped_total(),
        1,
        "the second event evicts one and counts it"
    );
    queue.push("output-matched", 0);
    assert_eq!(
        queue.dropped_total(),
        2,
        "each further drop increments the total"
    );
}

#[test]
fn abi_queue_events_dropped_records_empty_when_no_drops() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(4);
    queue.push("pane-focused", 1);
    queue.push("pane-focused", 1);
    assert!(
        queue.events_dropped_records().is_empty(),
        "coalesced events never produce a record"
    );
}

#[test]
fn abi_queue_events_dropped_records_merge_drops_per_event_case() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(1);
    for case in [
        "output-matched",
        "topic-published",
        "output-matched",
        "topic-published",
        "output-matched",
    ] {
        queue.push(case, 0);
    }
    assert_eq!(
        queue.dropped_total(),
        4,
        "four of the five pushes exceed a queue of one"
    );
    let mut records = queue.events_dropped_records();
    records.sort();
    assert_eq!(
        records,
        vec![
            ("output-matched".to_string(), 2),
            ("topic-published".to_string(), 2)
        ],
        "drops are merged into one record per event case, not one record per event"
    );
}

#[test]
fn abi_queue_coalesced_same_key_merges_into_one_slot() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(1024);
    abi_push_times(&mut queue, "screen-changed", 7, 2000);
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.dropped_total(), 0);
}

#[test]
fn abi_queue_coalesced_pending_key_on_full_queue_does_not_evict() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(2);
    queue.push("output-matched", 0);
    queue.push("pane-focused", 1);
    queue.push("pane-focused", 1);
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.dropped_total(), 0);
}

#[test]
fn abi_queue_interleaved_drops_are_counted_for_both_cases() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(1);
    queue.push("topic-published", 0);
    queue.push("output-matched", 0);
    queue.push("topic-published", 0);
    assert_eq!(
        queue.dropped_total(),
        2,
        "two drop-oldest events were displaced"
    );
    let mut records = queue.events_dropped_records();
    records.sort();
    assert_eq!(
        records,
        vec![
            ("output-matched".to_string(), 1),
            ("topic-published".to_string(), 1)
        ]
    );
}

#[test]
fn abi_queue_never_dropped_event_is_not_evicted_by_newer_drop_oldest_event() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(2);
    queue.push("agent-activity-changed", 1);
    queue.push("agent-activity-changed", 2);
    queue.push("topic-published", 0);
    assert_eq!(queue.len(), 2);
    assert_eq!(
        queue.dropped_total(),
        1,
        "the incoming drop-oldest event is the one discarded"
    );
}

#[test]
fn abi_queue_zero_capacity_discards_each_drop_oldest_event() {
    use ilium_plugin_api::abi::QueueModel;
    let mut queue = QueueModel::new(0);
    queue.push("output-matched", 0);
    assert!(queue.is_empty());
    assert_eq!(queue.dropped_total(), 1);
}

// ---------------------------------------------------------------- uitree

const UITREE_TEST_CAPS: ilium_plugin_api::uitree::UiCaps = ilium_plugin_api::uitree::UiCaps {
    max_nodes: 4096,
    max_kib: 1024,
    max_depth: 32,
};

fn uitree_node(
    key: Option<&str>,
    children: Vec<u32>,
) -> ilium_plugin_api::generated::types::UiNode {
    use ilium_plugin_api::generated::types::{NodeFlags, UiNode, Widget};
    UiNode {
        key: key.map(str::to_string),
        widget: Widget::Clear,
        children,
        flags: NodeFlags(0),
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

fn uitree_of(
    nodes: Vec<ilium_plugin_api::generated::types::UiNode>,
) -> ilium_plugin_api::generated::types::UiTree {
    ilium_plugin_api::generated::types::UiTree {
        version: 1,
        nodes,
        root: 0,
    }
}

fn uitree_chain(depth: u32) -> ilium_plugin_api::generated::types::UiTree {
    uitree_of(
        (0..depth)
            .map(|index| {
                uitree_node(
                    None,
                    if index + 1 < depth {
                        vec![index + 1]
                    } else {
                        vec![]
                    },
                )
            })
            .collect(),
    )
}

#[test]
fn uitree_single_root_tree_passes() {
    let tree = uitree_of(vec![uitree_node(None, vec![])]);
    assert_eq!(
        ilium_plugin_api::uitree::check_ui_tree(&tree, UITREE_TEST_CAPS),
        Ok(())
    );
}

#[test]
fn uitree_valid_tree_with_children_passes() {
    let tree = uitree_of(vec![
        uitree_node(None, vec![1, 2]),
        uitree_node(Some("left"), vec![]),
        uitree_node(Some("right"), vec![]),
    ]);
    assert_eq!(
        ilium_plugin_api::uitree::check_ui_tree(&tree, UITREE_TEST_CAPS),
        Ok(())
    );
}

#[test]
fn uitree_empty_tree_has_no_root_and_is_refused_as_invalid_tree() {
    let code = ilium_plugin_api::uitree::check_ui_tree(&uitree_of(vec![]), UITREE_TEST_CAPS)
        .expect_err("a tree with no nodes has no root and must be refused");
    assert_eq!(code.as_str(), "ui.invalid-tree");
}

#[test]
fn uitree_depth_of_32_levels_is_the_accepted_boundary() {
    assert_eq!(
        ilium_plugin_api::uitree::check_ui_tree(&uitree_chain(32), UITREE_TEST_CAPS),
        Ok(())
    );
}

#[test]
fn uitree_depth_of_33_levels_is_refused_as_too_deep() {
    let code = ilium_plugin_api::uitree::check_ui_tree(&uitree_chain(33), UITREE_TEST_CAPS)
        .expect_err("33 levels exceeds the depth cap of 32");
    assert_eq!(code.as_str(), "ui.too-deep");
}

#[test]
fn uitree_duplicate_node_key_is_refused_as_duplicate_key() {
    let tree = uitree_of(vec![
        uitree_node(None, vec![1, 2]),
        uitree_node(Some("save"), vec![]),
        uitree_node(Some("save"), vec![]),
    ]);
    let code = ilium_plugin_api::uitree::check_ui_tree(&tree, UITREE_TEST_CAPS)
        .expect_err("two widgets with the same key must be refused");
    assert_eq!(code.as_str(), "ui.duplicate-key");
}

// ---------------------------------------------------------------- limits

fn limits_request(entries: &[(&str, u64)]) -> ilium_plugin_api::limits::Limits {
    ilium_plugin_api::limits::Limits(
        entries
            .iter()
            .map(|(name, value)| ((*name).to_string(), *value))
            .collect(),
    )
}

#[test]
fn limits_default_of_memory_mib_matches_the_manual_default() {
    use ilium_plugin_api::limits::{default_of, LimitKey};
    assert_eq!(
        default_of(LimitKey::MemoryMib),
        32,
        "memory_mib defaults to 32 MiB"
    );
}

#[test]
fn limits_default_of_each_key_is_within_its_ceiling_for_every_tier() {
    use ilium_plugin_api::limits::{ceiling, default_of, LimitKey};
    use ilium_plugin_api::Tier;
    for key in LimitKey::ALL {
        for tier in Tier::ALL {
            assert!(
                default_of(key) <= ceiling(key, tier),
                "default of {key} must not exceed its {tier} ceiling"
            );
        }
    }
}

#[test]
fn limits_ceiling_matches_the_documented_tier_columns_for_memory_mib() {
    use ilium_plugin_api::limits::{ceiling, LimitKey};
    use ilium_plugin_api::Tier;
    assert_eq!(ceiling(LimitKey::MemoryMib, Tier::T1), 256, "T1 ceiling");
    assert_eq!(ceiling(LimitKey::MemoryMib, Tier::T2), 128, "T2 ceiling");
    assert_eq!(ceiling(LimitKey::MemoryMib, Tier::T3), 128, "T3 ceiling");
}

#[test]
fn limits_ceiling_of_frame_deadline_is_fifty_ms_for_every_tier() {
    use ilium_plugin_api::limits::{ceiling, LimitKey};
    use ilium_plugin_api::Tier;
    for tier in Tier::ALL {
        assert_eq!(
            ceiling(LimitKey::FrameDeadlineMs, tier),
            50,
            "frame deadline ceiling for {tier:?}"
        );
    }
}

#[test]
fn limits_ceiling_is_at_least_the_default_for_every_key_and_tier() {
    use ilium_plugin_api::limits::{ceiling, default_of, LimitKey};
    use ilium_plugin_api::Tier;
    for key in LimitKey::ALL {
        assert!(
            ceiling(key, Tier::T1) >= default_of(key),
            "T1 ceiling of {key} below default"
        );
        assert!(
            ceiling(key, Tier::T3) >= default_of(key),
            "T3 ceiling of {key} below default"
        );
    }
}

#[test]
fn limits_clamp_empty_request_yields_every_key_at_its_default() {
    use ilium_plugin_api::limits::{default_of, LimitKey, Limits};
    use ilium_plugin_api::Tier;
    let effective = ilium_plugin_api::limits::clamp(&Limits::default(), Tier::T3)
        .expect("an empty request is valid");
    assert_eq!(effective.len(), LimitKey::ALL.len());
    for key in LimitKey::ALL {
        assert_eq!(effective[&key], default_of(key), "{key}");
    }
}

#[test]
fn limits_clamp_zero_request_means_the_default_not_zero() {
    use ilium_plugin_api::limits::{default_of, LimitKey};
    use ilium_plugin_api::Tier;
    let requested = limits_request(&[("memory_mib", 0)]);
    let effective =
        ilium_plugin_api::limits::clamp(&requested, Tier::T3).expect("zero is a valid request");
    assert_eq!(
        effective[&LimitKey::MemoryMib],
        default_of(LimitKey::MemoryMib),
        "zero is the lowest request and resolves to the default, never to zero"
    );
}

#[test]
fn limits_clamp_value_below_ceiling_is_kept() {
    use ilium_plugin_api::limits::LimitKey;
    use ilium_plugin_api::Tier;
    let requested = limits_request(&[("memory_mib", 64)]);
    let effective = ilium_plugin_api::limits::clamp(&requested, Tier::T3).expect("valid request");
    assert_eq!(effective[&LimitKey::MemoryMib], 64);
}

#[test]
fn limits_clamp_value_equal_to_ceiling_is_kept() {
    use ilium_plugin_api::limits::{ceiling, LimitKey};
    use ilium_plugin_api::Tier;
    let ceiling_of_t3 = ceiling(LimitKey::MemoryMib, Tier::T3);
    let requested = limits_request(&[("memory_mib", ceiling_of_t3)]);
    let effective = ilium_plugin_api::limits::clamp(&requested, Tier::T3).expect("valid request");
    assert_eq!(effective[&LimitKey::MemoryMib], ceiling_of_t3);
}

#[test]
fn limits_clamp_value_above_ceiling_is_clamped_down_not_refused() {
    use ilium_plugin_api::limits::LimitKey;
    use ilium_plugin_api::Tier;
    let requested = limits_request(&[("memory_mib", 10_000)]);
    let effective = ilium_plugin_api::limits::clamp(&requested, Tier::T3)
        .expect("a request above the ceiling is clamped, never refused");
    assert_eq!(
        effective[&LimitKey::MemoryMib],
        128,
        "T3 ceiling of memory_mib"
    );
    let effective_t1 = ilium_plugin_api::limits::clamp(&requested, Tier::T1)
        .expect("a request above the ceiling is clamped, never refused");
    assert_eq!(
        effective_t1[&LimitKey::MemoryMib],
        256,
        "T1 ceiling of memory_mib"
    );
}

#[test]
fn limits_clamp_unknown_key_refuses_the_whole_request() {
    use ilium_plugin_api::Tier;
    let requested = limits_request(&[("not_a_limit", 1)]);
    let diagnostic = ilium_plugin_api::limits::clamp(&requested, Tier::T3)
        .expect_err("an unknown key must be refused");
    assert_eq!(diagnostic.code.as_str(), "manifest.unknown-key");
    assert_eq!(diagnostic.severity, ilium_plugin_api::Severity::Error);
}

#[test]
fn limits_clamp_session_key_is_not_a_manifest_key() {
    use ilium_plugin_api::Tier;
    let requested = limits_request(&[("session_memory_mib", 1)]);
    let diagnostic = ilium_plugin_api::limits::clamp(&requested, Tier::T1)
        .expect_err("session budgets are host settings, not manifest keys");
    assert_eq!(diagnostic.code.as_str(), "manifest.unknown-key");
}

// ---------------------------------------------------------------- version

fn version_refusal_code(
    result: Result<ilium_plugin_api::version::ApiVersion, ilium_plugin_api::manifest::Diagnostic>,
) -> Option<String> {
    result
        .err()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
}

#[test]
fn version_parse_valid_major_minor_patch_is_accepted() {
    use ilium_plugin_api::version::ApiVersion;
    assert_eq!(
        ApiVersion::parse("ilium:plugin@0.3.12"),
        Ok(ilium_plugin_api::version::ApiVersion {
            major: 0,
            minor: 3,
            patch: 12
        })
    );
}

#[test]
fn version_parse_pre_release_suffix_is_refused_as_manifest_schema() {
    use ilium_plugin_api::version::ApiVersion;
    assert_eq!(
        version_refusal_code(ApiVersion::parse("ilium:plugin@0.1.0-rc.1")),
        Some(ilium_plugin_api::codes::manifest::SCHEMA.to_owned()),
        "a pre-release suffix is not a plain X.Y.Z version"
    );
}

#[test]
fn version_parse_malformed_input_is_refused_as_manifest_schema() {
    use ilium_plugin_api::version::ApiVersion;
    for input in [
        "ilium:plugin@0.1",
        "ilium:plugin@0.1.0.0",
        "ilium:plugin@v0.1.0",
        "ilium:plugin@65536.0.0",
    ] {
        assert_eq!(
            version_refusal_code(ApiVersion::parse(input)),
            Some(ilium_plugin_api::codes::manifest::SCHEMA.to_owned()),
            "`{input}` is malformed"
        );
    }
}

#[test]
fn version_parse_empty_input_is_refused_as_manifest_schema() {
    use ilium_plugin_api::version::ApiVersion;
    assert_eq!(
        version_refusal_code(ApiVersion::parse("")),
        Some(ilium_plugin_api::codes::manifest::SCHEMA.to_owned())
    );
}

#[test]
fn version_from_custom_section_exact_length_decodes_big_endian_fields() {
    use ilium_plugin_api::version::ApiVersion;
    assert_eq!(
        ApiVersion::from_custom_section(&[0, 0, 0, 1, 0, 0]),
        Ok(ilium_plugin_api::version::ApiVersion {
            major: 0,
            minor: 1,
            patch: 0
        }),
        "the six-byte section 0.1.0 decodes big-endian"
    );
}

#[test]
fn version_from_custom_section_wrong_length_and_garbage_are_refused() {
    use ilium_plugin_api::version::ApiVersion;
    let inputs: [&[u8]; 4] = [&[], &[0, 0, 0, 1, 0], &[0, 0, 0, 1, 0, 0, 0], &[0xff, 0xfe]];
    for input in inputs {
        assert_eq!(
            version_refusal_code(ApiVersion::from_custom_section(input)),
            Some(ilium_plugin_api::codes::package::API_SECTION_INVALID.to_owned()),
            "a section of {} bytes must be refused",
            input.len()
        );
    }
}

#[test]
fn version_host_window_contains_current_and_previous_minor() {
    use ilium_plugin_api::version::{ApiVersion, CURRENT};
    let window = ApiVersion::host_window();
    assert!(window.contains(&CURRENT), "the current 0.1.0 is loaded");
    assert!(
        window.contains(&ilium_plugin_api::version::ApiVersion {
            major: 0,
            minor: 0,
            patch: 9
        }),
        "the previous minor is loaded"
    );
}

#[test]
fn version_host_window_excludes_next_minor_and_later_majors() {
    use ilium_plugin_api::version::ApiVersion;
    let window = ApiVersion::host_window();
    let version = |major, minor, patch| ApiVersion {
        major,
        minor,
        patch,
    };
    assert!(
        !window.contains(&version(0, 2, 0)),
        "0.2.0 is outside the window"
    );
    assert!(
        !window.contains(&version(1, 0, 0)),
        "1.0.0 is outside the window"
    );
}

#[test]
fn version_refusal_order_is_the_documented_check_order() {
    assert_eq!(
        ilium_plugin_api::version::REFUSAL_ORDER,
        [
            "manifest.schema",
            "package.api-unsupported",
            "manifest.api-version",
            "manifest.import-not-requested",
            "manifest.grant-unknown",
        ]
    );
}

// ---------------------------------------------------------------- error

#[test]
fn error_severity_all_lists_every_severity_most_severe_first() {
    use ilium_plugin_api::error::Severity;
    assert_eq!(
        Severity::ALL,
        [Severity::Error, Severity::Warning, Severity::Info]
    );
}

#[test]
fn error_severity_as_str_uses_the_documented_names() {
    use ilium_plugin_api::error::Severity;
    assert_eq!(Severity::Error.as_str(), "error");
    assert_eq!(
        Severity::Warning.as_str(),
        "warn",
        "the warning severity is written warn"
    );
    assert_eq!(Severity::Info.as_str(), "info");
}

#[test]
fn error_severity_from_str_parses_every_documented_name_and_round_trips() {
    use ilium_plugin_api::error::Severity;
    assert_eq!("error".parse::<Severity>(), Ok(Severity::Error));
    assert_eq!("warn".parse::<Severity>(), Ok(Severity::Warning));
    assert_eq!("info".parse::<Severity>(), Ok(Severity::Info));
    for severity in Severity::ALL {
        assert_eq!(severity.as_str().parse::<Severity>(), Ok(severity));
    }
}

#[test]
fn error_severity_from_str_rejects_long_warning_name_and_wrong_case() {
    use ilium_plugin_api::error::Severity;
    assert!(
        "warning".parse::<Severity>().is_err(),
        "the long name is not the documented word"
    );
    assert!("ERROR".parse::<Severity>().is_err());
    assert!("".parse::<Severity>().is_err());
}

#[test]
fn error_code_from_static_keeps_text_in_as_str_and_display() {
    use ilium_plugin_api::error::Code;
    let code = Code::from_static("manifest.grant-side");
    assert_eq!(code.as_str(), "manifest.grant-side");
    assert_eq!(format!("{code}"), "manifest.grant-side");
}

#[test]
fn error_code_namespace_is_the_part_before_the_first_dot() {
    use ilium_plugin_api::error::Code;
    assert_eq!(
        Code::from_static("manifest.grant-side").namespace(),
        "manifest"
    );
    assert_eq!(Code::from_static("trap.deadline").namespace(), "trap");
    assert_eq!(
        Code::from_static("extension.vetoed.sub").namespace(),
        "extension"
    );
}

#[test]
fn error_code_namespace_of_empty_or_leading_dot_text_is_empty() {
    use ilium_plugin_api::error::Code;
    assert_eq!(Code::from_static("").namespace(), "");
    assert_eq!(Code::from_static(".deadline").namespace(), "");
}

#[test]
fn error_code_base_maps_extension_codes_to_their_documented_base_codes() {
    use ilium_plugin_api::error::Code;
    use ilium_plugin_api::generated::types::ErrorCode;
    assert_eq!(
        Code::from_static("extension.vetoed").base(),
        ErrorCode::Conflict
    );
    assert_eq!(
        Code::from_static("extension.not-granted").base(),
        ErrorCode::NotPermitted
    );
    assert_eq!(
        Code::from_static("extension.unknown-point").base(),
        ErrorCode::NotFound
    );
    assert_eq!(
        Code::from_static("extension.schema-mismatch").base(),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        Code::from_static("extension.not-dynamic").base(),
        ErrorCode::Unsupported
    );
    assert_eq!(
        Code::from_static("extension.reentrant").base(),
        ErrorCode::Conflict
    );
    assert_eq!(
        Code::from_static("extension.depth-exceeded").base(),
        ErrorCode::Conflict
    );
    assert_eq!(
        Code::from_static("extension.entry-rejected").base(),
        ErrorCode::InvalidArgument
    );
}

#[test]
fn error_code_equality_compares_with_text() {
    use ilium_plugin_api::error::Code;
    let code = Code::from_static("trap.deadline");
    assert!(code == *"trap.deadline");
    assert!(code == "trap.deadline");
    assert!(code != "trap.memory");
}

#[test]
fn error_message_max_chars_is_256() {
    assert_eq!(ilium_plugin_api::error::MESSAGE_MAX_CHARS, 256);
}

#[test]
fn error_plugin_error_new_keeps_short_message_and_code() {
    use ilium_plugin_api::generated::types::{ErrorCode, PluginError};
    let error = PluginError::new(ErrorCode::NotFound, "pane 4 is gone");
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(error.message, "pane 4 is gone");
    assert_eq!(
        error.retry_after_ms, None,
        "a new error carries no retry hint"
    );
}

#[test]
fn error_plugin_error_new_accepts_empty_message() {
    use ilium_plugin_api::generated::types::{ErrorCode, PluginError};
    let error = PluginError::new(ErrorCode::Internal, "");
    assert_eq!(error.message, "");
}

#[test]
fn error_plugin_error_new_keeps_message_of_exactly_256_characters() {
    use ilium_plugin_api::generated::types::{ErrorCode, PluginError};
    let message = "a".repeat(256);
    let error = PluginError::new(ErrorCode::Io, message.as_str());
    assert_eq!(error.message, message);
}

#[test]
fn error_plugin_error_new_cuts_message_over_256_characters() {
    use ilium_plugin_api::generated::types::{ErrorCode, PluginError};
    let message = "a".repeat(300);
    let error = PluginError::new(ErrorCode::Io, message.as_str());
    assert_eq!(error.message.chars().count(), 256);
    assert_eq!(error.message, "a".repeat(256));
}

#[test]
fn error_plugin_error_new_counts_characters_not_bytes_when_cutting() {
    use ilium_plugin_api::generated::types::{ErrorCode, PluginError};
    let message = "é".repeat(300);
    let error = PluginError::new(ErrorCode::Io, message.as_str());
    assert_eq!(
        error.message.chars().count(),
        256,
        "256 characters, each two bytes"
    );
    assert_eq!(error.message, "é".repeat(256));
}

#[test]
fn error_with_prefix_starts_message_with_extension_code_colon_and_space() {
    use ilium_plugin_api::error::Code;
    use ilium_plugin_api::generated::types::ErrorCode;
    use ilium_plugin_api::generated::types::PluginError;
    let error = PluginError::with_prefix(
        ErrorCode::Conflict,
        &Code::from_static("extension.vetoed"),
        "quiet hours are on",
    );
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message, "extension.vetoed: quiet hours are on");
}

#[test]
fn error_with_prefix_and_empty_message_keeps_only_the_prefix() {
    use ilium_plugin_api::error::Code;
    use ilium_plugin_api::generated::types::ErrorCode;
    use ilium_plugin_api::generated::types::PluginError;
    let error = PluginError::with_prefix(
        ErrorCode::Conflict,
        &Code::from_static("extension.vetoed"),
        "",
    );
    assert_eq!(error.message, "extension.vetoed: ");
}

#[test]
fn error_with_prefix_caps_whole_message_at_256_characters() {
    use ilium_plugin_api::error::Code;
    use ilium_plugin_api::generated::types::ErrorCode;
    use ilium_plugin_api::generated::types::PluginError;
    let error = PluginError::with_prefix(
        ErrorCode::Conflict,
        &Code::from_static("extension.vetoed"),
        &"b".repeat(400),
    );
    assert_eq!(error.message.chars().count(), 256);
    assert!(error.message.starts_with("extension.vetoed: "));
}

#[test]
fn error_extension_code_reads_prefix_written_by_with_prefix() {
    use ilium_plugin_api::error::Code;
    use ilium_plugin_api::generated::types::ErrorCode;
    use ilium_plugin_api::generated::types::PluginError;
    let error = PluginError::with_prefix(
        ErrorCode::Conflict,
        &Code::from_static("extension.vetoed"),
        "quiet hours are on",
    );
    assert_eq!(
        error.extension_code(),
        Some(Code::from_static("extension.vetoed"))
    );
}

#[test]
fn error_extension_code_is_none_without_prefix_empty_or_misplaced_code() {
    use ilium_plugin_api::generated::types::ErrorCode;
    use ilium_plugin_api::generated::types::PluginError;
    let plain = PluginError::new(ErrorCode::NotFound, "pane 4 is gone");
    assert_eq!(plain.extension_code(), None, "no prefix");
    let empty = PluginError::new(ErrorCode::Internal, "");
    assert_eq!(empty.extension_code(), None, "empty message");
    let no_space = PluginError::new(ErrorCode::Conflict, "extension.vetoed:quiet");
    assert_eq!(
        no_space.extension_code(),
        None,
        "colon not followed by a space"
    );
    let not_leading = PluginError::new(ErrorCode::Conflict, "failed on extension.vetoed: quiet");
    assert_eq!(not_leading.extension_code(), None, "code not at the start");
    let no_namespace = PluginError::new(ErrorCode::Internal, "internal: failure");
    assert_eq!(
        no_namespace.extension_code(),
        None,
        "prefix without a namespace dot"
    );
}

#[test]
fn error_registry_maps_extension_codes_to_documented_base_codes() {
    use ilium_plugin_api::generated::types::ErrorCode;
    let base_of = |code: &str| {
        ilium_plugin_api::error::registry()
            .iter()
            .find(|info| info.code == code)
            .map(|info| info.base)
    };
    assert_eq!(base_of("extension.vetoed"), Some(ErrorCode::Conflict));
    assert_eq!(
        base_of("extension.schema-mismatch"),
        Some(ErrorCode::InvalidArgument)
    );
    assert_eq!(
        base_of("extension.unknown-point"),
        Some(ErrorCode::NotFound)
    );
}

#[test]
fn error_registry_codes_are_unique_namespaced_and_name_a_page() {
    let mut seen = std::collections::BTreeSet::new();
    for info in ilium_plugin_api::error::registry() {
        assert!(
            seen.insert(info.code),
            "duplicate registry code {}",
            info.code
        );
        assert!(
            info.code.contains('.'),
            "code {} is not namespaced",
            info.code
        );
        assert!(!info.page.is_empty(), "code {} names no page", info.code);
    }
    assert!(!seen.is_empty(), "the registry must not be empty");
}

#[test]
fn error_refusal_reasons_are_the_closed_list_in_table_order() {
    assert_eq!(ilium_plugin_api::error::REFUSAL_REASONS.len(), 15);
    assert_eq!(ilium_plugin_api::error::REFUSAL_REASONS[0], "declined");
    assert_eq!(ilium_plugin_api::error::REFUSAL_REASONS[14], "tls");
}

#[test]
fn error_refusal_reason_code_maps_not_permitted_reasons() {
    use ilium_plugin_api::error::{refusal_reason_code, REFUSAL_REASONS};
    use ilium_plugin_api::generated::types::ErrorCode;
    for reason in REFUSAL_REASONS {
        if matches!(reason, "response-size" | "stream-size" | "tls") {
            continue;
        }
        assert_eq!(
            refusal_reason_code(reason),
            Some(ErrorCode::NotPermitted),
            "{reason}"
        );
    }
}

#[test]
fn error_refusal_reason_code_maps_quota_and_network_reasons() {
    use ilium_plugin_api::error::refusal_reason_code;
    use ilium_plugin_api::generated::types::ErrorCode;
    assert_eq!(
        refusal_reason_code("response-size"),
        Some(ErrorCode::QuotaExceeded)
    );
    assert_eq!(
        refusal_reason_code("stream-size"),
        Some(ErrorCode::QuotaExceeded)
    );
    assert_eq!(refusal_reason_code("tls"), Some(ErrorCode::Network));
}

#[test]
fn error_refusal_reason_code_is_none_for_unknown_empty_or_wrong_case() {
    use ilium_plugin_api::error::refusal_reason_code;
    assert_eq!(refusal_reason_code("bogus"), None);
    assert_eq!(refusal_reason_code(""), None);
    assert_eq!(refusal_reason_code("Declined"), None);
}

#[test]
fn error_unknown_name_stores_kind_and_name_and_displays_quoted() {
    use ilium_plugin_api::error::UnknownName;
    let error = UnknownName::new("severity", "warning");
    assert_eq!(error.kind, "severity");
    assert_eq!(error.name, "warning");
    assert_eq!(format!("{error}"), r#"unknown severity name "warning""#);
}

#[test]
fn error_unknown_name_display_handles_empty_name() {
    use ilium_plugin_api::error::UnknownName;
    let error = UnknownName::new("tier", "");
    assert_eq!(format!("{error}"), r#"unknown tier name """#);
}

// ---------------------------------------------------------------- manifest

#[test]
fn manifest_parse_activation_startup_and_durable_timer_take_no_name() {
    use ilium_plugin_api::manifest::{parse_activation, ActivationTrigger};
    assert_eq!(
        parse_activation("startup").expect("startup parses"),
        ActivationTrigger::Startup
    );
    assert_eq!(
        parse_activation("durable-timer").expect("durable-timer parses"),
        ActivationTrigger::DurableTimer
    );
}

#[test]
fn manifest_parse_activation_maps_every_named_kind_to_its_variant() {
    use ilium_plugin_api::manifest::{parse_activation, ActivationTrigger};
    let expected: [(&str, ActivationTrigger); 15] = [
        (
            "event:example.tick",
            ActivationTrigger::Event("example.tick".to_string()),
        ),
        (
            "command:example.demo/show",
            ActivationTrigger::Command("example.demo/show".to_string()),
        ),
        (
            "view:example.demo.view",
            ActivationTrigger::View("example.demo.view".to_string()),
        ),
        (
            "pane-kind:example.notes.note",
            ActivationTrigger::PaneKind("example.notes.note".to_string()),
        ),
        (
            "scene:example.wave",
            ActivationTrigger::Scene("example.wave".to_string()),
        ),
        ("cli:demo", ActivationTrigger::Cli("demo".to_string())),
        (
            "http:/status",
            ActivationTrigger::Http("/status".to_string()),
        ),
        (
            "service:example.store.v1",
            ActivationTrigger::Service("example.store.v1".to_string()),
        ),
        (
            "topic:example.demo.changed",
            ActivationTrigger::Topic("example.demo.changed".to_string()),
        ),
        (
            "settings:example.tab",
            ActivationTrigger::Settings("example.tab".to_string()),
        ),
        (
            "catalog:example.colours",
            ActivationTrigger::Catalog("example.colours".to_string()),
        ),
        (
            "menu:example.menu",
            ActivationTrigger::Menu("example.menu".to_string()),
        ),
        (
            "virtual-rows:example.row",
            ActivationTrigger::VirtualRows("example.row".to_string()),
        ),
        (
            "language:example-lang",
            ActivationTrigger::Language("example-lang".to_string()),
        ),
        (
            "client-request:example.ask",
            ActivationTrigger::ClientRequest("example.ask".to_string()),
        ),
    ];
    for (text, expected_trigger) in expected {
        let trigger =
            parse_activation(text).unwrap_or_else(|diagnostic| panic!("{text}: {diagnostic}"));
        assert_eq!(trigger, expected_trigger, "wrong variant for {text}");
    }
}

#[test]
fn manifest_parse_activation_keeps_slashes_and_colons_in_name() {
    use ilium_plugin_api::manifest::{parse_activation, ActivationTrigger};
    assert_eq!(
        parse_activation("event:a:b").expect("event name with a colon parses"),
        ActivationTrigger::Event("a:b".to_string())
    );
}

#[test]
fn manifest_parse_activation_refuses_unknown_kind_empty_and_kind_without_name() {
    use ilium_plugin_api::manifest::parse_activation;
    for text in ["", "unknown:thing", "event", "event:", "startup:now"] {
        let diagnostic = parse_activation(text).expect_err("must be refused");
        assert_eq!(diagnostic.code.as_str(), "manifest.schema", "for {text:?}");
        assert_eq!(
            diagnostic.stage,
            Some(ilium_plugin_api::manifest::Stage(3)),
            "for {text:?}"
        );
    }
}

#[test]
fn manifest_parse_activation_refuses_names_with_whitespace() {
    use ilium_plugin_api::manifest::parse_activation;
    for text in ["event:a b", "topic:a\tb"] {
        let diagnostic = parse_activation(text).expect_err("whitespace is not allowed in a name");
        assert_eq!(diagnostic.code.as_str(), "manifest.schema", "for {text:?}");
    }
}

#[test]
fn manifest_parse_activation_kind_is_case_sensitive() {
    use ilium_plugin_api::manifest::parse_activation;
    let diagnostic = parse_activation("Event:example.tick").expect_err("kinds are lower case");
    assert_eq!(diagnostic.code.as_str(), "manifest.schema");
}

#[test]
fn manifest_parse_activation_name_at_256_characters_is_accepted() {
    use ilium_plugin_api::manifest::parse_activation;
    let exact = format!("event:{}", "e".repeat(256 - "event:".len()));
    assert_eq!(exact.len(), 256);
    assert!(
        parse_activation(&exact).is_ok(),
        "256 characters is the limit"
    );
}

#[test]
fn manifest_parse_activation_trigger_over_256_characters_is_refused() {
    use ilium_plugin_api::manifest::parse_activation;
    let over = format!("event:{}", "e".repeat(257 - "event:".len()));
    assert_eq!(over.len(), 257);
    let diagnostic = parse_activation(&over).expect_err("257 characters is over the limit");
    assert_eq!(diagnostic.code.as_str(), "manifest.schema");
}

#[test]
fn manifest_parse_when_accepts_manual_example_and_documented_forms() {
    use ilium_plugin_api::manifest::parse_when;
    assert!(parse_when(r#"pane.primitive == "agent" && agent.activity != "working""#).is_ok());
    assert!(
        parse_when(r#"(pane.primitive == "agent" || pane.primitive == "shell")"#).is_ok(),
        "grouping with parentheses and or must parse"
    );
}

#[test]
fn manifest_parse_when_refuses_empty_and_bad_syntax_as_when_syntax() {
    use ilium_plugin_api::manifest::parse_when;
    let diagnostic = parse_when("").expect_err("an empty expression is refused");
    assert_eq!(diagnostic.code.as_str(), "manifest.when-syntax");
    assert_eq!(diagnostic.stage, Some(ilium_plugin_api::manifest::Stage(3)));
    let bad = [
        r#"pane.primitive == agent"#,
        r#"pane.primitive = "agent""#,
        r#"pane.primitive == "agent" &&"#,
        r#"(pane.primitive == "agent""#,
        r#"pane.primitive == "agent")"#,
        r#"pane.primitive"#,
    ];
    for expression in bad {
        let diagnostic = parse_when(expression).expect_err("bad syntax must be refused");
        assert_eq!(
            diagnostic.code.as_str(),
            "manifest.when-syntax",
            "for {expression}"
        );
    }
}

#[test]
fn manifest_parse_when_refuses_unknown_key_as_when_key() {
    use ilium_plugin_api::manifest::parse_when;
    for expression in [
        r#"colour.name == "red""#,
        r#"pane.primitive == "agent" && agent.colour == "red""#,
        r#"Pane.primitive == "agent""#,
    ] {
        let diagnostic = parse_when(expression).expect_err("an unknown key must be refused");
        assert_eq!(
            diagnostic.code.as_str(),
            "manifest.when-key",
            "for {expression}"
        );
    }
}

#[test]
fn manifest_parse_chord_accepts_single_keys_named_keys_and_modifiers() {
    use ilium_plugin_api::manifest::parse_chord;
    for chord in [
        "p",
        "up",
        "enter",
        "page-up",
        "space",
        "f1",
        "f24",
        "ctrl+p",
        "alt+c",
        "shift+tab",
        "super+k",
        "ctrl+plus",
        "ctrl+space",
    ] {
        assert!(parse_chord(chord).is_ok(), "{chord} must parse");
    }
}

#[test]
fn manifest_parse_chord_splits_modifiers_then_key_in_any_order() {
    use ilium_plugin_api::manifest::parse_chord;
    let canonical = parse_chord("ctrl+shift+p").expect("ctrl+shift+p is valid");
    let reordered = parse_chord("shift+ctrl+p").expect("shift+ctrl+p is valid");
    let mut canonical_modifiers = canonical.0[..canonical.0.len() - 1].to_vec();
    let mut reordered_modifiers = reordered.0[..reordered.0.len() - 1].to_vec();
    canonical_modifiers.sort();
    reordered_modifiers.sort();
    assert_eq!(
        canonical_modifiers,
        vec!["ctrl".to_string(), "shift".to_string()]
    );
    assert_eq!(reordered_modifiers, canonical_modifiers);
    assert_eq!(canonical.0.last().map(String::as_str), Some("p"));
    assert_eq!(reordered.0.last().map(String::as_str), Some("p"));
}

#[test]
fn manifest_parse_chord_single_key_has_no_modifiers() {
    use ilium_plugin_api::manifest::{parse_chord, Chord};
    assert_eq!(
        parse_chord("p").expect("p parses"),
        Chord(vec!["p".to_string()])
    );
}

#[test]
fn manifest_parse_chord_refuses_empty_dangling_plus_and_unknown_modifier() {
    use ilium_plugin_api::manifest::parse_chord;
    assert!(parse_chord("").is_err(), "empty chord");
    for chord in ["ctrl+", "+p", "+", "ctrl++", "ctrl+shift+"] {
        assert!(
            parse_chord(chord).is_err(),
            "{chord}: a plus must join a modifier and a key"
        );
    }
    for chord in ["meta+p", "control+p", "cmd+p"] {
        assert!(
            parse_chord(chord).is_err(),
            "{chord}: only ctrl, alt, shift and super are modifiers"
        );
    }
}

#[test]
fn manifest_parse_chord_refuses_multi_character_or_unknown_key() {
    use ilium_plugin_api::manifest::parse_chord;
    for chord in [
        "ctrl+ab",
        "ctrl+enterkey",
        "ctrl+f0",
        "f25",
        "ctrl+f25",
        "ctrl+arrow-up",
    ] {
        assert!(
            parse_chord(chord).is_err(),
            "{chord}: the key must be one character or a named key"
        );
    }
}

#[test]
fn manifest_parse_chord_refuses_upper_case() {
    use ilium_plugin_api::manifest::parse_chord;
    for chord in ["Ctrl+p", "ctrl+P", "shift+A"] {
        assert!(
            parse_chord(chord).is_err(),
            "{chord}: chords are written in lower case"
        );
    }
}

#[test]
fn manifest_service_requirement_full_version_splits_name_and_version() {
    use ilium_plugin_api::manifest::ServiceRequirement;
    let parsed =
        ServiceRequirement::parse("ilium:inference/provider@0.1.0").expect("full version parses");
    assert_eq!(parsed.name, "ilium:inference/provider");
    assert_eq!(parsed.version, "0.1.0");
}

#[test]
fn manifest_service_requirement_short_or_range_version_is_service_version_error() {
    use ilium_plugin_api::manifest::ServiceRequirement;
    for text in [
        "ilium:inference/provider@0.1",
        "ilium:inference/provider@^0.1.0",
        "x/y@~1.2.0",
        "x/y@>=1.2.0",
    ] {
        let diagnostic =
            ServiceRequirement::parse(text).expect_err("a short or range version is refused");
        assert_eq!(
            diagnostic.code.as_str(),
            "manifest.service-version",
            "for {text:?}"
        );
    }
}

#[test]
fn manifest_service_requirement_without_version_is_requires_invalid() {
    use ilium_plugin_api::manifest::ServiceRequirement;
    let diagnostic = ServiceRequirement::parse("com.example.git/status")
        .expect_err("a full version is required");
    assert_eq!(diagnostic.code.as_str(), "manifest.requires-invalid");
}

#[test]
fn manifest_json_schema_is_a_json_object_document() {
    let schema = ilium_plugin_api::manifest::json_schema();
    let parsed: serde_json::Value = serde_json::from_str(schema).expect("the schema is valid JSON");
    assert!(parsed.is_object(), "the schema is a JSON object");
}

#[test]
fn manifest_parse_manifest_refuses_malformed_json_with_diagnostics() {
    use ilium_plugin_api::manifest::parse_manifest;
    let diagnostics = parse_manifest("{ not json").expect_err("malformed JSON must be refused");
    assert!(
        !diagnostics.is_empty(),
        "at least one diagnostic explains the refusal"
    );
}
