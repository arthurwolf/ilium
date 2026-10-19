//! Attach-cost measurement for a pane that retains the full 32 MiB journal.
//! Ignored by default: run it on purpose with `--ignored --nocapture`.

use crate::session::{PtyCommand, PtySession};
use std::time::{Duration, Instant};

const RETAINED_TARGET_BYTES: usize = 32 * 1024 * 1024;
// The journal trims whole chunks from its start, so retained bytes can sit a
// little under the cap. The journal is never complete once trimming begins.
const SLACK_BYTES: usize = 64 * 1024;

#[test]
#[ignore = "measurement only; prints attach timings for a 32 MiB pane"]
fn attach_cost_for_a_pane_holding_thirty_two_mib() {
    let session = PtySession::spawn(
        PtyCommand::new("/bin/sh", std::env::temp_dir(), 50, 200)
            .arg("-c")
            .arg("head -c 33554432 /dev/zero | tr '\\000' 'x'; printf '\\nend-of-output\\n'; exec cat"),
    )
    .expect("spawn 32 MiB output fixture");

    let fill_started = Instant::now();
    loop {
        let estimate = session.output_replay_estimate();
        let marker_on_screen = session.screen_snapshot().text.contains("end-of-output");
        if marker_on_screen && estimate.byte_len >= RETAINED_TARGET_BYTES - SLACK_BYTES {
            break;
        }
        assert!(
            fill_started.elapsed() < Duration::from_secs(120),
            "fixture did not reach its end marker with 32 MiB retained within two minutes"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!(
        "fill: retained journal reached 32 MiB after {:.2} s",
        fill_started.elapsed().as_secs_f64()
    );

    let old_started = Instant::now();
    let old_replay = session.output_replay();
    let old_build = old_started.elapsed();

    let snapshot_started = Instant::now();
    let snapshot = session.attach_snapshot().expect("parser readable");
    let snapshot_build = snapshot_started.elapsed();

    let mut client_parser = vt100::Parser::new(50, 200, 0);
    let client_started = Instant::now();
    client_parser.process(&snapshot.bytes);
    let client_apply = client_started.elapsed();

    let mut old_client_parser = vt100::Parser::new(50, 200, 1_000);
    let old_client_started = Instant::now();
    old_client_parser.process(&old_replay.bytes);
    let old_client_apply = old_client_started.elapsed();

    eprintln!(
        "old full replay: {} bytes, server build {:.3} ms, client parse {:.1} ms",
        old_replay.bytes.len(),
        old_build.as_secs_f64() * 1e3,
        old_client_apply.as_secs_f64() * 1e3
    );
    eprintln!(
        "new snapshot: {} bytes, server build {:.3} ms, client apply {:.3} ms",
        snapshot.bytes.len(),
        snapshot_build.as_secs_f64() * 1e3,
        client_apply.as_secs_f64() * 1e3
    );
    assert!(
        old_replay.bytes.len() >= RETAINED_TARGET_BYTES - SLACK_BYTES,
        "old full replay must carry the retained 32 MiB"
    );
    assert!(client_parser.screen().contents().contains("end-of-output"));
}
