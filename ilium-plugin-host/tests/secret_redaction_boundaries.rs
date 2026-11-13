use ilium_plugin_host::secrets::{
    base64_standard, base64_url, hex_lower, masked_row, percent_encode, redact,
};

#[test]
fn known_encoding_vectors_and_all_secret_variants() {
    assert_eq!(base64_standard(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64_url(&[251, 255]), "-_8");
    assert_eq!(hex_lower(&[0, 15, 255]), "000fff");
    assert_eq!(percent_encode("a b/é"), "a%20b%2F%C3%A9");
    let secret = "token+/é!".to_owned();
    for variant in [
        secret.clone(),
        base64_standard(secret.as_bytes()),
        base64_standard(secret.as_bytes())
            .trim_end_matches('=')
            .to_owned(),
        base64_url(secret.as_bytes()),
        percent_encode(&secret),
        hex_lower(secret.as_bytes()),
    ] {
        assert_eq!(
            redact(
                &format!("prefix {variant} suffix"),
                std::slice::from_ref(&secret)
            ),
            "prefix <redacted> suffix"
        );
    }
}

#[test]
fn overlap_unicode_threshold_case_and_repetition() {
    assert_eq!(
        redact(
            "abcdefghijkl abcdefgh",
            &["abcdefgh".into(), "abcdefghijkl".into()]
        ),
        "<redacted> <redacted>"
    );
    assert_eq!(
        redact("ééééé éééééé", &["ééééé".into(), "éééééé".into()]),
        "ééééé <redacted>"
    );
    assert_eq!(
        redact("ABCDEF abcdef abcdef", &["abcdef".into()]),
        "ABCDEF <redacted> <redacted>"
    );
    assert_eq!(redact("untouched", &[String::new()]), "untouched");
}

#[test]
fn masking_caps_reveal_in_unicode_characters() {
    assert_eq!(masked_row("secret-pattern", 0), "••••");
    assert_eq!(masked_row("secret-pattern", 255), "••••-pattern");
    assert_eq!(masked_row("é界", 8), "••••é界");
    assert_eq!(masked_row("", 8), "••••");
}
