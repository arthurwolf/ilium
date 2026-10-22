//! A user's trust never overrides machine policy (USR-253).

use crate::support::*;

/// REQ: USR-253
#[test]
fn usr_253_a_trusted_key_does_not_override_a_policy_denial() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.trust(&key);
    rig.write_system_policy("version = 1\n[plugins]\ndeny_ids = [\"com.example.trusted\"]\n");
    let package = spec("com.example.trusted", "1.0.0", &[]).signed_with(&key);
    let install = rig.install_file(&package, &["--yes"]);
    assert_eq!(result_exit_code(&install), 0, "{:?}", error_codes(&install));
    rig.start_server();
    assert_eq!(rig.instance_starts("com.example.trusted"), 0);
    let row = list_row(&mut rig, "com.example.trusted");
    let shown = format!("{row:?} {}", rig.notices().join(" "));
    assert!(shown.contains("Blocked by your administrator"), "{shown}");
}
