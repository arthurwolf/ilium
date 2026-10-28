//! ABI-077 and ABI-078 from reference-abi-major-versions.md.

use ilium_plugin_api::abi::{
    refusal_listing, render_refusal_listing, Installation, InstalledPlugin, LoadOutcome,
};
use ilium_plugin_api::version::{ApiVersion, CURRENT};

fn installed(name: &str, declared_api: ApiVersion, refusal: Option<&str>) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_owned(),
        declared_api,
        refusal: refusal.map(str::to_owned),
    }
}

/// REQ: ABI-077
#[test]
fn abi_077_refusal_listing_names_every_abi_refused_plugin_with_code_and_api() {
    let older_api = ApiVersion {
        major: CURRENT.major,
        minor: CURRENT.minor.saturating_sub(1),
        patch: 0,
    };
    let install_set = vec![
        installed("ok-plugin", CURRENT, None),
        installed("refused-minor", older_api, Some("abi.minor-unsupported")),
        installed(
            "refused-major",
            ApiVersion {
                major: CURRENT.major + 1,
                minor: 0,
                patch: 0,
            },
            Some("abi.major-unsupported"),
        ),
    ];
    let rows = refusal_listing(&install_set);
    assert_eq!(
        rows.len(),
        2,
        "only the two ABI-refused plugins may be listed"
    );
    let output = render_refusal_listing(&install_set);
    for row in &rows {
        assert!(
            output.contains(&row.code)
                && output.contains(&row.plugin)
                && output.contains(&row.declared_api.to_string()),
            "one output must show plugin {}, code {} and declared api {}",
            row.plugin,
            row.code,
            row.declared_api
        );
    }
    assert!(
        !output.contains("ok-plugin"),
        "a plugin that loads must not be listed as refused"
    );
}

/// REQ: ABI-078
#[test]
fn abi_078_host_rollback_leaves_plugin_state_and_refused_plugin_loads_again() {
    let mut installation = Installation::sample_mixed();
    let before = installation.snapshot();
    installation.upgrade_host(CURRENT);
    assert!(
        matches!(
            installation.load("refused-after-upgrade"),
            LoadOutcome::Refused(_)
        ),
        "the fixture plugin must be refused after the upgrade"
    );
    installation.rollback_host();
    assert_eq!(
        installation.snapshot(),
        before,
        "rolling the host back must not change plugin files, grants or stored settings"
    );
    assert!(
        matches!(
            installation.load("refused-after-upgrade"),
            LoadOutcome::Loaded
        ),
        "the plugin refused after the upgrade must load again unchanged after the rollback"
    );
}

/// REQ: ABI-077
#[test]
fn abi_077_listing_gives_the_refusal_code_and_declared_api_of_a_refused_plugin() {
    let older_api = ApiVersion {
        major: CURRENT.major,
        minor: CURRENT.minor.saturating_sub(1),
        patch: 0,
    };
    let install_set = vec![installed(
        "refused-minor",
        older_api,
        Some("abi.minor-unsupported"),
    )];
    let rows = refusal_listing(&install_set);
    assert_eq!(rows.len(), 1, "one refused plugin must give one listing row");
    assert_eq!(rows[0].plugin, "refused-minor");
    assert_eq!(rows[0].code, "abi.minor-unsupported");
    assert_eq!(rows[0].declared_api.to_string(), older_api.to_string());
}

/// REQ: ABI-078
#[test]
fn abi_078_upgrading_the_host_alone_changes_no_plugin_file_grant_or_setting() {
    let mut installation = Installation::sample_mixed();
    let before = installation.snapshot();
    installation.upgrade_host(CURRENT);
    assert_eq!(
        installation.snapshot(),
        before,
        "an upgrade by itself must not rewrite plugin files, grants or stored settings"
    );
}
