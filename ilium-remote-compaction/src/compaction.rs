//! The compaction pipeline: read, prepare, summarize, write.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde_json::json;

use crate::error::CompactionError;
use crate::input::{prepare_input, select_tail};
use crate::ledger::Ledger;
use crate::neutral::{parse_transcript, Role, Turn};
use crate::redact::redact_text;
use crate::report::Reporter;
use crate::summarize::{resolve_technique, summarize_history, SummarizeContext};
use crate::tokens::{estimate_tokens, latest_context_usage};
use crate::types::{
    AgentKind, CompactionEvent, CompactionOutcome, CompactionRequest, Summarizer, Technique,
};
use crate::write_claude::{build_claude_records, verify_claude, ClaudePlan};
use crate::write_codex::{build_codex_records, verify_codex, CodexPlan};
use crate::writer_common::{commit_rewrite, serialize_lines, Committed};

const STEP_TOTAL: usize = 4;
/// Estimated tokens of the wrapper text around a Claude summary.
const CLAUDE_WRAPPER_TOKENS: u64 = 60;

fn check_cancel(cancel: &AtomicBool) -> Result<(), CompactionError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(CompactionError::Cancelled);
    }
    Ok(())
}

/// Condenses the older part of the session transcript with `summarizer` and
/// rewrites the transcript in place (after backing it up). The session id is
/// unchanged. On any error, including cancellation, the original file is
/// untouched.
pub fn compact_session(
    request: &CompactionRequest,
    summarizer: &dyn Summarizer,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(CompactionEvent),
) -> Result<CompactionOutcome, CompactionError> {
    let started = Instant::now();
    let options = &request.options;
    let mut reporter = Reporter::new(emit);
    check_cancel(cancel)?;

    reporter.step(1, STEP_TOTAL, "Reading the transcript");
    reporter.progress(0.0);
    let parsed = parse_transcript(request.agent, &request.transcript_path)?;
    for warning in &parsed.warnings {
        reporter.log(format!("Warning: {warning}"));
    }
    let conversation_total: u64 = parsed.turns.iter().map(Turn::estimated_tokens).sum();
    let usage = latest_context_usage(request.agent, &request.transcript_path);
    reporter.tokens.conversation_total = conversation_total;
    reporter.tokens.before_context = usage.map_or(conversation_total, |(tokens, _)| tokens);
    reporter.tokens.window = request
        .context_window_tokens
        .or(usage.and_then(|(_, window)| window))
        .or(parsed.codex.as_ref().and_then(|tail| tail.context_window));
    reporter.publish_tokens();

    reporter.step(2, STEP_TOTAL, "Preparing the conversation");
    reporter.progress(0.05);
    let split = select_tail(&parsed.turns, options.tail_tokens);
    let (head_turns, tail_all) = parsed.turns.split_at(split);
    let tail_turns: Vec<Turn> = tail_all
        .iter()
        .filter(|turn| turn.role != Role::EarlierSummary)
        .cloned()
        .collect();
    if head_turns
        .iter()
        .all(|turn| turn.role == Role::EarlierSummary)
    {
        return Err(CompactionError::NothingToCompact {
            path: request.transcript_path.clone(),
        });
    }
    let prepared = prepare_input(head_turns, options);
    reporter.tokens.masked_savings = prepared.masked_savings;
    reporter.publish_tokens();
    reporter.log(format!(
        "{} turns summarized, {} kept verbatim, {} secrets redacted, {} tokens of old tool output masked",
        head_turns.len(),
        tail_turns.len(),
        prepared.redactions,
        prepared.masked_savings,
    ));
    reporter.progress(0.10);
    check_cancel(cancel)?;

    reporter.step(3, STEP_TOTAL, "Summarizing");
    let technique = resolve_technique(options);
    let facts = (technique == Technique::BestOfAllWorlds && !prepared.ledger.is_empty())
        .then(|| prepared.ledger.render());
    let context = SummarizeContext {
        technique,
        options,
        prepared: &prepared,
        summarizer,
        cancel,
        facts,
    };
    let result = summarize_history(&context, &mut reporter)?;
    check_cancel(cancel)?;

    let mut summary = result.text;
    if technique == Technique::BestOfAllWorlds
        && !result.used_fallback
        && !prepared.ledger.is_empty()
    {
        let appendix = ilium_prompts::render_value(
            "compaction/ledger-appendix",
            &json!({ "ledger": prepared.ledger.render() }),
        );
        summary = format!("{}\n\n{}", summary.trim_end(), appendix.trim_end());
    }
    let mut redactions = prepared.redactions;
    if options.redact_secrets {
        let (redacted, count) = redact_text(&summary);
        summary = redacted;
        redactions += count;
    }
    let summary_chars = summary.chars().count();

    reporter.step(4, STEP_TOTAL, "Writing the new transcript");
    reporter.progress(0.90);
    let committed = match request.agent {
        AgentKind::Claude => write_claude(
            request,
            &parsed,
            &summary,
            &tail_turns,
            started,
            &mut reporter,
        )?,
        AgentKind::Codex => write_codex(request, &parsed, &summary, &tail_turns, &mut reporter)?,
    };
    reporter.log(format!(
        "Backup written to {} ({} older backups removed)",
        committed.backup_path.display(),
        committed.pruned_backups,
    ));
    reporter.progress(1.0);
    reporter.publish_tokens();

    Ok(CompactionOutcome {
        session_id: request.session_id.clone(),
        transcript_path: request.transcript_path.clone(),
        backup_path: committed.backup_path,
        tokens: reporter.tokens.clone(),
        summary_chars,
        chunks: result.chunks,
        used_fallback: result.used_fallback,
        redactions,
    })
}

fn write_claude(
    request: &CompactionRequest,
    parsed: &crate::neutral::ParsedTranscript,
    summary: &str,
    tail: &[Turn],
    started: Instant,
    reporter: &mut Reporter<'_>,
) -> Result<Committed, CompactionError> {
    let head = parsed
        .claude
        .as_ref()
        .ok_or_else(|| CompactionError::NoAnchorRecord {
            path: request.transcript_path.clone(),
            reason: "the transcript was not parsed as a Claude Code session".to_string(),
        })?;
    let session_id = head
        .session_id
        .clone()
        .unwrap_or_else(|| request.session_id.clone());
    let tail_tokens: u64 = tail.iter().map(Turn::estimated_tokens).sum();
    reporter.tokens.tail_kept = tail_tokens;
    reporter.tokens.after_context = estimate_tokens(summary) + CLAUDE_WRAPPER_TOKENS + tail_tokens;
    reporter.publish_tokens();

    let plan = ClaudePlan {
        head,
        session_id: &session_id,
        summary,
        tail,
        pre_tokens: reporter.tokens.before_context,
        post_tokens: reporter.tokens.after_context,
        duration_ms: started.elapsed().as_millis() as u64,
    };
    let records = build_claude_records(&plan);
    let expected_new = records.len();
    commit_rewrite(
        &request.transcript_path,
        parsed.snapshot,
        parsed.shape,
        &serialize_lines(&records),
        request.options.keep_backups,
        &|path, offset, bytes| verify_claude(path, offset, bytes, expected_new, &session_id),
    )
}

fn write_codex(
    request: &CompactionRequest,
    parsed: &crate::neutral::ParsedTranscript,
    summary: &str,
    tail: &[Turn],
    reporter: &mut Reporter<'_>,
) -> Result<Committed, CompactionError> {
    let rollout = parsed
        .codex
        .as_ref()
        .ok_or_else(|| CompactionError::NoAnchorRecord {
            path: request.transcript_path.clone(),
            reason: "the transcript was not parsed as a Codex rollout".to_string(),
        })?;
    let activity = Ledger::from_tool_activity(tail);
    let tail_activity = (!activity.is_empty()).then(|| {
        ilium_prompts::render_value(
            "compaction/tail-activity",
            &json!({ "ledger": activity.render() }),
        )
    });
    let plan = CodexPlan {
        transcript_path: &request.transcript_path,
        rollout,
        summary,
        tail_activity: tail_activity.as_deref(),
        tail,
    };
    let built = build_codex_records(&plan)?;
    if !built.has_token_count {
        reporter.log("Warning: the rollout has no token_count event to copy; none was written");
    }
    reporter.tokens.tail_kept = tail
        .iter()
        .filter(|turn| matches!(turn.role, Role::User | Role::Assistant))
        .map(Turn::message_tokens)
        .sum();
    reporter.tokens.after_context = built.history_tokens;
    reporter.publish_tokens();

    let expected_new = built.records.len();
    let has_ordinals = rollout.has_ordinals;
    let previous_ordinal = rollout.last_ordinal;
    commit_rewrite(
        &request.transcript_path,
        parsed.snapshot,
        parsed.shape,
        &serialize_lines(&built.records),
        request.options.keep_backups,
        &|path, offset, bytes| {
            verify_codex(
                path,
                offset,
                bytes,
                expected_new,
                has_ordinals,
                previous_ordinal,
            )
        },
    )
}
