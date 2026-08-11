//! Pins that a fixture's output survives the pty round trip byte for byte.
//!
//! Written to settle a specific question. After the shell fixtures were
//! replaced by these executables, several tests began failing on macOS and
//! Windows with the *right* text on screen but characters missing or altered
//! -- `Cogiaating` for `Cogitating`, `escto` for `esc to`. Two explanations
//! fit: either a fixture's writes are mangled between here and the `vt100`
//! parser, or they arrive intact and the corruption happens further along, in
//! the client's own rendering.
//!
//! This test isolates the first half of that path -- fixture, pty, parser, no
//! ilium client anywhere -- so a failure here and a pass here point at
//! opposite halves of the pipeline.

use std::time::{Duration, Instant};

use ilium_pty::{PtyCommand, PtySession};
use ilium_test_fixtures::{install, FixtureBehavior};

/// The activity line `ilium-detect` matches on. Its prefix never changes
/// between repaints, which is exactly why a corruption in it is permanent:
/// a diffing renderer has no reason to repaint a cell it believes unchanged.
const ACTIVITY_LINE: &str = "Cogitating (esc to interrupt)";

/// Long enough for many repaints of a fixture that redraws once a second, so
/// this exercises repeated overwriting rather than a single first paint.
const OBSERVATION_WINDOW: Duration = Duration::from_secs(6);

fn screen_text(session: &PtySession) -> String {
    session.with_screen(|screen| screen.contents())
}

/// Repaints in place, once a second, forever -- the shape that was failing.
#[test]
fn a_repainting_fixture_keeps_its_text_intact_through_a_pty() {
    let directory = tempfile::tempdir().expect("temp dir");
    let installed = install(directory.path(), "codex", &FixtureBehavior::ChangeOnly);

    let command = PtyCommand::new(
        installed.path.to_string_lossy().to_string(),
        directory.path(),
        24,
        80,
    );
    let session = PtySession::spawn(command).expect("spawn the repainting fixture under a pty");

    // Sampled continuously rather than once at the end: a corruption that a
    // later repaint happens to fix would otherwise go unseen, and the whole
    // point is that these corruptions are permanent.
    let deadline = Instant::now() + OBSERVATION_WINDOW;
    let mut worst_seen: Option<String> = None;
    while Instant::now() < deadline {
        let text = screen_text(&session);
        // Only judge once the line has actually been painted.
        if text.contains("Cogi") && !text.contains(ACTIVITY_LINE) {
            worst_seen = Some(text);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        worst_seen.is_none(),
        "the fixture's activity line arrived corrupted through the pty. \
         Expected {ACTIVITY_LINE:?} somewhere on screen, got:\n{}",
        worst_seen.unwrap_or_default()
    );
    assert!(
        screen_text(&session).contains(ACTIVITY_LINE),
        "expected {ACTIVITY_LINE:?} on screen after the observation window, got:\n{}",
        screen_text(&session)
    );
}

/// The same question for a fixture that scrolls rather than repainting, since
/// the two exercise different paths through the parser.
#[test]
fn a_scrolling_fixture_keeps_its_text_intact_through_a_pty() {
    let directory = tempfile::tempdir().expect("temp dir");
    let installed = install(
        directory.path(),
        "codex",
        &FixtureBehavior::WorkingThenIdle {
            working_seconds: 30,
        },
    );

    let command = PtyCommand::new(
        installed.path.to_string_lossy().to_string(),
        directory.path(),
        24,
        80,
    );
    let session = PtySession::spawn(command).expect("spawn the scrolling fixture under a pty");

    let deadline = Instant::now() + OBSERVATION_WINDOW;
    let mut appeared = false;
    while Instant::now() < deadline {
        let text = screen_text(&session);
        if text.contains(ACTIVITY_LINE) {
            appeared = true;
        }
        if text.contains("Cogi") && !text.contains(ACTIVITY_LINE) {
            panic!("the fixture's activity line arrived corrupted through the pty:\n{text}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        appeared,
        "expected {ACTIVITY_LINE:?} to appear at least once, got:\n{}",
        screen_text(&session)
    );
}
