//! Page `reference-dependency-resolution.md`: versions, ranges, requirements, the resolver and its
//! decision table (PKG-310 .. PKG-348). Expected values are copied from the page.

use ilium_plugin_host::resolve::{
    resolve_plan, PlanAction, PlanPackage, PluginVersion, ResolveInput, SourcePackage, VersionRange,
};
use proptest::prelude::*;

use crate::support::*;

fn version(text: &str) -> PluginVersion {
    PluginVersion::parse(text).unwrap_or_else(|d| panic!("{text} should parse: {d:?}"))
}

fn range_ok(text: &str) -> VersionRange {
    VersionRange::parse(text).unwrap_or_else(|d| panic!("{text} should parse: {d:?}"))
}

fn matches(range: &str, candidate: &str) -> bool {
    range_ok(range).matches(&version(candidate), false)
}

fn code_of_version_error(text: &str) -> String {
    match PluginVersion::parse(text) {
        Ok(_) => String::new(),
        Err(d) => d.code.as_str().to_owned(),
    }
}

fn code_of_range_error(text: &str) -> String {
    match VersionRange::parse(text) {
        Ok(_) => String::new(),
        Err(d) => d.code.as_str().to_owned(),
    }
}

// ---------------------------------------------------------------- versions

/// REQ: PKG-310
#[test]
fn pkg_310_plain_and_prerelease_versions_parse() {
    for text in [
        "0.0.0",
        "1.2.3",
        "10.20.30",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-0.3.7",
    ] {
        assert!(PluginVersion::parse(text).is_ok(), "{text}");
    }
}

/// REQ: PKG-310
#[test]
fn pkg_310_numeric_parts_stop_at_2147483647() {
    assert!(PluginVersion::parse("2147483647.2147483647.2147483647").is_ok());
    for text in ["2147483648.0.0", "0.2147483648.0", "0.0.2147483648"] {
        assert_eq!(
            code_of_version_error(text),
            "manifest.version-invalid",
            "{text}"
        );
    }
}

/// REQ: PKG-310
#[test]
fn pkg_310_leading_zeros_and_missing_parts_are_refused() {
    for text in [
        "01.2.3", "1.02.3", "1.2.03", "1.2", "1", "1.2.3.4", "", "1.2.-3", "a.b.c",
    ] {
        assert_eq!(
            code_of_version_error(text),
            "manifest.version-invalid",
            "{text:?}"
        );
    }
}

proptest! {
    // REQ: PKG-310
    #[test]
    fn pkg_310_any_triple_in_range_round_trips(major in 0u32..=2_147_483_647, minor in 0u32..=2_147_483_647, patch in 0u32..=2_147_483_647) {
        let text = format!("{major}.{minor}.{patch}");
        prop_assert!(PluginVersion::parse(&text).is_ok());
    }

    // REQ: PKG-314
    #[test]
    fn pkg_314_release_ordering_matches_tuple_ordering(a in (0u32..50, 0u32..50, 0u32..50), b in (0u32..50, 0u32..50, 0u32..50)) {
        let left = version(&format!("{}.{}.{}", a.0, a.1, a.2));
        let right = version(&format!("{}.{}.{}", b.0, b.1, b.2));
        prop_assert_eq!(left.cmp(&right), a.cmp(&b));
    }
}

/// REQ: PKG-311
#[test]
fn pkg_311_prerelease_identifier_alphabet() {
    for ok in ["1.0.0-a-b", "1.0.0-0", "1.0.0-x.7.z.92", "1.0.0-A1.b2"] {
        assert!(PluginVersion::parse(ok).is_ok(), "{ok}");
    }
    for bad in [
        "1.0.0-",
        "1.0.0-a..b",
        "1.0.0-a_b",
        "1.0.0-é",
        "1.0.0-a.",
        "1.0.0-01",
        "1.0.0-a.01",
    ] {
        assert_eq!(
            code_of_version_error(bad),
            "manifest.version-invalid",
            "{bad}"
        );
    }
}

/// REQ: PKG-311
#[test]
fn pkg_311_version_length_64_ok_65_refused() {
    let base = "1.0.0-";
    let at_limit = format!("{base}{}", "a".repeat(64 - base.len()));
    let over = format!("{base}{}", "a".repeat(65 - base.len()));
    let under = format!("{base}{}", "a".repeat(63 - base.len()));
    assert_eq!(at_limit.len(), 64);
    assert!(PluginVersion::parse(&under).is_ok());
    assert!(PluginVersion::parse(&at_limit).is_ok());
    assert_eq!(code_of_version_error(&over), "manifest.version-invalid");
}

/// REQ: PKG-312
#[test]
fn pkg_312_leading_v_is_refused() {
    for text in ["v1.2.3", "V1.2.3", "=1.2.3-x"] {
        assert_eq!(
            code_of_version_error(text),
            "manifest.version-invalid",
            "{text}"
        );
    }
}

/// REQ: PKG-312
#[test]
fn pkg_312_manifest_with_v_version_is_refused_at_install() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "v1.2.3"));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("manifest.version-invalid"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-313 WARN
#[test]
fn pkg_313_build_metadata_is_ignored_in_comparison() {
    assert_eq!(version("1.0.0+build.5"), version("1.0.0+other"));
    assert_eq!(
        version("1.0.0+build.5").cmp(&version("1.0.0")),
        std::cmp::Ordering::Equal
    );
    assert!(PluginVersion::parse("1.0.0-rc.1+build.5").is_ok());
}

/// REQ: PKG-313 WARN
#[test]
fn pkg_313_registry_refuses_build_metadata() {
    let fx = Fx::new();
    let mut registry = ilium_plugin_host::testing::FakeRegistry::new(REGISTRY_URL);
    let published = registry.try_publish(Spec::new("example.a", "1.0.0+build.5").build(), "stable");
    assert_eq!(
        published.err().map(|d| d.code.as_str().to_owned()),
        Some("registry.version-invalid".to_owned())
    );
    let _ = fx;
}

/// REQ: PKG-314
#[test]
fn pkg_314_semver_section_11_precedence_chain() {
    let ordered = [
        "0.9.0",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.1",
        "1.1.0",
    ];
    for pair in ordered.windows(2) {
        assert!(
            version(pair[0]) < version(pair[1]),
            "{} < {}",
            pair[0],
            pair[1]
        );
        assert!(
            version(pair[1]) > version(pair[0]),
            "{} > {}",
            pair[1],
            pair[0]
        );
    }
}

/// REQ: PKG-314
#[test]
fn pkg_314_numeric_identifier_is_lower_than_alphanumeric_and_longer_set_is_higher() {
    assert!(version("1.0.0-1") < version("1.0.0-a"));
    assert!(version("1.0.0-alpha") < version("1.0.0-alpha.0"));
    assert!(version("1.0.0-b") > version("1.0.0-a"));
    assert!(version("1.0.0-beta.2") < version("1.0.0"));
}

// ---------------------------------------------------------------- ranges

/// REQ: PKG-315
#[test]
fn pkg_315_comparators_are_anded() {
    let range = ">=1.2.0 <1.5.0";
    assert!(matches(range, "1.2.0") && matches(range, "1.4.9"));
    assert!(!matches(range, "1.1.9") && !matches(range, "1.5.0"));
}

/// REQ: PKG-315
#[test]
fn pkg_315_or_operator_is_refused() {
    for text in ["^1.0 || ^2.0", "1.0.0||2.0.0", ">=1 || <0"] {
        assert_eq!(
            code_of_range_error(text),
            "manifest.requires-invalid",
            "{text}"
        );
    }
}

/// REQ: PKG-315
#[test]
fn pkg_315_comparators_need_single_space_separation() {
    for text in [">=1.2.0  <1.5.0", " >=1.2.0", ">=1.2.0 "] {
        assert_eq!(
            code_of_range_error(text),
            "manifest.requires-invalid",
            "{text:?}"
        );
    }
}

/// REQ: PKG-316
#[test]
fn pkg_316_each_comparator_form() {
    assert!(matches("=1.2.3", "1.2.3") && !matches("=1.2.3", "1.2.4"));
    // A bare version is a caret range (PKG-317).
    assert!(matches("1.2.3", "1.2.3") && matches("1.2.3", "1.2.9"));
    assert!(!matches("1.2.3", "2.0.0") && !matches("1.2.3", "1.2.2"));
    assert!(matches(">1.2.3", "1.2.4") && !matches(">1.2.3", "1.2.3"));
    assert!(matches(">=1.2.3", "1.2.3") && !matches(">=1.2.3", "1.2.2"));
    assert!(matches("<1.2.3", "1.2.2") && !matches("<1.2.3", "1.2.3"));
    assert!(matches("<=1.2.3", "1.2.3") && !matches("<=1.2.3", "1.2.4"));
    assert!(matches("^1.2.3", "1.9.0") && matches("~1.2.3", "1.2.9") && matches("*", "7.7.7"));
}

/// REQ: PKG-316
#[test]
fn pkg_316_unknown_operator_is_refused() {
    for text in ["!1.2.3", "==1.2.3", "^^1.2.3", ">>1.0.0", "x.y.z"] {
        assert_eq!(
            code_of_range_error(text),
            "manifest.requires-invalid",
            "{text}"
        );
    }
}

/// REQ: PKG-317
#[test]
fn pkg_317_bare_version_is_a_caret_range() {
    for (bare, caret) in [
        ("1.2.3", "^1.2.3"),
        ("1.2", "^1.2"),
        ("1", "^1"),
        ("0.1.0", "^0.1.0"),
    ] {
        for probe in [
            "0.0.9", "0.1.0", "0.1.5", "0.2.0", "1.0.0", "1.2.3", "1.9.9", "2.0.0",
        ] {
            assert_eq!(
                matches(bare, probe),
                matches(caret, probe),
                "{bare} vs {caret} at {probe}"
            );
        }
    }
}

/// REQ: PKG-318
#[test]
fn pkg_318_caret_table() {
    let cases: [(&str, &str, &str); 5] = [
        ("^1.2.3", "1.2.3", "2.0.0"),
        ("^0.2.3", "0.2.3", "0.3.0"),
        ("^0.0.3", "0.0.3", "0.0.4"),
        ("^0.0", "0.0.0", "0.1.0"),
        ("^0", "0.0.0", "1.0.0"),
    ];
    for (range, low, high) in cases {
        assert!(matches(range, low), "{range} accepts {low}");
        assert!(!matches(range, high), "{range} refuses {high}");
    }
    assert!(!matches("^1.2.3", "1.2.2") && matches("^1.2.3", "1.99.99"));
    assert!(matches("^0.2.3", "0.2.99") && !matches("^0.2.3", "0.2.2"));
}

/// REQ: PKG-319
#[test]
fn pkg_319_tilde_table() {
    assert!(
        matches("~1.2.3", "1.2.3") && matches("~1.2.3", "1.2.99") && !matches("~1.2.3", "1.3.0")
    );
    assert!(matches("~1.2", "1.2.0") && !matches("~1.2", "1.3.0"));
    assert!(matches("~1", "1.0.0") && matches("~1", "1.9.9") && !matches("~1", "2.0.0"));
    assert!(!matches("~1.2.3", "1.2.2"));
}

/// REQ: PKG-320
#[test]
fn pkg_320_star_accepts_releases_but_not_prereleases() {
    for text in ["0.0.0", "1.0.0", "2147483647.0.0"] {
        assert!(matches("*", text), "{text}");
    }
    assert!(!matches("*", "1.0.0-alpha"));
}

/// REQ: PKG-321
#[test]
fn pkg_321_prerelease_does_not_satisfy_a_plain_range() {
    assert!(!matches("^1.2.0", "1.3.0-beta.1"));
    assert!(!matches(">=1.0.0", "1.5.0-rc.1"));
}

/// REQ: PKG-321
#[test]
fn pkg_321_prerelease_comparator_on_same_triple_admits_it() {
    assert!(matches(">=1.3.0-alpha.1 <2.0.0", "1.3.0-beta.1"));
    assert!(!matches(">=1.3.0-alpha.1 <2.0.0", "1.4.0-beta.1"));
    assert!(matches("^1.3.0-beta.1", "1.3.0-beta.2"));
}

/// REQ: PKG-321
#[test]
fn pkg_321_beta_channel_admits_prereleases() {
    assert!(range_ok("^1.2.0").matches(&version("1.3.0-beta.1"), true));
}

/// REQ: PKG-322
#[test]
fn pkg_322_range_length_127_128_129() {
    // 7 + n + 7 * 17 characters: ">=1.0.<1 followed by n zeros>" then 17 times " <9.0.0".
    let build = |zeros: usize| format!(">=1.0.1{}{}", "0".repeat(zeros), " <9.0.0".repeat(17));
    for (zeros, length, ok) in [
        (0usize, 126usize, true),
        (1, 127, true),
        (2, 128, true),
        (3, 129, false),
    ] {
        let text = build(zeros);
        assert_eq!(text.len(), length);
        if ok {
            assert!(VersionRange::parse(&text).is_ok(), "{length}: {text}");
        } else {
            assert_eq!(code_of_range_error(&text), "manifest.requires-invalid");
        }
    }
}

/// REQ: PKG-322
#[test]
fn pkg_322_invalid_range_is_refused_at_install() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0").needs("example.b", "not-a-range", true));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("manifest.requires-invalid"),
        "{:?}",
        out.error_codes()
    );
}

// ---------------------------------------------------------------- requires

/// REQ: PKG-323
#[test]
fn pkg_323_requiring_itself_is_refused_requires_self() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0").needs("example.a", "^1.0", false));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("manifest.requires-self"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-323
#[test]
fn pkg_323_requiring_another_id_is_not_requires_self() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", true));
    assert!(!out.has_error("manifest.requires-self"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
}

/// REQ: PKG-324
#[test]
fn pkg_324_duplicate_required_id_is_refused_requires_invalid() {
    let fx = Fx::new();
    let spec = Spec::new("example.a", "1.0.0")
        .needs("example.b", "^1.0", true)
        .needs("example.b", "^2.0", true);
    let out = fx.install(&spec);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("manifest.requires-invalid"),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-324
#[test]
fn pkg_324_two_different_ids_are_fine() {
    let fx = Fx::new();
    let out = fx.install(
        &Spec::new("example.a", "1.0.0")
            .needs("example.b", "^1.0", true)
            .needs("example.c", "^1.0", true),
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
}

/// REQ: PKG-325
#[test]
fn pkg_325_host_range_not_satisfied_is_refused_host_version() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0").host(">=9.0.0"));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("dependency.host-version"),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.a").is_none());
}

/// REQ: PKG-325
#[test]
fn pkg_325_host_range_satisfied_installs() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0").host(">=0.1.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
}

/// REQ: PKG-325
#[test]
fn pkg_325_host_boundary_equal_passes_one_above_fails() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.eq", "1.0.0").host(">=0.1.0"))
            .exit_code,
        0
    );
    assert!(fx
        .install(&Spec::new("example.hi", "1.0.0").host(">=0.1.1"))
        .has_error("dependency.host-version"));
}

/// REQ: PKG-325
#[test]
fn pkg_325_a_later_load_keeps_the_plugin_disabled_with_a_reason() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").host(">=0.1.0"))
            .exit_code,
        0
    );
    let mut env = fx.env();
    env.host_release = "0.0.9".to_owned();
    let owned = vec!["list".to_owned(), "--json".to_owned()];
    let out = ilium_plugin_host::cli::run(&owned, &env);
    let text = out.all_text();
    assert!(text.contains("example.a"), "{text}");
    assert!(text.contains("disabled"), "{text}");
    assert!(text.contains("dependency.host-version"), "{text}");
}

/// REQ: PKG-326
#[test]
fn pkg_326_wait_ms_5000_ok_5001_refused() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.ok", "1.0.0").wait_ms(5000))
            .exit_code,
        0
    );
    let out = fx.install(&Spec::new("example.bad", "1.0.0").wait_ms(5001));
    assert_eq!(out.exit_code, 1);
    assert_eq!(
        out.error_codes(),
        vec!["manifest.requires-invalid".to_owned()]
    );
    assert!(fx.record("example.bad").is_none());
}

/// REQ: PKG-326
#[test]
fn pkg_326_wait_ms_4999_and_zero() {
    let fx = Fx::new();
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").wait_ms(4999))
            .exit_code,
        0
    );
    assert_eq!(
        fx.install(&Spec::new("example.b", "1.0.0").wait_ms(0))
            .exit_code,
        0
    );
}

/// REQ: PKG-327
#[test]
fn pkg_327_service_form_requires_a_full_version_meaning_caret() {
    assert!(
        ilium_plugin_host::resolve::parse_service_requirement("ilium:inference/provider@0.1")
            .is_err()
    );
    let long =
        ilium_plugin_host::resolve::parse_service_requirement("ilium:inference/provider@0.1.0")
            .expect("long");
    assert!(
        long.range.matches(&version("0.1.7"), false)
            && !long.range.matches(&version("0.2.0"), false)
    );
}

/// REQ: PKG-327
#[test]
fn pkg_327_service_form_refuses_ranges() {
    for text in [
        "ilium:inference/provider@>=0.1.0 <0.3.0",
        "ilium:inference/provider@^0.1.0",
    ] {
        assert!(
            ilium_plugin_host::resolve::parse_service_requirement(text).is_err(),
            "{text}"
        );
    }
}

/// REQ: PKG-328
#[test]
fn pkg_328_extension_point_reference_bare_version_is_caret() {
    let point = ilium_plugin_host::resolve::parse_point_reference("example.host/toolbar@1.2")
        .expect("point");
    assert_eq!(point.plugin, "example.host");
    assert_eq!(point.point, "toolbar");
    assert!(
        point.range.matches(&version("1.9.0"), false)
            && !point.range.matches(&version("2.0.0"), false)
    );
}

/// REQ: PKG-328
#[test]
fn pkg_328_requires_points_does_not_exist() {
    let fx = Fx::new();
    let mut spec = Spec::new("example.a", "1.0.0");
    spec.extra_top_level.push((
        "requires".to_owned(),
        "{\"points\": [\"example.host/toolbar@1.0\"]}".to_owned(),
    ));
    let out = fx.install(&spec);
    assert_eq!(out.exit_code, 1, "requires.points is not a manifest key");
}

/// REQ: PKG-329
#[test]
fn pkg_329_missing_service_provider_does_not_block_install() {
    let fx = Fx::new();
    let out =
        fx.install(&Spec::new("example.a", "1.0.0").service("ilium:inference/provider@0.1.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.a").is_some());
}

/// REQ: PKG-329
#[test]
fn pkg_329_missing_point_owner_does_not_block_install() {
    let fx = Fx::new();
    let mut spec = Spec::new("example.a", "1.0.0");
    spec.extra_top_level.push((
        "contributes".to_owned(),
        "{\"extensions\": [{\"point\": \"example.host/toolbar@1.0\", \"id\": \"t\"}]}".to_owned(),
    ));
    let out = fx.install(&spec);
    assert!(
        !out.error_codes()
            .iter()
            .any(|c| c.starts_with("dependency.")),
        "{:?}",
        out.error_codes()
    );
}

/// REQ: PKG-329
#[test]
fn pkg_329_missing_required_plugin_does_block_install() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", false));
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("dependency.unresolved"),
        "{:?}",
        out.error_codes()
    );
}

// ---------------------------------------------------------------- resolver

fn pkg(id: &str, ver: &str) -> PlanPackage {
    PlanPackage {
        id: id.to_owned(),
        version: ver.to_owned(),
        needs: Vec::new(),
        host: None,
    }
}

fn needs(mut package: PlanPackage, dependency: &str, range: &str, optional: bool) -> PlanPackage {
    package
        .needs
        .push((dependency.to_owned(), range.to_owned(), optional));
    package
}

fn source(package: PlanPackage, rank: usize, sha: &str) -> SourcePackage {
    SourcePackage {
        package,
        sha256: sha.to_owned(),
        yanked: false,
        channel: "stable".to_owned(),
        source_rank: rank,
    }
}

fn input(
    incoming: Vec<PlanPackage>,
    installed: Vec<PlanPackage>,
    sources: Vec<SourcePackage>,
) -> ResolveInput {
    ResolveInput {
        incoming,
        installed,
        sources,
        channel: "stable".to_owned(),
        host_release: HOST_RELEASE.to_owned(),
        is_update: false,
        pins: Vec::new(),
    }
}

fn codes(
    result: &Result<ilium_plugin_host::resolve::ResolvedPlan, Vec<ilium_plugin_api::Diagnostic>>,
) -> Vec<String> {
    match result {
        Ok(_) => Vec::new(),
        Err(list) => list.iter().map(|d| d.code.as_str().to_owned()).collect(),
    }
}

fn step_list(
    result: Result<ilium_plugin_host::resolve::ResolvedPlan, Vec<ilium_plugin_api::Diagnostic>>,
) -> Vec<(PlanAction, String, String)> {
    result
        .unwrap_or_else(|d| panic!("plan expected: {d:?}"))
        .steps
        .into_iter()
        .map(|s| (s.action, s.id, s.version))
        .collect()
}

fn b_sources() -> Vec<SourcePackage> {
    vec![
        source(pkg("example.b", "1.0.0"), 0, "a1"),
        source(pkg("example.b", "1.2.0"), 0, "a2"),
        source(pkg("example.b", "2.0.0"), 0, "a3"),
    ]
}

/// REQ: PKG-330
#[test]
fn pkg_330_pins_sources_and_channel_are_inputs() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let mut pinned = input(
        incoming.clone(),
        vec![pkg("example.b", "1.0.0")],
        b_sources(),
    );
    pinned.pins = vec!["example.b".to_owned()];
    pinned.is_update = true;
    let steps = step_list(resolve_plan(&pinned));
    assert!(
        steps
            .iter()
            .any(|(a, id, v)| *a == PlanAction::Keep && id == "example.b" && v == "1.0.0"),
        "{steps:?}"
    );
    let unpinned = input(incoming, Vec::new(), b_sources());
    assert_eq!(step_list(resolve_plan(&unpinned))[0].2, "1.2.0");
}

/// REQ: PKG-331
#[test]
fn pkg_331_highest_version_satisfying_all_ranges_wins() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let installed = vec![needs(
        pkg("example.c", "1.0.0"),
        "example.b",
        "<1.2.0",
        false,
    )];
    let steps = step_list(resolve_plan(&input(incoming, installed, b_sources())));
    assert!(
        steps
            .iter()
            .any(|(_, id, v)| id == "example.b" && v == "1.0.0"),
        "{steps:?}"
    );
}

/// REQ: PKG-331
#[test]
fn pkg_331_yanked_version_is_not_chosen() {
    let mut sources = b_sources();
    sources[1].yanked = true;
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let steps = step_list(resolve_plan(&input(incoming, Vec::new(), sources)));
    assert!(
        steps
            .iter()
            .any(|(_, id, v)| id == "example.b" && v == "1.0.0"),
        "{steps:?}"
    );
}

/// REQ: PKG-331
#[test]
fn pkg_331_version_from_a_channel_not_permitted_is_not_chosen() {
    let mut sources = b_sources();
    sources[1].channel = "beta".to_owned();
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let steps = step_list(resolve_plan(&input(incoming, Vec::new(), sources)));
    assert!(
        steps
            .iter()
            .any(|(_, id, v)| id == "example.b" && v == "1.0.0"),
        "{steps:?}"
    );
}

/// REQ: PKG-332
#[test]
fn pkg_332_installed_version_is_preferred_over_a_higher_one_when_not_an_update() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let steps = step_list(resolve_plan(&input(
        incoming.clone(),
        vec![pkg("example.b", "1.0.0")],
        b_sources(),
    )));
    assert!(
        steps
            .iter()
            .any(|(a, id, v)| *a == PlanAction::Keep && id == "example.b" && v == "1.0.0"),
        "{steps:?}"
    );
    let mut update = input(incoming, vec![pkg("example.b", "1.0.0")], b_sources());
    update.is_update = true;
    let steps = step_list(resolve_plan(&update));
    assert!(
        steps
            .iter()
            .any(|(_, id, v)| id == "example.b" && v == "1.2.0"),
        "{steps:?}"
    );
}

/// REQ: PKG-333
#[test]
fn pkg_333_no_common_version_is_a_conflict_listing_constraints_and_versions() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^2.0", false)];
    let installed = vec![needs(pkg("example.c", "1.0.0"), "example.b", "^1.0", false)];
    let result = resolve_plan(&input(incoming, installed, b_sources()));
    assert_eq!(codes(&result), vec!["dependency.conflict".to_owned()]);
    let message = result.expect_err("err")[0].message.clone();
    for fragment in ["example.a", "example.c", "^2.0", "^1.0", "1.2.0"] {
        assert!(
            message.contains(fragment),
            "{fragment} missing from {message}"
        );
    }
}

/// REQ: PKG-334
#[test]
fn pkg_334_every_missing_id_is_listed_with_its_range() {
    let incoming = vec![needs(
        needs(pkg("example.a", "1.0.0"), "example.x", "^1.0", false),
        "example.y",
        "~2.1",
        false,
    )];
    let result = resolve_plan(&input(incoming, Vec::new(), Vec::new()));
    assert_eq!(codes(&result), vec!["dependency.unresolved".to_owned(); 2]);
    let text: String = result
        .expect_err("err")
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    for fragment in ["example.x", "^1.0", "example.y", "~2.1"] {
        assert!(text.contains(fragment), "{fragment} missing from {text}");
    }
}

/// REQ: PKG-335
#[test]
fn pkg_335_hard_cycle_is_refused_and_named_in_order() {
    let a = needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false);
    let b = needs(pkg("example.b", "1.0.0"), "example.a", "^1.0", false);
    let result = resolve_plan(&input(vec![a, b], Vec::new(), Vec::new()));
    assert_eq!(codes(&result), vec!["dependency.cycle".to_owned()]);
    let message = result.expect_err("err")[0].message.clone();
    let a_at = message.find("example.a").expect("a named");
    let b_at = message.find("example.b").expect("b named");
    assert!(a_at < b_at, "{message}");
}

/// REQ: PKG-335
#[test]
fn pkg_335_cycle_through_an_optional_edge_never_blocks() {
    let a = needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false);
    let b = needs(pkg("example.b", "1.0.0"), "example.a", "^1.0", true);
    assert!(resolve_plan(&input(vec![a, b], Vec::new(), Vec::new())).is_ok());
}

/// REQ: PKG-336
#[test]
fn pkg_336_tie_break_highest_version_then_source_order_then_lowest_sha() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let sources = vec![
        source(pkg("example.b", "1.1.0"), 1, "ff"),
        source(pkg("example.b", "1.2.0"), 1, "ee"),
        source(pkg("example.b", "1.2.0"), 0, "dd"),
        source(pkg("example.b", "1.2.0"), 0, "cc"),
    ];
    let first = resolve_plan(&input(incoming.clone(), Vec::new(), sources.clone())).expect("plan");
    let chosen = first.chosen_source_sha("example.b").expect("sha");
    assert_eq!(chosen, "cc");
    let mut reversed = sources;
    reversed.reverse();
    let second = resolve_plan(&input(incoming, Vec::new(), reversed)).expect("plan");
    assert_eq!(
        second.chosen_source_sha("example.b").expect("sha"),
        "cc",
        "deterministic"
    );
}

/// REQ: PKG-336
#[test]
fn pkg_336_source_order_wins_over_sha_when_versions_tie() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let sources = vec![
        source(pkg("example.b", "1.2.0"), 1, "00"),
        source(pkg("example.b", "1.2.0"), 0, "ff"),
    ];
    let plan = resolve_plan(&input(incoming, Vec::new(), sources)).expect("plan");
    assert_eq!(plan.chosen_source_sha("example.b").expect("sha"), "ff");
}

/// REQ: PKG-337
#[test]
fn pkg_337_dry_run_prints_the_plan_and_changes_nothing() {
    let fx = Fx::new();
    let before = fx.snapshot_tree(&fx.data_dir());
    let out = fx.install_with(
        &Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", true),
        &["--dry-run"],
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(out.all_text().contains("example.a"));
    assert_eq!(fx.snapshot_tree(&fx.data_dir()), before);
}

/// REQ: PKG-338
#[test]
fn pkg_338_none_installed_highest_caret_match_then_dependant() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let steps = step_list(resolve_plan(&input(incoming, Vec::new(), b_sources())));
    assert_eq!(
        steps,
        vec![
            (
                PlanAction::Install,
                "example.b".to_owned(),
                "1.2.0".to_owned()
            ),
            (
                PlanAction::Install,
                "example.a".to_owned(),
                "1.0.0".to_owned()
            )
        ]
    );
}

/// REQ: PKG-339
#[test]
fn pkg_339_installed_dependency_is_kept() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let steps = step_list(resolve_plan(&input(
        incoming,
        vec![pkg("example.b", "1.0.0")],
        b_sources(),
    )));
    assert_eq!(
        steps,
        vec![
            (PlanAction::Keep, "example.b".to_owned(), "1.0.0".to_owned()),
            (
                PlanAction::Install,
                "example.a".to_owned(),
                "1.0.0".to_owned()
            )
        ]
    );
}

/// REQ: PKG-340
#[test]
fn pkg_340_range_forces_a_dependency_update_before_the_dependant() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.1", false)];
    let steps = step_list(resolve_plan(&input(
        incoming,
        vec![pkg("example.b", "1.0.0")],
        b_sources(),
    )));
    assert_eq!(
        steps,
        vec![
            (
                PlanAction::Update,
                "example.b".to_owned(),
                "1.2.0".to_owned()
            ),
            (
                PlanAction::Install,
                "example.a".to_owned(),
                "1.0.0".to_owned()
            )
        ]
    );
}

/// REQ: PKG-341
#[test]
fn pkg_341_two_dependants_with_disjoint_ranges_conflict_naming_both() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^2.0", false)];
    let installed = vec![
        needs(pkg("example.c", "1.0.0"), "example.b", "^1.0", false),
        pkg("example.b", "1.0.0"),
    ];
    let result = resolve_plan(&input(incoming, installed, b_sources()));
    assert_eq!(codes(&result), vec!["dependency.conflict".to_owned()]);
    let message = result.expect_err("err")[0].message.clone();
    assert!(
        message.contains("example.a") && message.contains("example.c"),
        "{message}"
    );
}

/// REQ: PKG-342
#[test]
fn pkg_342_no_source_has_the_dependency() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    assert_eq!(
        codes(&resolve_plan(&input(incoming, Vec::new(), Vec::new()))),
        vec!["dependency.unresolved".to_owned()]
    );
}

/// REQ: PKG-343
#[test]
fn pkg_343_hard_mutual_requirement_is_a_cycle() {
    let a = needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false);
    let sources = vec![source(
        needs(pkg("example.b", "1.0.0"), "example.a", "^1.0", false),
        0,
        "b1",
    )];
    assert_eq!(
        codes(&resolve_plan(&input(vec![a], Vec::new(), sources))),
        vec!["dependency.cycle".to_owned()]
    );
}

/// REQ: PKG-344
#[test]
fn pkg_344_optional_missing_dependency_installs_the_dependant_only() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", true)];
    let steps = step_list(resolve_plan(&input(incoming, Vec::new(), Vec::new())));
    assert_eq!(
        steps,
        vec![(
            PlanAction::Install,
            "example.a".to_owned(),
            "1.0.0".to_owned()
        )]
    );
}

/// REQ: PKG-345
#[test]
fn pkg_345_missing_service_provider_installs_and_waits() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0").service("example:svc/s@0.1.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["state"], "waiting-dependency");
    let listed = fx.run(&["list", "--json"]);
    assert!(
        listed.all_text().contains("example.a") && listed.all_text().contains("waiting-dependency"),
        "{}",
        listed.all_text()
    );
}

/// REQ: PKG-345
#[test]
fn pkg_345_waiting_record_names_the_missing_service() {
    let fx = Fx::new();
    let out = fx.install(&Spec::new("example.a", "1.0.0").service("example:svc/s@0.1.0"));
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    let record = fx.record("example.a").expect("record");
    assert_eq!(record["state"], "waiting-dependency");
    assert!(
        record.to_string().contains("example:svc/s"),
        "the waiting record must name service S: {record}"
    );
}

/// REQ: PKG-346
#[test]
fn pkg_346_host_range_above_running_release_is_host_version() {
    let mut package = pkg("example.a", "1.0.0");
    package.host = Some(">=9.0.0".to_owned());
    let result = resolve_plan(&input(vec![package], Vec::new(), Vec::new()));
    assert_eq!(codes(&result), vec!["dependency.host-version".to_owned()]);
}

/// REQ: PKG-346
#[test]
fn pkg_346_host_range_equal_to_running_release_passes() {
    let mut package = pkg("example.a", "1.0.0");
    package.host = Some(">=0.1.0".to_owned());
    assert!(resolve_plan(&input(vec![package], Vec::new(), Vec::new())).is_ok());
}

/// REQ: PKG-347
#[test]
fn pkg_347_only_a_prerelease_exists_on_the_stable_channel() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let sources = vec![source(pkg("example.b", "1.3.0-beta.1"), 0, "b1")];
    assert_eq!(
        codes(&resolve_plan(&input(
            incoming.clone(),
            Vec::new(),
            sources.clone()
        ))),
        vec!["dependency.unresolved".to_owned()]
    );
    let mut beta = input(
        incoming,
        Vec::new(),
        vec![SourcePackage {
            channel: "beta".to_owned(),
            ..sources[0].clone()
        }],
    );
    beta.channel = "beta".to_owned();
    assert!(resolve_plan(&beta).is_ok());
}

/// REQ: PKG-348
#[test]
fn pkg_348_update_that_strands_a_dependant_is_a_conflict_offering_update_all() {
    let installed = vec![
        needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false),
        pkg("example.b", "1.0.0"),
    ];
    let mut request = input(vec![pkg("example.b", "2.0.0")], installed, b_sources());
    request.is_update = true;
    let result = resolve_plan(&request);
    assert_eq!(codes(&result), vec!["dependency.conflict".to_owned()]);
    let message = result.expect_err("err")[0].message.clone();
    assert!(
        message.contains("update --all") || message.contains("update example.b example.a"),
        "{message}"
    );
}

/// REQ: PKG-348
#[test]
fn pkg_348_update_staying_inside_the_dependant_range_is_fine() {
    let installed = vec![
        needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false),
        pkg("example.b", "1.0.0"),
    ];
    let mut request = input(vec![pkg("example.b", "1.2.0")], installed, b_sources());
    request.is_update = true;
    assert!(resolve_plan(&request).is_ok());
}

// ---------------------------------------------------------------- uninstall of a required plugin

fn b_then_a(fx: &Fx) {
    assert_eq!(fx.install(&Spec::new("example.b", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", false))
            .exit_code,
        0
    );
}

/// REQ: PKG-349
#[test]
fn pkg_349_uninstall_of_a_required_plugin_is_refused_in_use() {
    let fx = Fx::new();
    b_then_a(&fx);
    let out = fx.run(&["uninstall", "example.b"]);
    assert_eq!(out.exit_code, 1);
    assert!(
        out.has_error("dependency.in-use"),
        "{:?}",
        out.error_codes()
    );
    assert!(fx.record("example.b").is_some());
}

/// REQ: PKG-349
#[test]
fn pkg_349_force_removes_it_and_disables_the_dependant_with_a_reason() {
    let fx = Fx::new();
    b_then_a(&fx);
    let out = fx.run(&["uninstall", "example.b", "--force"]);
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(fx.record("example.b").is_none());
    let dependant = fx.record("example.a").expect("dependant stays installed");
    assert_ne!(dependant["state"], "installed-enabled");
    assert!(fx
        .run(&["list", "--json"])
        .all_text()
        .contains("requires example.b"));
}

/// REQ: PKG-349
#[test]
fn pkg_349_optional_dependants_do_not_block() {
    let fx = Fx::new();
    assert_eq!(fx.install(&Spec::new("example.b", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.install(&Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", true))
            .exit_code,
        0
    );
    assert_eq!(fx.run(&["uninstall", "example.b"]).exit_code, 0);
}

/// REQ: PKG-349
#[test]
fn pkg_349_installing_b_again_re_enables_the_dependant() {
    let fx = Fx::new();
    b_then_a(&fx);
    assert_eq!(fx.run(&["uninstall", "example.b", "--force"]).exit_code, 0);
    assert_eq!(fx.install(&Spec::new("example.b", "1.0.0")).exit_code, 0);
    assert_eq!(
        fx.record("example.a").expect("record")["state"],
        "installed-enabled"
    );
}

/// REQ: PKG-337
#[test]
fn pkg_337_dry_run_plan_names_the_dependency_it_would_install() {
    let fx = Fx::new();
    let out = fx.install_with(
        &Spec::new("example.a", "1.0.0").needs("example.b", "^1.0", true),
        &["--dry-run"],
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    assert!(
        out.all_text().contains("example.b"),
        "the printed plan must name the dependency: {}",
        out.all_text()
    );
}

/// REQ: PKG-342
#[test]
fn pkg_342_a_source_with_a_version_outside_the_range_does_not_satisfy_the_dependency() {
    let incoming = vec![needs(pkg("example.a", "1.0.0"), "example.b", "^1.0", false)];
    let sources = vec![source(pkg("example.b", "0.9.0"), 0, &"b".repeat(64))];
    assert_eq!(
        codes(&resolve_plan(&input(incoming, Vec::new(), sources))),
        vec!["dependency.unresolved".to_owned()]
    );
}
