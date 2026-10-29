use ilium_plugin_host::sync::constant_time_eq;

#[test]
fn equality_checks_every_position_and_rejects_length_mismatches() {
    let token = [0x5a; 32];
    assert!(constant_time_eq(&token, &token));
    for index in 0..token.len() {
        let mut changed = token;
        changed[index] ^= 1;
        assert!(!constant_time_eq(&token, &changed));
    }
    assert!(!constant_time_eq(&token, &token[..31]));
    assert!(!constant_time_eq(&token[..31], &token));
    assert!(constant_time_eq(&[], &[]));
    assert!(!constant_time_eq(&[], &[0]));
}
