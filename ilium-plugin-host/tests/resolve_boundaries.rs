//! Pure dependency resolver boundaries; no registry or filesystem fixtures.
use ilium_plugin_host::resolve::{
    parse_point_reference, parse_service_requirement, resolve_plan, PlanAction, PlanPackage,
    PluginVersion, ResolveInput, SourcePackage, VersionRange,
};

fn package(id: &str, version: &str, needs: &[(&str, &str, bool)]) -> PlanPackage {
    PlanPackage {
        id: id.into(),
        version: version.into(),
        needs: needs
            .iter()
            .map(|(id, range, optional)| ((*id).into(), (*range).into(), *optional))
            .collect(),
        host: None,
    }
}
fn source(package: PlanPackage, rank: usize, digest: &str) -> SourcePackage {
    SourcePackage {
        package,
        source_rank: rank,
        sha256: digest.into(),
        channel: "stable".into(),
        yanked: false,
    }
}
fn input(incoming: Vec<PlanPackage>, sources: Vec<SourcePackage>) -> ResolveInput {
    ResolveInput {
        incoming,
        sources,
        installed: Vec::new(),
        channel: "stable".into(),
        host_release: "0.1.0".into(),
        is_update: false,
        pins: Vec::new(),
    }
}

#[test]
fn prerelease_order_is_numeric_and_metadata_does_not_affect_identity() {
    let version = |text| PluginVersion::parse(text).unwrap();
    assert!(version("1.0.0-beta.2") < version("1.0.0-beta.11"));
    assert!(version("1.0.0-beta.11") < version("1.0.0"));
    assert_eq!(version("1.0.0+one"), version("1.0.0+two"));
    assert!(PluginVersion::parse("2147483648.0.0").is_err());
}

#[test]
fn prerelease_admission_is_global_to_the_range_not_each_comparator() {
    let range = VersionRange::parse(">=1.2.0-beta.1 <2.0.0").unwrap();
    assert!(range.matches(&PluginVersion::parse("1.2.0-beta.2").unwrap(), false));
    assert!(!range.matches(&PluginVersion::parse("1.3.0-beta.1").unwrap(), false));
    assert!(range.matches(&PluginVersion::parse("1.3.0-beta.1").unwrap(), true));
    for invalid in [">= 1.0", "1.*", "1.0 || 2.0", "^1.0  <2.0"] {
        assert!(VersionRange::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn backtracking_reconciles_constraints_from_later_dependants() {
    let request = input(
        vec![
            package("example.a", "1.0.0", &[("example.b", "^1", false)]),
            package("example.z", "1.0.0", &[("example.b", "<1.1.0", false)]),
        ],
        vec![
            source(package("example.b", "1.2.0", &[]), 0, "high"),
            source(package("example.b", "1.0.0", &[]), 0, "low"),
        ],
    );
    let plan = resolve_plan(&request).unwrap();
    assert_eq!(plan.steps[0].id, "example.b");
    assert_eq!(plan.steps[0].version, "1.0.0");
    assert_eq!(plan.chosen_source_sha("example.b").as_deref(), Some("low"));
}

#[test]
fn optional_cycles_do_not_block_and_hard_cycles_do() {
    let a = package("example.a", "1.0.0", &[("example.b", "*", false)]);
    let b = package("example.b", "1.0.0", &[("example.a", "*", true)]);
    assert!(resolve_plan(&input(vec![a.clone(), b], Vec::new())).is_ok());
    let b = package("example.b", "1.0.0", &[("example.a", "*", false)]);
    assert_eq!(
        resolve_plan(&input(vec![a, b], Vec::new())).unwrap_err()[0].code,
        "dependency.cycle"
    );
}

#[test]
fn installed_preference_pins_and_optional_absence_are_preserved() {
    let mut request = input(
        vec![package(
            "example.a",
            "1.0.0",
            &[("example.b", "^1", false), ("example.missing", "*", true)],
        )],
        vec![source(package("example.b", "1.2.0", &[]), 0, "new")],
    );
    request.installed = vec![package("example.b", "1.0.0", &[])];
    let plan = resolve_plan(&request).unwrap();
    assert_eq!(plan.steps[0].action, PlanAction::Keep);
    request.is_update = true;
    request.pins = vec!["example.b".into()];
    assert_eq!(resolve_plan(&request).unwrap().steps[0].version, "1.0.0");
    request.incoming[0].needs[0].1 = "^2".into();
    assert_eq!(
        resolve_plan(&request).unwrap_err()[0].code,
        "dependency.conflict"
    );
}

#[test]
fn service_full_versions_and_point_ranges_remain_distinct() {
    assert!(parse_service_requirement("example.host/thing@1.2.0").is_ok());
    assert!(parse_service_requirement("example.host/thing@^1.2").is_err());
    let point = parse_point_reference("example.host/toolbar@1.2").unwrap();
    assert!(point
        .range
        .matches(&PluginVersion::parse("1.9.0").unwrap(), false));
    assert!(!point
        .range
        .matches(&PluginVersion::parse("2.0.0").unwrap(), false));
}

#[test]
fn dependency_capacity_is_exactly_thirty_two_without_dropping_entries() {
    let mut root = package("example.root", "1.0.0", &[]);
    root.needs = (0..32)
        .map(|index| (format!("example.optional{index}"), "*".into(), true))
        .collect();
    assert!(resolve_plan(&input(vec![root.clone()], Vec::new())).is_ok());
    root.needs
        .push(("example.overflow".into(), "*".into(), true));
    assert_eq!(
        resolve_plan(&input(vec![root], Vec::new())).unwrap_err()[0].code,
        "manifest.requires-invalid"
    );
}
