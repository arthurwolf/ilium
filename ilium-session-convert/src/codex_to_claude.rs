//! Codex -> Claude Code: no importer exists on the Claude side, so the
//! rollout is converted here into a transcript `claude --resume` accepts.

use ilium_agent_session::TranscriptLocator;
use ilium_core::AgentClass;
use std::io::Read;

use crate::claude_writer::{build_transcript, validate_transcript, write_atomically, BuildInput};
use crate::codex_rollout::parse_rollout;
use crate::error::ConvertError;
use crate::paths::{
    canonical_or_original, claude_project_dir, locate_codex_rollout, resolve_codex_home,
};
use crate::report::Reporter;
use crate::{ConversionOutcome, ConversionRequest};

pub(crate) const TOTAL_STEPS: usize = 6;

pub(crate) fn convert(
    request: &ConversionRequest,
    reporter: &mut Reporter<'_>,
) -> Result<ConversionOutcome, ConvertError> {
    let codex_home = resolve_codex_home(&request.home_dir, request.codex_home.as_deref());
    let project = canonical_or_original(&request.project_cwd);

    // Step 1: locate the Codex rollout.
    reporter.step(1, "Locate the Codex rollout", 0.0);
    let rollout = locate_codex_rollout(
        &request.home_dir,
        &project,
        &codex_home,
        &request.source_session_id,
    )
    .filter(|rollout| rollout.cwd_matches)
    .ok_or_else(|| ConvertError::SourceNotFound {
        agent: "Codex",
        session_id: request.source_session_id.clone(),
        project: project.clone(),
    })?;
    let size = std::fs::metadata(&rollout.path)
        .map(|m| m.len())
        .unwrap_or(0);
    reporter.log(format!("Found {} ({size} bytes)", rollout.path.display()));
    reporter.progress(0.05);
    reporter.check_cancel()?;

    // Step 2: parse it.
    reporter.step(2, "Parse the rollout", 0.05);
    let mut last_logged_percent = 0u32;
    let parsed = parse_rollout(&rollout.path, &mut |fraction| {
        reporter.progress(0.05 + 0.45 * fraction);
        let percent = (fraction * 100.0) as u32;
        if percent >= last_logged_percent + 25 {
            last_logged_percent = percent;
            reporter.log(format!("Parsed {percent}% of the rollout"));
        }
        !reporter.is_cancelled()
    })?;
    reporter.progress(0.50);
    reporter.log(format!(
        "{} records: {} conversation items kept, {} dropped, {} unreadable lines",
        parsed.json_lines + parsed.malformed_lines,
        parsed.items.len(),
        parsed.dropped_total() - parsed.malformed_lines,
        parsed.malformed_lines,
    ));
    for (kind, count) in &parsed.dropped_by_kind {
        reporter.log(format!("  dropped {count} x {kind}"));
    }
    reporter.check_cancel()?;

    // Step 3: convert.
    reporter.step(3, "Convert to Claude Code lines", 0.50);
    let new_session_id = uuid::Uuid::new_v4().to_string();
    let cwd_text = project.to_string_lossy().into_owned();
    let built = build_transcript(&BuildInput {
        items: &parsed.items,
        session_id: &new_session_id,
        cwd: &cwd_text,
        git_branch: parsed.meta.git_branch.as_deref(),
        fallback_timestamp: chrono::Utc::now(),
    });
    if built.lines.is_empty() || built.converted_items == 0 {
        return Err(ConvertError::EmptyConversation {
            path: rollout.path.clone(),
        });
    }
    reporter.log(format!(
        "{} tool calls ({} without a recorded output), {} oversized outputs truncated",
        built.tool_calls, built.synthesized_results, built.truncated_results
    ));
    if built.synthesized_first_prompt {
        reporter.log("The conversation began with an assistant message; a placeholder first user message was added");
    }
    reporter.progress(0.65);
    reporter.check_cancel()?;

    // Step 4: write atomically.
    reporter.step(4, "Write the Claude Code transcript", 0.65);
    let directory = claude_project_dir(&request.home_dir, &project);
    let path = write_atomically(&directory, &new_session_id, &built.lines, &|| {
        reporter.is_cancelled()
    })?;
    reporter.log(format!("Wrote {}", path.display()));
    reporter.progress(0.80);

    // Steps 5 and 6 validate what landed on disk; a failure removes the file so
    // no transcript Claude Code could misread is left behind.
    match verify_and_register(request, &project, &path, &new_session_id, reporter) {
        Ok(()) => {}
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
    }
    reporter.progress(1.0);
    Ok(ConversionOutcome {
        new_session_id,
        target_transcript_path: path,
        converted_items: built.converted_items,
        dropped_items: parsed.dropped_total() + built.dropped_items,
    })
}

fn verify_and_register(
    request: &ConversionRequest,
    project: &std::path::Path,
    path: &std::path::Path,
    session_id: &str,
    reporter: &mut Reporter<'_>,
) -> Result<(), ConvertError> {
    // Step 5: structural validation of the written file.
    reporter.step(5, "Verify the transcript", 0.80);
    let file = ilium_platform::secure_fs::open_regular_file(path).map_err(|error| {
        ConvertError::TargetWrite {
            path: path.to_path_buf(),
            error,
        }
    })?;
    let size = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    if size > crate::MAX_TRANSCRIPT_BYTES {
        return Err(ConvertError::FileTooLarge {
            path: path.to_path_buf(),
            bytes: size,
            maximum: crate::MAX_TRANSCRIPT_BYTES,
        });
    }
    let mut content = String::new();
    file.take(crate::MAX_TRANSCRIPT_BYTES + 1)
        .read_to_string(&mut content)
        .map_err(|error| ConvertError::TargetWrite {
            path: path.to_path_buf(),
            error,
        })?;
    if content.len() as u64 > crate::MAX_TRANSCRIPT_BYTES {
        return Err(ConvertError::FileTooLarge {
            path: path.to_path_buf(),
            bytes: content.len() as u64,
            maximum: crate::MAX_TRANSCRIPT_BYTES,
        });
    }
    let summary =
        validate_transcript(&content, session_id).map_err(ConvertError::TargetVerification)?;
    reporter.log(format!(
        "Verified {} messages, {} tool calls, uuid chain intact",
        summary.message_lines, summary.tool_uses
    ));
    reporter.progress(0.90);
    reporter.check_cancel()?;

    // Step 6: Claude Code discovers sessions by scanning the project
    // directory, so "registering" means proving its locator resolves the id.
    reporter.step(6, "Register with Claude Code", 0.90);
    let found = TranscriptLocator::new(&request.home_dir, project)
        .transcript_for_session(&AgentClass::Claude, session_id);
    match found {
        Some(located) if located.path == path => {
            reporter.log(format!("Resume with: claude --resume {session_id}"));
            Ok(())
        }
        Some(located) => Err(ConvertError::TargetVerification(format!(
            "the transcript locator resolved {} instead of {}",
            located.path.display(),
            path.display()
        ))),
        None => Err(ConvertError::TargetVerification(
            "the transcript locator cannot find the new session for this project".to_string(),
        )),
    }
}
