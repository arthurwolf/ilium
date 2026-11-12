//! Codex -> Claude Code: no importer exists on the Claude side, so the
//! rollout is converted here into a transcript `claude --resume` accepts.
//!
//! The rollout is read twice and never held in memory. Pass 1 (step 2) scans
//! it for the small facts the writer needs: tool-call ids and where their
//! outputs sit, the first timestamp, the title. Pass 2 (step 4) scans it
//! again and streams each item's Claude lines straight into the target
//! file, fetching tool outputs by byte offset as their calls are written.
//! Between the passes the source must not change; its size and modification
//! time are compared before pass 2 and every offset is re-checked while it
//! is used.

use std::io::BufReader;
use std::path::Path;

use ilium_agent_session::TranscriptLocator;
use ilium_core::AgentClass;

use crate::claude_writer::{
    validate_transcript, ConversationFacts, RandomIdentifiers, RolloutOutputSource,
    TranscriptAssembler, TranscriptFileWriter, TranscriptTarget,
};
use crate::codex_rollout::{RolloutScanner, SourceSignature};
use crate::error::ConvertError;
use crate::paths::{
    canonical_or_original, claude_project_dir, locate_codex_rollout, resolve_codex_home,
};
use crate::report::Reporter;
use crate::{ConversionOutcome, ConversionRequest};

pub(crate) const TOTAL_STEPS: usize = 6;

/// Progress span of pass 1 (the scan) and of pass 2 (the write).
const SCAN_PROGRESS: (f32, f32) = (0.05, 0.35);
const WRITE_PROGRESS: (f32, f32) = (0.35, 0.80);

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
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    reporter.log(format!("Found {} ({size} bytes)", rollout.path.display()));
    reporter.progress(SCAN_PROGRESS.0);
    reporter.check_cancel()?;

    // Step 2, pass 1: scan the rollout for the facts the writer needs.
    reporter.step(2, "Scan the rollout", SCAN_PROGRESS.0);
    let mut facts = ConversationFacts::default();
    let (meta, counts, signature) = {
        let mut on_progress = scan_progress(reporter, SCAN_PROGRESS, "Scanned");
        let mut scanner = RolloutScanner::open(&rollout.path, &mut on_progress)?;
        while let Some(scanned) = scanner.next_item()? {
            facts.observe(scanned.offset, &scanned.item);
        }
        (
            scanner.meta().clone(),
            scanner.counts().clone(),
            scanner.source_signature(),
        )
    };
    reporter.progress(SCAN_PROGRESS.1);
    let unpaired_outputs = facts.unpaired_outputs();
    reporter.log(format!(
        "{} records: {} conversation items kept, {} dropped, {} unreadable lines",
        counts.json_lines + counts.malformed_lines,
        facts.conversation_items,
        counts.dropped_total() - counts.malformed_lines + unpaired_outputs,
        counts.malformed_lines,
    ));
    for (kind, count) in &counts.dropped_by_kind {
        reporter.log(format!("  dropped {count} x {kind}"));
    }
    if unpaired_outputs > 0 {
        reporter.log(format!(
            "  dropped {unpaired_outputs} x tool output without a matching call"
        ));
    }
    if facts.conversation_items == 0 {
        return Err(ConvertError::EmptyConversation {
            path: rollout.path.clone(),
        });
    }
    reporter.check_cancel()?;

    // Step 3: decide where the transcript goes and that it will fit.
    reporter.step(3, "Prepare the target", SCAN_PROGRESS.1);
    let new_session_id = uuid::Uuid::new_v4().to_string();
    let directory = claude_project_dir(&request.home_dir, &project);
    let estimated_bytes = facts.estimated_output_bytes(signature.length);
    ensure_disk_space(&directory, estimated_bytes)?;
    reporter.log(format!(
        "Target {} (at most {estimated_bytes} bytes)",
        directory.join(format!("{new_session_id}.jsonl")).display()
    ));
    reporter.check_cancel()?;

    // Step 4, pass 2: stream the Claude lines into the target file.
    reporter.step(4, "Write the Claude Code transcript", WRITE_PROGRESS.0);
    let current_signature =
        SourceSignature::of(&rollout.path).map_err(|error| ConvertError::SourceUnreadable {
            path: rollout.path.clone(),
            error,
        })?;
    if current_signature != signature {
        return Err(ConvertError::SourceChanged {
            path: rollout.path.clone(),
        });
    }
    let cwd_text = project.to_string_lossy().into_owned();
    let target = TranscriptTarget {
        session_id: &new_session_id,
        cwd: &cwd_text,
        git_branch: meta.git_branch.as_deref(),
        fallback_timestamp: chrono::Utc::now(),
    };
    let (path, stats, bytes_written) = {
        let mut file_writer = TranscriptFileWriter::create(
            &directory,
            &new_session_id,
            crate::MAX_OUTPUT_TRANSCRIPT_BYTES,
        )?;
        let mut identifiers = RandomIdentifiers;
        let mut assembler =
            TranscriptAssembler::begin(facts, &target, &mut identifiers, &mut file_writer)?;
        let mut outputs = RolloutOutputSource::open(&rollout.path)?;
        {
            let mut on_progress = scan_progress(reporter, WRITE_PROGRESS, "Written");
            let mut scanner = RolloutScanner::open(&rollout.path, &mut on_progress)?;
            while let Some(scanned) = scanner.next_item()? {
                assembler.emit(scanned.item, &mut outputs)?;
            }
            if scanner.source_signature() != signature {
                return Err(ConvertError::SourceChanged {
                    path: rollout.path.clone(),
                });
            }
        }
        let stats = assembler.finish()?;
        let bytes_written = file_writer.bytes_written();
        // A cancel request is honoured right before the rename, so a
        // cancelled conversion never leaves a transcript behind.
        let path = file_writer.finish(&|| reporter.is_cancelled())?;
        (path, stats, bytes_written)
    };
    reporter.log(format!(
        "{} tool calls ({} without a recorded output), {} oversized outputs truncated",
        stats.tool_calls, stats.synthesized_results, stats.truncated_results
    ));
    if stats.synthesized_first_prompt {
        reporter.log("The conversation began with an assistant message; a placeholder first user message was added");
    }
    reporter.log(format!("Wrote {} ({bytes_written} bytes)", path.display()));
    reporter.progress(WRITE_PROGRESS.1);

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
        converted_items: stats.converted_items,
        dropped_items: counts.dropped_total() + unpaired_outputs,
    })
}

/// A scanner progress callback that maps the scanned fraction onto `span`,
/// logs every quarter and cancels when the reporter says so.
fn scan_progress<'r, 'c>(
    reporter: &'r mut Reporter<'c>,
    span: (f32, f32),
    verb: &'static str,
) -> impl FnMut(f32) -> bool + use<'r, 'c> {
    let mut last_logged_percent = 0u32;
    move |fraction: f32| {
        reporter.progress(span.0 + (span.1 - span.0) * fraction);
        let percent = (fraction * 100.0) as u32;
        if percent >= last_logged_percent + 25 {
            last_logged_percent = percent;
            reporter.log(format!("{verb} {percent}% of the rollout"));
        }
        !reporter.is_cancelled()
    }
}

/// Fails when the file system holding `directory` (or its nearest existing
/// ancestor) cannot take `required` bytes. A file system that cannot report
/// its free space is not treated as full.
fn ensure_disk_space(directory: &Path, required: u64) -> Result<(), ConvertError> {
    match ilium_platform::disk_space::available_bytes(directory) {
        Ok(available) if available < required => Err(ConvertError::InsufficientDiskSpace {
            directory: directory.to_path_buf(),
            required,
            available,
        }),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn verify_and_register(
    request: &ConversionRequest,
    project: &Path,
    path: &Path,
    session_id: &str,
    reporter: &mut Reporter<'_>,
) -> Result<(), ConvertError> {
    // Step 5: structural validation of the written file, one line at a time.
    reporter.step(5, "Verify the transcript", WRITE_PROGRESS.1);
    let file = ilium_platform::secure_fs::open_regular_file(path).map_err(|error| {
        ConvertError::TargetWrite {
            path: path.to_path_buf(),
            error,
        }
    })?;
    let summary = validate_transcript(BufReader::new(file), session_id)
        .map_err(ConvertError::TargetVerification)?;
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
