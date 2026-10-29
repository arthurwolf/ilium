use ilium_plugin_api::lifecycle::{
    next_instance_state, next_package_state, InstanceInput as I, InstanceState as S,
    PackageInput as P, PackageState as K, RestartDecision as D, RestartPolicy,
};

#[test]
fn restart_window_expires_exactly_at_ten_minutes_and_reset_is_immediate() {
    let mut policy = RestartPolicy::default();
    assert_eq!(policy.on_failure(0), D::RestartAfterMs(1000));
    assert_eq!(policy.on_failure(1000), D::RestartAfterMs(5000));
    assert_eq!(policy.on_failure(600000), D::RestartAfterMs(5000));
    assert_eq!(policy.failures_in_window(600000), 2);
    policy.reset();
    assert_eq!(policy.on_failure(600000), D::RestartAfterMs(1000));
}

#[test]
fn failure_count_stays_exact_after_restart_limit() {
    let mut policy = RestartPolicy::default();
    for now in 0..5 {
        policy.on_failure(now);
    }
    assert_eq!(policy.failures_in_window(5), 5);
    assert_eq!(policy.failures_in_window(600002), 2);
}

#[test]
fn disabled_completion_cannot_make_an_instance_runnable() {
    let stopping = next_instance_state(S::Active, &I::Disable).unwrap();
    assert_eq!(stopping, S::Deactivating);
    let stopped = next_instance_state(stopping, &I::DeactivateDoneStopped).unwrap();
    assert_eq!(stopped, S::Stopped);
    assert!(next_instance_state(stopped, &I::ActivateOk).is_err());
    assert_eq!(
        next_instance_state(stopped, &I::Enable).unwrap(),
        S::Inactive
    );
}

#[test]
fn dependency_expiry_is_not_a_restart_and_policy_cannot_be_enabled_implicitly() {
    assert_eq!(
        next_instance_state(S::WaitingDependency, &I::DependencyWaitExpired).unwrap(),
        S::Inactive
    );
    assert!(next_package_state(K::DisabledByPolicy, &P::Enable).is_err());
    assert!(next_package_state(K::DisabledBlocked, &P::Enable).is_err());
    assert!(next_package_state(K::RefusedAtLoad, &P::Enable).is_err());
}
