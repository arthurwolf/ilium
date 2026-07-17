//! Whole-tree restructure: one manual LLM call given every pane/folder's
//! current title and a content extract, asked to return a brand-new tree
//! shape -- fresh titles for everything (including new groups it invents)
//! and a regrouping of related panes -- rather than a diff. See
//! `ilium_core::RestructurePlan`/`Tree::apply_restructure` for the
//! atomic-apply half of this feature; this module owns only gathering
//! context and turning the LLM's JSON reply into that plan.
//!
//! Context gathering happens in two passes because it crosses a thread
//! boundary: `gather_leaf_contexts` runs on the main loop (it needs
//! `&Tree`/`&PaneRuntime`, which aren't `Send` across the background
//! worker thread `crate::naming_workers` spawns this on) and fills in
//! everything already in memory -- screen text, an editor's buffer, a
//! board's columns. An agent pane's transcript is disk I/O, so it's left
//! as a `(AgentClass, session_id)` marker and resolved by
//! `resolve_content_extracts`, which the worker thread calls instead,
//! mirroring how `session_naming::infer_pane_title` reads its transcript
//! from inside its own spawned closure.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use handlebars::Handlebars;
use ilium_core::{
    AgentActivity, AgentClass, NodeId, NodeKind, PaneStatus, RestructureNode, RestructurePlan,
    SplitOrientation, Tree,
};
use ilium_inference::{InferenceRequest, InferenceSettings};
use serde::{Deserialize, Serialize};

use crate::app::PaneRuntime;

/// Output budget for a whole-tree reply (every existing pane/folder
/// retitled, plus any new groups/split-views) -- much larger than the
/// single-title pipeline's 1536 (`ilium_inference::InferenceRequest::json_only`),
/// which is sized for a 2-7 word title, not a full nested JSON tree.
const RESTRUCTURE_MAX_TOKENS: u32 = 4096;

/// How many lines of a content extract to keep from the start/end when it
/// exceeds `HEAD_LINES + TAIL_LINES` -- mirrors `terminal_naming`'s
/// character-based clip, but line-based here since the user asked for
/// "beginning 20 lines + [...] + ending 20 lines".
const CONTEXT_HEAD_LINES: usize = 20;
const CONTEXT_TAIL_LINES: usize = 20;

const RESTRUCTURE_TEMPLATE: &str = r#"<instructions>
You are reorganizing a developer's workspace of terminals, coding agents, editors, and boards into groups by what they are working on, and giving every item -- including any new group you create -- a clear title.

Return one JSON object with a single "children" array describing the COMPLETE new structure. This is a full replacement, not an edit: every existing item listed below must appear exactly once somewhere in "children" (or nested inside a group/split_view within it), referenced by its exact numeric "id". Do not invent an id that isn't listed below. Do not omit any listed id. Do not reference any id more than once.

Each entry in "children" (and in any nested "children") is exactly one of:
- {"kind":"pane","id":<number>,"title":"...","short_title":"..."} -- an existing pane, referenced by id
- {"kind":"folder","id":<number>,"title":"...","short_title":"..."} -- an existing folder, referenced by id
- {"kind":"group","title":"...","short_title":"...","children":[...]} -- a brand-new group; never has an id
- {"kind":"split_view","orientation":"vertical"|"horizontal","title":"...","short_title":"...","children":[...]} -- a brand-new split view; never has an id; its "children" must all be "pane" entries, at most 4 of them

"title" is a full descriptive title (5 to 7 words); "short_title" is a short form (2 to 3 words). Group items together under one new "group" only when they share a clear common task (e.g. an agent and a terminal working on the same feature); an item with no clear relation to anything else should stay directly in the outermost "children" array instead of being forced into a group.
</instructions>
<items>
{{#each items}}
<item id="{{id}}" kind="{{kind_label}}">
    <current-title>{{current_title}}</current-title>
    {{#if filename}}<filename>{{filename}}</filename>{{/if}}
    <content>
{{{content_extract}}}
    </content>
</item>
{{/each}}
</items>
<output-example>{"children":[{"kind":"group","title":"Auth Refactor Across Backend And Frontend","short_title":"Auth Refactor","children":[{"kind":"pane","id":12,"title":"Backend Agent Fixing Login Bug","short_title":"Backend Agent"},{"kind":"pane","id":7,"title":"Frontend Dev Server Watching Auth","short_title":"Frontend Shell"}]},{"kind":"folder","id":3,"title":"Project Root Directory","short_title":"Project Root"}]}</output-example>
<response-format>Return exactly one JSON object following the output example's shape. Do not wrap it in Markdown.</response-format>"#;

/// One pane or folder's current identity and content, as sent to the LLM.
/// `agent_lookup` is intentionally excluded from the rendered prompt
/// (`#[serde(skip)]`) -- it is this module's own bookkeeping for
/// `resolve_content_extracts`, not something the model needs to see.
#[derive(Debug, Clone, Serialize)]
pub struct LeafContext {
    pub id: NodeId,
    pub kind_label: String,
    pub current_title: String,
    pub filename: Option<String>,
    pub content_extract: String,
    #[serde(skip)]
    agent_lookup: Option<(AgentClass, String)>,
}

/// Sends one already-rendered prompt to the current inference provider and
/// returns its raw text reply, at `RESTRUCTURE_MAX_TOKENS` rather than the
/// single-title pipeline's default -- kept as its own small trait (instead
/// of reusing `crate::naming::PromptCompletionClient`) precisely so this
/// call can ask for a larger budget than every other naming call does.
pub trait RestructureCompletionClient {
    fn complete_restructure_prompt(&self, prompt: &str) -> anyhow::Result<String>;
}

impl RestructureCompletionClient for InferenceSettings {
    fn complete_restructure_prompt(&self, prompt: &str) -> anyhow::Result<String> {
        let request = InferenceRequest {
            system_prompt: "Return concise, valid JSON only.".to_string(),
            user_prompt: prompt.to_string(),
            max_tokens: RESTRUCTURE_MAX_TOKENS,
        };
        Ok(ilium_inference::provider_from_settings(self)
            .complete(&request)?
            .text)
    }
}

/// Walks every pane and folder in tree order, building its context entry
/// from whatever is already in memory. Agent panes with a known session ID
/// get `agent_lookup` set instead of a content extract -- see module docs.
pub fn gather_leaf_contexts(
    tree: &Tree,
    panes: &HashMap<NodeId, PaneRuntime>,
    agent_session_ids: &HashMap<NodeId, String>,
) -> Vec<LeafContext> {
    let mut contexts = Vec::new();

    for pane_id in tree.pane_ids_in_tree_order() {
        let Some(node) = tree.get(pane_id) else {
            continue;
        };
        let NodeKind::Pane { status, .. } = &node.kind else {
            continue;
        };
        let mut context = LeafContext {
            id: pane_id,
            kind_label: describe_pane_status(status),
            current_title: node.name.clone(),
            filename: None,
            content_extract: String::new(),
            agent_lookup: None,
        };
        match (panes.get(&pane_id), status) {
            (
                Some(PaneRuntime::Terminal(_)),
                PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _),
            ) if agent_session_ids.contains_key(&pane_id) => {
                context.agent_lookup = Some((class.clone(), agent_session_ids[&pane_id].clone()));
            }
            (Some(PaneRuntime::Terminal(view)), _) => {
                context.content_extract = clip_lines(&view.with_screen(|screen| screen.contents()));
            }
            (Some(PaneRuntime::Editor(editor)), _) => {
                context.filename = editor
                    .path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned());
                context.content_extract = clip_lines(&editor.textarea.lines().join("\n"));
            }
            (Some(PaneRuntime::Board(board)), _) => {
                context.content_extract =
                    clip_lines(&crate::board::board_text_extract(&board.columns));
            }
            (None, _) => {}
        }
        contexts.push(context);
    }

    let mut folder_ids: Vec<NodeId> = tree
        .all_ids()
        .filter(|id| tree.get(*id).is_some_and(|node| node.is_folder()))
        .collect();
    folder_ids.sort_by_key(|id| id.0);
    for folder_id in folder_ids {
        if let Some(node) = tree.get(folder_id) {
            contexts.push(LeafContext {
                id: folder_id,
                kind_label: "Folder".to_string(),
                current_title: node.name.clone(),
                filename: None,
                content_extract: String::new(),
                agent_lookup: None,
            });
        }
    }

    contexts
}

/// Resolves every `agent_lookup` left by `gather_leaf_contexts` into a real
/// content extract by reading that agent's transcript -- disk I/O, so this
/// is meant to run on the background worker thread, not the main loop.
/// A transcript that can't be located or read falls back to a placeholder
/// rather than failing the whole restructure over one pane.
pub fn resolve_content_extracts(contexts: &mut [LeafContext], home: &Path, cwd: &Path) {
    for context in contexts.iter_mut() {
        let Some((class, session_id)) = context.agent_lookup.take() else {
            continue;
        };
        let prompts = ilium_agent_session::TranscriptLocator::new(home, cwd)
            .transcript_for_session(&class, &session_id)
            .and_then(|transcript| {
                crate::transcript_prompts::recent_user_prompts(&class, &transcript.path).ok()
            });
        context.content_extract = match prompts {
            Some(prompts) if !prompts.is_empty() => clip_lines(&prompts.join("\n\n")),
            _ => "(no transcript available)".to_string(),
        };
    }
}

fn describe_pane_status(status: &PaneStatus) -> String {
    match status {
        PaneStatus::PlainShell => "Plain shell".to_string(),
        PaneStatus::Agent(class, activity) | PaneStatus::AgentWithGoal(class, activity) => {
            format!("{} agent ({})", class.label(), describe_activity(activity))
        }
        PaneStatus::Editor { .. } => "Editor".to_string(),
        PaneStatus::Board => "Board".to_string(),
    }
}

fn describe_activity(activity: &AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Working => "working",
        AgentActivity::WaitingBackground => "waiting on background tasks",
        AgentActivity::WaitingApproval => "waiting for your approval",
        AgentActivity::Done => "done",
        AgentActivity::Idle => "idle",
    }
}

/// Keeps the first `CONTEXT_HEAD_LINES` and last `CONTEXT_TAIL_LINES` lines
/// with a `[...]` marker between them when `text` is longer than that,
/// otherwise returns it untouched.
fn clip_lines(text: &str) -> String {
    let trimmed = text.trim();
    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() <= CONTEXT_HEAD_LINES + CONTEXT_TAIL_LINES {
        return trimmed.to_string();
    }
    let head = lines[..CONTEXT_HEAD_LINES].join("\n");
    let tail = lines[lines.len() - CONTEXT_TAIL_LINES..].join("\n");
    format!("{head}\n[...]\n{tail}")
}

/// Strips a Markdown code fence (`` ```json `` / `` ``` `` ... `` ``` ``)
/// if the model wrapped its reply in one despite the prompt saying not to,
/// then falls back to the outermost `{...}` span if prose still surrounds
/// it. Free-tier models routinely do one or the other; a plain, already-bare
/// JSON object passes through unchanged.
fn extract_json_object(response: &str) -> &str {
    let trimmed = response.trim();
    let unfenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```JSON"))
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim();
    let unfenced = unfenced.strip_suffix("```").unwrap_or(unfenced).trim();
    match (unfenced.find('{'), unfenced.rfind('}')) {
        (Some(start), Some(end)) if start <= end => &unfenced[start..=end],
        _ => unfenced,
    }
}

#[derive(Serialize)]
struct RestructurePromptContext<'a> {
    items: &'a [LeafContext],
}

/// LLM-facing mirror of `ilium_core::RestructureNode`, tagged for a clean
/// `{"kind":"pane",...}` JSON shape. Kept separate from the core type
/// (which must stay bincode-compatible for the client/server wire, and
/// bincode -- unlike JSON -- cannot decode an internally-tagged enum)
/// rather than adding a tag attribute to it directly.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LlmRestructureNode {
    Pane {
        id: NodeId,
        title: String,
        short_title: Option<String>,
    },
    Folder {
        id: NodeId,
        title: String,
        short_title: Option<String>,
    },
    Group {
        title: String,
        short_title: Option<String>,
        #[serde(default)]
        children: Vec<LlmRestructureNode>,
    },
    SplitView {
        orientation: LlmSplitOrientation,
        title: String,
        short_title: Option<String>,
        #[serde(default)]
        children: Vec<LlmRestructureNode>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LlmSplitOrientation {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmRestructurePlan {
    children: Vec<LlmRestructureNode>,
}

/// A free-tier router (`openrouter/free`) occasionally lands on a backend
/// that ignores the prompt entirely -- observed in practice replying with a
/// bare moderation-style verdict ("User Safety: safe") instead of JSON, on
/// top of the already-documented Markdown-fence/prose wrapping. Since which
/// underlying model a "free" route lands on varies per call, a retry has a
/// real chance of landing somewhere that behaves; this is a manual,
/// user-initiated call, so a few extra seconds on a retry is worth it
/// rather than making the user re-click by hand.
const RESTRUCTURE_MAX_ATTEMPTS: u32 = 3;

/// Renders the prompt from already-gathered contexts, then calls `generator`
/// and parses+validates its reply into a plan ready for
/// `ClientRequest::ApplyRestructurePlan`, retrying up to
/// `RESTRUCTURE_MAX_ATTEMPTS` times while the reply itself is malformed
/// (not valid/expected JSON). A gateway-level error (network, auth,
/// configuration) is not retried here -- `ilium_inference`'s providers
/// already retry transport-level failures themselves, so surfacing it
/// immediately is more informative than masking it behind more attempts.
pub fn infer_restructure_plan<G: RestructureCompletionClient>(
    generator: &G,
    contexts: &[LeafContext],
) -> anyhow::Result<RestructurePlan> {
    if contexts.is_empty() {
        anyhow::bail!("no panes or folders to restructure");
    }

    let mut handlebars = Handlebars::new();
    // Plain-text prompt for an LLM, not HTML -- see `naming::render_and_complete`'s
    // matching comment on why the default escape fn would otherwise mangle
    // shell/code content with HTML entities.
    handlebars.register_escape_fn(handlebars::no_escape);
    handlebars.register_template_string("restructure", RESTRUCTURE_TEMPLATE)?;
    let prompt = handlebars.render("restructure", &RestructurePromptContext { items: contexts })?;

    let call_id = debug_call_id();
    maybe_debug_log(call_id, "prompt", &prompt);

    let mut last_parse_error = None;
    for attempt in 1..=RESTRUCTURE_MAX_ATTEMPTS {
        let response = match generator.complete_restructure_prompt(&prompt) {
            Ok(response) => response,
            Err(error) => {
                maybe_debug_log(
                    call_id,
                    &format!("error-attempt-{attempt}"),
                    &format!("gateway call failed: {error}"),
                );
                return Err(error);
            }
        };
        maybe_debug_log(call_id, &format!("response-attempt-{attempt}"), &response);

        match parse_restructure_response(&response, contexts) {
            Ok(plan) => return Ok(plan),
            Err(error) => {
                maybe_debug_log(
                    call_id,
                    &format!("error-attempt-{attempt}"),
                    &error.to_string(),
                );
                last_parse_error = Some(error);
            }
        }
    }
    Err(last_parse_error
        .expect("the loop above always records an error before exhausting attempts"))
}

/// Millisecond Unix timestamp used to correlate one call's prompt/response/
/// error debug files -- collisions are harmless (a same-millisecond retry
/// would just interleave into the same file set, which is still readable).
fn debug_call_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

/// `infer_restructure_plan`'s own call sites, gated so the unit tests in
/// this module (which exercise that function dozens of times with fixture
/// text) don't spam `/tmp/ilium-debug/` with noise that would bury a real
/// call's files. `debug_log` itself stays unconditional -- see its own
/// dedicated test, which calls it directly to verify the write path works.
fn maybe_debug_log(call_id: u128, suffix: &str, content: &str) {
    if !cfg!(test) {
        debug_log(call_id, suffix, content);
    }
}

/// How many distinct restructure calls' debug files to keep under
/// `/tmp/ilium-debug/` -- bounds the directory to a constant size (at most
/// this many call-id groups, each up to 7 files) regardless of how long the
/// server/client stays up, instead of growing forever. `/tmp` on this host
/// is tmpfs, so unbounded growth here is unbounded resident RAM, not just
/// disk.
const RESTRUCTURE_DEBUG_RETAINED_CALLS: usize = 20;

/// Best-effort debug capture of exactly what this feature sent to and
/// received from the LLM, under `/tmp/ilium-debug/`. Requested explicitly
/// for debugging why a free model's reply sometimes fails to parse; never
/// fails the actual restructure call -- an unwritable `/tmp` just means no
/// log this time.
fn debug_log(call_id: u128, suffix: &str, content: &str) {
    let dir = std::path::Path::new("/tmp/ilium-debug");
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // Prune once per call rather than once per file: "prompt" is always the
    // first write `infer_restructure_plan` makes for a given `call_id`, so
    // this still bounds the directory without scanning it on every one of a
    // call's up-to-7 writes.
    if suffix == "prompt" {
        prune_debug_dir(dir, RESTRUCTURE_DEBUG_RETAINED_CALLS);
    }
    let _ = std::fs::write(
        dir.join(format!("restructure-{call_id}-{suffix}.txt")),
        content,
    );
}

/// Deletes debug files belonging to any call older than the
/// `retained_calls` most recent distinct call ids found in `dir`. Best
/// effort like `debug_log` itself: an unreadable directory or a file that
/// won't delete just leaves that entry for next time, it never fails the
/// caller.
fn prune_debug_dir(dir: &std::path::Path, retained_calls: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    // Each entry's call id is the `{n}` in `restructure-{n}-{suffix}.txt`;
    // group by it so a single stale call's several files sort together.
    let mut files_by_call_id: HashMap<u128, Vec<std::path::PathBuf>> = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Some(rest) = stem.strip_prefix("restructure-") else {
            continue;
        };
        let Some((call_id_text, _suffix)) = rest.split_once('-') else {
            continue;
        };
        let Ok(call_id) = call_id_text.parse::<u128>() else {
            continue;
        };
        files_by_call_id.entry(call_id).or_default().push(path);
    }

    if files_by_call_id.len() <= retained_calls {
        return;
    }

    let mut call_ids: Vec<u128> = files_by_call_id.keys().copied().collect();
    call_ids.sort_unstable();
    let stale_count = call_ids.len() - retained_calls;
    for call_id in &call_ids[..stale_count] {
        for path in &files_by_call_id[call_id] {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Parses `response` after stripping whatever a free model tacked on around
/// the JSON despite the prompt's instructions not to -- a Markdown code
/// fence, or stray prose before/after the object -- rather than failing on
/// the first byte that isn't `{`. See `extract_json_object`.
fn parse_restructure_response(
    response: &str,
    contexts: &[LeafContext],
) -> anyhow::Result<RestructurePlan> {
    let candidate = extract_json_object(response);
    let parsed: LlmRestructurePlan = serde_json::from_str(candidate)
        .map_err(|error| anyhow::anyhow!("restructure response was not valid JSON: {error}"))?;

    let mut referenced = Vec::new();
    collect_referenced_ids(&parsed.children, &mut referenced);
    let mut referenced_set = HashSet::new();
    for id in &referenced {
        if !referenced_set.insert(*id) {
            anyhow::bail!("restructure response referenced id {id:?} more than once");
        }
    }
    let expected_set: HashSet<NodeId> = contexts.iter().map(|context| context.id).collect();
    if referenced_set != expected_set {
        anyhow::bail!(
            "restructure response covered {} of {} existing item(s) -- expected exactly the same set",
            referenced_set.len(),
            expected_set.len()
        );
    }
    validate_titles(&parsed.children)?;

    Ok(RestructurePlan {
        children: parsed.children.into_iter().map(convert_node).collect(),
    })
}

fn collect_referenced_ids(nodes: &[LlmRestructureNode], out: &mut Vec<NodeId>) {
    for node in nodes {
        match node {
            LlmRestructureNode::Pane { id, .. } | LlmRestructureNode::Folder { id, .. } => {
                out.push(*id)
            }
            LlmRestructureNode::Group { children, .. }
            | LlmRestructureNode::SplitView { children, .. } => {
                collect_referenced_ids(children, out)
            }
        }
    }
}

fn validate_titles(nodes: &[LlmRestructureNode]) -> anyhow::Result<()> {
    for node in nodes {
        match node {
            LlmRestructureNode::Pane { title, .. } | LlmRestructureNode::Folder { title, .. } => {
                if title.trim().is_empty() {
                    anyhow::bail!("restructure response contained an empty title");
                }
            }
            LlmRestructureNode::Group {
                title, children, ..
            }
            | LlmRestructureNode::SplitView {
                title, children, ..
            } => {
                if title.trim().is_empty() {
                    anyhow::bail!("restructure response contained an empty title");
                }
                validate_titles(children)?;
            }
        }
    }
    Ok(())
}

fn convert_node(node: LlmRestructureNode) -> RestructureNode {
    match node {
        LlmRestructureNode::Pane {
            id,
            title,
            short_title,
        } => RestructureNode::Pane {
            id,
            title: title.trim().to_string(),
            short_title: normalize_optional(short_title),
        },
        LlmRestructureNode::Folder {
            id,
            title,
            short_title,
        } => RestructureNode::Folder {
            id,
            title: title.trim().to_string(),
            short_title: normalize_optional(short_title),
        },
        LlmRestructureNode::Group {
            title,
            short_title,
            children,
        } => RestructureNode::Group {
            title: title.trim().to_string(),
            short_title: normalize_optional(short_title),
            children: children.into_iter().map(convert_node).collect(),
        },
        LlmRestructureNode::SplitView {
            orientation,
            title,
            short_title,
            children,
        } => RestructureNode::SplitView {
            orientation: match orientation {
                LlmSplitOrientation::Vertical => SplitOrientation::Vertical,
                LlmSplitOrientation::Horizontal => SplitOrientation::Horizontal,
            },
            title: title.trim().to_string(),
            short_title: normalize_optional(short_title),
            children: children.into_iter().map(convert_node).collect(),
        },
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct FakeGenerator {
        calls: Cell<u8>,
        last_prompt: RefCell<Option<String>>,
        // One entry per successive call; the last entry repeats once
        // exhausted, so `new` (a single-element sequence) still returns the
        // same response every time as before.
        responses: RefCell<Vec<String>>,
    }

    impl FakeGenerator {
        fn new(response: impl Into<String>) -> Self {
            Self::sequence([response.into()])
        }

        fn sequence(responses: impl IntoIterator<Item = String>) -> Self {
            Self {
                calls: Cell::new(0),
                last_prompt: RefCell::new(None),
                responses: RefCell::new(responses.into_iter().collect()),
            }
        }
    }

    impl RestructureCompletionClient for FakeGenerator {
        fn complete_restructure_prompt(&self, prompt: &str) -> anyhow::Result<String> {
            self.calls.set(self.calls.get() + 1);
            *self.last_prompt.borrow_mut() = Some(prompt.to_string());
            let mut responses = self.responses.borrow_mut();
            if responses.len() > 1 {
                return Ok(responses.remove(0));
            }
            Ok(responses[0].clone())
        }
    }

    fn leaf(id: u64, title: &str) -> LeafContext {
        LeafContext {
            id: NodeId(id),
            kind_label: "Plain shell".to_string(),
            current_title: title.to_string(),
            filename: None,
            content_extract: "$ cargo build".to_string(),
            agent_lookup: None,
        }
    }

    #[test]
    fn empty_contexts_never_call_the_gateway() {
        let generator = FakeGenerator::new("{}");
        let result = infer_restructure_plan(&generator, &[]);
        assert!(result.is_err());
        assert_eq!(generator.calls.get(), 0);
    }

    #[test]
    fn valid_response_builds_the_expected_plan() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"group","title":"Auth Refactor Work","short_title":"Auth","children":[{"kind":"pane","id":1,"title":"Backend Agent","short_title":null},{"kind":"pane","id":2,"title":"Frontend Shell","short_title":"Frontend"}]}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a"), leaf(2, "shell-b")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert_eq!(plan.children.len(), 1);
        let RestructureNode::Group {
            title, children, ..
        } = &plan.children[0]
        else {
            panic!("expected a group");
        };
        assert_eq!(title, "Auth Refactor Work");
        assert_eq!(children.len(), 2);
        assert!(matches!(
            &children[0],
            RestructureNode::Pane { id, title, short_title }
                if *id == NodeId(1) && title == "Backend Agent" && short_title.is_none()
        ));
    }

    #[test]
    fn rejects_a_response_missing_an_existing_id() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"Only one","short_title":null}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a"), leaf(2, "shell-b")];

        let result = infer_restructure_plan(&generator, &contexts);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_a_response_with_a_duplicated_id() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"A","short_title":null},{"kind":"pane","id":1,"title":"B","short_title":null}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        let result = infer_restructure_plan(&generator, &contexts);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_a_response_with_an_empty_title() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"   ","short_title":null}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        let result = infer_restructure_plan(&generator, &contexts);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_non_json_responses() {
        let generator = FakeGenerator::new("not json");
        let contexts = vec![leaf(1, "shell-a")];

        let result = infer_restructure_plan(&generator, &contexts);
        assert!(result.is_err());
    }

    #[test]
    fn tolerates_a_response_wrapped_in_a_json_code_fence() {
        let generator = FakeGenerator::new(
            "```json\n{\"children\":[{\"kind\":\"pane\",\"id\":1,\"title\":\"A\",\"short_title\":null}]}\n```",
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert_eq!(plan.children.len(), 1);
    }

    #[test]
    fn tolerates_a_response_wrapped_in_a_plain_code_fence() {
        let generator = FakeGenerator::new(
            "```\n{\"children\":[{\"kind\":\"pane\",\"id\":1,\"title\":\"A\",\"short_title\":null}]}\n```",
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert_eq!(plan.children.len(), 1);
    }

    #[test]
    fn tolerates_prose_surrounding_the_json_object() {
        let generator = FakeGenerator::new(
            "Sure, here is the restructure plan:\n{\"children\":[{\"kind\":\"pane\",\"id\":1,\"title\":\"A\",\"short_title\":null}]}\nLet me know if you need changes!",
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert_eq!(plan.children.len(), 1);
    }

    #[test]
    fn retries_after_a_non_json_reply_and_succeeds_on_a_later_attempt() {
        // Mirrors what a "free" router's dynamic model selection actually
        // does in practice: a bare non-JSON reply (observed verbatim: "User
        // Safety: safe") on the first attempt, then a well-formed reply
        // once a different backend answers.
        let generator = FakeGenerator::sequence([
            "User Safety: safe".to_string(),
            r#"{"children":[{"kind":"pane","id":1,"title":"A","short_title":null}]}"#.to_string(),
        ]);
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert_eq!(plan.children.len(), 1);
        assert_eq!(generator.calls.get(), 2);
    }

    #[test]
    fn gives_up_after_exhausting_every_retry_on_persistent_garbage() {
        let generator = FakeGenerator::new("User Safety: safe");
        let contexts = vec![leaf(1, "shell-a")];

        let result = infer_restructure_plan(&generator, &contexts);

        assert!(result.is_err());
        assert_eq!(
            generator.calls.get(),
            u8::try_from(RESTRUCTURE_MAX_ATTEMPTS).unwrap()
        );
    }

    #[test]
    fn extract_json_object_leaves_a_bare_object_unchanged() {
        assert_eq!(extract_json_object(r#"{"a":1}"#), r#"{"a":1}"#);
    }

    #[test]
    fn debug_log_writes_readable_files_under_tmp_ilium_debug() {
        let call_id = debug_call_id() + 1; // avoid colliding with a real concurrent call
        debug_log(call_id, "prompt", "the prompt text");
        debug_log(call_id, "response", "the response text");

        let dir = std::path::Path::new("/tmp/ilium-debug");
        let prompt =
            std::fs::read_to_string(dir.join(format!("restructure-{call_id}-prompt.txt"))).unwrap();
        let response =
            std::fs::read_to_string(dir.join(format!("restructure-{call_id}-response.txt")))
                .unwrap();
        assert_eq!(prompt, "the prompt text");
        assert_eq!(response, "the response text");

        let _ = std::fs::remove_file(dir.join(format!("restructure-{call_id}-prompt.txt")));
        let _ = std::fs::remove_file(dir.join(format!("restructure-{call_id}-response.txt")));
    }

    #[test]
    fn prune_debug_dir_deletes_only_the_calls_beyond_the_retained_count() {
        // Isolated directory (not the shared `/tmp/ilium-debug/`) so this
        // doesn't race the other debug-log test or a concurrent real call.
        let dir =
            std::env::temp_dir().join(format!("ilium-restructure-prune-test-{}", debug_call_id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Three synthetic calls, oldest to newest, one prompt file each.
        for call_id in [100_u128, 200, 300] {
            std::fs::write(
                dir.join(format!("restructure-{call_id}-prompt.txt")),
                "prompt",
            )
            .unwrap();
        }

        // Retain only the newest 2 of the 3 calls.
        prune_debug_dir(&dir, 2);

        assert!(!dir.join("restructure-100-prompt.txt").exists());
        assert!(dir.join("restructure-200-prompt.txt").exists());
        assert!(dir.join("restructure-300-prompt.txt").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn split_view_with_a_nested_group_child_is_rejected_by_the_core_apply_not_here() {
        // This module only checks the leaf-id-set invariant and title
        // non-emptiness; split-view-pane-only and capacity are
        // `Tree::apply_restructure`'s job (see ilium-core's tests), so a
        // split view containing a nested group parses fine here.
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"split_view","orientation":"vertical","title":"Split","short_title":null,"children":[{"kind":"group","title":"Nested","short_title":null,"children":[{"kind":"pane","id":1,"title":"A","short_title":null}]}]}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();
        assert_eq!(plan.children.len(), 1);
    }

    #[test]
    fn prompt_includes_every_item_and_the_json_output_example() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"A","short_title":null}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        infer_restructure_plan(&generator, &contexts).unwrap();

        let prompt = generator.last_prompt.borrow().clone().unwrap();
        assert!(prompt.contains("<item id=\"1\""));
        assert!(prompt.contains("shell-a"));
        assert!(prompt.contains("$ cargo build"));
        assert!(prompt.contains("<output-example>"));
    }

    #[test]
    fn clip_lines_keeps_head_and_tail_with_a_marker_when_over_the_limit() {
        let head: Vec<String> = (0..CONTEXT_HEAD_LINES)
            .map(|i| format!("head-{i}"))
            .collect();
        let tail: Vec<String> = (0..CONTEXT_TAIL_LINES)
            .map(|i| format!("tail-{i}"))
            .collect();
        let middle: Vec<String> = (0..5).map(|i| format!("middle-{i}")).collect();
        let text = [head.clone(), middle, tail.clone()].concat().join("\n");

        let clipped = clip_lines(&text);

        assert!(clipped.contains("head-0"));
        assert!(clipped.contains(&format!("head-{}", CONTEXT_HEAD_LINES - 1)));
        assert!(clipped.contains("[...]"));
        assert!(clipped.contains("tail-0"));
        assert!(!clipped.contains("middle-0"));
    }

    #[test]
    fn clip_lines_leaves_short_text_untouched() {
        assert_eq!(clip_lines("  line one\nline two  "), "line one\nline two");
    }

    #[test]
    fn resolve_content_extracts_falls_back_when_no_transcript_is_found() {
        let home = std::env::temp_dir().join(format!(
            "ilium-restructure-tests-{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let cwd = Path::new("/home/developer/dev/ai/ilium");

        let mut contexts = vec![LeafContext {
            id: NodeId(1),
            kind_label: "Claude agent (working)".to_string(),
            current_title: "shell".to_string(),
            filename: None,
            content_extract: String::new(),
            agent_lookup: Some((
                AgentClass::Claude,
                "00000000-0000-4000-8000-000000000000".to_string(),
            )),
        }];

        resolve_content_extracts(&mut contexts, &home, cwd);

        assert_eq!(contexts[0].content_extract, "(no transcript available)");
        assert!(contexts[0].agent_lookup.is_none());
    }
}
