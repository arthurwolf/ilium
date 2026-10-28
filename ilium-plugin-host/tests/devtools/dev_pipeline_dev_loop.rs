//! The rebuild debounce of `ilium plugin dev` (DEV-880): a rebuild waits for 300 ms without a file
//! change, and a burst of writes owes exactly one rebuild.

use std::time::Duration;

use ilium_plugin_host::cli::dev::{Debouncer, DEBOUNCE};

/// REQ: DEV-880
#[test]
fn burst_of_fifty_writes_within_100_ms_owes_exactly_one_rebuild() {
    assert_eq!(DEBOUNCE, Duration::from_millis(300));
    let mut debouncer = Debouncer::new(DEBOUNCE);
    for write in 0..50u64 {
        debouncer.change(write * 2);
    }
    assert_eq!(
        debouncer.due(397),
        0,
        "no rebuild before 300 ms without a change"
    );
    assert_eq!(
        debouncer.due(398),
        1,
        "one rebuild 300 ms after the last write"
    );
    assert_eq!(debouncer.due(1_000), 0, "the burst owes no second rebuild");
}

/// REQ: DEV-880
#[test]
fn a_single_write_owes_one_rebuild_and_a_later_write_owes_another() {
    let mut debouncer = Debouncer::new(DEBOUNCE);
    debouncer.change(0);
    assert_eq!(debouncer.due(300), 1, "one rebuild 300 ms after the write");
    debouncer.change(500);
    assert_eq!(debouncer.due(800), 1, "a later write owes its own rebuild");
    assert_eq!(debouncer.due(1_500), 0, "no further rebuild is owed");
}
