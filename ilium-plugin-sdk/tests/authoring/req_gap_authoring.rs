//! Gap requirements of the author guides and onboarding: translated text is never case-mapped by
//! the plugin (AUT-000), and the first `ilium plugin dev` run of a fresh project shows the dev
//! notice and badge, or fails with `session.unreachable` (ONB-000).

use std::path::{Path, PathBuf};

use ilium_plugin_host::devtools::lint;
use ilium_plugin_host::i18n::Lookup;
use ilium_plugin_host::testing::ops::{OpsRig, PackageSpec};
use ilium_plugin_sdk::types::{BadgeSpec, Tone, Widget};
use ilium_plugin_sdk::ui::widgets::badge;

/// A fresh scratch project folder with the files `plugin new` writes, under the system temp dir.
fn scratch_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ilium-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("scratch project folder");
    std::fs::write(
        dir.join("manifest.json"),
        r#"{"id":"notes","version":"0.1.0","description":"Notes plugin","license":"MIT"}"#,
    )
    .expect("manifest.json");
    std::fs::write(dir.join("README.md"), "# notes\n").expect("README.md");
    std::fs::write(dir.join("CHANGELOG.md"), "# Changelog\n").expect("CHANGELOG.md");
    dir
}

/// Source of a plugin that passes a catalogue string through a locale-independent case mapping.
fn write_case_mapped_source(project: &Path) {
    std::fs::write(
        project.join("src").join("lib.rs"),
        "fn badge_text(catalogue_text: &str) -> String {\n    catalogue_text.to_uppercase()\n}\n",
    )
    .expect("source file");
}

/// REQ: AUT-000
#[test]
fn req_aut_000_case_mapped_catalogue_text_raises_lint_text_case_mapped_a() {
    let project = scratch_project("aut-000-lint");
    write_case_mapped_source(&project);
    let findings = lint(&project);
    let _ = std::fs::remove_dir_all(&project);
    assert!(
        findings
            .iter()
            .any(|finding| finding.rule == "lint.text-case-mapped"),
        "a catalogue string passed to to_uppercase must raise lint.text-case-mapped; got {:?}",
        findings
            .iter()
            .map(|finding| finding.rule.clone())
            .collect::<Vec<_>>()
    );
}

/// REQ: AUT-000
#[test]
fn req_aut_000_turkish_badge_shows_catalogue_text_byte_for_byte_b() {
    let catalogue = Lookup::from_entries(&[("tr", &[("iptal", "İptal")])]);
    let shown = catalogue.text("tr", "iptal");
    assert_eq!(
        shown.as_bytes(),
        "İptal".as_bytes(),
        "the Turkish message iptal must reach the screen unchanged"
    );
    let widget = badge(BadgeSpec {
        label: Some(shown.clone()),
        count: None,
        dot: false,
        tone: Tone::Info,
    });
    let spec = match widget {
        Widget::Badge(spec) => spec,
        _ => BadgeSpec {
            label: None,
            count: None,
            dot: false,
            tone: Tone::Info,
        },
    };
    assert_eq!(
        spec.label.as_deref().map(str::as_bytes),
        Some("İptal".as_bytes()),
        "the badge label must equal the catalogue string, not an upper-cased copy"
    );
    assert_ne!(
        shown.to_uppercase(),
        shown,
        "Turkish iptal is case-mapped by to_uppercase, which is why the plugin must not do it"
    );
}

/// REQ: ONB-000
#[test]
fn req_onb_000_first_dev_run_shows_footer_notice_and_dev_badge_a() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    let session = rig.dev_start(&PackageSpec::new("notes", "0.1.0"), &[]);
    assert_eq!(session.attached_session(), "s1");
    assert!(
        !session.reload_log().is_empty(),
        "the first dev run must print a reload record"
    );
    let notices = server.notices();
    assert!(
        notices.iter().any(|notice| notice.contains(
            "notes (dev) is running. Press Ctrl+B then : to open Settings > Plugins."
        )),
        "the tree footer notice must name the running dev plugin; got {notices:?}"
    );
    assert!(
        server.has_dev_package("notes"),
        "the plugin row must carry the dev badge"
    );
}

/// REQ: ONB-000
#[test]
fn req_onb_000_dev_without_session_fails_with_session_unreachable_naming_project_b() {
    let project = scratch_project("onb-000-no-session");
    let project_text = project.display().to_string();
    let rig = OpsRig::new();
    let output = rig.dev_cli(&["--json", "--cwd", &project_text]);
    let _ = std::fs::remove_dir_all(&project);
    assert_eq!(output.exit_code, 6, "no reachable session must exit 6");
    let combined = format!("{}{}", output.stdout_raw, output.stderr);
    assert!(
        combined.contains("session.unreachable"),
        "the failure must carry code session.unreachable; got {combined}"
    );
    assert!(
        combined.contains(&project_text),
        "the hint must name the project folder {project_text} in which to start ilium; got {combined}"
    );
}
