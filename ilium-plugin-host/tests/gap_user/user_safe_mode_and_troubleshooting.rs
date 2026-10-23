//! Safe mode entry paths of the page `user-safe-mode-and-troubleshooting` (USR-345).

use crate::common::*;

/// REQ: USR-345
#[test]
fn usr_345_safe_mode_by_flag_env_or_three_crashing_starts_names_its_cause_in_the_banner() {
    let by_flag = rig();
    install_ok(&by_flag, &pkg(NOTES, "1.0.0"));
    let flagged = by_flag.start_server_with_args("s1", &["--safe-mode"]);
    assert!(
        flagged.banner_text().contains("--safe-mode"),
        "banner: {}",
        flagged.banner_text()
    );

    let by_env = rig();
    install_ok(&by_env, &pkg(NOTES, "1.0.0"));
    let from_env = by_env.start_server_with_env("s1", &[("ILIUM_SAFE_MODE", "1")]);
    assert!(
        from_env.banner_text().contains("ILIUM_SAFE_MODE"),
        "banner: {}",
        from_env.banner_text()
    );

    let automatic = rig();
    install_ok(&automatic, &pkg(NOTES, "1.0.0"));
    automatic.record_host_crashes_while_loading(3, secs(300));
    let after_crashes = automatic.start_server("s1");
    let banner = after_crashes.banner_text();
    assert!(
        banner.contains("crash"),
        "the automatic cause must be named: {banner}"
    );
    assert!(
        banner.contains("3"),
        "the crash count must be named: {banner}"
    );
}

/// REQ: USR-345
#[test]
fn usr_345_two_crashing_starts_do_not_enter_safe_mode_automatically() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.record_host_crashes_while_loading(2, secs(300));
    let server = rig.start_server("s1");
    assert!(
        !server.banner_text().contains("crash"),
        "two crashes must not trigger safe mode"
    );
}
