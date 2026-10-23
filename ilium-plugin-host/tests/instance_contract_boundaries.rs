//! Production lifecycle ownership and ABI boundaries, without a Wasm backend.
use std::sync::{Arc, Mutex};

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_api::{ErrorCode, PluginError};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::instance::{ExportCall, ExportReply, GuestExports, PluginInstance};

#[derive(Default)]
struct Observations {
    activates: usize,
    events: usize,
    deactivates: usize,
    drops: usize,
    restore_size: Option<usize>,
}

struct Guest {
    observations: Arc<Mutex<Observations>>,
    submissions: usize,
    panic_events: bool,
    activation_error: bool,
    saved_size: usize,
}

impl Drop for Guest {
    fn drop(&mut self) { self.observations.lock().unwrap().drops += 1; }
}

impl GuestExports for Guest {
    fn activate(&mut self, activation: Activation) -> Result<Result<(), PluginError>, Trap> {
        let mut observations = self.observations.lock().unwrap();
        observations.activates += 1;
        observations.restore_size = activation.restored.as_ref().map(Vec::len);
        Ok(if self.activation_error { Err(PluginError::new(ErrorCode::InvalidArgument, "bad settings")) } else { Ok(()) })
    }
    fn deactivate(&mut self, _: DeactivationReason) -> Result<Option<Vec<u8>>, Trap> {
        self.observations.lock().unwrap().deactivates += 1;
        Ok(Some(vec![0; self.saved_size]))
    }
    fn on_events(&mut self, _: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        self.observations.lock().unwrap().events += 1;
        assert!(!self.panic_events, "fixture panic");
        Ok((0..self.submissions).map(|correlation| Submission {
            intent: Intent::PlaySound("bell".to_owned()), correlation: correlation as u64, dedup_key: None,
        }).collect())
    }
    fn render(&mut self, _: ViewTarget, _: Area) -> Result<UiTree, Trap> { Ok(UiTree::default()) }
    fn call_export(&mut self, _: ExportCall) -> Result<ExportReply, Trap> { Ok(ExportReply::Job(Ok(vec![1]))) }
}

fn fixture() -> (Guest, Arc<Mutex<Observations>>) {
    let observations = Arc::new(Mutex::new(Observations::default()));
    (Guest { observations: observations.clone(), submissions: 0, panic_events: false, activation_error: false, saved_size: 0 }, observations)
}

#[test]
fn lifecycle_calls_are_owned_and_teardown_consumes_the_guest_once() {
    let (guest, observations) = fixture();
    let mut instance = PluginInstance::from_guest(Box::new(guest));
    assert!(instance.on_events(Vec::new()).is_err());
    instance.activate(Activation::default()).unwrap();
    assert!(instance.activate(Activation::default()).is_err());
    assert_eq!(instance.state(), InstanceState::Active);
    assert_eq!(instance.deactivate(DeactivationReason::Shutdown), Some(Vec::new()));
    assert_eq!(instance.deactivate(DeactivationReason::Shutdown), None);
    assert!(instance.on_events(Vec::new()).is_err());
    assert!(instance.activate(Activation::default()).is_err());
    let observations = observations.lock().unwrap();
    assert_eq!(observations.activates, 1);
    assert_eq!(observations.deactivates, 1);
    assert_eq!(observations.drops, 1);
}

#[test]
fn a_host_batch_over_256_events_is_not_delivered() {
    let (guest, observations) = fixture();
    let mut instance = PluginInstance::from_guest(Box::new(guest));
    instance.activate(Activation::default()).unwrap();
    assert!(instance.on_events(vec![Event::TreeChanged(Vec::new()); 257]).is_err());
    assert_eq!(observations.lock().unwrap().events, 0);
    assert_eq!(instance.state(), InstanceState::Active);
    assert!(instance.on_events(vec![Event::TreeChanged(Vec::new()); 256]).is_ok());
}

#[test]
fn a_submission_overflow_discards_the_whole_return_and_ends_the_instance() {
    let (mut guest, observations) = fixture();
    guest.submissions = 257;
    let mut instance = PluginInstance::from_guest(Box::new(guest));
    instance.activate(Activation::default()).unwrap();
    assert_eq!(instance.on_events(Vec::new()).unwrap_err().code, "trap.submission-batch");
    assert_eq!(observations.lock().unwrap().drops, 1);
    assert!(instance.on_events(Vec::new()).is_err());
}

#[test]
fn native_unwind_is_a_trap_and_never_leaves_a_live_guest() {
    let (mut guest, observations) = fixture();
    guest.panic_events = true;
    let mut instance = PluginInstance::from_guest(Box::new(guest));
    instance.activate(Activation::default()).unwrap();
    assert_eq!(instance.on_events(Vec::new()).unwrap_err().code, "trap.panic");
    assert_eq!(observations.lock().unwrap().drops, 1);
    assert_eq!(instance.last_trap().unwrap().code, "trap.panic");
}

#[test]
fn restore_caps_and_activation_errors_do_not_retain_guest_memory() {
    let (mut guest, observations) = fixture();
    guest.saved_size = (1 << 20) + 1;
    let mut instance = PluginInstance::from_guest(Box::new(guest));
    let activation = Activation {
        restored: Some(vec![0; (1 << 20) + 1]),
        ..Activation::default()
    };
    instance.activate(activation).unwrap();
    assert_eq!(observations.lock().unwrap().restore_size, None);
    assert_eq!(instance.deactivate(DeactivationReason::Granted), None);
    assert_eq!(instance.state(), InstanceState::Stopped);
    let (mut guest, observations) = fixture();
    guest.activation_error = true;
    let mut instance = PluginInstance::from_guest(Box::new(guest));
    assert_eq!(instance.activate(Activation::default()).unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(observations.lock().unwrap().drops, 1);
}

#[test]
fn dedicated_job_instances_run_once_without_activation_or_events() {
    let (guest, observations) = fixture();
    let mut instance = PluginInstance::for_job(Box::new(guest));
    assert!(instance.activate(Activation::default()).is_err());
    assert!(instance.on_events(Vec::new()).is_err());
    assert!(matches!(instance.call_export(ExportCall::Job { kind: "one".to_owned(), input: Vec::new() }), Ok(ExportReply::Job(Ok(_)))));
    assert_eq!(instance.state(), InstanceState::Stopped);
    assert_eq!(observations.lock().unwrap().activates, 0);
    assert_eq!(observations.lock().unwrap().drops, 1);
    assert!(instance.call_export(ExportCall::Job { kind: "two".to_owned(), input: Vec::new() }).is_err());
}

#[test]
fn exactly_256_submissions_are_returned_after_a_successful_call() {
    let (mut guest, _) = fixture();
    guest.submissions = 256;
    let mut instance = PluginInstance::from_guest(Box::new(guest));
    instance.activate(Activation::default()).unwrap();
    assert_eq!(instance.on_events(Vec::new()).unwrap().len(), 256);
    assert_eq!(instance.state(), InstanceState::Active);
}

#[test]
fn malformed_render_trees_and_wrong_export_reply_types_are_traps() {
    let (guest, observations) = fixture();
    let mut instance = PluginInstance::from_guest(Box::new(guest));
    instance.activate(Activation::default()).unwrap();
    assert_eq!(instance.render(ViewTarget::Panel("view".to_owned()), Area::default()).unwrap_err().code, "trap.invalid-ui-tree");
    assert_eq!(observations.lock().unwrap().drops, 1);
    let (guest, _) = fixture();
    let mut migration = PluginInstance::for_migration(Box::new(guest));
    assert_eq!(migration.call_export(ExportCall::Migrate { from_version: 1, data: Vec::new() }).unwrap_err().code, "trap.abi");
}

#[cfg(feature = "testing")]
#[test]
fn native_fixture_bridge_uses_production_lifecycle_guards() {
    use ilium_plugin_host::testing::native_guest::{NativeGuest, NativeGuestAdapter};
    struct Quiet;
    impl NativeGuest for Quiet {
        fn on_events(&mut self, _: Vec<Event>) -> Result<Vec<Submission>, Trap> { Ok(Vec::new()) }
    }
    let mut instance = PluginInstance::from_guest(Box::new(NativeGuestAdapter::new(Box::new(Quiet))));
    assert!(instance.on_events(Vec::new()).is_err());
    instance.activate(Activation::default()).unwrap();
    assert!(instance.on_events(Vec::new()).unwrap().is_empty());
    instance.deactivate(DeactivationReason::Shutdown);
    assert!(instance.on_events(Vec::new()).is_err());
}
