//! Security boundaries of scoped capability coverage.
use ilium_plugin_api::grants::{check_grant, info, is_high_risk, table, Grant};
use ilium_plugin_api::{Side, Tier};

fn grant(text: &str) -> Grant {
    Grant::parse(text).unwrap()
}

#[test]
fn host_wildcards_respect_domain_and_port_boundaries() {
    let held = grant("http:*.example.com");
    for allowed in ["http:api.example.com", "http:deep.api.example.com"] {
        assert!(held.covers(&grant(allowed)), "{allowed}");
    }
    for denied in [
        "http:example.com", "http:badexample.com", "http:example.com.evil.org",
        "http:api.example.com:8443", "http:*.com", "http:setting:host",
    ] {
        assert!(!held.covers(&grant(denied)), "{denied}");
    }
    assert!(grant("http:*.example.com:8443").covers(&grant("http:api.example.com:8443")));
    assert!(!grant("http:*.example.com:8443").covers(&grant("http:api.example.com")));
}

#[test]
fn filesystem_coverage_never_crosses_roots_or_access_families() {
    let held = grant("fs.read:path:{home}/Pictures/**");
    assert!(held.covers(&grant("fs.read:path:{home}/Pictures/holidays/**")));
    for denied in [
        "fs.read:path:{home}/Pictures-other/**",
        "fs.read:path:{home}/Pictures/../Secrets/**",
        "fs.read:path:{home}/Pictures/./Secrets/**",
        "fs.read:path:{home}/**",
        "fs.write:path:{home}/Pictures/**",
        "fs.read:project",
    ] {
        assert!(!held.covers(&grant(denied)), "{denied}");
    }
    assert!(!grant("fs.read:path:{home}/Pictures").covers(&grant("fs.read:path:{home}/Pictures/holidays")));
}

#[test]
fn command_and_topic_patterns_cover_only_their_namespace() {
    let commands = grant("commands.invoke:example.notes/*");
    assert!(commands.covers(&grant("commands.invoke:example.notes/save")));
    assert!(!commands.covers(&grant("commands.invoke:example.notes-other/save")));
    assert!(!commands.covers(&grant("commands.invoke:example.notes/save/all")));
    let topics = grant("topics.subscribe:example.notes/*");
    assert!(topics.covers(&grant("topics.subscribe:example.notes/events/new")));
    assert!(!topics.covers(&grant("topics.subscribe:example.notes-other/events")));
}

#[test]
fn registry_is_unique_and_unknown_families_are_refused() {
    let mut families = std::collections::BTreeSet::new();
    for entry in table() {
        assert!(families.insert(entry.family));
        assert!(std::ptr::eq(info(entry.family).unwrap(), entry));
    }
    for text in ["http.admin:example.com", "fs.read-more:project", "tree.reed"] {
        assert_eq!(Grant::parse(text).unwrap_err().code, "manifest.grant-unknown");
    }
}

#[test]
fn private_network_and_configuration_permissions_are_high_risk() {
    for text in ["http:127.0.0.1:8080", "http:[::1]:8080", "http:10.0.0.1:443", "http:[fd00::1]:443", "fs.write:agent-config:claude"] {
        assert!(is_high_risk(text), "{text}");
    }
    // Normative capability table allows configuration writes, but no watches.
    assert!(check_grant(&grant("fs.write:agent-config:claude"), Tier::T2, Side::Server).is_ok());
    assert_eq!(check_grant(&grant("fs.watch:agent-config:claude"), Tier::T2, Side::Server).unwrap_err().code, "manifest.grant-scope");
    assert_eq!(check_grant(&grant("fs.read:path:/srv/logs/**"), Tier::T2, Side::Server).unwrap_err().code, "manifest.grant-tier");
}

#[test]
fn malformed_arguments_cannot_gain_coverage_even_when_equal() {
    for text in ["http:*", "http:localhost", "fs.read:path:{home}/../**", "process.run:git*", "extensions.provide:example.notes/*"] {
        let malformed = grant(text);
        assert!(!malformed.covers(&malformed), "{text}");
    }
}
