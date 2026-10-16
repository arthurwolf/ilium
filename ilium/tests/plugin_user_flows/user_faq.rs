//! Plugin FAQ (user-faq.md): USR-110.

use crate::support::*;

fn faq() -> String {
    manual_page("user-faq")
}

/// REQ: USR-110
#[test]
fn doc_usr_110_faq_answers_whether_a_store_exists() {
    let page = faq();
    assert!(page.contains("Is there a plugin store?"));
    assert!(page.contains("Ilium ships none and runs no central store"));
}

/// REQ: USR-110
#[test]
fn doc_usr_110_faq_answers_which_systems_run_third_party_plugins() {
    let page = faq();
    assert!(page.contains("Do plugins work on Windows and macOS?"));
    assert!(page.contains("Only the bundled plugins"));
    assert!(page.contains("sandbox that makes third-party plugins safe exists on Linux only"));
}

/// REQ: USR-110
#[test]
fn doc_usr_110_faq_answers_what_declining_a_permission_does() {
    let page = faq();
    assert!(page.contains("What if I say no to a permission?"));
    assert!(page.contains("The plugin still installs and runs. The calls that need the permission fail"));
}

/// REQ: USR-110
#[test]
fn doc_usr_110_faq_answers_how_to_leave_a_bad_plugin_behind() {
    let page = faq();
    assert!(page.contains("uninstall it with `--purge`"));
    assert!(page.contains("`ilium plugin trust remove`"));
    assert!(page.contains("ilium --safe-mode") || page.contains("--safe-mode"));
}
