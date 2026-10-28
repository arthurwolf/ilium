//! Pure policy decisions, using explicit ownership facts rather than chmod simulations.
use ilium_plugin_api::{grants::Grant, Tier};
use ilium_plugin_host::policy::{
    InstallPolicyContext, PolicyEngine, PolicyInput, PolicyOwnership, PolicySourceKind,
    PolicyVerdict,
};
fn document(text: &str) -> PolicyInput {
    PolicyInput::Document {
        text: text.into(),
        ownership: Some(PolicyOwnership {
            administrator_owned: true,
            non_administrator_writable: false,
        }),
    }
}
fn policy(system: &str, user: &str) -> PolicyEngine {
    PolicyEngine::from_inputs(document(system), document(user))
}
fn grant(text: &str) -> Grant {
    Grant::parse(text).unwrap()
}
fn package() -> InstallPolicyContext<'static> {
    InstallPolicyContext {
        plugin: "example.notes",
        tier: Tier::T2,
        publisher_key: None,
        source: "https://registry.example/plugins",
        source_kind: PolicySourceKind::Registry,
        channel: "stable",
        development: false,
        replaces: false,
    }
}
fn denied(code: &str) -> PolicyVerdict {
    PolicyVerdict::Deny(code.into())
}

#[test]
fn user_host_allow_lists_cannot_widen_system_wildcards() {
    let engine = policy(
        "version=1\n[network]\nallow_hosts=['*.example.com']",
        "version=1\n[network]\nallow_hosts=['a.example.com','outside.test']",
    );
    assert!(engine.is_valid(), "{:?}", engine.errors());
    assert_eq!(
        engine.check_egress("example.notes", "a.example.com"),
        PolicyVerdict::Allow
    );
    assert_eq!(
        engine.check_egress("example.notes", "b.example.com"),
        denied("policy.host-denied")
    );
    assert_eq!(
        engine.check_egress("example.notes", "outside.test"),
        denied("policy.host-denied")
    );
}
#[test]
fn offline_and_denies_win_over_user_allows() {
    let engine = policy(
        "version=1\n[network]\noffline=true",
        "version=1\n[network]\noffline=false",
    );
    assert_eq!(
        engine.check_egress("example.notes", "localhost:8080"),
        denied("offline")
    );
    let engine = policy("version=1\n[grants]\ndeny=['clipboard.*']", "version=1");
    assert_eq!(
        engine.check_grant("example.notes", &grant("clipboard.write")),
        denied("policy.grant-forbidden")
    );
    assert_eq!(engine.check_install(&package()), PolicyVerdict::Allow);
}
#[test]
fn denied_host_subdomains_exclude_apex_and_lookalikes() {
    let engine = policy(
        "version=1\n[grants]\ndeny=['http:*.pastebin.example']",
        "version=1",
    );
    assert_eq!(
        engine.check_grant("example.notes", &grant("http:a.pastebin.example")),
        denied("policy.grant-forbidden")
    );
    for host in [
        "pastebin.example",
        "notpastebin.example",
        "a.pastebin.example.evil",
    ] {
        assert_eq!(
            engine.check_grant("example.notes", &grant(&format!("http:{host}"))),
            PolicyVerdict::Allow
        );
    }
}
#[test]
fn malformed_patterns_fail_closed_for_third_parties_and_preserve_bundled_tier() {
    for text in [
        "version=2",
        "version=1\nunknown=true",
        "version=1\n[grants]\ndeny=['net.*']",
        "version=1\n[network]\noffline='false'",
    ] {
        let engine = policy(text, "version=1");
        assert!(!engine.is_valid(), "{text}");
        assert_eq!(engine.check_install(&package()), denied("policy.invalid"));
        let mut bundled = package();
        bundled.tier = Tier::T1;
        assert_eq!(engine.check_install(&bundled), PolicyVerdict::Allow);
    }
}
#[test]
fn known_insecure_system_file_is_ignored_but_unknown_ownership_is_not_trusted() {
    let text = "version=1\n[packages]\nallow_third_party=false";
    let insecure = PolicyInput::Document {
        text: text.into(),
        ownership: Some(PolicyOwnership {
            administrator_owned: false,
            non_administrator_writable: true,
        }),
    };
    let engine = PolicyEngine::from_inputs(insecure, PolicyInput::Missing);
    assert_eq!(engine.check_install(&package()), PolicyVerdict::Allow);
    assert_eq!(
        engine.notices(),
        ["Admin policy ignored: file is writable by non-administrators"]
    );
    let engine = PolicyEngine::from_inputs(
        PolicyInput::Document {
            text: text.into(),
            ownership: None,
        },
        PolicyInput::Missing,
    );
    assert_eq!(engine.check_install(&package()), denied("policy.invalid"));
}
#[test]
fn first_failure_order_and_bundled_exemption_are_explicit() {
    let engine = policy("version=1\n[packages]\nallow_third_party=false\nallowed_tiers=[]\n[plugins]\ndeny_ids=['example.*']", "version=1");
    assert_eq!(
        engine.check_install(&package()),
        denied("policy.third-party-denied")
    );
    let mut bundled = package();
    bundled.tier = Tier::T1;
    assert_eq!(engine.check_install(&bundled), PolicyVerdict::Allow);
}
#[test]
fn source_prefixes_have_component_boundaries_and_distinguish_registry_from_bare_url() {
    let engine = policy("version=1\n[packages]\nallowed_sources=['https://registry.example/plugins/']\nallow_url_installs=false", "version=1");
    assert_eq!(engine.check_install(&package()), PolicyVerdict::Allow);
    for source in [
        "https://registry.example/plugins-evil",
        "https://registry.example/plugins/../elsewhere",
        "https://registry.example.evil/plugins",
    ] {
        let mut candidate = package();
        candidate.source = source;
        assert_eq!(
            engine.check_install(&candidate),
            denied("policy.source-denied")
        );
    }
    let mut direct = package();
    direct.source_kind = PolicySourceKind::Url;
    assert_eq!(
        engine.check_install(&direct),
        denied("policy.source-denied")
    );
}
#[test]
fn user_preapproval_cannot_manufacture_administrator_authority() {
    let engine = policy(
        "version=1",
        "version=1\n[grants.preapproved]\n'example.notes'=['clipboard.write']",
    );
    assert!(engine
        .preapproved("example.notes", &[grant("clipboard.write")])
        .is_empty());
}
#[test]
fn forced_disable_wins_conflict_without_preventing_disable() {
    let engine = policy(
        "version=1\n[plugins]\nforce_enabled=['example.notes']\nforce_disabled=['example.notes']",
        "version=1",
    );
    assert_eq!(
        engine.check_enabled_change("example.notes", true),
        denied("policy.forced-state")
    );
    assert_eq!(
        engine.check_enabled_change("example.notes", false),
        PolicyVerdict::Allow
    );
}

#[test]
fn wildcard_policy_never_authorizes_ip_literals_and_denies_cover_ports() {
    let engine = policy(
        "version=1\n[network]\nallow_hosts=['*.example.com']",
        "version=1",
    );
    assert_eq!(
        engine.check_egress("example.notes", "203.0.113.5"),
        denied("policy.host-denied")
    );
    let engine = policy(
        "version=1\n[network]\ndeny_hosts=['*.example.com']",
        "version=1",
    );
    assert_eq!(
        engine.check_egress("example.notes", "api.example.com:8443"),
        denied("policy.host-denied")
    );
}

#[test]
fn combination_policy_does_not_block_unrelated_individual_grants() {
    let engine = policy("version=1\n[grants]\ndeny_combinations=true", "version=1");
    assert_eq!(
        engine.check_grant_for("example.notes", Tier::T2, &grant("sound.play")),
        PolicyVerdict::Allow
    );
    // A member cannot be certified without the actual retained/requested set.
    assert_eq!(
        engine.check_grant_for("example.notes", Tier::T2, &grant("tree.read")),
        denied("policy.context-required")
    );
}

#[test]
fn actual_same_side_pair_locks_members_but_not_unrelated_grants() {
    let engine = policy("version=1\n[grants]\ndeny_combinations=true", "version=1");
    let requested = [
        grant("tree.read"),
        grant("http:example.com"),
        grant("sound.play"),
    ];
    let review = engine.review_install_grants(&package(), &requested);
    assert_eq!(review.installation, PolicyVerdict::Allow);
    assert_eq!(
        review.grants,
        vec![
            (requested[0].clone(), denied("policy.grant-forbidden")),
            (requested[1].clone(), denied("policy.grant-forbidden")),
            (requested[2].clone(), PolicyVerdict::Allow),
        ]
    );
    // Removing either checked member removes the combination restriction.
    for member in &requested[..2] {
        assert_eq!(
            engine.check_grant_in_set_for(
                "example.notes",
                Tier::T2,
                member,
                std::slice::from_ref(member)
            ),
            PolicyVerdict::Allow
        );
    }
}

#[test]
fn candidate_is_checked_with_existing_grants_even_when_not_in_set() {
    let engine = policy("version=1\n[grants]\ndeny_combinations=true", "version=1");
    assert_eq!(
        engine.check_grant_in_set_for(
            "example.notes",
            Tier::T2,
            &grant("http:example.com"),
            &[grant("tree.read")]
        ),
        denied("policy.grant-forbidden")
    );
    assert_eq!(
        engine.check_grant_in_set_for(
            "example.notes",
            Tier::T2,
            &grant("tree.read"),
            &[grant("http:example.com")]
        ),
        denied("policy.grant-forbidden")
    );
    // Private data reads are outside the fixed SEC-006 read group.
    assert_eq!(
        engine.check_grant_in_set_for(
            "example.notes",
            Tier::T2,
            &grant("http:example.com"),
            &[grant("fs.read:data")]
        ),
        PolicyVerdict::Allow
    );
}

#[test]
fn different_side_sets_are_not_combined_and_explicit_denies_still_apply() {
    let engine = policy(
        "version=1\n[grants]\ndeny_combinations=true\ndeny=['sound.play']",
        "version=1",
    );
    for requested in [[grant("tree.read")], [grant("http:example.com")]] {
        let review = engine.review_install_grants(&package(), &requested);
        assert_eq!(review.grants[0].1, PolicyVerdict::Allow);
    }
    assert_eq!(
        engine.check_grant_in_set_for("example.notes", Tier::T2, &grant("sound.play"), &[]),
        denied("policy.grant-forbidden")
    );
}

#[test]
fn preapproval_cannot_bypass_combination_policy() {
    let engine = policy("version=1\n[grants]\ndeny_combinations=true\n[grants.preapproved]\n'example.notes'=['tree.read','http:example.com','sound.play']", "version=1");
    let requested = [
        grant("tree.read"),
        grant("http:example.com"),
        grant("sound.play"),
    ];
    assert_eq!(
        engine.preapproved("example.notes", &requested),
        vec![grant("sound.play")]
    );
    assert_eq!(
        engine.preapproved("example.notes", &[grant("tree.read")]),
        vec![grant("tree.read")]
    );
}

#[test]
fn requested_review_uses_retained_permissions_without_claiming_new_possession() {
    let engine = policy("version=1\n[grants]\ndeny_combinations=true", "version=1");
    let requested = [grant("http:example.com"), grant("sound.play")];
    let result =
        engine.review_install_grants_with_retained(&package(), &requested, &[grant("tree.read")]);
    assert_eq!(result.installation, PolicyVerdict::Allow);
    assert_eq!(result.grants[0].1, denied("policy.grant-forbidden"));
    assert_eq!(result.grants[1].1, PolicyVerdict::Allow);
    assert_eq!(result.grants.len(), requested.len());
    assert_eq!(
        engine
            .review_install_grants_with_retained(&package(), &requested, &[])
            .grants[0]
            .1,
        PolicyVerdict::Allow
    );
}
