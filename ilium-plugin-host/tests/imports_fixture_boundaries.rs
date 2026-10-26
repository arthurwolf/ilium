use std::time::Duration;
use ilium_plugin_api::{ErrorCode, Side, Tier, generated::{host::{Bucket, log::Level}, primitives::basic::HostBasic}};
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;

#[test]
fn shared_manual_clock_preserves_wall_steps_and_advances_log_windows() {
    let mut fixture=ImportsFixture::builder().tier(Tier::T2).build();
    assert_eq!(fixture.clock().now_unix_millis(),1_893_456_000_000);
    for index in 0..205 {fixture.log().log(Level::Info,format!("entry {index}"));}
    assert_eq!(fixture.logs().len(),200);
    fixture.advance(Duration::from_secs(1));
    assert!(fixture.logs().iter().any(|line|line.level=="warn" && line.message.contains("5 lines dropped")));
    let monotonic=fixture.clock().monotonic_millis();
    fixture.set_wall_clock_unix_millis(10);
    assert_eq!(fixture.clock().now_unix_millis(),10);
    assert_eq!(fixture.clock().monotonic_millis(),monotonic);
    fixture.log().log(Level::Info,"after correction".into());
    assert!(fixture.logs().iter().any(|line|line.message=="after correction"));
}
#[test]
fn storage_is_real_isolated_and_manual_lease_expiry_uses_the_same_engine() {
    let mut first=ImportsFixture::builder().build();
    let mut second=ImportsFixture::builder().build();
    for bucket in [Bucket::Global,Bucket::Project,Bucket::Session,Bucket::Shared] {
        first.storage().put(bucket,"key".into(),vec![1,2,3]).unwrap();
        assert_eq!(first.storage().get(bucket,"key".into()),Some(vec![1,2,3]));
        assert_eq!(second.storage().get(bucket,"key".into()),None);
    }
    assert!(first.storage().lease("refresh".into(),100));
    assert!(!first.other_caller_lease("refresh",100));
    first.advance(Duration::from_millis(99));
    assert!(!first.other_caller_lease("refresh",100));
    first.advance(Duration::from_millis(1));
    assert!(first.other_caller_lease("refresh",100));
    assert!(!first.storage().lease("refresh".into(),100));
}
#[test]
fn stopped_storage_reports_real_refusal_and_cannot_write() {
    let mut fixture=ImportsFixture::builder().side(Side::Client).build();
    fixture.storage().put(Bucket::Global,"key".into(),vec![1]).unwrap();
    fixture.stop_plugin();
    assert!(fixture.plugin_is_stopped());
    assert_eq!(fixture.storage().put(Bucket::Global,"key".into(),vec![2]).unwrap_err().code,ErrorCode::NotPermitted);
    assert_eq!(fixture.storage().get(Bucket::Global,"key".into()),None);
    assert_eq!(fixture.last_import_error_code(),Some(ErrorCode::NotPermitted));
    assert!(fixture.replace_grants(&["tree.metadata"]).is_err());
}
#[test]
fn registered_primitive_uses_current_gate_and_owner_even_for_retained_handles() {
    let mut fixture=ImportsFixture::builder().manifest_json(r#"{"id":"example.fixture","contributes":{"pane_kinds":[{"id":"example.fixture.notes","primitive":"basic"}]}}"#).grants(&["panes.provide-kind","tree.metadata"]).build();
    let pane=fixture.add_basic_pane("example.fixture.notes","Before",80,24);
    let terminal=fixture.add_terminal_pane("Terminal",80,24);
    let mut basic=fixture.basic();
    basic.set_title(pane,"After".into()).unwrap();
    assert_eq!(fixture.kernel().pane_title(pane),Some("After".into()));
    assert_eq!(basic.set_title(terminal,"Forbidden".into()).unwrap_err().code,ErrorCode::NotPermitted);
    fixture.replace_grants(&[]).unwrap();
    assert_eq!(basic.set_dirty(pane,true).unwrap_err().code,ErrorCode::NotPermitted);
    assert!(!fixture.kernel().pane_is_dirty(pane));
    assert!(fixture.logs().iter().any(|line|line.message.contains("refused")));
    drop(fixture);
    assert_eq!(basic.set_title(pane,"After owner dropped".into()).unwrap_err().code,ErrorCode::NotPermitted);
}
#[test]
fn invalid_configuration_fails_setup_instead_of_creating_an_admitted_instance() {
    assert!(ImportsFixture::builder().manifest_json("{").try_build().is_err());
    assert!(ImportsFixture::builder().manifest_json(r#"{"id":"../escape"}"#).try_build().is_err());
    assert!(ImportsFixture::builder().grants(&["unknown.authority"]).try_build().is_err());
    assert!(ImportsFixture::builder().limit("not_a_limit",10).try_build().is_err());
}
#[test]
fn future_bucket_version_is_observed_by_the_real_storage_adapter() {
    let mut fixture=ImportsFixture::builder().build();
    fixture.storage().put(Bucket::Global,"key".into(),vec![1]).unwrap();
    fixture.make_bucket_data_version_newer(Bucket::Global);
    assert_eq!(fixture.storage().get(Bucket::Global,"key".into()),None);
    assert_eq!(fixture.last_import_error_code(),Some(ErrorCode::Conflict));
}
