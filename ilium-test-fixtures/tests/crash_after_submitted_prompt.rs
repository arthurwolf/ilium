//! Deterministic unflushed provider crashes without invoking a real agent CLI.
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ilium_pty::{PtyCommand, PtySession};
use ilium_test_fixtures::{install, FixtureBehavior};

fn prompt() -> String {
    format!(
        "{}\nUnicode: café 日本語 🦀\r\nlast line  \n",
        "long prompt  ".repeat(600)
    )
}

fn submission(prompt: &str) -> Vec<u8> {
    [b"\x1b[200~".as_slice(), prompt.as_bytes(), b"\x1b[201~\r"].concat()
}

#[test]
fn both_provider_names_crash_after_preserving_one_exact_unflushed_submission() {
    for (name, mouse_tracking) in [("codex", true), ("claude", false)] {
        let directory = tempfile::tempdir().expect("isolated fixture directory");
        let prompt_path = directory.path().join("prompt.txt");
        let transcript_path = directory.path().join("transcript.jsonl");
        let original = b"{\"type\":\"existing transcript\"}\n";
        std::fs::write(&transcript_path, original).expect("write existing transcript");
        let fixture = install(
            directory.path(),
            name,
            &FixtureBehavior::CrashAfterSubmittedPrompt {
                prompt_path: prompt_path.clone(),
                transcript_path: Some(transcript_path.clone()),
                exit_code: 42,
                mouse_tracking,
            },
        );
        assert!(fixture.path.is_absolute());
        let mut child = Command::new(&fixture.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn absolute fake provider");
        let prompt = prompt();
        assert!(prompt.len() > 4096);
        child
            .stdin
            .take()
            .expect("fixture stdin")
            .write_all(&submission(&prompt))
            .expect("submit bracketed paste and Enter");
        let output = child
            .wait_with_output()
            .expect("collect actual exit status");
        assert_eq!(
            output.status.code(),
            Some(42),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(&prompt_path).expect("captured prompt"),
            prompt.as_bytes()
        );
        assert_eq!(
            std::fs::read(&transcript_path).expect("retained transcript"),
            original
        );
        let output = String::from_utf8(output.stdout).expect("UTF8 fixture output");
        assert!(output.contains("FATAL_FIXTURE_CRASH"));
        assert!(output.contains("\x1b[?2004h"));
        for mode in [1000, 1002, 1003, 1006] {
            assert_eq!(output.contains(&format!("\x1b[?{mode}h")), mouse_tracking);
            assert!(!output.contains(&format!("\x1b[?{mode}l")));
        }
    }
}

#[test]
fn a_real_pty_retains_mouse_negotiation_after_the_fake_agent_crashes() {
    let directory = tempfile::tempdir().expect("isolated fixture directory");
    let prompt_path = directory.path().join("prompt.txt");
    let fixture = install(
        directory.path(),
        "codex",
        &FixtureBehavior::CrashAfterSubmittedPrompt {
            prompt_path: prompt_path.clone(),
            transcript_path: None,
            exit_code: 43,
            mouse_tracking: true,
        },
    );
    let mut session = PtySession::spawn(PtyCommand::new(
        fixture.path.to_string_lossy().to_string(),
        directory.path(),
        24,
        80,
    ))
    .expect("spawn fake provider under PTY");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.with_screen(|screen| screen.bracketed_paste()) {
        assert!(Instant::now() < deadline, "fixture never negotiated paste");
        std::thread::sleep(Duration::from_millis(10));
    }
    let prompt = prompt();
    session
        .write(&submission(&prompt))
        .expect("submit to real composer");
    while !session.has_exited() || !session.screen_text().contains("FATAL_FIXTURE_CRASH") {
        assert!(Instant::now() < deadline, "fixture did not finish crashing");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read(prompt_path).expect("captured prompt"),
        prompt.as_bytes()
    );
    session.with_screen(|screen| {
        assert!(screen.bracketed_paste());
        assert_eq!(
            screen.mouse_protocol_mode(),
            vt100::MouseProtocolMode::AnyMotion
        );
        assert_eq!(
            screen.mouse_protocol_encoding(),
            vt100::MouseProtocolEncoding::Sgr
        );
    });
}
