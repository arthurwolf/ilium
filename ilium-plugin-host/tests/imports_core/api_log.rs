//! api-log.md: `log.log` levels, the 16,384-byte message cap and the per-second line budget.

use ilium_plugin_api::generated::host::{HostLog, Level};
use ilium_plugin_api::Tier;
use proptest::prelude::*;

use crate::support::*;

const CAP: usize = 16_384;
const MARKER: &str = "…[truncated]";

fn lines_of(
    fixture: &ilium_plugin_host::testing::imports_fixture::ImportsFixture,
) -> Vec<(String, String)> {
    fixture
        .logs()
        .into_iter()
        .filter(|line| line.plugin == PLUGIN)
        .map(|line| (line.level, line.message))
        .collect()
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_each_level_arrives_with_its_name() {
    let mut fixture = fixture_t2(&[]);
    for (level, name) in [
        (Level::Trace, "trace"),
        (Level::Debug, "debug"),
        (Level::Info, "info"),
        (Level::Warn, "warn"),
        (Level::Error, "error"),
    ] {
        fixture.log().log(level, format!("message {name}"));
        fixture.advance(std::time::Duration::from_secs(1));
    }
    let lines = lines_of(&fixture);
    for name in ["trace", "debug", "info", "warn", "error"] {
        assert!(
            lines.contains(&(name.to_owned(), format!("message {name}"))),
            "{name}: {lines:?}"
        );
    }
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_message_of_exactly_16384_bytes_is_kept_whole() {
    let mut fixture = fixture_t2(&[]);
    fixture.log().log(Level::Info, ascii(CAP));
    let lines = lines_of(&fixture);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].1, ascii(CAP));
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_message_of_16385_bytes_is_cut_and_marked_within_the_cap() {
    let mut fixture = fixture_t2(&[]);
    fixture.log().log(Level::Info, ascii(CAP + 1));
    let message = lines_of(&fixture).remove(0).1;
    assert!(message.ends_with(MARKER), "marker missing");
    // ASCII input: every byte is a whole character, so the cut fills the cap exactly.
    assert_eq!(message.len(), CAP);
    let kept = &ascii(CAP + 1)[..CAP - MARKER.len()];
    assert!(message.starts_with(kept), "kept prefix differs");
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_cut_happens_at_a_character_boundary() {
    let mut fixture = fixture_t2(&[]);
    fixture.log().log(Level::Info, "é".repeat(CAP));
    let message = lines_of(&fixture).remove(0).1;
    assert!(message.ends_with(MARKER));
    assert!(message.len() <= CAP);
    assert!(std::str::from_utf8(message.as_bytes()).is_ok());
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_empty_message_is_accepted() {
    let mut fixture = fixture_t2(&[]);
    fixture.log().log(Level::Info, String::new());
    assert_eq!(lines_of(&fixture).len(), 1);
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_200_lines_in_one_second_all_arrive_for_a_t2_instance() {
    let mut fixture = fixture_t2(&[]);
    for index in 0..200 {
        fixture.log().log(Level::Info, format!("line {index}"));
    }
    assert_eq!(lines_of(&fixture).len(), 200);
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_line_201_in_one_second_is_dropped_for_a_t2_instance() {
    let mut fixture = fixture_t2(&[]);
    for index in 0..201 {
        fixture.log().log(Level::Info, format!("line {index}"));
    }
    let kept = lines_of(&fixture)
        .iter()
        .filter(|(_, m)| m.starts_with("line "))
        .count();
    assert_eq!(kept, 200);
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_budget_is_restored_in_the_next_second() {
    let mut fixture = fixture_t2(&[]);
    for index in 0..250 {
        fixture.log().log(Level::Info, format!("a{index}"));
    }
    fixture.advance(std::time::Duration::from_secs(1));
    fixture.log().log(Level::Info, "after".to_owned());
    assert!(lines_of(&fixture).iter().any(|(_, m)| m == "after"));
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_dropped_lines_are_summarised_in_one_warn_line_per_second() {
    let mut fixture = fixture_t2(&[]);
    for index in 0..230 {
        fixture.log().log(Level::Info, format!("line {index}"));
    }
    fixture.advance(std::time::Duration::from_secs(1));
    fixture.pump();
    let summaries: Vec<_> = lines_of(&fixture)
        .into_iter()
        .filter(|(level, message)| level == "warn" && message.contains("30"))
        .collect();
    assert_eq!(summaries.len(), 1, "one summary naming 30 dropped lines");
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_t1_instance_may_exceed_200_lines_up_to_its_ceiling() {
    let mut fixture = fixture_t1(&[]);
    for index in 0..400 {
        fixture.log().log(Level::Info, format!("line {index}"));
    }
    let kept = lines_of(&fixture).len();
    // manual does not pin the default T1 budget below the 1,000 ceiling; only the bounds.
    assert!(kept > 200, "T1 kept {kept} lines");
    assert!(kept <= 1000, "T1 kept {kept} lines");
}

/// REQ: WIT:ilium:host/log.log
#[test]
fn log_log_never_reports_an_error_to_the_caller() {
    let mut fixture = fixture_t2(&[]);
    for index in 0..400 {
        // The call has no result channel; the loop completing is the observable.
        fixture.log().log(Level::Error, format!("flood {index}"));
    }
    let flood = lines_of(&fixture)
        .iter()
        .filter(|(_, m)| m.starts_with("flood "))
        .count();
    assert_eq!(flood, 200);
    fixture.advance(std::time::Duration::from_secs(1));
    fixture.pump();
    let summaries: Vec<_> = lines_of(&fixture)
        .into_iter()
        .filter(|(level, message)| level == "warn" && message.contains("200"))
        .collect();
    assert_eq!(summaries.len(), 1, "one summary naming 200 dropped lines");
}

proptest! {
    // REQ: WIT:ilium:host/log.log
    #[test]
    fn log_log_any_message_yields_a_line_of_at_most_the_cap(length in 0usize..40_000, multi in any::<bool>()) {
        let mut fixture = fixture(Tier::T2, &[]);
        let message = if multi { "é".repeat(length / 2) } else { ascii(length) };
        fixture.log().log(Level::Info, message.clone());
        let lines = lines_of(&fixture);
        prop_assert_eq!(lines.len(), 1);
        prop_assert!(lines[0].1.len() <= CAP);
        if message.len() <= CAP {
            prop_assert_eq!(&lines[0].1, &message);
        } else {
            prop_assert!(lines[0].1.ends_with(MARKER));
        }
    }
}
