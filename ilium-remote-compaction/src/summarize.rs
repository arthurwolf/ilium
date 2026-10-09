//! The summarization pipeline: chunking, anchored per-chunk calls, a merge
//! call, retries, re-chunking on `ContextTooLong` and the deterministic
//! fallback when the model cannot be used.

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;

use crate::error::{CompactionError, SummarizerError};
use crate::input::{chunk_end, chunk_text, plan_chunks, PreparedInput};
use crate::report::Reporter;
use crate::technique::{
    clean_summary, effective_technique, render_merge_prompt, render_prompt, validate_summary,
    PromptInputs, RenderedPrompt,
};
use crate::tokens::estimate_tokens;
use crate::types::{CompactionOptions, Summarizer, SummaryRequest, Technique};

/// Progress range of the summarizing step within the whole run.
const PROGRESS_START: f32 = 0.10;
const PROGRESS_END: f32 = 0.90;
/// How many times a chunk is re-split in half after `ContextTooLong`.
const MAX_HALVINGS: usize = 3;
/// Bound the concatenated merge prompt independently of transcript chunk count.
const MAX_RETAINED_MERGE_SUMMARY_BYTES: usize = 16 * 1024 * 1024;
/// Empty summaries still occupy a `String` header in the retained vector.
const MAX_RETAINED_MERGE_SUMMARIES: usize = 256;
/// Smallest chunk the pipeline plans, however small the summarizer window.
const MINIMUM_CHUNK_TOKENS: u64 = 256;
/// Reserve for the user-prompt wrapper text and estimation error.
const PROMPT_MARGIN_TOKENS: u64 = 600;

pub(crate) struct SummaryResult {
    /// Cleaned summary text, before any appendix.
    pub(crate) text: String,
    pub(crate) chunks: usize,
    pub(crate) used_fallback: bool,
}

pub(crate) struct SummarizeContext<'a> {
    pub(crate) technique: Technique,
    pub(crate) options: &'a CompactionOptions,
    pub(crate) prepared: &'a PreparedInput,
    pub(crate) summarizer: &'a dyn Summarizer,
    pub(crate) cancel: &'a AtomicBool,
    /// Code-computed facts given to the prompt (only some techniques use them).
    pub(crate) facts: Option<String>,
}

enum CallOutcome {
    Summary(String),
    ContextTooLong(String),
    Failed(String),
}

impl SummarizeContext<'_> {
    fn check_cancel(&self) -> Result<(), CompactionError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(CompactionError::Cancelled);
        }
        Ok(())
    }

    fn custom_prompt(&self) -> Option<&str> {
        self.options.custom_prompt.as_deref()
    }

    /// Tokens available for the conversation text of one call.
    fn chunk_budget(&self) -> Result<u64, CompactionError> {
        let probe = render_prompt(self.technique, &self.prompt_inputs("", None, None, None))?;
        let overhead = estimate_tokens(&probe.system)
            + self.facts.as_deref().map_or(0, estimate_tokens)
            + self.options.summarizer_max_output_tokens
            + PROMPT_MARGIN_TOKENS;
        Ok(self
            .options
            .summarizer_context_tokens
            .saturating_sub(overhead)
            .max(MINIMUM_CHUNK_TOKENS))
    }

    fn prompt_inputs<'a>(
        &'a self,
        conversation: &'a str,
        prior_summary: Option<&'a str>,
        part_label: Option<&'a str>,
        retry_problem: Option<&'a str>,
    ) -> PromptInputs<'a> {
        PromptInputs {
            conversation,
            prior_summary,
            facts: self.facts.as_deref(),
            custom_prompt: self.custom_prompt(),
            part_label,
            retry_problem,
        }
    }

    /// One summarizer call with one retry for a malformed reply or a failure.
    fn call(
        &self,
        reporter: &mut Reporter<'_>,
        label: &str,
        make_prompt: &dyn Fn(Option<&str>) -> Result<RenderedPrompt, CompactionError>,
    ) -> Result<CallOutcome, CompactionError> {
        let mut problem: Option<String> = None;
        let mut last_failure = String::new();
        for attempt in 0..2 {
            self.check_cancel()?;
            let prompt = make_prompt(problem.as_deref())?;
            let request = SummaryRequest {
                max_output_tokens: self.options.summarizer_max_output_tokens,
                label: label.to_string(),
                system: prompt.system,
                user: prompt.user,
            };
            let attempt_number = attempt + 1;
            reporter.log(format!(
                "Summarizer call {label} (attempt {attempt_number})"
            ));
            match self.summarizer.summarize(&request) {
                Ok(response) => {
                    reporter.tokens.summarizer_input +=
                        response.input_tokens.unwrap_or_else(|| {
                            estimate_tokens(&request.system) + estimate_tokens(&request.user)
                        });
                    reporter.tokens.summarizer_output += response
                        .output_tokens
                        .unwrap_or_else(|| estimate_tokens(&response.text));
                    reporter.publish_tokens();
                    let cleaned = clean_summary(self.technique, &response.text);
                    match validate_summary(self.technique, &cleaned) {
                        Ok(()) => return Ok(CallOutcome::Summary(cleaned)),
                        Err(reason) => {
                            reporter
                                .log(format!("Summarizer reply for {label} rejected: {reason}"));
                            last_failure = reason.clone();
                            problem = Some(reason);
                        }
                    }
                }
                Err(SummarizerError::ContextTooLong(message)) => {
                    return Ok(CallOutcome::ContextTooLong(message));
                }
                Err(SummarizerError::Cancelled) => return Err(CompactionError::Cancelled),
                Err(SummarizerError::Failed(message)) => {
                    reporter.log(format!("Summarizer call {label} failed: {message}"));
                    last_failure = message;
                    problem = None;
                }
            }
        }
        Ok(CallOutcome::Failed(last_failure))
    }
}

fn fallback_text(
    prepared: &PreparedInput,
    partial_summary: Option<&str>,
) -> Result<String, CompactionError> {
    let earlier = if partial_summary.is_some() {
        None
    } else {
        prepared.prior_summary.as_deref()
    };
    let rendered = ilium_prompts::render(
        "compaction/fallback-summary",
        &json!({
            "earlier_summary": earlier.unwrap_or_default(),
            "partial_summary": partial_summary.unwrap_or_default(),
            "user_requests": prepared.user_requests,
            "ledger": prepared.ledger.render(),
        }),
    )?;
    Ok(rendered.trim().to_string())
}

fn publish_progress(reporter: &mut Reporter<'_>, calls_done: usize, calls_total: usize) {
    let fraction = calls_done as f32 / calls_total.max(1) as f32;
    reporter.progress(PROGRESS_START + (PROGRESS_END - PROGRESS_START) * fraction.min(1.0));
}

/// Summarizes the prepared history. Errors only on cancellation or a prompt
/// rendering failure; an unusable summarizer yields the fallback summary.
pub(crate) fn summarize_history(
    context: &SummarizeContext<'_>,
    reporter: &mut Reporter<'_>,
) -> Result<SummaryResult, CompactionError> {
    let prepared = context.prepared;
    let rendered = &prepared.rendered;
    let mut budget = context.chunk_budget()?;
    let mut halvings = 0;
    let mut next = 0;
    let mut chunks_done = 0;
    let mut running = prepared.prior_summary.clone();
    let mut partials: Vec<String> = Vec::new();
    let mut partial_bytes = 0usize;
    let mut merge_partials_exceeded_limit = false;

    while next < rendered.len() {
        context.check_cancel()?;
        let end = chunk_end(rendered, next, budget);
        let remaining_chunks = plan_chunks(&rendered[next..], budget).len();
        let total_chunks = chunks_done + remaining_chunks;
        let is_single = total_chunks == 1;
        let label = if is_single {
            "summary".to_string()
        } else {
            format!("chunk {}/{}", chunks_done + 1, total_chunks)
        };
        let part_label =
            (!is_single).then(|| format!("part {} of {}", chunks_done + 1, total_chunks));
        let conversation = chunk_text(rendered, next..end, budget);
        let anchor = running.clone();
        let outcome = context.call(reporter, &label, &|problem| {
            render_prompt(
                context.technique,
                &context.prompt_inputs(
                    &conversation,
                    anchor.as_deref(),
                    part_label.as_deref(),
                    problem,
                ),
            )
        })?;
        match outcome {
            CallOutcome::Summary(text) => {
                if !merge_partials_exceeded_limit {
                    let next_partial_bytes = partial_bytes.saturating_add(text.capacity());
                    if partials.len() >= MAX_RETAINED_MERGE_SUMMARIES
                        || next_partial_bytes > MAX_RETAINED_MERGE_SUMMARY_BYTES
                    {
                        partials = Vec::new();
                        partial_bytes = 0;
                        merge_partials_exceeded_limit = true;
                        reporter.log(
                            "Retained summaries reached the memory limit; keeping the latest progressive summary and skipping the final merge"
                                .to_string(),
                        );
                    } else {
                        partial_bytes = next_partial_bytes;
                        partials.push(text.clone());
                    }
                }
                running = Some(text);
                chunks_done += 1;
                next = end;
                let calls_total = total_chunks + usize::from(total_chunks > 1);
                publish_progress(reporter, chunks_done, calls_total);
            }
            CallOutcome::ContextTooLong(message) => {
                halvings += 1;
                if halvings > MAX_HALVINGS {
                    reporter.log(format!(
                        "The summarizer rejected the input {MAX_HALVINGS} times after halving it ({message}); using the deterministic fallback summary"
                    ));
                    let partial_summary = partials.last().map(String::as_str).or_else(|| {
                        merge_partials_exceeded_limit
                            .then(|| running.as_deref())
                            .flatten()
                    });
                    return fallback_result(prepared, partial_summary, chunks_done);
                }
                budget = (budget / 2).max(MINIMUM_CHUNK_TOKENS);
                reporter.log(format!(
                    "The summarizer input was too long ({message}); retrying with chunks of about {budget} tokens"
                ));
            }
            CallOutcome::Failed(message) => {
                reporter.log(format!(
                    "The summarizer could not produce a usable summary ({message}); using the deterministic fallback summary"
                ));
                let partial_summary = partials.last().map(String::as_str).or_else(|| {
                    merge_partials_exceeded_limit
                        .then(|| running.as_deref())
                        .flatten()
                });
                return fallback_result(prepared, partial_summary, chunks_done);
            }
        }
    }

    if merge_partials_exceeded_limit {
        publish_progress(reporter, chunks_done, chunks_done);
        return Ok(SummaryResult {
            text: running.unwrap_or_default(),
            chunks: chunks_done,
            used_fallback: false,
        });
    }

    let Some(last) = partials.last().cloned() else {
        // Nothing to summarize beyond the anchor: keep the anchor unchanged.
        let text = prepared.prior_summary.clone().unwrap_or_default();
        return Ok(SummaryResult {
            text,
            chunks: 0,
            used_fallback: false,
        });
    };
    if partials.len() == 1 {
        return Ok(SummaryResult {
            text: last,
            chunks: 1,
            used_fallback: false,
        });
    }

    let calls_total = chunks_done + 1;
    let merged = context.call(reporter, "merge", &|problem| {
        render_merge_prompt(
            context.technique,
            &partials,
            context.custom_prompt(),
            problem,
        )
    })?;
    publish_progress(reporter, calls_total, calls_total);
    let text = match merged {
        CallOutcome::Summary(text) => text,
        CallOutcome::ContextTooLong(message) | CallOutcome::Failed(message) => {
            // Each chunk summary already builds on the previous one, so the
            // last one covers the whole history.
            reporter.log(format!(
                "The merge call failed ({message}); using the last chunk summary, which already covers every chunk"
            ));
            last
        }
    };
    Ok(SummaryResult {
        text,
        chunks: chunks_done,
        used_fallback: false,
    })
}

fn fallback_result(
    prepared: &PreparedInput,
    partial: Option<&str>,
    chunks_done: usize,
) -> Result<SummaryResult, CompactionError> {
    Ok(SummaryResult {
        text: fallback_text(prepared, partial)?,
        chunks: chunks_done,
        used_fallback: true,
    })
}

/// Technique used for a request, resolving `Custom` without text.
pub(crate) fn resolve_technique(options: &CompactionOptions) -> Technique {
    effective_technique(options.technique, options.custom_prompt.as_deref())
}

#[cfg(test)]
mod tests;
