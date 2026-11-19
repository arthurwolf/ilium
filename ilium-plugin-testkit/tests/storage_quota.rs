//! The native host must report storage refusals to the plugin making the write.

use std::cell::RefCell;

use ilium_plugin_sdk_core::generated::host::{Bucket, HostStorage};
use ilium_plugin_sdk_core::types::{
    Activation, DeactivationReason, ErrorCode, Event, PluginError, Submission,
};
use ilium_plugin_sdk_core::{host, NativeEntry};
use ilium_plugin_testkit::FakeHost;

thread_local! {
    static WRITE_RESULT: RefCell<Option<Result<(), PluginError>>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct QuotaProbe;

impl NativeEntry for QuotaProbe {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        let result = HostStorage::put(
            &mut *host(),
            Bucket::Project,
            "too-large".to_owned(),
            vec![1; 1024 * 1024 + 1],
        );
        WRITE_RESULT.with(|slot| *slot.borrow_mut() = Some(result));
        Ok(())
    }

    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }

    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

#[test]
fn an_oversized_write_returns_an_invalid_argument_error_instead_of_success() {
    let mut fake = FakeHost::new();
    fake.activate::<QuotaProbe>();
    let result = WRITE_RESULT.with(|slot| slot.borrow_mut().take()).expect("write ran");
    assert_eq!(
        result.expect_err("oversized write is refused").code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(fake.storage().get(Bucket::Project, "too-large"), None);
}
