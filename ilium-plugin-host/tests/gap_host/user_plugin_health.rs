//! Manual page `user-plugin-health`: the Plugins screen fields, update rate and render budget
//! (HST-640).

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-640
#[test]
fn hst_640_plugins_screen_shows_health_error_rates_and_memory_and_renders_64_plugins_within_16_ms()
{
    let rig = rig();
    rig.install_many(64);
    let screen = rig.open_plugins_screen();
    let row = screen.row_for(NOTES).expect("a row per plugin");
    for field in [
        "health",
        "last_error_code",
        "last_error_time",
        "calls_per_minute",
        "p95_call_ms",
        "memory_bytes",
    ] {
        assert!(row.field(field).is_some(), "row shows {field}");
    }
    let started = std::time::Instant::now();
    screen.render_all();
    assert!(
        started.elapsed().as_millis() <= 16,
        "64 plugins rendered within 16 ms"
    );
    assert!(
        rig.plugins_screen_refresh_interval_ms() >= 1_000,
        "updates at most once per second"
    );
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
}
