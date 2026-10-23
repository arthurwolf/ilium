use ilium_plugin_host::lifecycle::{
    SafeMode, StartNode, resolve_safe_mode, shutdown_order, start_order,
};

fn node(id: &str, tier: u8, plugins: &[&str], services: &[&str]) -> StartNode {
    StartNode {
        id: id.into(),
        tier,
        requires_plugins: plugins.iter().map(|id| (*id).into()).collect(),
        requires_service_providers: services.iter().map(|id| (*id).into()).collect(),
    }
}
fn flags(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

#[test]
fn flags_override_even_invalid_environment_values() {
    assert_eq!(
        resolve_safe_mode(&flags(&["--safe-mode"]), Some("invalid")),
        Ok(SafeMode::ThirdParty)
    );
    assert_eq!(
        resolve_safe_mode(&flags(&["--safe-plugins", "--no-plugins"]), Some("invalid")),
        Ok(SafeMode::NoPlugins)
    );
    assert_eq!(
        resolve_safe_mode(&flags(&["--no-plugins", "--safe-mode=all-optional"]), None),
        Ok(SafeMode::NoPlugins)
    );
}

#[test]
fn invalid_flag_values_fail_even_when_the_host_would_be_disabled() {
    for value in [
        "--safe-mode=",
        "--safe-mode=0",
        "--safe-mode=1",
        "--safe-mode=off",
        "--no-plugins=true",
        "--safe-plugins=true",
    ] {
        let error = resolve_safe_mode(&flags(&["--no-plugins", value]), Some("0")).unwrap_err();
        assert_eq!(error.code, "usage.invalid-value");
        assert_eq!(error.exit_code, 2);
    }
}

#[test]
fn environment_values_are_exact_and_option_terminator_is_respected() {
    for value in ["", " true", "third-party ", "OFF"] {
        assert!(resolve_safe_mode(&[], Some(value)).is_err());
    }
    assert_eq!(
        resolve_safe_mode(&flags(&["--", "--no-plugins"]), Some("all-optional")),
        Ok(SafeMode::AllOptional)
    );
    assert_eq!(resolve_safe_mode(&[], Some("1")), Ok(SafeMode::ThirdParty));
    assert_eq!(resolve_safe_mode(&[], Some("0")), Ok(SafeMode::Off));
}

#[test]
fn newly_ready_nodes_compete_with_all_other_ready_nodes() {
    let nodes = [
        node("consumer", 1, &["provider"], &[]),
        node("provider", 2, &[], &[]),
        node("z", 3, &[], &[]),
    ];
    assert_eq!(start_order(&nodes).unwrap(), ["provider", "consumer", "z"]);
    let nodes = [
        node("c", 1, &["d"], &[]),
        node("d", 3, &[], &[]),
        node("a", 2, &[], &[]),
        node("b", 1, &[], &[]),
    ];
    assert_eq!(start_order(&nodes).unwrap(), ["b", "a", "d", "c"]);
}

#[test]
fn repeated_and_shared_provider_edges_count_once() {
    let nodes = [
        node(
            "consumer",
            1,
            &["provider", "provider"],
            &["provider", "provider"],
        ),
        node("provider", 3, &[], &[]),
    ];
    assert_eq!(start_order(&nodes).unwrap(), ["provider", "consumer"]);
    assert_eq!(shutdown_order(&nodes).unwrap(), ["consumer", "provider"]);
}

#[test]
fn missing_requirements_never_return_partial_activation_order() {
    let nodes = [
        node("independent", 1, &[], &[]),
        node("consumer", 2, &["missing"], &[]),
    ];
    let errors = start_order(&nodes).unwrap_err();
    assert_eq!(errors[0].code.as_str(), "dependency.unresolved");
    assert!(errors[0].message.contains("missing"));
    assert!(shutdown_order(&nodes).is_err());
}

#[test]
fn cycles_name_the_actual_cycle_and_distinguish_service_edges() {
    let nodes = [
        node("blocked", 1, &["a"], &[]),
        node("a", 2, &["b"], &[]),
        node("b", 3, &["a"], &[]),
    ];
    let errors = start_order(&nodes).unwrap_err();
    assert_eq!(errors[0].code.as_str(), "dependency.cycle");
    assert!(errors[0].message.contains("a -> b -> a"));
    assert!(!errors[0].message.contains("blocked"));
    assert_eq!(
        start_order(&[node("a", 1, &[], &["a"])]).unwrap_err()[0]
            .code
            .as_str(),
        "manifest.service-cycle"
    );
}

#[test]
fn duplicate_ids_and_invalid_tiers_fail_closed_and_empty_graph_is_valid() {
    assert!(start_order(&[node("same", 1, &[], &[]), node("same", 2, &[], &[])]).is_err());
    for tier in [0, 4, u8::MAX] {
        assert!(start_order(&[node("bad", tier, &[], &[])]).is_err());
    }
    assert!(start_order(&[]).unwrap().is_empty());
    assert!(shutdown_order(&[]).unwrap().is_empty());
}
