#![cfg(feature = "native-network")]
use ilium_animation_js::{
    error::{AnimationError, Result},
    http::{request, DnsResolver, HttpAuthority, HttpOptions, HttpPhase},
};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use url::Url;

struct RejectOrigin;
impl HttpAuthority for RejectOrigin {
    fn authorize(
        &mut self,
        phase: HttpPhase,
        _: ilium_animation_js::permissions::HttpMethod,
        _: &Url,
        addresses: &[SocketAddr],
    ) -> Result<()> {
        assert_eq!(phase, HttpPhase::Preflight);
        assert!(addresses.is_empty());
        Err(AnimationError::Runtime("origin denied".into()))
    }
    fn credential_headers(&mut self, _: &str, _: &Url) -> Result<BTreeMap<String, String>> {
        panic!("a denied origin must not acquire credentials")
    }
}
struct RecordingDns(AtomicUsize);
impl DnsResolver for RecordingDns {
    fn resolve(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(vec!["127.0.0.1:443".parse().unwrap()])
    }
}
#[test]
fn denied_origin_has_no_dns_or_credential_side_effect() {
    let options: HttpOptions = serde_json::from_value(serde_json::json!({
        "url":"https://example.org/", "response":"bytes", "max_bytes":1024,
        "timeout_ms":1000, "credential":"opaque-credential"
    }))
    .unwrap();
    let dns = RecordingDns(AtomicUsize::new(0));
    let result = request(&options, &mut RejectOrigin, &dns, &AtomicBool::new(false));
    assert!(result.is_err());
    assert_eq!(dns.0.load(Ordering::Relaxed), 0);
}

struct GetOnly;
impl HttpAuthority for GetOnly {
    fn authorize(
        &mut self,
        _: HttpPhase,
        method: ilium_animation_js::permissions::HttpMethod,
        _: &Url,
        _: &[SocketAddr],
    ) -> Result<()> {
        if method != ilium_animation_js::permissions::HttpMethod::Get {
            return Err(AnimationError::PermissionDenied(
                "GET scope cannot authorize POST".into(),
            ));
        }
        Ok(())
    }
    fn credential_headers(&mut self, _: &str, _: &Url) -> Result<BTreeMap<String, String>> {
        panic!("method denial must precede credential acquisition")
    }
}
#[test]
fn a_get_scope_cannot_issue_post_or_resolve_its_target() {
    let options: HttpOptions = serde_json::from_value(serde_json::json!({
        "url":"https://example.org/", "method":"POST", "body":"payload",
        "response":"bytes", "max_bytes":1024, "timeout_ms":1000
    }))
    .unwrap();
    let dns = RecordingDns(AtomicUsize::new(0));
    let result = request(&options, &mut GetOnly, &dns, &AtomicBool::new(false));
    assert!(matches!(result, Err(AnimationError::PermissionDenied(_))));
    assert_eq!(dns.0.load(Ordering::Relaxed), 0);
}
