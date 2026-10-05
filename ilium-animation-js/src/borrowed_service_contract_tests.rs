//! Native service copies must admit destination storage before copying borrowed planes.
use super::*;

#[test]
fn borrowed_u16_completion_keeps_independent_bytes_and_debit_until_last_alias_drops() {
    let (_serial, quota) = super::inventory_contracts::fixture_lock();
    let baseline = quota.snapshot().worker_bytes;
    let source_admission = quota.reserve_external_storage(4096).unwrap();
    let source = vec![0x34, 0x12, 0xff, 0xff];
    let source_charge = quota.snapshot().worker_bytes;
    let metadata = serde_json::json!({"ok":true,"value":{"blocks":{"$ilium_binary":"b0"}}});
    let arrays = [ArraySpec {
        name: "b0".into(),
        kind: TypedArrayKind::U16,
        elements: 2,
    }];
    let borrowed = BTreeMap::from([("b0".to_owned(), source.as_slice())]);
    let value = ServiceValue::copy_from_borrowed_host(
        &metadata,
        &arrays,
        &borrowed,
        &EngineLimits::default(),
        quota.clone(),
    )
    .unwrap();
    assert_eq!(value.planes()["b0"], source);
    assert!(quota.snapshot().worker_bytes > source_charge);
    assert_ne!(value.planes()["b0"].as_ptr(), source.as_ptr());
    drop(borrowed);
    drop(source);
    drop(source_admission);
    assert_eq!(value.planes()["b0"], [0x34, 0x12, 0xff, 0xff]);
    let copied_charge = quota.snapshot().worker_bytes;
    assert!(copied_charge > baseline);
    let alias = value.clone();
    drop(value);
    assert_eq!(quota.snapshot().worker_bytes, copied_charge);
    assert_eq!(alias.arrays()[0].kind, TypedArrayKind::U16);
    drop(alias);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}

#[test]
fn malformed_borrowed_plane_shape_leaves_original_quota_unchanged() {
    let (_serial, quota) = super::inventory_contracts::fixture_lock();
    let baseline = quota.snapshot().worker_bytes;
    let source = [0_u8; 3];
    let metadata = serde_json::json!({"ok":true,"value":{"$ilium_binary":"b0"}});
    let arrays = [ArraySpec {
        name: "b0".into(),
        kind: TypedArrayKind::U16,
        elements: 2,
    }];
    let borrowed = BTreeMap::from([("b0".to_owned(), source.as_slice())]);
    assert!(ServiceValue::copy_from_borrowed_host(
        &metadata,
        &arrays,
        &borrowed,
        &EngineLimits::default(),
        quota.clone(),
    )
    .is_err());
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}

#[test]
fn borrowed_native_completion_refuses_when_original_root_has_no_copy_budget() {
    let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 1,
        input_bytes: 4096,
        result_bytes: 4096,
        worker_threads: 1,
        worker_bytes: 0,
    });
    let source = [1_u8, 0];
    let metadata = serde_json::json!({"ok":true,"value":{"$ilium_binary":"b0"}});
    let arrays = [ArraySpec {
        name: "b0".into(),
        kind: TypedArrayKind::U16,
        elements: 1,
    }];
    let borrowed = BTreeMap::from([("b0".to_owned(), source.as_slice())]);
    assert!(ServiceValue::copy_from_borrowed_host(
        &metadata,
        &arrays,
        &borrowed,
        &EngineLimits::default(),
        quota.clone(),
    )
    .is_err());
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
