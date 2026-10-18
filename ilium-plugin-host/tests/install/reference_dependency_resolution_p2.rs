//! Page `reference-dependency-resolution.md`, part 2: service cycles, activation order, replaces,
//! channels, pins and lock files (PKG-350 .. PKG-360, PKG-654). Expected values come from the page.

use ilium_plugin_host::lifecycle::{
    shutdown_order, start_order, StartNode, MAX_CONCURRENT_ACTIVATIONS,
};
use ilium_plugin_host::resolve::{
    resolve_plan, PlanAction, PlanPackage, ResolveInput, SourcePackage,
};
use ilium_plugin_host::testing::FakeRegistry;
use proptest::prelude::*;
use serde_json::Value;

use crate::support::*;

const FLAGS: [&str; 3] = ["--allow-unsigned", "--yes", "--registry"];

fn run_registry(fx: &Fx, args: &[&str]) -> ilium_plugin_host::cli::CliOutput {
    let mut full: Vec<&str> = args.to_vec();
    full.extend_from_slice(&FLAGS);
    full.push(REGISTRY_URL);
    fx.run(&full)
}

/// Serves a registry with the given `(spec, channel)` packages on the fixture fetcher.
fn serve_registry(fx: &Fx, packages: &[(Spec, &str)]) {
    let mut registry = FakeRegistry::new(REGISTRY_URL);
    for (spec, channel) in packages {
        registry.publish(spec.build(), channel);
    }
    for (url, body) in registry.documents() {
        fx.fetcher.serve(&url, body);
    }
}

fn b(version: &str) -> Spec {
    Spec::new("example.b", version)
}

fn reg_fx(packages: &[(&str, &str)]) -> Fx {
    let fx = Fx::new();
    let list: Vec<(Spec, &str)> = packages
        .iter()
        .map(|(version, channel)| (b(version), *channel))
        .collect();
    serve_registry(&fx, &list);
    fx
}

// ---------------------------------------------------------------- PKG-654 service cycles

/// A package that provides `<id>/<service>` at 1.0.0 and requires the listed services.
fn service_plugin(id: &str, provides: &str, requires: &[&str]) -> Spec {
    let mut spec = Spec::new(id, "1.0.0");
    for service in requires {
        spec = spec.service(&format!("{service}@1.0.0"));
    }
    spec.extra_top_level.push((
        "contributes".to_owned(),
        format!("{{\"services\": {{\"provides\": [{{\"name\": \"{id}/{provides}\", \"version\": \"1.0.0\", \"schema\": \"schema.json\"}}]}}}}"),
    ));
    spec.extra_files
        .push(("schema.json".to_owned(), b"{}".to_vec()));
    spec
}

/// REQ: PKG-654
#[test]
fn pkg_654_two_plugin_service_cycle_is_refused_service_cycle() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&service_plugin("example.a", "sa", &["example.b/sb"]))
            .exit_code,
        0
    );
    let out = fx.install(&service_plugin("example.b", "sb", &["example.a/sa"]));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("manifest.service-cycle"),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.b").is_none());
}

/// REQ: PKG-654
#[test]
fn pkg_654_cycle_message_lists_names_starting_with_the_smallest() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&service_plugin("example.b", "sb", &["example.a/sa"]))
            .exit_code,
        0
    );
    let out = fx.install(&service_plugin("example.a", "sa", &["example.b/sb"]));
    let text = out.all_text();
    let first = text.find("example.a/sa").expect("smallest name listed");
    let second = text.find("example.b/sb").expect("other name listed");
    assert!(first < second, "{text}");
}

/// REQ: PKG-654
#[test]
fn pkg_654_three_service_cycle_lists_all_names_smallest_first() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&service_plugin("example.x", "sb", &["example.y/sc"]))
            .exit_code,
        0
    );
    assert_eq!(
        fx.install(&service_plugin("example.y", "sc", &["example.z/sa"]))
            .exit_code,
        0
    );
    let out = fx.install(&service_plugin("example.z", "sa", &["example.x/sb"]));
    assert!(
        out.has_error("manifest.service-cycle"),
        "{:?}",
        out.error_codes()
    );
    let text = out.all_text();
    let smallest = text.find("example.x/sb").expect("example.x/sb");
    assert!(
        smallest < text.find("example.y/sc").expect("example.y/sc"),
        "{text}"
    );
    assert!(
        smallest < text.find("example.z/sa").expect("example.z/sa"),
        "{text}"
    );
}

/// REQ: PKG-654
#[test]
fn pkg_654_service_chain_without_a_cycle_installs() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&service_plugin("example.a", "sa", &["example.b/sb"]))
            .exit_code,
        0
    );
    assert_eq!(
        fx.install(&service_plugin("example.b", "sb", &[]))
            .exit_code,
        0
    );
}

/// REQ: PKG-654
#[test]
fn pkg_654_cycle_inside_one_package_is_refused() {
    let fx = Fx::new();
    let out = fx.install(&service_plugin("example.a", "sa", &["example.a/sa"]));
    assert!(
        out.has_error("manifest.service-cycle"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-654
#[test]
fn pkg_654_check_reports_the_cycle_at_stage_10() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&service_plugin("example.a", "sa", &["example.b/sb"]))
            .exit_code,
        0
    );
    let spec = service_plugin("example.b", "sb", &["example.a/sa"]);
    let path = fx.write_file("b.iliumpkg", &spec.build());
    let out = fx.run(&["install", &path, "--allow-unsigned", "--yes", "--dry-run"]);
    assert!(
        out.has_error("manifest.service-cycle"),
        "{:?}",
        out.error_codes()
    );
}

// ---------------------------------------------------------------- PKG-350 / PKG-351 order

fn node(id: &str, tier: u8, plugins: &[&str], providers: &[&str]) -> StartNode {
    StartNode {
        id: id.to_owned(),
        tier,
        requires_plugins: plugins.iter().map(|text| (*text).to_owned()).collect(),
        requires_service_providers: providers.iter().map(|text| (*text).to_owned()).collect(),
    }
}

/// REQ: PKG-350
#[test]
fn pkg_350_independent_nodes_start_by_tier_then_id() {
    let nodes = vec![
        node("b", 1, &[], &[]),
        node("a", 2, &[], &[]),
        node("a2", 1, &[], &[]),
    ];
    assert_eq!(start_order(&nodes).expect("order"), vec!["a2", "b", "a"]);
}

/// REQ: PKG-350
#[test]
fn pkg_350_same_tier_uses_byte_order_of_the_id() {
    let nodes = vec![
        node("b", 1, &[], &[]),
        node("B", 1, &[], &[]),
        node("a", 1, &[], &[]),
    ];
    assert_eq!(start_order(&nodes).expect("order"), vec!["B", "a", "b"]);
}

/// REQ: PKG-350
#[test]
fn pkg_350_plugin_dependency_starts_before_its_consumer_even_in_a_later_tier() {
    let nodes = vec![node("a", 1, &["z"], &[]), node("z", 2, &[], &[])];
    assert_eq!(start_order(&nodes).expect("order"), vec!["z", "a"]);
}

/// REQ: PKG-350
#[test]
fn pkg_350_service_edges_order_provider_first() {
    let nodes = vec![
        node("a", 1, &[], &["m"]),
        node("m", 1, &[], &[]),
        node("b", 1, &[], &[]),
    ];
    assert_eq!(start_order(&nodes).expect("order"), vec!["b", "m", "a"]);
}

/// REQ: PKG-350
#[test]
fn pkg_350_ready_node_with_smallest_key_is_taken_at_every_step() {
    let nodes = vec![
        node("c", 1, &["d"], &[]),
        node("d", 3, &[], &[]),
        node("a", 2, &[], &[]),
        node("b", 1, &[], &[]),
    ];
    assert_eq!(
        start_order(&nodes).expect("order"),
        vec!["b", "d", "c", "a"]
    );
}

/// REQ: PKG-350
#[test]
fn pkg_350_at_most_four_activations_run_at_once() {
    assert_eq!(MAX_CONCURRENT_ACTIVATIONS, 4);
}

/// REQ: PKG-350
#[test]
fn doc_pkg_350_page_states_the_order_rule() {
    let page = manual_page("reference-dependency-resolution.md");
    assert!(page.contains("Kahn's algorithm"));
    assert!(page.contains("Up to 4 activations run at once"));
}

proptest! {
    /// REQ: PKG-350
    #[test]
    fn pkg_350_order_of_independent_nodes_does_not_depend_on_input_order(
        ids in proptest::collection::btree_set("[a-z]{1,4}", 1..8),
        tiers in proptest::collection::vec(1u8..=3, 8),
        seed in 0usize..64,
    ) {
        let mut nodes: Vec<StartNode> = ids.iter().enumerate().map(|(index, id)| node(id, tiers[index], &[], &[])).collect();
        let mut expected = nodes.clone();
        expected.sort_by_key(|left| (left.tier, left.id.clone()));
        let expected: Vec<String> = expected.into_iter().map(|n| n.id).collect();
        let len = nodes.len();
        nodes.rotate_left(seed % len);
        prop_assert_eq!(start_order(&nodes).expect("order"), expected);
    }

    /// REQ: PKG-350, PKG-351
    #[test]
    fn pkg_351_every_dependency_starts_before_and_stops_after_its_consumer(
        edges in proptest::collection::vec((0usize..6, 0usize..6), 0..8),
    ) {
        let names = ["n0", "n1", "n2", "n3", "n4", "n5"];
        let mut nodes: Vec<StartNode> = names.iter().map(|name| node(name, 1, &[], &[])).collect();
        for (consumer, provider) in edges {
            if provider < consumer {
                nodes[consumer].requires_plugins.push(names[provider].to_owned());
            }
        }
        let start = start_order(&nodes).expect("acyclic");
        let stop = shutdown_order(&nodes).expect("acyclic");
        let position = |list: &[String], id: &str| list.iter().position(|item| item == id).expect("listed");
        for consumer in &nodes {
            for provider in &consumer.requires_plugins {
                prop_assert!(position(&start, provider) < position(&start, &consumer.id));
                prop_assert!(position(&stop, &consumer.id) < position(&stop, provider));
            }
        }
    }
}

/// REQ: PKG-351
#[test]
fn pkg_351_shutdown_is_the_exact_reverse_of_the_start_order() {
    let nodes = vec![
        node("c", 1, &["d"], &[]),
        node("d", 3, &[], &[]),
        node("a", 2, &[], &[]),
        node("b", 1, &[], &[]),
    ];
    let mut start = start_order(&nodes).expect("order");
    start.reverse();
    assert_eq!(shutdown_order(&nodes).expect("order"), start);
}

/// REQ: PKG-351
#[test]
fn pkg_351_consumer_stops_before_the_provider_it_requires() {
    let nodes = vec![
        node("provider", 1, &[], &[]),
        node("consumer", 1, &[], &["provider"]),
    ];
    assert_eq!(
        shutdown_order(&nodes).expect("order"),
        vec!["consumer", "provider"]
    );
}

/// REQ: PKG-351
#[test]
fn doc_pkg_351_page_states_reverse_order_and_serial_stops() {
    let page = manual_page("reference-dependency-resolution.md");
    assert!(page.contains("Shutdown runs the exact reverse of the start order"));
    assert!(page.contains("Stops run one at a time"));
}

// ---------------------------------------------------------------- PKG-352 replaces

fn replacement(version: &str) -> Spec {
    let mut spec = Spec::new("example.new", version);
    spec.extra_top_level
        .push(("replaces".to_owned(), "[\"example.old\"]".to_owned()));
    spec
}

/// REQ: PKG-352
#[test]
fn pkg_352_replacement_satisfies_a_requirement_for_the_replaced_id() {
    let fx = Fx::new();
    assert_eq!(fx.install(&replacement("2.0.0")).exit_code, 0);
    let out = fx.install(&Spec::new("example.a", "1.0.0").needs("example.old", "^2.0", false));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
}

/// REQ: PKG-352
#[test]
fn pkg_352_replacement_own_version_is_compared_with_the_range() {
    let fx = Fx::new();
    assert_eq!(fx.install(&replacement("2.0.0")).exit_code, 0);
    let out = fx.install(&Spec::new("example.a", "1.0.0").needs("example.old", "^1.0", false));
    assert_eq!(out.exit_code, 1);
    // The replacement (2.0.0) is outside ^1.0 and no other provider exists, so nothing satisfies
    // the range at all (manual does not pin a code for this case; unresolved is the no-provider code).
    assert_eq!(out.error_codes(), vec!["dependency.unresolved".to_owned()]);
}

/// REQ: PKG-352
#[test]
fn pkg_352_optional_dependency_on_the_old_series_is_fine() {
    let fx = Fx::new();
    assert_eq!(fx.install(&replacement("2.0.0")).exit_code, 0);
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").needs("example.old", "^1.0", true))
            .exit_code,
        0
    );
}

/// REQ: PKG-352
#[test]
fn pkg_352_disabled_replacement_does_not_satisfy_the_requirement() {
    let fx = Fx::new();
    assert_eq!(fx.install(&replacement("2.0.0")).exit_code, 0);
    assert_eq!(fx.run(&["disable", "example.new"]).exit_code, 0);
    let out = fx.install(&Spec::new("example.a", "1.0.0").needs("example.old", "^2.0", false));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.error_codes()
            .iter()
            .any(|code| code.starts_with("dependency.")),
        "{:?}",
        out.error_codes()
    );
}

// ---------------------------------------------------------------- PKG-353 channel membership

/// REQ: PKG-353
#[test]
fn pkg_353_prerelease_version_belongs_to_beta() {
    let mut registry = FakeRegistry::new(REGISTRY_URL);
    assert!(registry
        .try_publish(b("1.0.0-beta.1").build(), "beta")
        .is_ok());
}

/// REQ: PKG-353
#[test]
fn pkg_353_prerelease_version_cannot_be_published_to_stable() {
    let mut registry = FakeRegistry::new(REGISTRY_URL);
    // manual does not pin the channel-mismatch error code.
    let refusal = registry
        .try_publish(b("1.0.0-beta.1").build(), "stable")
        .expect_err("a prerelease cannot go to stable");
    assert!(!refusal.code.as_str().is_empty());
    // The refused version left no document behind.
    assert!(!registry
        .documents()
        .iter()
        .any(|(_, body)| String::from_utf8_lossy(body).contains("1.0.0-beta.1")));
}

/// REQ: PKG-353
#[test]
fn pkg_353_release_version_may_be_stable_or_beta() {
    let mut registry = FakeRegistry::new(REGISTRY_URL);
    assert!(registry.try_publish(b("1.0.0").build(), "stable").is_ok());
    assert!(registry.try_publish(b("1.1.0").build(), "beta").is_ok());
}

/// REQ: PKG-353
#[test]
fn pkg_353_dev_is_never_a_registry_channel() {
    let mut registry = FakeRegistry::new(REGISTRY_URL);
    assert!(registry.try_publish(b("1.0.0").build(), "dev").is_err());
}

/// REQ: PKG-353
#[test]
fn pkg_353_a_version_has_exactly_one_channel_in_the_index() {
    let mut registry = FakeRegistry::new(REGISTRY_URL);
    registry.publish(b("1.0.0").build(), "stable");
    registry.publish(b("1.1.0-beta.1").build(), "beta");
    let text: String = registry
        .documents()
        .iter()
        .map(|(_, body)| String::from_utf8_lossy(body).into_owned())
        .collect();
    assert_eq!(
        text.matches("\"channel\": \"stable\"").count()
            + text.matches("\"channel\":\"stable\"").count(),
        1,
        "{text}"
    );
}

// ---------------------------------------------------------------- PKG-354 channel offers

/// REQ: PKG-354
#[test]
fn pkg_354_default_channel_offers_only_stable_versions() {
    let fx = reg_fx(&[("1.0.0", "stable"), ("1.2.0", "beta")]);
    let out = run_registry(&fx, &["install", "example.b@^1.0"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.0.0"));
}

/// REQ: PKG-354
#[test]
fn pkg_354_beta_channel_offers_beta_and_stable_versions() {
    let fx = reg_fx(&[("1.0.0", "stable"), ("1.2.0", "beta")]);
    let out = run_registry(&fx, &["install", "example.b@^1.0", "--channel", "beta"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.2.0"));
}

/// REQ: PKG-354
#[test]
fn pkg_354_beta_channel_still_offers_a_stable_version_when_it_is_the_highest() {
    let fx = reg_fx(&[("1.3.0", "stable"), ("1.2.0", "beta")]);
    assert_eq!(
        run_registry(&fx, &["install", "example.b@^1.0", "--channel", "beta"]).exit_code,
        0
    );
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.3.0"));
}

/// REQ: PKG-354
#[test]
fn pkg_354_channel_command_sets_the_channel_of_one_plugin_for_later_updates() {
    let fx = reg_fx(&[("1.0.0", "stable")]);
    assert_eq!(
        run_registry(&fx, &["install", "example.b@^1.0"]).exit_code,
        0
    );
    serve_registry(&fx, &[(b("1.0.0"), "stable"), (b("1.2.0"), "beta")]);
    assert_eq!(fx.run(&["channel", "example.b", "beta"]).exit_code, 0);
    let out = run_registry(&fx, &["update", "example.b"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.2.0"));
}

/// REQ: PKG-354
#[test]
fn pkg_354_stable_plugin_is_not_offered_a_beta_update() {
    let fx = reg_fx(&[("1.0.0", "stable")]);
    assert_eq!(
        run_registry(&fx, &["install", "example.b@^1.0"]).exit_code,
        0
    );
    serve_registry(&fx, &[(b("1.0.0"), "stable"), (b("1.2.0"), "beta")]);
    assert_eq!(fx.run(&["channel", "example.b", "stable"]).exit_code, 0);
    let _ = run_registry(&fx, &["update", "example.b"]);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.0.0"));
}

/// REQ: PKG-354
#[test]
fn pkg_354_channel_other_than_stable_or_beta_is_refused() {
    let fx = reg_fx(&[("1.0.0", "stable")]);
    assert_eq!(
        run_registry(&fx, &["install", "example.b@^1.0"]).exit_code,
        0
    );
    assert_ne!(fx.run(&["channel", "example.b", "nightly"]).exit_code, 0);
    assert_ne!(fx.run(&["channel", "example.b", "dev"]).exit_code, 0);
}

/// REQ: PKG-354
#[test]
fn pkg_354_default_channel_comes_from_the_plugins_section_of_the_config() {
    let fx = reg_fx(&[("1.0.0", "stable"), ("1.2.0", "beta")]);
    std::fs::create_dir_all(fx.config_dir()).expect("config dir");
    std::fs::write(
        fx.config_dir().join("config.toml"),
        "[plugins]\nchannel = \"beta\"\n",
    )
    .expect("config");
    assert_eq!(
        run_registry(&fx, &["install", "example.b@^1.0"]).exit_code,
        0
    );
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.2.0"));
}

/// REQ: PKG-354
#[test]
fn pkg_354_resolver_filters_sources_by_channel() {
    let mut beta = source_in("example.b", "1.2.0", "beta", "a2");
    beta.channel = "beta".to_owned();
    let stable = source_in("example.b", "1.0.0", "stable", "a1");
    let mut request = request_for(
        "example.a",
        "example.b",
        "^1.0",
        vec![stable.clone(), beta.clone()],
    );
    request.channel = "stable".to_owned();
    assert_eq!(chosen_version(&request, "example.b"), "1.0.0");
    request.channel = "beta".to_owned();
    assert_eq!(chosen_version(&request, "example.b"), "1.2.0");
}

fn source_in(id: &str, version: &str, channel: &str, sha: &str) -> SourcePackage {
    SourcePackage {
        package: PlanPackage {
            id: id.to_owned(),
            version: version.to_owned(),
            needs: Vec::new(),
            host: None,
        },
        sha256: sha.to_owned(),
        yanked: false,
        channel: channel.to_owned(),
        source_rank: 0,
    }
}

fn request_for(
    consumer: &str,
    dependency: &str,
    range: &str,
    sources: Vec<SourcePackage>,
) -> ResolveInput {
    ResolveInput {
        incoming: vec![PlanPackage {
            id: consumer.to_owned(),
            version: "1.0.0".to_owned(),
            needs: vec![(dependency.to_owned(), range.to_owned(), false)],
            host: None,
        }],
        installed: Vec::new(),
        sources,
        channel: "stable".to_owned(),
        host_release: HOST_RELEASE.to_owned(),
        is_update: false,
        pins: Vec::new(),
    }
}

fn chosen_version(request: &ResolveInput, id: &str) -> String {
    let plan = resolve_plan(request).unwrap_or_else(|d| panic!("plan expected: {d:?}"));
    plan.steps
        .into_iter()
        .find(|step| step.id == id)
        .map(|step| step.version)
        .unwrap_or_default()
}

// ---------------------------------------------------------------- PKG-355 downgrades

fn beta_then_stable_fx() -> Fx {
    let fx = reg_fx(&[("2.0.0-beta.1", "beta")]);
    assert_eq!(
        run_registry(
            &fx,
            &["install", "example.b@2.0.0-beta.1", "--channel", "beta"]
        )
        .exit_code,
        0
    );
    serve_registry(&fx, &[(b("2.0.0-beta.1"), "beta"), (b("1.9.0"), "stable")]);
    assert_eq!(fx.run(&["channel", "example.b", "stable"]).exit_code, 0);
    fx
}

/// REQ: PKG-355
#[test]
fn pkg_355_switch_to_stable_does_not_install_a_lower_version() {
    let fx = beta_then_stable_fx();
    let _ = run_registry(&fx, &["update", "example.b"]);
    assert_eq!(
        fx.active_version("example.b").as_deref(),
        Some("2.0.0-beta.1")
    );
}

/// REQ: PKG-355
#[test]
fn pkg_355_a_higher_stable_version_is_taken_after_the_switch() {
    let fx = beta_then_stable_fx();
    serve_registry(
        &fx,
        &[
            (b("2.0.0-beta.1"), "beta"),
            (b("1.9.0"), "stable"),
            (b("2.0.0"), "stable"),
        ],
    );
    assert_eq!(run_registry(&fx, &["update", "example.b"]).exit_code, 0);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("2.0.0"));
}

/// REQ: PKG-355
#[test]
fn pkg_355_explicit_install_with_allow_downgrade_goes_back() {
    let fx = beta_then_stable_fx();
    let out = run_registry(&fx, &["install", "example.b@1.9.0", "--allow-downgrade"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.9.0"));
}

/// REQ: PKG-355
#[test]
fn pkg_355_explicit_install_without_the_flag_is_refused_downgrade() {
    let fx = beta_then_stable_fx();
    let out = run_registry(&fx, &["install", "example.b@1.9.0"]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("package.downgrade"),
        "{:?}",
        out.error_codes()
    );
    assert_eq!(
        fx.active_version("example.b").as_deref(),
        Some("2.0.0-beta.1")
    );
}

// ---------------------------------------------------------------- PKG-356 file installs

/// REQ: PKG-356
#[test]
fn pkg_356_file_install_keeps_whatever_version_the_file_holds() {
    let fx = Fx::new();
    assert_eq!(fx.install(&b("3.0.0-rc.1")).exit_code, 0);
    assert_eq!(
        fx.active_version("example.b").as_deref(),
        Some("3.0.0-rc.1")
    );
}

/// REQ: PKG-356
#[test]
fn pkg_356_file_install_records_channel_file() {
    let fx = Fx::new();
    assert_eq!(fx.install(&b("1.0.0")).exit_code, 0);
    assert_eq!(fx.record("example.b").expect("record")["channel"], "file");
}

/// REQ: PKG-356
#[test]
fn pkg_356_file_install_ignores_a_stable_default_channel_for_prereleases() {
    let fx = Fx::new();
    std::fs::create_dir_all(fx.config_dir()).expect("config dir");
    std::fs::write(
        fx.config_dir().join("config.toml"),
        "[plugins]\nchannel = \"stable\"\n",
    )
    .expect("config");
    assert_eq!(fx.install(&b("1.0.0-alpha.1")).exit_code, 0);
    assert_eq!(fx.record("example.b").expect("record")["channel"], "file");
}

/// REQ: PKG-356
#[test]
fn doc_pkg_356_page_states_dev_and_file_channel_values() {
    let page = manual_page("reference-dependency-resolution.md");
    assert!(page.contains("`channel: dev` for `ilium plugin dev`"));
    assert!(page.contains("`channel: file` for other files"));
}

// ---------------------------------------------------------------- PKG-357 allowed channels

fn with_policy(fx: &Fx, toml: &str, args: &[&str]) -> ilium_plugin_host::cli::CliOutput {
    let policy = fx.dir.path().join("policy").join("policy.toml");
    std::fs::create_dir_all(policy.parent().expect("parent")).expect("policy dir");
    std::fs::write(&policy, toml).expect("policy");
    let mut env = fx.env();
    env.policy_file = Some(policy);
    let owned: Vec<String> = args.iter().map(|argument| (*argument).to_owned()).collect();
    ilium_plugin_host::cli::run(&owned, &env)
}

fn install_args() -> Vec<&'static str> {
    vec![
        "install",
        "example.b@^1.0",
        "--channel",
        "beta",
        "--allow-unsigned",
        "--yes",
        "--registry",
        REGISTRY_URL,
    ]
}

/// REQ: PKG-357
#[test]
fn pkg_357_policy_limited_to_stable_refuses_the_beta_channel() {
    let fx = reg_fx(&[("1.0.0", "stable"), ("1.2.0", "beta")]);
    let out = with_policy(
        &fx,
        "version = 1\n[packages]\nallowed_channels = [\"stable\"]\n",
        &install_args(),
    );
    assert_eq!(out.exit_code, 7, "{:?}", out.stdout_jsonl);
    assert!(
        out.error_codes()
            .iter()
            .any(|code| code.starts_with("policy.")),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.b").is_none());
}

/// REQ: PKG-357
#[test]
fn pkg_357_policy_limited_to_stable_still_allows_stable() {
    let fx = reg_fx(&[("1.0.0", "stable"), ("1.2.0", "beta")]);
    let args = [
        "install",
        "example.b@^1.0",
        "--allow-unsigned",
        "--yes",
        "--registry",
        REGISTRY_URL,
    ];
    let out = with_policy(
        &fx,
        "version = 1\n[packages]\nallowed_channels = [\"stable\"]\n",
        &args,
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.0.0"));
}

/// REQ: PKG-357
#[test]
fn pkg_357_without_the_key_both_channels_are_allowed() {
    let fx = reg_fx(&[("1.0.0", "stable"), ("1.2.0", "beta")]);
    let out = with_policy(&fx, "version = 1\n", &install_args());
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.2.0"));
}

/// REQ: PKG-357
#[test]
fn doc_pkg_357_admin_policy_page_defines_the_key_and_its_default() {
    let page = manual_page("reference-admin-policy.md");
    assert!(page.contains("allowed_channels = [\"stable\"]       # default: [\"stable\",\"beta\"]"));
}

// ---------------------------------------------------------------- PKG-358 pins

fn pin_fx() -> Fx {
    let fx = reg_fx(&[("1.0.0", "stable"), ("1.1.0", "stable")]);
    let out = run_registry(&fx, &["install", "example.b@1.0.0"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    fx
}

/// REQ: PKG-358
#[test]
fn pkg_358_pin_records_the_exact_version() {
    let fx = pin_fx();
    assert_eq!(fx.run(&["pin", "example.b@1.0.0"]).exit_code, 0);
    assert_eq!(fx.record("example.b").expect("record")["pinned"], true);
}

/// REQ: PKG-358
#[test]
fn pkg_358_update_skips_a_pinned_plugin() {
    let fx = pin_fx();
    assert_eq!(fx.run(&["pin", "example.b@1.0.0"]).exit_code, 0);
    let _ = run_registry(&fx, &["update", "example.b"]);
    let _ = run_registry(&fx, &["update", "--all"]);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.0.0"));
}

/// REQ: PKG-358
#[test]
fn pkg_358_unpin_lets_updates_proceed() {
    let fx = pin_fx();
    assert_eq!(fx.run(&["pin", "example.b@1.0.0"]).exit_code, 0);
    assert_eq!(fx.run(&["unpin", "example.b"]).exit_code, 0);
    assert_eq!(fx.record("example.b").expect("record")["pinned"], false);
    assert_eq!(run_registry(&fx, &["update", "example.b"]).exit_code, 0);
    assert_eq!(fx.active_version("example.b").as_deref(), Some("1.1.0"));
}

/// REQ: PKG-358
#[test]
fn pkg_358_pin_of_a_plugin_that_is_not_installed_fails() {
    let fx = Fx::new();
    assert_ne!(fx.run(&["pin", "example.b@1.0.0"]).exit_code, 0);
}

/// REQ: PKG-358
#[test]
fn pkg_358_resolver_treats_a_pin_as_the_only_allowed_version() {
    let mut request = request_for(
        "example.a",
        "example.b",
        "^1.2",
        vec![
            source_in("example.b", "1.2.0", "stable", "a2"),
            source_in("example.b", "1.0.0", "stable", "a1"),
        ],
    );
    request.installed = vec![PlanPackage {
        id: "example.b".to_owned(),
        version: "1.0.0".to_owned(),
        needs: Vec::new(),
        host: None,
    }];
    request.pins = vec!["example.b".to_owned()];
    let codes: Vec<String> = match resolve_plan(&request) {
        Ok(plan) => plan
            .steps
            .iter()
            .map(|step| format!("{:?}:{}", step.action, step.version))
            .collect(),
        Err(list) => list.iter().map(|d| d.code.as_str().to_owned()).collect(),
    };
    assert_eq!(codes, vec!["dependency.conflict".to_owned()]);
}

/// REQ: PKG-358
#[test]
fn pkg_358_resolver_keeps_the_pinned_version_when_the_range_allows_it() {
    let mut request = request_for(
        "example.a",
        "example.b",
        "^1.0",
        vec![source_in("example.b", "1.2.0", "stable", "a2")],
    );
    request.installed = vec![PlanPackage {
        id: "example.b".to_owned(),
        version: "1.0.0".to_owned(),
        needs: Vec::new(),
        host: None,
    }];
    request.pins = vec!["example.b".to_owned()];
    let plan = resolve_plan(&request).expect("plan");
    assert!(plan
        .steps
        .iter()
        .any(|s| s.action == PlanAction::Keep && s.id == "example.b" && s.version == "1.0.0"));
}

// ---------------------------------------------------------------- PKG-359 / PKG-360 lock files

fn locked_fx() -> (Fx, Value) {
    let fx = Fx::new();
    serve_registry(
        &fx,
        &[
            (b("1.0.0"), "stable"),
            (
                Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", false),
                "stable",
            ),
        ],
    );
    assert_eq!(
        run_registry(&fx, &["install", "example.a@^1.0"]).exit_code,
        0
    );
    let out = fx.run(&["lock"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    let text = std::fs::read_to_string(fx.cwd.join("ilium-plugins.lock")).unwrap_or_default();
    let lock: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (fx, lock)
}

fn write_lock(fx: &Fx, lock: &Value) -> String {
    fx.write_file("ilium-plugins.lock", lock.to_string().as_bytes())
}

fn fresh_with_same_registry(source: &Fx) -> Fx {
    let fx = Fx::new();
    serve_registry(
        &fx,
        &[
            (b("1.0.0"), "stable"),
            (
                Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", false),
                "stable",
            ),
        ],
    );
    let _ = source;
    fx
}

/// REQ: PKG-359
#[test]
fn pkg_359_lock_writes_the_documented_json_shape() {
    let (_fx, lock) = locked_fx();
    assert_eq!(lock["format"], "ilium-lock/1");
    let plugins = lock["plugins"].as_array().expect("plugins array");
    assert_eq!(plugins.len(), 2);
    for entry in plugins {
        let mut keys: Vec<&str> = entry
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["id", "sha256", "source", "version"]);
        assert!(is_sha256_hex(entry["sha256"].as_str().unwrap_or("")));
    }
}

/// REQ: PKG-359
#[test]
fn pkg_359_lock_entries_are_sorted_by_id() {
    let (_fx, lock) = locked_fx();
    let ids: Vec<&str> = lock["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["example.a", "example.b"]);
}

/// REQ: PKG-359
#[test]
fn pkg_359_lock_holds_installed_versions_and_hashes() {
    let (fx, lock) = locked_fx();
    let entry = &lock["plugins"][1];
    assert_eq!(entry["version"], "1.0.0");
    assert_eq!(
        entry["sha256"],
        fx.record("example.b").expect("record")["package_sha256"]
    );
}

/// REQ: PKG-359
#[test]
fn pkg_359_locked_install_installs_exactly_the_locked_versions() {
    let (fx, lock) = locked_fx();
    let target = fresh_with_same_registry(&fx);
    let path = write_lock(&target, &lock);
    let out = run_registry(&target, &["install", "--locked", &path]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert_eq!(target.active_version("example.a").as_deref(), Some("1.0.0"));
    assert_eq!(target.active_version("example.b").as_deref(), Some("1.0.0"));
}

/// REQ: PKG-359
#[test]
fn pkg_359_locked_version_that_no_source_offers_is_unresolved() {
    let (fx, mut lock) = locked_fx();
    lock["plugins"][1]["version"] = Value::String("1.7.0".to_owned());
    let target = fresh_with_same_registry(&fx);
    let path = write_lock(&target, &lock);
    let out = run_registry(&target, &["install", "--locked", &path]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("dependency.unresolved"),
        "{:?}",
        out.error_codes()
    );
    assert!(target.record("example.a").is_none());
}

/// REQ: PKG-359
#[test]
fn pkg_359_resolution_that_differs_from_the_lock_is_a_lock_mismatch() {
    let (fx, mut lock) = locked_fx();
    let plugins = lock["plugins"].as_array_mut().expect("plugins");
    plugins.retain(|entry| entry["id"] == "example.a");
    let target = fresh_with_same_registry(&fx);
    let path = write_lock(&target, &lock);
    let out = run_registry(&target, &["install", "--locked", &path]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("dependency.lock-mismatch"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-359
#[test]
fn pkg_359_lock_of_an_empty_installed_set_has_no_plugins() {
    let fx = Fx::new();
    assert_eq!(fx.run(&["lock"]).exit_code, 0);
    let text = std::fs::read_to_string(fx.cwd.join("ilium-plugins.lock")).unwrap_or_default();
    let lock: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    assert_eq!(lock["format"], "ilium-lock/1");
    assert_eq!(lock["plugins"].as_array().map(Vec::len), Some(0));
}

/// REQ: PKG-360
#[test]
fn pkg_360_lock_hash_that_differs_from_the_download_is_refused() {
    let (fx, mut lock) = locked_fx();
    lock["plugins"][1]["sha256"] = Value::String("0".repeat(64));
    let target = fresh_with_same_registry(&fx);
    let path = write_lock(&target, &lock);
    let out = run_registry(&target, &["install", "--locked", &path]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("registry.hash-mismatch"),
        "{:?}",
        out.error_codes()
    );
    assert!(target.record("example.b").is_none());
}

/// REQ: PKG-360
#[test]
fn pkg_360_matching_hashes_install_without_a_mismatch() {
    let (fx, lock) = locked_fx();
    let target = fresh_with_same_registry(&fx);
    let path = write_lock(&target, &lock);
    let out = run_registry(&target, &["install", "--locked", &path]);
    assert!(
        !out.has_error("registry.hash-mismatch"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-360
#[test]
fn pkg_360_a_one_character_hash_difference_is_enough() {
    let (fx, mut lock) = locked_fx();
    let mut sha = lock["plugins"][0]["sha256"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let last = if sha.ends_with('0') { '1' } else { '0' };
    sha.pop();
    sha.push(last);
    lock["plugins"][0]["sha256"] = Value::String(sha);
    let target = fresh_with_same_registry(&fx);
    let path = write_lock(&target, &lock);
    let out = run_registry(&target, &["install", "--locked", &path]);
    assert!(
        out.has_error("registry.hash-mismatch"),
        "{:?}",
        out.error_codes()
    );
}
