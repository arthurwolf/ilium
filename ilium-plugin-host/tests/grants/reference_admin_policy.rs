//! SEC-101 (schema version) and SEC-115 (reason codes) of reference-admin-policy.md, host half.
//! The policy rules run through the real `PolicyEngine` API; the text rules read the manual page.
//! The two-minor-release read window and the audit readback of a refusal are release-process or
//! storage facts, so they are only partly covered here (see the register cell of each ID).

use ilium_plugin_api::codes;
use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::Tier;
use ilium_plugin_host::policy::{
    InstallPolicyContext, PolicyEngine, PolicyInput, PolicyOwnership, PolicySourceKind,
    PolicyVerdict,
};

const ADMIN_POLICY_PAGE: &str = "reference-admin-policy.md";

fn manual_page(page_file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs/plugins-manual/pages")
        .join(page_file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} must be readable: {error}", path.display());
    })
}

/// An administrator-owned system layer, the only layer whose rules restrict third parties.
fn system_policy(text: &str) -> PolicyEngine {
    PolicyEngine::from_inputs(
        PolicyInput::Document {
            text: text.to_owned(),
            ownership: Some(PolicyOwnership {
                administrator_owned: true,
                non_administrator_writable: false,
            }),
        },
        PolicyInput::Missing,
    )
}

/// A user-layer document; ownership is not checked for this layer.
fn user_policy(text: &str) -> PolicyEngine {
    PolicyEngine::from_inputs(
        PolicyInput::Missing,
        PolicyInput::Document {
            text: text.to_owned(),
            ownership: None,
        },
    )
}

fn grant(text: &str) -> Grant {
    Grant::parse(text).expect("grant parses")
}

fn registry_package(plugin: &'static str) -> InstallPolicyContext<'static> {
    InstallPolicyContext {
        plugin,
        tier: Tier::T2,
        publisher_key: None,
        source: "https://registry.example/plugins",
        source_kind: PolicySourceKind::Registry,
        channel: "stable",
        development: false,
        replaces: false,
    }
}

fn refused(code: &str) -> PolicyVerdict {
    PolicyVerdict::Deny(code.to_owned())
}

// ---------------------------------------------------------------- SEC-101 schema version

/// REQ: SEC-101
#[test]
fn sec_101_version_two_is_refused_as_malformed_and_third_parties_do_not_load() {
    let engine = user_policy("version = 2\n");
    assert!(!engine.is_valid(), "a version this server does not read is an invalid policy");
    assert!(
        engine.errors().iter().any(|error| error == "version must be 1"),
        "the refusal names the version rule, got {:?}",
        engine.errors()
    );
    assert_eq!(
        engine.check_grant_for("example.notes", Tier::T3, &grant("clipboard.write")),
        refused(codes::policy::INVALID),
        "a third-party grant is refused with policy.invalid while the file is malformed"
    );
}

/// REQ: SEC-101
#[test]
fn sec_101_version_one_is_the_accepted_schema_and_grants_proceed() {
    let engine = user_policy("version = 1\n");
    assert!(engine.is_valid(), "version 1 is accepted: {:?}", engine.errors());
    assert_eq!(
        engine.check_grant_for("example.notes", Tier::T3, &grant("clipboard.write")),
        PolicyVerdict::Allow,
        "a valid version-1 policy with no restriction allows the grant"
    );
}

/// REQ: SEC-101
#[test]
fn sec_101_a_file_without_a_version_or_with_version_zero_is_malformed() {
    for text in ["", "version = 0\n"] {
        let engine = user_policy(text);
        assert!(
            !engine.is_valid(),
            "policy text {text:?} names no supported version and must be malformed"
        );
    }
}

/// REQ: SEC-101
#[test]
fn sec_101_an_unsupported_version_does_not_stop_first_party_plugins() {
    let engine = user_policy("version = 2\n");
    assert_eq!(
        engine.check_grant_for("example.notes", Tier::T1, &grant("clipboard.write")),
        PolicyVerdict::Allow,
        "a T1 plugin runs normally even when the policy file is malformed"
    );
}

/// REQ: SEC-101
#[test]
fn sec_101_the_page_states_version_one_and_the_two_minor_release_read_window() {
    let page = manual_page(ADMIN_POLICY_PAGE);
    assert!(
        page.contains("Schema version `1` is the only version in 0.1.0"),
        "the page states that schema version 1 is the only version in 0.1.0"
    );
    assert!(
        page.contains("a server MUST keep reading the previous version for two minor releases"),
        "the page states the two-minor-release read window"
    );
    assert!(
        page.contains("treats the file as malformed under [SEC-187]"),
        "the page routes an unsupported version to SEC-187 malformed handling"
    );
}

/// REQ: SEC-101
#[test]
fn sec_101_the_page_gives_the_staged_path_and_the_refused_version_example() {
    let page = manual_page(ADMIN_POLICY_PAGE);
    assert!(
        page.contains(
            "write the file as `version = 1` until every machine runs a server that reads the new key"
        ),
        "the page gives the staged path that keeps version 1 until every server reads the new version"
    );
    assert!(
        page.contains("a file with `version = 2` on a server that supports 1 is refused with `policy.invalid`"),
        "the page names the version-2 refusal example"
    );
}

// ---------------------------------------------------------------- SEC-115 reason codes

/// REQ: SEC-115
#[test]
fn sec_115_the_page_names_the_reason_codes_as_the_stable_contract() {
    let page = manual_page(ADMIN_POLICY_PAGE);
    assert!(
        page.contains("The reason codes are the stable contract for every notice on this page"),
        "the page declares the reason codes as the stable contract"
    );
    for code in [
        "policy.invalid",
        "policy.tier-denied",
        "policy.id-denied",
        "policy.publisher-denied",
        "policy.source-denied",
        "policy.replacement",
        "policy.version-denied",
        "policy.version-pinned",
    ] {
        assert!(
            page.contains(&format!("`{code}`")),
            "the page lists the reason code `{code}`"
        );
    }
}

/// REQ: SEC-115
#[test]
fn sec_115_the_page_states_english_only_text_and_the_code_mapping_rule() {
    let page = manual_page(ADMIN_POLICY_PAGE);
    assert!(
        page.contains(
            "The notice and refusal texts are host English text in 0.1.0, with no translation shipped and no locale switch."
        ),
        "the page states that 0.1.0 text is English only"
    );
    assert!(
        page.contains("A localised text MUST map from the code"),
        "a localised text must map from the code"
    );
    assert!(
        page.contains("support staff MUST match a user's text to the code in the `audit` record"),
        "support staff match text to the code in the audit record"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_the_page_leaves_the_text_conflict_open_for_the_owner_to_choose() {
    let page = manual_page(ADMIN_POLICY_PAGE);
    assert!(
        page.contains("The owner MUST choose one text per surface before release."),
        "the owner must choose one text per surface before release"
    );
    assert!(
        page.contains("`Disabled by your administrator`") && page.contains("`Blocked by policy`"),
        "the open item names both conflicting texts"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_every_refusal_code_is_a_dotted_lowercase_code_without_spaces() {
    let host_codes = [
        codes::policy::INVALID,
        codes::policy::TIER_DENIED,
        codes::policy::ID_DENIED,
        codes::policy::PUBLISHER_DENIED,
        codes::policy::SOURCE_DENIED,
        codes::policy::REPLACEMENT,
        codes::policy::GRANT_FORBIDDEN,
    ];
    for code in host_codes {
        assert!(code.starts_with("policy."), "{code} is a policy code");
        assert!(
            code["policy.".len()..]
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '-'),
            "{code} carries no spaces, capitals or English sentence text"
        );
    }
}

/// REQ: SEC-115
#[test]
fn sec_115_the_page_lists_every_code_the_host_defines() {
    let page = manual_page(ADMIN_POLICY_PAGE);
    for code in [
        codes::policy::INVALID,
        codes::policy::TIER_DENIED,
        codes::policy::ID_DENIED,
        codes::policy::PUBLISHER_DENIED,
        codes::policy::SOURCE_DENIED,
        codes::policy::REPLACEMENT,
    ] {
        assert!(
            page.contains(&format!("`{code}`")),
            "the page lists the host code {code}"
        );
    }
}

/// REQ: SEC-115
#[test]
fn sec_115_an_invalid_policy_refusal_carries_the_invalid_code() {
    let engine = user_policy("version = 2\n");
    assert_eq!(
        engine.check_grant_for("example.notes", Tier::T3, &grant("clipboard.write")),
        refused("policy.invalid"),
        "a malformed policy refuses with the code, not sentence text"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_valid_policy_does_not_emit_the_invalid_code() {
    let engine = user_policy("version = 1\n");
    assert_ne!(
        engine.check_grant_for("example.notes", Tier::T3, &grant("clipboard.write")),
        refused(codes::policy::INVALID),
        "a valid policy must not refuse with policy.invalid"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_an_id_denied_refusal_carries_the_id_code() {
    let engine = system_policy("version = 1\n[plugins]\ndeny_ids = [\"example.*\"]\n");
    assert_eq!(
        engine.check_install(&registry_package("example.notes")),
        refused(codes::policy::ID_DENIED),
        "an id the administrator denies is refused with policy.id-denied"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_an_id_not_matching_the_deny_pattern_is_not_refused() {
    let engine = system_policy("version = 1\n[plugins]\ndeny_ids = [\"other.*\"]\n");
    assert_eq!(
        engine.check_install(&registry_package("example.notes")),
        PolicyVerdict::Allow,
        "an id outside the deny pattern installs"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_tier_denied_refusal_carries_the_tier_code() {
    let engine = system_policy("version = 1\n[packages]\nallowed_tiers = [\"t1\"]\n");
    assert_eq!(
        engine.check_install(&registry_package("example.notes")),
        refused(codes::policy::TIER_DENIED),
        "a tier outside allowed_tiers is refused with policy.tier-denied"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_tier_inside_allowed_tiers_is_not_refused() {
    let engine = system_policy("version = 1\n[packages]\nallowed_tiers = [\"t1\", \"t2\"]\n");
    assert_eq!(
        engine.check_install(&registry_package("example.notes")),
        PolicyVerdict::Allow,
        "a T2 package inside allowed_tiers installs"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_publisher_denied_refusal_carries_the_publisher_code() {
    let engine = system_policy("version = 1\n[publishers]\ndeny = [\"key-bad\"]\n");
    let mut package = registry_package("example.notes");
    package.publisher_key = Some("key-bad");
    assert_eq!(
        engine.check_install(&package),
        refused(codes::policy::PUBLISHER_DENIED),
        "a denied publisher key is refused with policy.publisher-denied"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_publisher_key_not_on_the_deny_list_is_not_refused() {
    let engine = system_policy("version = 1\n[publishers]\ndeny = [\"key-bad\"]\n");
    let mut package = registry_package("example.notes");
    package.publisher_key = Some("key-good");
    assert_eq!(
        engine.check_install(&package),
        PolicyVerdict::Allow,
        "a publisher key outside the deny list installs"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_source_denied_refusal_carries_the_source_code() {
    let engine = system_policy("version = 1\n[packages]\nallow_url_installs = false\n");
    let mut package = registry_package("example.notes");
    package.source_kind = PolicySourceKind::File;
    assert_eq!(
        engine.check_install(&package),
        refused(codes::policy::SOURCE_DENIED),
        "a file install with URL installs disabled is refused with policy.source-denied"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_registry_install_is_not_refused_by_the_url_rule() {
    let engine = system_policy("version = 1\n[packages]\nallow_url_installs = false\n");
    assert_eq!(
        engine.check_install(&registry_package("example.notes")),
        PolicyVerdict::Allow,
        "a registry source is not a URL or file install"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_replacement_refusal_carries_the_replacement_code() {
    let engine = system_policy("version = 1\n[extensions]\nallow_replacement = false\n");
    let mut package = registry_package("example.notes");
    package.replaces = true;
    assert_eq!(
        engine.check_install(&package),
        refused(codes::policy::REPLACEMENT),
        "a replacement with allow_replacement = false is refused with policy.replacement"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_package_that_does_not_replace_is_not_refused_by_the_replacement_rule() {
    let engine = system_policy("version = 1\n[extensions]\nallow_replacement = false\n");
    assert_eq!(
        engine.check_install(&registry_package("example.notes")),
        PolicyVerdict::Allow,
        "a plain install is not a replacement"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_grant_forbidden_refusal_carries_the_grant_code() {
    let engine = system_policy("version = 1\n[grants]\ndeny = [\"clipboard.*\"]\n");
    assert_eq!(
        engine.check_grant_for("example.notes", Tier::T3, &grant("clipboard.write")),
        refused(codes::policy::GRANT_FORBIDDEN),
        "a denied grant is refused with policy.grant-forbidden"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_grant_without_a_deny_rule_is_not_refused() {
    let engine = user_policy("version = 1\n");
    assert_eq!(
        engine.check_grant_for("example.notes", Tier::T3, &grant("clipboard.write")),
        PolicyVerdict::Allow,
        "with no deny rule the grant is allowed"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_version_denied_refusal_carries_its_code() {
    let engine = system_policy(
        "version = 1\n[packages]\nblocked_versions = [\"example.notes@2.0.0\"]\n",
    );
    assert_eq!(
        engine.check_install_version("example.notes", "2.0.0"),
        refused(codes::policy::VERSION_DENIED),
        "a blocked exact version is refused with policy.version-denied"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_version_outside_the_allowed_range_is_not_refused_as_version_denied() {
    let engine = system_policy(
        "version = 1\n[packages]\nblocked_versions = [\"example.notes@2.0.0\"]\n",
    );
    assert_eq!(
        engine.check_install_version("example.notes", "2.0.1"),
        PolicyVerdict::Allow,
        "another version of the same id is not blocked"
    );
    assert_eq!(
        engine.check_install_version("example.other", "2.0.0"),
        PolicyVerdict::Allow,
        "the same version of another id is not blocked"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_a_version_pinned_refusal_carries_its_code() {
    let engine = system_policy(
        "version = 1\n[packages.pinned_versions]\n\"example.notes\" = \"1.4.2\"\n",
    );
    assert_eq!(
        engine.check_install_version("example.notes", "2.0.1"),
        refused(codes::policy::VERSION_PINNED),
        "a version other than the pin is refused with policy.version-pinned"
    );
}

/// REQ: SEC-115
#[test]
fn sec_115_the_pinned_version_itself_is_not_refused_as_version_pinned() {
    let engine = system_policy(
        "version = 1\n[packages.pinned_versions]\n\"example.notes\" = \"1.4.2\"\n",
    );
    assert_eq!(
        engine.check_install_version("example.notes", "1.4.2"),
        PolicyVerdict::Allow,
        "the pinned version installs"
    );
    assert_eq!(
        engine.check_install_version("example.other", "9.9.9"),
        PolicyVerdict::Allow,
        "a pin on one id does not constrain another"
    );
}
