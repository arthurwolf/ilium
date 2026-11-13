//! `ilium:host/clipboard` (api-clipboard.md).

use ilium_plugin_api::generated::host::clipboard::HostClipboard;
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["clipboard.read", "clipboard.write"];

/// REQ: WIT:ilium:host/clipboard.read-text
#[test]
fn wit_clipboard_read_text_returns_the_clipboard_text() {
    let rig = rig(GRANTS);
    rig.kernel.set_clipboard(Some("hello".to_owned()));
    assert_eq!(rig.host.clipboard().read_text(), Some("hello".to_owned()));
}

/// REQ: WIT:ilium:host/clipboard.read-text
#[test]
fn wit_clipboard_read_text_empty_clipboard_is_none() {
    let rig = rig(GRANTS);
    rig.kernel.set_clipboard(None);
    assert_eq!(rig.host.clipboard().read_text(), None);
}

/// REQ: WIT:ilium:host/clipboard.read-text
#[test]
fn wit_clipboard_read_text_declined_grant_is_none() {
    let rig = declined_rig(&["clipboard.read"]);
    rig.kernel.set_clipboard(Some("secret".to_owned()));
    assert_eq!(rig.host.clipboard().read_text(), None);
}

/// REQ: WIT:ilium:host/clipboard.write-text
#[test]
fn wit_clipboard_write_text_sets_the_clipboard() {
    let rig = rig(GRANTS);
    rig.host
        .clipboard()
        .write_text("copied".to_owned())
        .expect("writes");
    assert_eq!(rig.kernel.clipboard(), Some("copied".to_owned()));
}

/// REQ: WIT:ilium:host/clipboard.write-text
#[test]
fn wit_clipboard_write_text_one_mib_boundary_invalid_argument() {
    let rig = rig(GRANTS);
    assert!(rig.host.clipboard().write_text(text_of(MIB - 1)).is_ok());
    assert!(rig.host.clipboard().write_text(text_of(MIB)).is_ok());
    assert_eq!(
        code_of(rig.host.clipboard().write_text(text_of(MIB + 1))),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/clipboard.write-text
#[test]
fn wit_clipboard_write_text_declined_grant_not_permitted() {
    let rig = declined_rig(&["clipboard.write"]);
    assert_eq!(
        code_of(rig.host.clipboard().write_text("x".to_owned())),
        ErrorCode::NotPermitted
    );
}
