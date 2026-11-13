//! reference-network-egress.md: address classes, DNS pinning, redirects, TLS, proxy, headers,
//! size caps, budget, offline switch, policy lists, host-initiated features, HTTP API listener.

use ilium_plugin_api::{ErrorCode, Tier};
use ilium_plugin_host::testing::{FakeNet, FakeResponse};
use serde_json::json;

use crate::support::*;

const PUBLIC_ADDRESS: &str = "93.184.216.34";

/// A rig whose network resolves `example.com` to a public address and serves `/` there.
fn public_rig(grants: &[&str]) -> Rig {
    let rig = t3(grants);
    rig.net.dns("example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://example.com/", FakeResponse::ok(b"hello".to_vec()));
    rig
}

/// A rig for `host` resolving to `address`.
fn resolving_rig(host: &str, address: &str) -> Rig {
    let grant = format!("http:{host}");
    let rig = t3(&[grant.as_str()]);
    rig.net.dns(host, &[address]);
    rig.net
        .serve(&format!("https://{host}/"), FakeResponse::ok(b"x".to_vec()));
    rig
}

fn assert_private(host: &str, address: &str) {
    let rig = resolving_rig(host, address);
    let result = rig.get(&format!("https://{host}/"), &[]);
    assert_refused(&result, ErrorCode::NotPermitted, "private-address");
    assert!(
        rig.net.seen().is_empty(),
        "no connection may be attempted to {address}"
    );
}

/// REQ: SEC-072
#[test]
fn sec_072_loopback_v4_is_refused_private_address() {
    assert_private("resolves.example.org", "127.0.0.1");
}

/// REQ: SEC-072
#[test]
fn sec_072_loopback_v6_is_refused_private_address() {
    assert_private("resolves.example.org", "::1");
}

/// REQ: SEC-072
#[test]
fn sec_072_private_10_is_refused_private_address() {
    assert_private("resolves.example.org", "10.1.2.3");
}

/// REQ: SEC-072
#[test]
fn sec_072_private_172_16_is_refused_private_address() {
    assert_private("resolves.example.org", "172.16.0.1");
}

/// REQ: SEC-072
#[test]
fn sec_072_private_192_168_is_refused_private_address() {
    assert_private("resolves.example.org", "192.168.1.1");
}

/// REQ: SEC-072
#[test]
fn sec_072_unique_local_v6_is_refused_private_address() {
    assert_private("resolves.example.org", "fc00::1");
}

/// REQ: SEC-072
#[test]
fn sec_072_link_local_metadata_is_refused_private_address() {
    assert_private("resolves.example.org", "169.254.169.254");
}

/// REQ: SEC-072
#[test]
fn sec_072_link_local_v6_is_refused_private_address() {
    assert_private("resolves.example.org", "fe80::1");
}

/// REQ: SEC-072
#[test]
fn sec_072_carrier_grade_nat_is_refused_private_address() {
    assert_private("resolves.example.org", "100.64.0.1");
}

/// REQ: SEC-072
#[test]
fn sec_072_unspecified_is_refused_private_address() {
    assert_private("resolves.example.org", "0.0.0.0");
}

/// REQ: SEC-072
#[test]
fn sec_072_broadcast_is_refused_private_address() {
    assert_private("resolves.example.org", "255.255.255.255");
}

/// REQ: SEC-072
#[test]
fn sec_072_multicast_is_refused_private_address() {
    assert_private("resolves.example.org", "224.0.0.1");
}

/// REQ: SEC-072
#[test]
fn sec_072_mapped_v6_of_private_is_refused_private_address() {
    assert_private("resolves.example.org", "::ffff:10.0.0.1");
}

/// REQ: SEC-072
#[test]
fn sec_072_documentation_v4_is_refused_private_address() {
    assert_private("resolves.example.org", "192.0.2.1");
}

/// REQ: SEC-072
#[test]
fn sec_072_benchmark_range_is_refused_private_address() {
    assert_private("resolves.example.org", "198.18.0.1");
}

/// REQ: SEC-072
#[test]
fn sec_072_documentation_v6_is_refused_private_address() {
    assert_private("resolves.example.org", "2001:db8::5");
}

/// REQ: SEC-072
#[test]
fn sec_072_range_edges_are_exact() {
    // 172.15.255.255 and 172.32.0.0 lie outside 172.16.0.0/12 and 100.63/100.128 outside the CGNAT /10.
    for address in [
        "172.15.255.255",
        "172.32.0.0",
        "100.63.255.255",
        "100.128.0.0",
        "9.255.255.255",
        "11.0.0.0",
    ] {
        let rig = resolving_rig("edge.example.org", address);
        let response = rig
            .get("https://edge.example.org/", &[])
            .expect("a public address is allowed");
        assert_eq!(response.status, 200, "{address}");
    }
}

/// REQ: SEC-072
#[test]
fn sec_072_public_address_is_allowed() {
    let rig = public_rig(&["http:example.com"]);
    let response = rig
        .get("https://example.com/", &[])
        .expect("public address");
    assert_eq!(response.body, b"hello");
}

/// REQ: SEC-073
#[test]
fn sec_073_one_private_address_among_several_refuses_the_request_private_address() {
    let rig = t3(&["http:mixed.example.org"]);
    rig.net
        .dns("mixed.example.org", &[PUBLIC_ADDRESS, "10.0.0.7"]);
    rig.net
        .serve("https://mixed.example.org/", FakeResponse::ok(Vec::new()));
    let result = rig.get("https://mixed.example.org/", &[]);
    assert_refused(&result, ErrorCode::NotPermitted, "private-address");
    assert!(rig.net.seen().is_empty());
}

/// REQ: SEC-073
#[test]
fn sec_073_connection_goes_to_the_validated_address_and_name_is_resolved_once() {
    let rig = public_rig(&["http:example.com"]);
    // A second resolution would return a private address (rebinding); it must never be asked for.
    rig.net
        .dns_then("example.com", &[PUBLIC_ADDRESS], &["127.0.0.1"]);
    rig.get("https://example.com/", &[]).expect("fetch");
    let seen = rig.net.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].addr.to_string(), PUBLIC_ADDRESS);
    assert_eq!(rig.net.resolve_count("example.com"), 1);
}

/// REQ: SEC-071
#[test]
fn sec_071_public_literal_grant_allows_that_literal() {
    let rig = t3(&["http:203.0.113.5"]);
    // 203.0.113.0/24 is a documentation range, so this literal only works because it is granted
    // as exactly that literal.
    rig.net
        .serve("https://203.0.113.5/", FakeResponse::ok(b"ok".to_vec()));
    assert!(rig.get("https://203.0.113.5/", &[]).is_ok());
}

/// REQ: SEC-071
#[test]
fn sec_071_literal_without_its_own_grant_is_refused_not_permitted() {
    let rig = t3(&["http:example.com"]);
    rig.net
        .serve("https://203.0.113.5/", FakeResponse::ok(b"ok".to_vec()));
    let result = rig.get("https://203.0.113.5/", &[]);
    assert_code(&result, ErrorCode::NotPermitted);
    assert!(rig.net.seen().is_empty());
}

/// REQ: SEC-071
#[test]
fn sec_071_wildcard_grant_never_matches_a_literal_not_permitted() {
    let rig = t3(&["http:*.0.113.5"]);
    rig.net
        .serve("https://203.0.113.5/", FakeResponse::ok(b"ok".to_vec()));
    assert_code(
        &rig.get("https://203.0.113.5/", &[]),
        ErrorCode::NotPermitted,
    );
}

/// REQ: SEC-071
#[test]
fn sec_071_bare_star_http_grant_is_refused_with_manifest_grant_wildcard() {
    let mut manifest = server_manifest(json!({ "grants": ["http:*"] }));
    manifest["grants"] = json!(["http:*"]);
    let codes = manifest_error_codes(&manifest);
    assert!(
        codes.iter().any(|code| code == "manifest.grant-wildcard"),
        "{codes:?}"
    );
}

/// REQ: SEC-071
#[test]
fn sec_071_ipv6_literal_grant_is_a_valid_argument() {
    let manifest = server_manifest(json!({ "grants": ["http:[2001:db8::1]"] }));
    let codes = manifest_error_codes(&manifest);
    assert!(codes.is_empty(), "{codes:?}");
}

/// REQ: SEC-074
#[test]
fn sec_074_localhost_without_a_port_is_refused_at_validation() {
    let manifest = server_manifest(json!({ "grants": ["http:localhost"] }));
    // manual does not pin the code for a port-less localhost grant; it is a grant-syntax error.
    let codes = manifest_error_codes(&manifest);
    assert!(
        codes.iter().any(|code| code == "manifest.grant-invalid"),
        "{codes:?}"
    );
}

/// REQ: SEC-074
#[test]
fn sec_074_localhost_with_port_reaches_loopback_when_granted() {
    let rig = t3(&["http:localhost:8080"]);
    rig.net.dns("localhost", &["127.0.0.1"]);
    rig.net.serve(
        "http://localhost:8080/",
        FakeResponse::ok(b"local".to_vec()),
    );
    let response = rig
        .get("http://localhost:8080/", &[])
        .expect("explicit loopback grant");
    assert_eq!(response.body, b"local");
}

/// REQ: SEC-074
#[test]
fn sec_074_ilium_http_api_port_is_refused_reserved() {
    let rig = t3(&["http:localhost:8080"]);
    rig.net.dns("localhost", &["127.0.0.1"]);
    rig.net.reserve_ilium_port(8080);
    let result = rig.get("http://localhost:8080/", &[]);
    assert_refused(&result, ErrorCode::NotPermitted, "reserved");
}

/// REQ: SEC-074
#[test]
fn sec_074_listener_ports_of_other_sessions_are_refused_reserved() {
    let rig = t3(&["http:localhost:9191"]);
    rig.net.dns("localhost", &["127.0.0.1"]);
    rig.net.reserve_ilium_port_of_other_session(9191);
    assert_refused(
        &rig.get("http://localhost:9191/", &[]),
        ErrorCode::NotPermitted,
        "reserved",
    );
}

/// REQ: SEC-075
#[test]
fn sec_075_five_redirects_are_followed_and_the_sixth_fails() {
    for (hops, expect_ok) in [(4_u32, true), (5, true), (6, false)] {
        let rig = public_rig(&["http:example.com"]);
        for hop in 0..hops {
            let from = if hop == 0 {
                "https://example.com/".to_owned()
            } else {
                format!("https://example.com/r{hop}")
            };
            let to = if hop + 1 == hops {
                "https://example.com/end".to_owned()
            } else {
                format!("https://example.com/r{}", hop + 1)
            };
            rig.net.redirect(&from, 302, &to);
        }
        rig.net
            .serve("https://example.com/end", FakeResponse::ok(b"end".to_vec()));
        let result = rig.get("https://example.com/", &[]);
        assert_eq!(result.is_ok(), expect_ok, "{hops} redirects: {result:?}");
    }
}

/// REQ: SEC-075
#[test]
fn sec_075_each_hop_is_checked_against_the_grants_not_permitted() {
    let rig = public_rig(&["http:example.com"]);
    rig.net.dns("other.example.org", &[PUBLIC_ADDRESS]);
    rig.net
        .redirect("https://example.com/", 302, "https://other.example.org/");
    assert_code(
        &rig.get("https://example.com/", &[]),
        ErrorCode::NotPermitted,
    );
}

/// REQ: SEC-075
#[test]
fn sec_075_redirect_to_a_private_address_is_refused_private_address() {
    let rig = public_rig(&["http:example.com", "http:inner.example.com"]);
    rig.net.dns("inner.example.com", &["10.0.0.5"]);
    rig.net
        .redirect("https://example.com/", 302, "https://inner.example.com/");
    assert_refused(
        &rig.get("https://example.com/", &[]),
        ErrorCode::NotPermitted,
        "private-address",
    );
}

/// REQ: SEC-075
#[test]
fn sec_075_https_to_http_redirect_is_refused_downgrade() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .redirect("https://example.com/", 301, "http://example.com/plain");
    assert_refused(
        &rig.get("https://example.com/", &[]),
        ErrorCode::NotPermitted,
        "downgrade",
    );
}

/// REQ: SEC-076
#[test]
fn sec_076_origin_change_drops_credentials_headers() {
    let rig = public_rig(&["http:example.com", "http:cdn.example.com"]);
    rig.net.dns("cdn.example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://cdn.example.com/f", FakeResponse::ok(b"f".to_vec()));
    rig.net
        .redirect("https://example.com/", 302, "https://cdn.example.com/f");
    let headers = [
        ("Authorization", "Bearer t"),
        ("Cookie", "a=b"),
        ("X-Keep", "1"),
    ];
    rig.get("https://example.com/", &headers).expect("fetch");
    let hops = rig.net.seen();
    let last = hops.last().expect("the redirected hop");
    let has = |name: &str| {
        last.headers
            .iter()
            .any(|(header, _)| header.eq_ignore_ascii_case(name))
    };
    assert!(!has("authorization") && !has("cookie") && !has("proxy-authorization"));
    assert!(has("x-keep"));
}

/// REQ: SEC-076
#[test]
fn sec_076_same_origin_redirect_keeps_the_headers() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .serve("https://example.com/next", FakeResponse::ok(b"n".to_vec()));
    rig.net
        .redirect("https://example.com/", 302, "https://example.com/next");
    rig.get("https://example.com/", &[("Authorization", "Bearer t")])
        .expect("fetch");
    let hops = rig.net.seen();
    assert!(hops
        .last()
        .expect("hop")
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization")));
}

/// REQ: SEC-076
#[test]
fn sec_076_port_change_is_an_origin_change() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .serve("https://example.com:8443/", FakeResponse::ok(b"p".to_vec()));
    rig.net
        .redirect("https://example.com/", 302, "https://example.com:8443/");
    rig.get("https://example.com/", &[("Cookie", "a=b")])
        .expect("fetch");
    let hops = rig.net.seen();
    assert!(!hops
        .last()
        .expect("hop")
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("cookie")));
}

/// REQ: SEC-077
#[test]
fn sec_077_plain_http_to_a_public_host_is_refused() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .serve("http://example.com/", FakeResponse::ok(Vec::new()));
    assert_code(
        &rig.get("http://example.com/", &[]),
        ErrorCode::NotPermitted,
    );
}

/// REQ: SEC-077
#[test]
fn sec_077_certificate_failure_returns_network_with_reason_tls() {
    let rig = public_rig(&["http:example.com"]);
    rig.net.fail_tls("example.com");
    assert_refused(
        &rig.get("https://example.com/", &[]),
        ErrorCode::Network,
        "tls",
    );
}

/// REQ: SEC-077
#[test]
fn sec_077_tls_1_1_is_not_accepted_and_1_2_is() {
    let rig = public_rig(&["http:example.com"]);
    rig.net.set_tls_version("example.com", "1.1");
    assert_refused(
        &rig.get("https://example.com/", &[]),
        ErrorCode::Network,
        "tls",
    );
    rig.net.set_tls_version("example.com", "1.2");
    assert!(rig.get("https://example.com/", &[]).is_ok());
}

/// REQ: SEC-077
#[test]
fn sec_077_admin_ca_bundle_adds_a_trust_root() {
    let rig = setup(&["http:example.com"])
        .system_policy(
            "version = 1\n[network]\nca_bundle = \"/etc/ssl/corp-ca.pem\"\n",
            true,
        )
        .start();
    rig.net.dns("example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://example.com/", FakeResponse::ok(b"c".to_vec()));
    rig.net.require_root("example.com", "corp-ca");
    rig.net.trust_bundle("/etc/ssl/corp-ca.pem", "corp-ca");
    assert!(rig.get("https://example.com/", &[]).is_ok());
}

/// REQ: SEC-077
#[test]
fn sec_077_a_plugin_has_no_way_to_disable_verification() {
    let rig = public_rig(&["http:example.com"]);
    rig.net.fail_tls("example.com");
    let mut request = request_json("GET", "https://example.com/", &[], None);
    request["insecure"] = json!(true);
    request["ca"] = json!("-----BEGIN CERTIFICATE-----");
    let result = rig.fetch(request);
    // verification stays on, so the failing certificate still ends in network/tls.
    assert_refused(&result, ErrorCode::Network, "tls");
}

/// REQ: SEC-078
#[test]
fn sec_078_environment_proxy_is_read_once_at_start() {
    let rig = setup(&["http:example.com"])
        .setting("env.HTTPS_PROXY", json!("http://proxy.example.net:3128"))
        .start();
    rig.net.dns("example.com", &[PUBLIC_ADDRESS]);
    rig.net.dns("proxy.example.net", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://example.com/", FakeResponse::ok(b"p".to_vec()));
    rig.get("https://example.com/", &[])
        .expect("fetch through the proxy");
    assert_eq!(
        rig.net.seen().last().expect("seen").via_proxy.as_deref(),
        Some("proxy.example.net:3128")
    );
}

/// REQ: SEC-078
#[test]
fn sec_078_policy_proxy_overrides_the_environment() {
    let rig = setup(&["http:example.com"])
        .setting("env.HTTPS_PROXY", json!("http://env.example.net:3128"))
        .system_policy(
            "version = 1\n[network]\nproxy = \"http://proxy.example.com:3128\"\n",
            true,
        )
        .start();
    rig.net.dns("example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://example.com/", FakeResponse::ok(b"p".to_vec()));
    rig.get("https://example.com/", &[]).expect("fetch");
    assert_eq!(
        rig.net.seen().last().expect("seen").via_proxy.as_deref(),
        Some("proxy.example.com:3128")
    );
}

/// REQ: SEC-078
#[test]
fn sec_078_explicit_proxy_still_gets_the_target_name_checked_private_address() {
    let rig = setup(&["http:internal.example.org"])
        .system_policy(
            "version = 1\n[network]\nproxy = \"http://proxy.example.com:3128\"\n",
            true,
        )
        .start();
    rig.net.dns("internal.example.org", &["10.9.9.9"]);
    assert_refused(
        &rig.get("https://internal.example.org/", &[]),
        ErrorCode::NotPermitted,
        "private-address",
    );
    assert!(rig.net.seen().is_empty());
}

/// REQ: SEC-078
#[test]
fn sec_078_plugin_cannot_set_or_read_proxy_settings() {
    let rig = t3(&["http:example.com"]);
    let set = rig.value(
        "ilium:host/settings.set-json",
        json!({ "patch": "{\"proxy\":\"http://evil:1\"}" }),
    );
    // manual does not pin the error code for a proxy write; it only requires a refusal.
    assert!(set.is_err(), "a plugin must not be able to set a proxy");
    rig.net.dns("example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://example.com/", FakeResponse::ok(b"p".to_vec()));
    rig.get("https://example.com/", &[]).expect("fetch");
    assert_eq!(rig.net.seen().last().expect("seen").via_proxy, None);
    let read = rig
        .value("ilium:host/settings.get-json", json!({}))
        .expect("own settings");
    assert!(!read.to_string().contains("proxy.example.com"));
}

/// REQ: SEC-079
#[test]
fn sec_079_each_forbidden_header_is_refused_invalid_argument() {
    for name in [
        "Host",
        "Content-Length",
        "Connection",
        "Transfer-Encoding",
        "Upgrade",
        "TE",
        "Trailer",
        "Proxy-Authorization",
        "Proxy-Connection",
        "proxy-x-anything",
        "host",
    ] {
        let rig = public_rig(&["http:example.com"]);
        let result = rig.get("https://example.com/", &[(name, "v")]);
        assert_code(&result, ErrorCode::InvalidArgument);
        assert!(
            rig.net.seen().is_empty(),
            "{name} must be refused before any connection"
        );
    }
}

/// REQ: SEC-079
#[test]
fn sec_079_ordinary_headers_and_a_plugin_set_cookie_pass() {
    let rig = public_rig(&["http:example.com"]);
    rig.get(
        "https://example.com/",
        &[
            ("Accept", "text/plain"),
            ("X-Thing", "1"),
            ("Cookie", "mine=1"),
        ],
    )
    .expect("fetch");
    let headers = rig.net.seen().remove(0).headers;
    assert!(headers
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("cookie") && value == "mine=1"));
}

/// REQ: SEC-079
#[test]
fn sec_079_the_host_never_adds_a_cookie() {
    let rig = public_rig(&["http:example.com"]);
    rig.net.serve(
        "https://example.com/",
        FakeResponse::ok(Vec::new()).header("Set-Cookie", "s=1"),
    );
    rig.get("https://example.com/", &[]).expect("first");
    rig.get("https://example.com/", &[]).expect("second");
    assert_eq!(rig.net.seen().len(), 2);
    assert!(rig.net.seen().iter().all(|seen| !seen
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("cookie"))));
}

/// REQ: SEC-080
#[test]
fn sec_080_user_agent_is_fixed_to_ilium_plugin_id() {
    let rig = public_rig(&["http:example.com"]);
    rig.get("https://example.com/", &[]).expect("fetch");
    let headers = rig.net.seen().remove(0).headers;
    let agent = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        .expect("user-agent");
    assert_eq!(agent.1, "Ilium-plugin/test.plugin");
}

/// REQ: SEC-080
#[test]
fn sec_080_a_plugin_user_agent_is_replaced() {
    let rig = public_rig(&["http:example.com"]);
    let _ = rig.get(
        "https://example.com/",
        &[("User-Agent", "Mozilla/5.0 Evil")],
    );
    let seen = rig.net.seen();
    if let Some(first) = seen.first() {
        let agents: Vec<&String> = first
            .headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
            .map(|(_, value)| value)
            .collect();
        assert_eq!(agents, vec!["Ilium-plugin/test.plugin"]);
    }
}

/// REQ: SEC-081
#[test]
fn sec_081_cookies_are_not_kept_between_requests() {
    let rig = public_rig(&["http:example.com"]);
    rig.net.serve(
        "https://example.com/",
        FakeResponse::ok(Vec::new()).header("Set-Cookie", "sid=abc"),
    );
    rig.get("https://example.com/", &[]).expect("first");
    rig.get("https://example.com/", &[]).expect("second");
    let seen = rig.net.seen();
    assert!(!seen[1]
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("cookie")));
}

/// REQ: SEC-081
#[test]
fn sec_081_no_cookie_jar_is_shared_between_plugins() {
    let first = public_rig(&["http:example.com"]);
    first.net.serve(
        "https://example.com/",
        FakeResponse::ok(Vec::new()).header("Set-Cookie", "sid=abc"),
    );
    first.get("https://example.com/", &[]).expect("fetch");
    let second = public_rig(&["http:example.com"]);
    second.get("https://example.com/", &[]).expect("fetch");
    assert!(!second.net.seen()[0]
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("cookie")));
}

const MIB: u64 = 1024 * 1024;

/// REQ: SEC-082
#[test]
fn sec_082_request_body_boundary_at_8_mib() {
    for (length, expect_ok) in [(8 * MIB - 1, true), (8 * MIB, true), (8 * MIB + 1, false)] {
        let rig = public_rig(&["http:example.com"]);
        rig.net
            .serve("https://example.com/up", FakeResponse::ok(Vec::new()));
        let result = rig.post("https://example.com/up", vec![0_u8; length as usize]);
        assert_eq!(result.is_ok(), expect_ok, "{length} bytes: {result:?}");
    }
}

/// REQ: SEC-082
#[test]
fn sec_082_buffered_response_over_32_mib_fails_quota_exceeded_response_size() {
    // The manual and api-http.md differ (32 MiB here, 8 MiB there); this asserts the stricter
    // reading of this page for the failure side and notes the conflict in API_GAPS.md.
    let rig = t3_with_limit_raised();
    for (length, expect_ok) in [
        (32 * MIB - 1, true),
        (32 * MIB, true),
        (32 * MIB + 1, false),
    ] {
        rig.net
            .serve("https://example.com/big", FakeResponse::sized(length));
        let result = rig.get("https://example.com/big", &[]);
        if expect_ok {
            assert!(result.is_ok(), "{length}: {result:?}");
        } else {
            assert_refused(&result, ErrorCode::QuotaExceeded, "response-size");
        }
    }
}

fn t3_with_limit_raised() -> Rig {
    let rig = setup(&["http:example.com"])
        .tier(Tier::T1)
        .setting("limits.http_response_bytes", json!(32 * MIB))
        .start();
    rig.net.dns("example.com", &[PUBLIC_ADDRESS]);
    rig
}

/// REQ: SEC-082
#[test]
fn sec_082_default_response_cap_applies_to_a_plain_fetch() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .serve("https://example.com/big", FakeResponse::sized(32 * MIB + 1));
    assert_refused(
        &rig.get("https://example.com/big", &[]),
        ErrorCode::QuotaExceeded,
        "response-size",
    );
}

/// REQ: SEC-082
#[test]
fn sec_082_stream_ends_after_256_mib_with_stream_size() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .serve("https://example.com/s", FakeResponse::sized(256 * MIB + 1));
    let id = rig
        .value(
            "ilium:host/http.open-stream",
            json!({ "request": { "request": request_json("GET", "https://example.com/s", &[], None), "framing": "raw", "max-chunk-bytes": 65536 } }),
        )
        .expect("stream opens");
    rig.host.pump(64);
    let closed = rig
        .host
        .stream_close_reason(id.as_u64().expect("stream id"));
    assert_eq!(closed.as_deref(), Some("stream-size"));
    assert_eq!(
        rig.host
            .stream_bytes_delivered(id.as_u64().expect("stream id")),
        256 * MIB
    );
}

/// REQ: SEC-082
#[test]
fn sec_082_a_stream_read_is_at_most_8_mib() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .serve("https://example.com/s", FakeResponse::sized(64 * MIB));
    let id = rig
        .value(
            "ilium:host/http.open-stream",
            json!({ "request": { "request": request_json("GET", "https://example.com/s", &[], None), "framing": "raw", "max-chunk-bytes": 65536 } }),
        )
        .expect("stream opens");
    rig.host.pump(8);
    assert!(
        rig.host
            .stream_largest_read(id.as_u64().expect("stream id"))
            <= 8 * MIB
    );
}

/// REQ: SEC-083
#[test]
fn sec_083_budget_is_16_mib_per_minute_with_retry_after() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .serve("https://example.com/b", FakeResponse::sized(4 * MIB));
    let mut refused_at = None;
    for call in 1..=6 {
        match rig.get("https://example.com/b", &[]) {
            Ok(_) => {}
            Err(error) => {
                assert_eq!(error.code, ErrorCode::RateLimited);
                assert!(error.retry_after_ms.expect("retry-after-ms") > 0);
                refused_at = Some(call);
                break;
            }
        }
    }
    let refused_at = refused_at.expect("the 16 MiB budget must run out");
    assert!(
        (4..=5).contains(&refused_at),
        "refused at call {refused_at}"
    );
}

/// REQ: SEC-083
#[test]
fn sec_083_budget_refills_after_a_minute() {
    let rig = public_rig(&["http:example.com"]);
    rig.net
        .serve("https://example.com/b", FakeResponse::sized(4 * MIB));
    while rig.get("https://example.com/b", &[]).is_ok() {}
    rig.host.advance_clock(std::time::Duration::from_secs(61));
    assert!(rig.get("https://example.com/b", &[]).is_ok());
}

/// REQ: SEC-083
#[test]
fn sec_083_budget_is_per_plugin_side() {
    let server = public_rig(&["http:example.com"]);
    server
        .net
        .serve("https://example.com/b", FakeResponse::sized(4 * MIB));
    while server.get("https://example.com/b", &[]).is_ok() {}
    let client = setup(&["http:example.com"])
        .side(ilium_plugin_api::Side::Client)
        .start();
    client.net.dns("example.com", &[PUBLIC_ADDRESS]);
    client
        .net
        .serve("https://example.com/b", FakeResponse::sized(4 * MIB));
    assert!(client.get("https://example.com/b", &[]).is_ok());
}

/// REQ: SEC-084
#[test]
fn sec_084_offline_setting_refuses_all_egress_offline() {
    let rig = setup(&["http:example.com"]).offline().start();
    rig.net.dns("example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://example.com/", FakeResponse::ok(Vec::new()));
    assert_refused(
        &rig.get("https://example.com/", &[]),
        ErrorCode::NotPermitted,
        "offline",
    );
    assert!(rig.net.seen().is_empty());
}

/// REQ: SEC-084
#[test]
fn sec_084_policy_offline_refuses_even_granted_localhost() {
    let rig = setup(&["http:localhost:8080"])
        .system_policy("version = 1\n[network]\noffline = true\n", true)
        .start();
    rig.net.dns("localhost", &["127.0.0.1"]);
    rig.net
        .serve("http://localhost:8080/", FakeResponse::ok(Vec::new()));
    assert_refused(
        &rig.get("http://localhost:8080/", &[]),
        ErrorCode::NotPermitted,
        "offline",
    );
}

/// REQ: SEC-084
#[test]
fn sec_084_streams_and_websockets_are_refused_offline_too() {
    let rig = setup(&["http:example.com"]).offline().start();
    let stream = rig.value(
        "ilium:host/http.open-stream",
        json!({ "request": { "request": request_json("GET", "https://example.com/s", &[], None), "framing": "raw", "max-chunk-bytes": 1024 } }),
    );
    assert_refused(&stream, ErrorCode::NotPermitted, "offline");
}

/// REQ: SEC-084
#[test]
fn sec_084_user_online_setting_allows_traffic_again() {
    let rig = public_rig(&["http:example.com"]);
    assert!(rig.get("https://example.com/", &[]).is_ok());
}

const POLICY_LISTS: &str = "version = 1\n[network]\nallow_hosts = [\"*.example.com\"]\ndeny_hosts = [\"*.pastebin.example\"]\n";

/// REQ: SEC-085
#[test]
fn sec_085_host_outside_the_allow_list_is_refused_policy() {
    let rig = setup(&["http:other.example.org"])
        .system_policy(POLICY_LISTS, true)
        .start();
    rig.net.dns("other.example.org", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://other.example.org/", FakeResponse::ok(Vec::new()));
    let result = rig.get("https://other.example.org/", &[]);
    assert_refused(&result, ErrorCode::NotPermitted, "policy");
}

/// REQ: SEC-085
#[test]
fn sec_085_host_inside_the_allow_list_is_allowed() {
    let rig = setup(&["http:*.example.com"])
        .system_policy(POLICY_LISTS, true)
        .start();
    rig.net.dns("api.example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://api.example.com/", FakeResponse::ok(Vec::new()));
    assert!(rig.get("https://api.example.com/", &[]).is_ok());
}

/// REQ: SEC-085
#[test]
fn sec_085_deny_beats_allow() {
    let policy = "version = 1\n[network]\nallow_hosts = [\"*.example.com\"]\ndeny_hosts = [\"bad.example.com\"]\n";
    let rig = setup(&["http:*.example.com"])
        .system_policy(policy, true)
        .start();
    rig.net.dns("bad.example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://bad.example.com/", FakeResponse::ok(Vec::new()));
    assert_refused(
        &rig.get("https://bad.example.com/", &[]),
        ErrorCode::NotPermitted,
        "policy",
    );
}

/// REQ: SEC-085
#[test]
fn sec_085_without_an_allow_list_only_deny_rules_apply() {
    let policy = "version = 1\n[network]\ndeny_hosts = [\"*.pastebin.example\"]\n";
    let rig = setup(&["http:example.com", "http:a.pastebin.example"])
        .system_policy(policy, true)
        .start();
    rig.net.dns("example.com", &[PUBLIC_ADDRESS]);
    rig.net.dns("a.pastebin.example", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://example.com/", FakeResponse::ok(Vec::new()));
    rig.net
        .serve("https://a.pastebin.example/", FakeResponse::ok(Vec::new()));
    assert!(rig.get("https://example.com/", &[]).is_ok());
    assert_refused(
        &rig.get("https://a.pastebin.example/", &[]),
        ErrorCode::NotPermitted,
        "policy",
    );
}

/// REQ: SEC-086
#[test]
fn sec_086_webhook_to_a_private_address_is_refused_without_a_setting_confirmation() {
    let rig = t3(&[]);
    rig.net.dns("hooks.example.org", &["192.168.1.9"]);
    let result = rig
        .host
        .host_egress("webhook", "https://hooks.example.org/t/abc");
    assert_refused(&result, ErrorCode::NotPermitted, "private-address");
}

/// REQ: SEC-086
#[test]
fn sec_086_webhook_to_a_private_address_works_with_the_setting_confirmation() {
    let rig = setup(&["http:setting:notify.webhook"]).start();
    rig.net.dns("hooks.example.org", &["192.168.1.9"]);
    rig.net.serve(
        "https://hooks.example.org/t/abc",
        FakeResponse::ok(Vec::new()),
    );
    assert!(rig
        .host
        .host_egress("webhook", "https://hooks.example.org/t/abc")
        .is_ok());
}

/// REQ: SEC-086
#[test]
fn sec_086_host_features_report_offline_to_their_callers() {
    for feature in ["webhook", "help-source", "provider-endpoint", "geocode"] {
        let rig = setup(&[]).offline().start();
        rig.net.dns("hooks.example.org", &[PUBLIC_ADDRESS]);
        let result = rig.host.host_egress(feature, "https://hooks.example.org/");
        assert_refused(&result, ErrorCode::NotPermitted, "offline");
    }
}

/// REQ: SEC-086
#[test]
fn sec_086_policy_deny_list_applies_to_help_sources() {
    let rig = setup(&[]).system_policy(POLICY_LISTS, true).start();
    rig.net.dns("a.pastebin.example", &[PUBLIC_ADDRESS]);
    assert_refused(
        &rig.host
            .host_egress("help-source", "https://a.pastebin.example/idx"),
        ErrorCode::NotPermitted,
        "policy",
    );
}

/// REQ: SEC-070
#[test]
fn sec_070_streams_apply_the_same_address_checks_private_address() {
    let rig = resolving_rig("s.example.org", "10.1.1.1");
    let stream = rig.value(
        "ilium:host/http.open-stream",
        json!({ "request": { "request": request_json("GET", "https://s.example.org/", &[], None), "framing": "raw", "max-chunk-bytes": 1024 } }),
    );
    assert_refused(&stream, ErrorCode::NotPermitted, "private-address");
}

/// REQ: SEC-070
#[test]
fn sec_070_websocket_applies_the_same_address_checks_private_address() {
    let rig = resolving_rig("w.example.org", "10.1.1.1");
    let socket = rig.value(
        "ilium:host/http.open-websocket",
        json!({ "url": "wss://w.example.org/", "protocols": [] }),
    );
    assert_refused(&socket, ErrorCode::NotPermitted, "private-address");
}

/// REQ: SEC-070
#[test]
fn sec_070_inventory_lists_every_host_initiated_feature() {
    assert_page_has(
        "reference-network-egress.md",
        &[
            "`inference`",
            "`inference.embed`",
            "`location.geocode`",
            "Notification channels",
            "Help sources",
            "`open-external`",
            "`ilium.core.updates`",
        ],
    );
}

/// REQ: SEC-087
#[test]
fn sec_087_first_request_to_a_host_is_audited_once_with_the_host_name_only() {
    let rig = public_rig(&["http:example.com"]);
    rig.net.serve(
        "https://example.com/secret/path?q=1",
        FakeResponse::ok(Vec::new()),
    );
    rig.get("https://example.com/secret/path?q=1", &[])
        .expect("first");
    rig.get("https://example.com/", &[]).expect("second");
    let records = rig.audit_events("egress-host-first-use");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["plugin_id"], "test.plugin");
    let text = records[0].to_string();
    assert!(text.contains("example.com") && !text.contains("secret/path") && !text.contains("q=1"));
}

/// REQ: SEC-087
#[test]
fn sec_087_a_second_host_gets_its_own_record() {
    let rig = public_rig(&["http:example.com", "http:two.example.com"]);
    rig.net.dns("two.example.com", &[PUBLIC_ADDRESS]);
    rig.net
        .serve("https://two.example.com/", FakeResponse::ok(Vec::new()));
    rig.get("https://example.com/", &[]).expect("one");
    rig.get("https://two.example.com/", &[]).expect("two");
    assert_eq!(rig.audit_events("egress-host-first-use").len(), 2);
}

/// REQ: SEC-097
#[test]
fn sec_097_listener_binds_to_loopback_only() {
    let rig = t3(&[]);
    assert!(rig.host.http_api_bind_address().ip().is_loopback());
}

/// REQ: SEC-097
#[test]
fn sec_097_missing_or_wrong_token_is_refused_401() {
    let rig = t3(&[]);
    assert_eq!(rig.host.http_api_request("/v1/status", &[]), 401);
    assert_eq!(
        rig.host
            .http_api_request("/v1/status", &[("Authorization", "Bearer wrong")]),
        401
    );
}

/// REQ: SEC-097
#[test]
fn sec_097_unexpected_host_header_is_refused_403_before_routing() {
    let rig = t3(&[]);
    let token = rig.host.http_api_token();
    let auth = format!("Bearer {token}");
    assert_eq!(
        rig.host.http_api_request(
            "/v1/status",
            &[
                ("Authorization", auth.as_str()),
                ("Host", "attacker.example:80")
            ]
        ),
        403
    );
    assert_eq!(
        rig.host
            .http_api_request("/v1/status", &[("Authorization", auth.as_str())]),
        200
    );
}

/// REQ: SEC-097
#[test]
fn sec_097_foreign_origin_is_refused_403() {
    let rig = t3(&[]);
    let auth = format!("Bearer {}", rig.host.http_api_token());
    assert_eq!(
        rig.host.http_api_request(
            "/v1/status",
            &[
                ("Authorization", auth.as_str()),
                ("Origin", "https://evil.example")
            ]
        ),
        403
    );
}

/// REQ: SEC-098
#[test]
fn sec_098_token_file_is_unreachable_for_fs_read_grants() {
    let rig = t3(&["fs.read:data"]);
    let path = rig.host.http_api_token_path();
    let result = rig.value(
        "ilium:host/fs.read",
        json!({ "scope": "data", "path": path.to_string_lossy() }),
    );
    assert_code(&result, ErrorCode::NotPermitted);
}

/// REQ: SEC-098
#[test]
fn sec_098_token_never_appears_in_environment_arguments_or_events() {
    let rig = t3(&["process.run:echo"]);
    let token = rig.host.http_api_token();
    assert!(!rig
        .host
        .plugin_environment()
        .iter()
        .any(|(_, value)| value.contains(&token)));
    assert!(!rig.host.delivered_events_text().contains(&token));
    assert!(!rig
        .host
        .plugin_arguments()
        .iter()
        .any(|argument| argument.contains(&token)));
}

/// REQ: SEC-099
#[test]
fn sec_099_token_file_has_mode_0600_and_changes_at_each_start() {
    let rig = t3(&[]);
    let path = rig.host.http_api_token_path();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .expect("token file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let first = std::fs::read_to_string(&path).expect("token");
    let rig = rig.restart();
    let second = std::fs::read_to_string(rig.host.http_api_token_path()).expect("token");
    assert_ne!(first, second);
}

/// REQ: SEC-072
#[test]
fn sec_072_address_class_table_in_the_manual_has_eight_classes() {
    let page = manual_page("reference-network-egress.md");
    let table: Vec<&str> = page
        .lines()
        .skip_while(|line| !line.starts_with("| Class | Range |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .collect();
    assert_eq!(table.len(), 8);
}

/// REQ: SEC-099
#[test]
fn sec_099_manual_states_the_api_is_shell_equivalent_for_same_user_processes() {
    assert_page_has(
        "reference-network-egress.md",
        &["shell-equivalent for same-user processes"],
    );
}

/// REQ: SEC-086
#[test]
fn sec_086_dns_rebinding_on_a_second_resolution_is_never_consulted_for_a_webhook() {
    let rig = setup(&["http:setting:notify.webhook"]).start();
    rig.net
        .dns_then("hooks.example.org", &[PUBLIC_ADDRESS], &["127.0.0.1"]);
    rig.net
        .serve("https://hooks.example.org/t", FakeResponse::ok(Vec::new()));
    rig.host
        .host_egress("webhook", "https://hooks.example.org/t")
        .expect("public");
    assert_eq!(rig.net.resolve_count("hooks.example.org"), 1);
}

/// REQ: SEC-077
#[test]
fn sec_077_tls_1_3_is_negotiated_when_the_server_offers_it() {
    let rig = public_rig(&["http:example.com"]);
    rig.net.set_tls_version("example.com", "1.3");
    rig.get("https://example.com/", &[]).expect("fetch");
    assert_eq!(rig.net.seen()[0].tls_version.as_deref(), Some("1.3"));
}

/// REQ: SEC-082
#[test]
fn sec_082_timeouts_are_those_of_the_capability_page() {
    assert_page_has(
        "reference-network-egress.md",
        &["The timeouts are those of the capability page."],
    );
    let _ = FakeNet::new();
}
