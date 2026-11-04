//! Gap-review requirements of the imports-core lane: the `[ui] language` setting (SET-006) and
//! the Settings language row (SET-000). Uses the public `i18n` language choice and the public
//! `cli` entry point (`ilium plugin doctor`). These tests are written test-first: the behaviour
//! they check is not implemented yet, so they are expected to fail at runtime.

use std::sync::Arc;
use std::time::SystemTime;

use ilium_plugin_host::cli::{run, CliEnv};
use ilium_plugin_host::i18n::{choose_ui_language, LangEnv};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};

/// A fetcher for offline `doctor` runs: the doctor must not reach the network.
struct OfflineFetcher;

impl Fetcher for OfflineFetcher {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// A CLI environment rooted in a fresh temporary folder, with `[ui] language` written to the
/// config file and the given locale variables set.
fn doctor_environment(
    root: &std::path::Path,
    ui_language: &str,
    locale_variables: &[(&str, &str)],
) -> CliEnv {
    let config_dir = root.join("config");
    std::fs::create_dir_all(&config_dir).expect("create the config folder");
    std::fs::write(
        config_dir.join("config.toml"),
        format!("[ui]\nlanguage = \"{ui_language}\"\n"),
    )
    .expect("write config.toml");
    let mut env = CliEnv::new(
        root.join("data"),
        root.join("state"),
        config_dir,
        root.to_path_buf(),
        SystemTime::now(),
        true,
        Arc::new(OfflineFetcher),
    );
    for (name, value) in locale_variables {
        env = env.with_var(name, value);
    }
    env
}

/// REQ: SET-006
#[test]
fn req_set_006_invalid_ui_language_falls_back_to_locale_a() {
    // Normal path: the malformed `[ui] language` value is skipped, so the next source of the
    // language order (here LANG=fr_FR.UTF-8) decides, and the session runs in `fr`.
    let environment = LangEnv::from_pairs(&[("LANG", "fr_FR.UTF-8")]);
    let choice = choose_ui_language(&environment, Some("english!"));
    assert_eq!(choice.tag, "fr", "the session must run in the locale language");
    assert_eq!(choice.source, "LANG", "the choice must come from LANG");

    // `ilium plugin doctor` prints the rejected value next to the chosen tag.
    let root = tempfile::tempdir().expect("temporary folder");
    let env = doctor_environment(root.path(), "english!", &[("LANG", "fr_FR.UTF-8")]);
    let output = run(&["doctor".to_owned()], &env);
    assert!(
        output.stdout_text.contains("english!"),
        "doctor must print the rejected value, got stdout: {}",
        output.stdout_text
    );
    assert!(
        output.stdout_text.contains("fr"),
        "doctor must print the chosen tag fr, got stdout: {}",
        output.stdout_text
    );
}

/// REQ: SET-006
#[test]
fn req_set_006_invalid_ui_language_is_rejected_with_notice_b() {
    // Refusal path: the malformed tag must never be chosen, and the notice must name the key
    // `[ui] language` and the bad value `english!`.
    let environment = LangEnv::from_pairs(&[("LANG", "fr_FR.UTF-8")]);
    let choice = choose_ui_language(&environment, Some("english!"));
    assert_ne!(choice.tag, "english!", "a malformed BCP 47 tag must be rejected");
    assert_ne!(
        choice.source, "[ui] language",
        "the rejected setting must not be reported as the source"
    );

    let root = tempfile::tempdir().expect("temporary folder");
    let env = doctor_environment(root.path(), "english!", &[("LANG", "fr_FR.UTF-8")]);
    let output = run(&["doctor".to_owned()], &env);
    let notice_text = format!("{}{}", output.stderr, output.stdout_text);
    assert!(
        notice_text.contains("[ui] language"),
        "the notice must name the key [ui] language, got: {notice_text}"
    );
    assert!(
        notice_text.contains("english!"),
        "the notice must name the bad value english!, got: {notice_text}"
    );
}

/// REQ: SET-000
#[test]
fn req_set_000_language_row_unset_value_uses_environment_order_a() {
    // Choosing the unset value leaves `[ui] language` empty, so the environment order applies:
    // with LANG=de_DE.UTF-8 the next session starts in `de`.
    let locale_only = LangEnv::from_pairs(&[("LANG", "de_DE.UTF-8")]);
    let choice = choose_ui_language(&locale_only, None);
    assert_eq!(choice.tag, "de", "the next session must start in de");
    assert_eq!(choice.source, "LANG", "the unset value must defer to LANG");

    // `--language en-XA` (carried to the host as ILIUM_LANG here) activates the pseudo-locale.
    let pseudo = LangEnv::from_pairs(&[("ILIUM_LANG", "en-XA"), ("LANG", "de_DE.UTF-8")]);
    let pseudo_choice = choose_ui_language(&pseudo, None);
    assert_eq!(
        pseudo_choice.tag, "en-XA",
        "an explicit en-XA launch language must activate the pseudo-locale"
    );
}

/// REQ: SET-000
#[test]
fn req_set_000_language_row_never_offers_pseudo_locale_b() {
    // The pseudo-locale is not a launch language of the Settings row, so a plain system locale
    // of `en_XA` must never select `en-XA` on its own.
    let locale = LangEnv::from_pairs(&[("LANG", "en_XA.UTF-8")]);
    let choice = choose_ui_language(&locale, None);
    assert_ne!(
        choice.tag, "en-XA",
        "en-XA must not be selected from the environment locale; it is only accepted from --language, ILIUM_LANG or a hand-written config value"
    );

    // The unset row value (an empty setting) must not pin the pseudo-locale either.
    let empty_setting = LangEnv::from_pairs(&[("LANG", "de_DE.UTF-8")]);
    let empty_choice = choose_ui_language(&empty_setting, Some(""));
    assert_ne!(
        empty_choice.tag, "en-XA",
        "the unset language value must never resolve to en-XA"
    );
}
