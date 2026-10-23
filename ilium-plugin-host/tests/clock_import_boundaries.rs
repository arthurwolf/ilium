use ilium_plugin_api::generated::host::clock::HostClock;
use ilium_plugin_host::imports::clock::{ClockImport, duration_millis, unix_millis_at};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn wall_clock_observation_is_between_actual_host_reads() {
    let mut clock = ClockImport;
    let before = unix_millis_at(SystemTime::now());
    let observed = clock.now_unix_millis();
    let after = unix_millis_at(SystemTime::now());
    assert!(before <= observed && observed <= after,
        "wall clock changed during the real observation: {before} <= {observed} <= {after}");
}

#[test]
fn monotonic_clock_never_resets_when_an_import_is_constructed() {
    let mut first = ClockImport;
    let before = first.monotonic_millis();
    std::thread::sleep(Duration::from_millis(5));
    let progressed = first.monotonic_millis();
    assert!(progressed > before);
    let mut newly_constructed = ClockImport;
    assert!(newly_constructed.monotonic_millis() >= progressed);
    for _ in 0..100 {
        let earlier = first.monotonic_millis();
        let later = newly_constructed.monotonic_millis();
        assert!(later >= earlier);
    }
}

#[test]
fn unix_conversion_clamps_before_epoch_and_truncates_submilliseconds() {
    assert_eq!(unix_millis_at(UNIX_EPOCH), 0);
    assert_eq!(unix_millis_at(UNIX_EPOCH - Duration::from_nanos(1)), 0);
    assert_eq!(unix_millis_at(UNIX_EPOCH + Duration::from_nanos(999_999)), 0);
    assert_eq!(unix_millis_at(UNIX_EPOCH + Duration::from_micros(1_999)), 1);
    assert_eq!(unix_millis_at(UNIX_EPOCH + Duration::from_secs(1_234)), 1_234_000);
}

#[test]
fn duration_conversion_saturates_instead_of_truncating_the_integer() {
    assert_eq!(duration_millis(Duration::from_millis(u64::MAX)), u64::MAX);
    assert_eq!(duration_millis(Duration::from_secs(u64::MAX)), u64::MAX);
    assert_eq!(duration_millis(Duration::new(2, 999_999_999)), 2_999);
}
