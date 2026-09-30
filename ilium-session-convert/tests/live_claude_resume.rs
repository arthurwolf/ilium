//! Live proof helper: converts the Codex fixture into a *real* Claude Code
//! store so `claude --resume <id>` can be run against it by hand. Ignored by
//! default; it writes under `$ILIUM_E2E_HOME/.claude/projects/` for exactly the
//! project `$ILIUM_E2E_CWD` (a scratch directory), and prints the new id.
//!
//! ILIUM_E2E_HOME=$HOME ILIUM_E2E_CWD=/tmp/scratch \
//!   cargo test -p ilium-session-convert --test live_claude_resume -- --ignored --nocapture

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use ilium_core::BuiltinAgentProvider;
use ilium_session_convert::{convert_session, ConversionEvent, ConversionRequest};

const CODEX_SESSION_ID: &str = "01a0f000-1111-7222-8333-444444444444";
const FIXTURE: &str = include_str!("fixtures/codex_rollout_sample.jsonl");

#[test]
#[ignore = "writes into a real Claude Code store; needs ILIUM_E2E_HOME and ILIUM_E2E_CWD"]
fn converts_the_fixture_into_a_real_claude_store() {
    let home = PathBuf::from(std::env::var_os("ILIUM_E2E_HOME").expect("ILIUM_E2E_HOME"));
    let cwd = std::fs::canonicalize(std::env::var_os("ILIUM_E2E_CWD").expect("ILIUM_E2E_CWD"))
        .expect("ILIUM_E2E_CWD exists");
    let codex_home = tempfile::tempdir().unwrap();
    let directory = codex_home.path().join("sessions/2026/09/30");
    std::fs::create_dir_all(&directory).unwrap();
    // `ILIUM_E2E_ROLLOUT` swaps the fixture for a real rollout copy; its
    // session_meta cwd must equal `ILIUM_E2E_CWD`.
    let (session_id, content) = match std::env::var_os("ILIUM_E2E_ROLLOUT") {
        Some(path) => {
            let path = PathBuf::from(path);
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let id = stem[stem.len() - 36..].to_string();
            (id, std::fs::read_to_string(&path).unwrap())
        }
        None => (
            CODEX_SESSION_ID.to_string(),
            FIXTURE.replace("__CWD__", &cwd.to_string_lossy()),
        ),
    };
    std::fs::write(
        directory.join(format!("rollout-2026-09-30T10-00-00-{session_id}.jsonl")),
        content,
    )
    .unwrap();

    let request = ConversionRequest {
        home_dir: home,
        project_cwd: cwd,
        source: BuiltinAgentProvider::Codex,
        target: BuiltinAgentProvider::Claude,
        source_session_id: session_id,
        codex_home: Some(codex_home.path().to_path_buf()),
        codex_executable: None,
    };
    let cancel = AtomicBool::new(false);
    let outcome = convert_session(&request, &cancel, &mut |event| {
        if let ConversionEvent::Log(line) = event {
            println!("  {line}");
        }
    })
    .expect("conversion succeeds");
    println!(
        "converted={} dropped={}",
        outcome.converted_items, outcome.dropped_items
    );
    println!("NEW_SESSION_ID={}", outcome.new_session_id);
    println!("TRANSCRIPT={}", outcome.target_transcript_path.display());
}
