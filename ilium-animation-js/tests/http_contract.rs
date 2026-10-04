#![cfg(feature = "native-network")]
use ilium_animation_js::http::HttpOptions;
use serde_json::json;

#[test]
fn scripts_cannot_supply_transport_credentials_or_unbounded_requests() {
    let parse = |fields| {
        serde_json::from_value::<HttpOptions>(fields)
            .unwrap()
            .validate()
    };
    assert!(parse(
        json!({"url":"https://example.org/","response":"bytes","max_bytes":1024,"timeout_ms":1000})
    )
    .is_ok());
    for url in [
        "http://example.org/",
        "https://user:password@example.org/",
        "file:///tmp/file",
        "https://example.org/#secret",
    ] {
        assert!(
            parse(json!({"url":url,"response":"bytes","max_bytes":1024,"timeout_ms":1000}))
                .is_err()
        );
    }
    for headers in [
        json!({"Host":"private.invalid"}),
        json!({"Authorization":"Bearer invented"}),
        json!({"Cookie":"browser=secret"}),
        json!({"Connection":"upgrade"}),
    ] {
        assert!(parse(json!({"url":"https://example.org/","response":"bytes","max_bytes":1024,"timeout_ms":1000,"headers":headers})).is_err());
    }
    assert!(parse(json!({"url":"https://example.org/","response":"bytes","max_bytes":1073741824,"timeout_ms":1000})).is_err());
}
