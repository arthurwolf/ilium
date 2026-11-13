//! Error record and diagnostic registry contract edge cases.
use ilium_plugin_api::error::{refusal_reason_code, registry, Code, REFUSAL_REASONS};
use ilium_plugin_api::{ErrorCode, PluginError};

#[test]
fn messages_are_capped_by_unicode_characters() {
    let message = format!("{}tail", "🦀".repeat(256));
    let error = PluginError::new(ErrorCode::InvalidArgument, message);
    assert_eq!(error.message, "🦀".repeat(256));
    assert_eq!(error.retry_after_ms, None);
    assert_eq!(PluginError::new(ErrorCode::Busy, "").message, "");
}

#[test]
fn extension_prefix_preserves_the_context_dependent_base() {
    let code = Code::from_static("extension.cap-exceeded");
    assert_eq!(code.base(), ErrorCode::InvalidArgument);
    let error = PluginError::with_prefix(ErrorCode::QuotaExceeded, &code, "point full");
    assert_eq!(error.code, ErrorCode::QuotaExceeded);
    assert_eq!(error.message, "extension.cap-exceeded: point full");
    assert_eq!(error.extension_code(), Some(code));
}

#[test]
fn only_registered_prefixes_at_the_start_are_recognized() {
    for message in [
        "unknown.code: detail",
        "extension.unknown-point",
        "prefix extension.unknown-point: detail",
        "extension.unknown-point:detail",
        "",
    ] {
        assert_eq!(
            PluginError::new(ErrorCode::Internal, message).extension_code(),
            None
        );
    }
    let error = PluginError::new(ErrorCode::NotFound, "extension.unknown-point: ");
    assert_eq!(error.extension_code().unwrap().namespace(), "extension");
    assert_eq!(Code::from_static("plain").namespace(), "plain");
    assert_eq!(Code::from_static("").namespace(), "");
}

#[test]
fn refusal_reasons_are_a_closed_set_with_documented_base_codes() {
    for reason in REFUSAL_REASONS {
        let expected = match reason {
            "response-size" | "stream-size" => ErrorCode::QuotaExceeded,
            "tls" => ErrorCode::Network,
            _ => ErrorCode::NotPermitted,
        };
        assert_eq!(refusal_reason_code(reason), Some(expected));
    }
    for reason in ["", "Declined", "declined: ", "unknown"] {
        assert_eq!(refusal_reason_code(reason), None);
    }
}

#[test]
fn registry_covers_each_constant_once_and_preserves_extension_mappings() {
    let entries = registry();
    assert_eq!(entries.len(), ilium_plugin_api::codes::ALL.len());
    for code in ilium_plugin_api::codes::ALL {
        assert_eq!(
            entries.iter().filter(|entry| entry.code == *code).count(),
            1
        );
    }
    for (code, base) in [
        ("extension.unknown-point", ErrorCode::NotFound),
        ("extension.not-granted", ErrorCode::NotPermitted),
        ("extension.schema-mismatch", ErrorCode::InvalidArgument),
        ("extension.reentrant", ErrorCode::Conflict),
        ("extension.not-dynamic", ErrorCode::Unsupported),
        ("extension.vetoed", ErrorCode::Conflict),
        ("extension.depth-exceeded", ErrorCode::Conflict),
    ] {
        let entry = entries.iter().find(|entry| entry.code == code).unwrap();
        assert_eq!(entry.base, base);
        assert!(!entry.retryable);
        assert_eq!(entry.page, "api-extension-points.md");
    }
    assert_eq!(
        Code::from_static("unknown.code").base(),
        ErrorCode::Internal
    );
}
