//! Project-scoped restructure: one independent LLM call receives one
//! project's pane/folder titles and extracts, then returns a replacement
//! shape for that project only. See
//! `ilium_core::RestructurePlan`/`Tree::apply_project_restructure` for the
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

/// Output budget for a project-scoped reply (every existing pane/folder
/// retitled, plus any new groups/split-views) -- much larger than the
/// single-title pipeline's 1536 (`ilium_inference::InferenceRequest::json_only`),
/// which is sized for a 2-7 word title, not a full nested JSON tree.
const RESTRUCTURE_MAX_TOKENS: u32 = 4096;

/// How many lines of a content extract to keep from the start/end when it
/// exceeds `HEAD_LINES + TAIL_LINES` -- mirrors `terminal_naming`'s
/// character-based clip, but line-based here. Kept generous, same rationale
/// as `naming::LLM_CONTEXT_EDGE_CHARS`: a restructure decision covering an
/// entire project benefits from seeing enough of each item to actually judge
/// what it's doing, not just its first and last screenful.
const CONTEXT_HEAD_LINES: usize = 60;
const CONTEXT_TAIL_LINES: usize = 60;

const RESTRUCTURE_TEMPLATE: &str = r#"<instructions>
You are reorganizing a developer's workspace of terminals, coding agents, editors, and boards into groups by what they are working on, and giving every item -- including any new group you create -- a clear title.

Return one JSON object with a single "children" array describing the COMPLETE new structure. This is a full replacement, not an edit: every existing item listed below must appear exactly once somewhere in "children" (or nested inside a group/split_view within it), referenced by its exact numeric "id". Do not invent an id that isn't listed below. Do not omit any listed id. Do not reference any id more than once.

Each entry in "children" (and in any nested "children") is exactly one of:
- {"kind":"pane","id":<number>,"title":"...","short_title":"...","icon":"...","command_hint":"..."} -- an existing pane, referenced by id
- {"kind":"folder","id":<number>,"title":"...","short_title":"...","icon":"..."} -- an existing folder, referenced by id
- {"kind":"group","title":"...","short_title":"...","icon":"...","children":[...]} -- a brand-new group; never has an id
- {"kind":"split_view","orientation":"vertical"|"horizontal","title":"...","short_title":"...","icon":"...","children":[...]} -- a brand-new split view; never has an id; its "children" must all be "pane" entries, at most 4 of them

"title" is a full descriptive title (5 to 7 words); "short_title" is a short form (2 to 3 words); "icon" is one compact UTF-8 icon/emoticon. Existing items include their current icon in the context: preserve that exact icon across restructures. Changing a familiar icon is confusing, so only choose an icon for an item with no existing icon, and keep equivalent recreated groups' icons stable when the current structure already shows one. Group items together under one new "group" only when they share a clear common task (e.g. an agent and a terminal working on the same feature); an item with no clear relation to anything else should stay directly in the outermost "children" array instead of being forced into a group.

Every "pane" entry whose item below has kind="Plain shell" (a plain terminal, not an agent/editor/board) must also carry a "command_hint": the short form of whichever single command is currently running, most recently finished, or whose output is what's currently in that item's content -- or "" if none is clearly identifiable. Rules for "command_hint":
- Keep only the program name, plus its first argument when that argument is a subcommand (e.g. "git commit", "cargo build", "docker ps", "npm run"), or its short flags when the flags are essential to what the command does (e.g. "ps faux", "ls -la").
- Never include full argument lists, file paths, quoted strings, commit messages, URLs, environment variables, or anything piped/redirected after the first command.
- Keep it under 20 characters. Do not wrap it in brackets yourself; that's done for you, and it is added in front of "title"/"short_title", so do not restate the command inside those two fields either.
For every other pane kind (an agent, editor, or board), set "command_hint" to "" -- this rule is terminal-only. A pane's current-title above may already start with a "[...]" bracket from a previous restructure; ignore that bracket when writing the new "title"/"short_title" and let "command_hint" carry the command instead.
</instructions>
<current-structure>
The following is the project's current hierarchy. Use it as context and preserve useful continuity where it still matches the items' current work. Entries marked "manual" reflect deliberate user organization and deserve particular weight; entries marked "LLM restructure" came from a previous AI reorganization and may be retained when still useful. This is inspiration, not a constraint: do not reproduce it mechanically, and reorganize it whenever the current item content supports a clearer structure.
{{{current_structure}}}
</current-structure>
<items>
{{#each items}}
<item id="{{id}}" kind="{{kind_label}}">
    <current-title>{{current_title}}</current-title>
    <current-icon>{{current_icon}}</current-icon>
    {{#if filename}}<filename>{{filename}}</filename>{{/if}}
    <content>
{{{content_extract}}}
    </content>
</item>
{{/each}}
</items>
<output-example>{"children":[{"kind":"group","title":"Auth Refactor Across Backend And Frontend","short_title":"Auth Refactor","icon":"🔐","children":[{"kind":"pane","id":12,"title":"Backend Agent Fixing Login Bug","short_title":"Backend Agent","icon":"🔧","command_hint":""},{"kind":"pane","id":7,"title":"Frontend Dev Server Watching Auth","short_title":"Frontend Shell","icon":"🖥️","command_hint":"npm run dev"}]},{"kind":"folder","id":3,"title":"Project Root Directory","short_title":"Project Root","icon":"📁"}]}</output-example>
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
    pub current_icon: Option<String>,
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
            current_icon: node.inferred_icon.clone(),
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
                current_icon: node.inferred_icon.clone(),
                filename: None,
                content_extract: String::new(),
                agent_lookup: None,
            });
        }
    }

    contexts
}

/// Gathers only the panes and persisted folder roots owned by one project.
/// Project identity remains a tree concern; the LLM never sees entries from
/// another project in this scoped call.
pub fn gather_project_leaf_contexts(
    tree: &Tree,
    panes: &HashMap<NodeId, PaneRuntime>,
    agent_session_ids: &HashMap<NodeId, String>,
    project_id: NodeId,
) -> Vec<LeafContext> {
    gather_leaf_contexts(tree, panes, agent_session_ids)
        .into_iter()
        .filter(|context| tree.project_ancestor(context.id) == Some(project_id))
        .collect()
}

/// Renders one project's exact current hierarchy for the restructure prompt.
/// It deliberately follows the tree's stored child order instead of deriving
/// a second, flattened representation, so the model can preserve meaningful
/// manual organization without being forced to copy it.
pub fn render_project_structure(tree: &Tree, project_id: NodeId) -> anyhow::Result<String> {
    let project = tree
        .get(project_id)
        .filter(|node| node.is_project())
        .ok_or_else(|| anyhow::anyhow!("project {project_id:?} no longer exists"))?;
    let mut lines = vec![format!(
        "project id=\"{}\" title=\"{}\" source=\"{}\"",
        project_id.0,
        project.name,
        project.structure_source.prompt_label(),
    )];
    render_structure_children(tree, project_id, 1, &mut lines)?;
    Ok(lines.join("\n"))
}

fn render_structure_children(
    tree: &Tree,
    parent_id: NodeId,
    depth: usize,
    lines: &mut Vec<String>,
) -> anyhow::Result<()> {
    for child_id in tree.children_of(parent_id)? {
        let child = tree
            .get(*child_id)
            .ok_or_else(|| anyhow::anyhow!("tree child {child_id:?} no longer exists"))?;
        let kind = match &child.kind {
            NodeKind::Container(container) if container.is_group() => "group".to_string(),
            NodeKind::Container(container) if container.is_split_view() => {
                let orientation = match tree
                    .split_orientation(*child_id)
                    .expect("split views always have an orientation")
                {
                    SplitOrientation::Vertical => "vertical",
                    SplitOrientation::Horizontal => "horizontal",
                };
                format!("split_view({orientation})")
            }
            NodeKind::Pane { .. } => "pane".to_string(),
            NodeKind::Folder { .. } => "folder".to_string(),
            NodeKind::Container(_) => "container".to_string(),
        };
        lines.push(format!(
            "{}{} id=\"{}\" title=\"{}\" icon=\"{}\" source=\"{}\"",
            "  ".repeat(depth),
            kind,
            child.id.0,
            child.name,
            child.inferred_icon.as_deref().unwrap_or(""),
            child.structure_source.prompt_label(),
        ));
        if child.is_container() {
            render_structure_children(tree, *child_id, depth + 1, lines)?;
        }
    }
    Ok(())
}

/// Resolves every `agent_lookup` left by `gather_leaf_contexts` into a real
/// content extract by reading that agent's transcript -- disk I/O, so this
/// is meant to run on the background worker thread, not the main loop.
/// A transcript that can't be located or read falls back to a placeholder
/// rather than failing the whole restructure over one pane. Reads the full
/// typed user/assistant/tool stream (`transcript_context::recent_transcript_entries`),
/// the same one `session_naming::infer_pane_title` uses, rather than only
/// user prompts -- what an agent has actually been doing/answering matters
/// just as much to a restructure decision as what it was asked to do.
pub fn resolve_content_extracts(contexts: &mut [LeafContext], home: &Path, cwd: &Path) {
    for context in contexts.iter_mut() {
        let Some((class, session_id)) = context.agent_lookup.take() else {
            continue;
        };
        let entries = ilium_agent_session::TranscriptLocator::new(home, cwd)
            .transcript_for_session(&class, &session_id)
            .and_then(|transcript| {
                crate::transcript_context::recent_transcript_entries(&class, &transcript.path).ok()
            });
        context.content_extract = match entries {
            Some(entries) if !entries.is_empty() => {
                clip_lines(&format_transcript_entries(&entries))
            }
            _ => "(no transcript available)".to_string(),
        };
    }
}

/// Renders typed transcript entries as `[role] content` lines, one entry per
/// paragraph, so a restructure item's content extract shows the same
/// role-labeled shape `session_naming`'s prompt does.
fn format_transcript_entries(entries: &[crate::transcript_context::TranscriptEntry]) -> String {
    entries
        .iter()
        .map(|entry| format!("[{}] {}", entry.kind.prompt_label(), entry.content))
        .collect::<Vec<_>>()
        .join("\n\n")
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
    current_structure: &'a str,
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
        icon: Option<String>,
        /// Short form of the pane's currently-running/most-recent command --
        /// only meaningful for a plain-shell terminal pane (see the
        /// "command_hint" prompt rule above); `apply_command_hints` restricts
        /// it to those panes and folds it into `title`/`short_title` as a
        /// "[cmd] " prefix before this struct converts into
        /// `ilium_core::RestructureNode`, which has no field for it.
        #[serde(default)]
        command_hint: Option<String>,
    },
    Folder {
        id: NodeId,
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
    },
    Group {
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
        #[serde(default)]
        children: Vec<LlmRestructureNode>,
    },
    SplitView {
        orientation: LlmSplitOrientation,
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
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
    infer_restructure_plan_with_structure(generator, contexts, "(no prior structure available)")
}

/// Infers a plan from the live item extracts plus the project's current
/// hierarchy. The separate structure argument keeps this pure and allows
/// callers/tests to make the exact LLM request observable.
pub fn infer_restructure_plan_with_structure<G: RestructureCompletionClient>(
    generator: &G,
    contexts: &[LeafContext],
    current_structure: &str,
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
    let prompt = handlebars.render(
        "restructure",
        &RestructurePromptContext {
            items: contexts,
            current_structure,
        },
    )?;

    static NEXT_OPERATION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let operation_id = NEXT_OPERATION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tracing::info!(
        operation_id,
        prompt = %prompt,
        item_count = contexts.len(),
        "restructure inference started"
    );

    let mut last_parse_error = None;
    for attempt in 1..=RESTRUCTURE_MAX_ATTEMPTS {
        let response = match generator.complete_restructure_prompt(&prompt) {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(
                    operation_id,
                    attempt,
                    error = %error,
                    error_debug = ?error,
                    "restructure inference request failed"
                );
                return Err(error);
            }
        };
        tracing::info!(
            operation_id,
            attempt,
            response = %response,
            "restructure inference response received"
        );

        match parse_restructure_response(&response, contexts) {
            Ok(plan) => {
                tracing::info!(
                    operation_id,
                    attempt,
                    "restructure inference response parsed"
                );
                return Ok(plan);
            }
            Err(error) => {
                tracing::error!(
                    operation_id,
                    attempt,
                    error = %error,
                    response = %response,
                    "restructure inference response could not be parsed"
                );
                last_parse_error = Some(error);
            }
        }
    }
    Err(last_parse_error
        .expect("the loop above always records an error before exhausting attempts"))
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
    let mut parsed: LlmRestructurePlan = serde_json::from_str(candidate)
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
    // A restructure may reorganize existing leaves, but it must never
    // arbitrarily rebrand them. Keep each already-persisted icon authoritative
    // even when a model returns a different suggestion for that same item.
    preserve_existing_leaf_icons(&mut parsed.children, contexts);
    validate_titles(&parsed.children)?;

    // The "[cmd] " prefix rule is terminal-only (see the prompt's
    // "command_hint" instructions): a model that mislabels some other pane
    // kind with a command_hint must not have it applied, so this set --
    // not the model's own claim -- is what actually gates the prefix.
    let terminal_pane_ids: HashSet<NodeId> = contexts
        .iter()
        .filter(|context| context.kind_label == "Plain shell")
        .map(|context| context.id)
        .collect();

    Ok(RestructurePlan {
        children: parsed
            .children
            .into_iter()
            .map(|node| convert_node(node, &terminal_pane_ids))
            .collect(),
    })
}

fn preserve_existing_leaf_icons(nodes: &mut [LlmRestructureNode], contexts: &[LeafContext]) {
    let existing_icons: HashMap<NodeId, &str> = contexts
        .iter()
        .filter_map(|context| {
            context
                .current_icon
                .as_deref()
                .map(|icon| (context.id, icon))
        })
        .collect();
    preserve_leaf_icons_recursive(nodes, &existing_icons);
}

fn preserve_leaf_icons_recursive(
    nodes: &mut [LlmRestructureNode],
    existing_icons: &HashMap<NodeId, &str>,
) {
    for node in nodes {
        match node {
            LlmRestructureNode::Pane { id, icon, .. }
            | LlmRestructureNode::Folder { id, icon, .. } => {
                if let Some(existing_icon) = existing_icons.get(id) {
                    *icon = Some((*existing_icon).to_string());
                }
            }
            LlmRestructureNode::Group { children, .. }
            | LlmRestructureNode::SplitView { children, .. } => {
                preserve_leaf_icons_recursive(children, existing_icons);
            }
        }
    }
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
            LlmRestructureNode::Pane { title, icon, .. }
            | LlmRestructureNode::Folder { title, icon, .. } => {
                if title.trim().is_empty() {
                    anyhow::bail!("restructure response contained an empty title");
                }
                if icon
                    .as_deref()
                    .and_then(crate::naming::normalize_icon)
                    .is_none()
                {
                    anyhow::bail!("restructure response contained an invalid icon");
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
                let icon = match node {
                    LlmRestructureNode::Group { icon, .. }
                    | LlmRestructureNode::SplitView { icon, .. } => icon,
                    _ => unreachable!(),
                };
                if icon
                    .as_deref()
                    .and_then(crate::naming::normalize_icon)
                    .is_none()
                {
                    anyhow::bail!("restructure response contained an invalid icon");
                }
                validate_titles(children)?;
            }
        }
    }
    Ok(())
}

/// Converts one LLM-shaped node into the core plan type, threading
/// `terminal_pane_ids` down so the `Pane` arm can gate its "[cmd] " prefix
/// (see `parse_restructure_response`) on the item actually being a plain-shell
/// terminal rather than trusting the model's own `command_hint` placement.
fn convert_node(node: LlmRestructureNode, terminal_pane_ids: &HashSet<NodeId>) -> RestructureNode {
    match node {
        LlmRestructureNode::Pane {
            id,
            title,
            short_title,
            icon,
            command_hint,
        } => {
            let command_hint = terminal_pane_ids
                .contains(&id)
                .then_some(command_hint)
                .flatten();
            RestructureNode::Pane {
                id,
                title: crate::naming::format_with_command_hint(
                    title.trim().to_string(),
                    command_hint.as_deref(),
                ),
                short_title: normalize_optional(short_title).map(|short| {
                    crate::naming::format_with_command_hint(short, command_hint.as_deref())
                }),
                icon: icon.and_then(|value| crate::naming::normalize_icon(&value)),
            }
        }
        LlmRestructureNode::Folder {
            id,
            title,
            short_title,
            icon,
        } => RestructureNode::Folder {
            id,
            title: title.trim().to_string(),
            short_title: normalize_optional(short_title),
            icon: icon.and_then(|value| crate::naming::normalize_icon(&value)),
        },
        LlmRestructureNode::Group {
            title,
            short_title,
            icon,
            children,
        } => RestructureNode::Group {
            title: title.trim().to_string(),
            short_title: normalize_optional(short_title),
            icon: icon.and_then(|value| crate::naming::normalize_icon(&value)),
            children: children
                .into_iter()
                .map(|child| convert_node(child, terminal_pane_ids))
                .collect(),
        },
        LlmRestructureNode::SplitView {
            orientation,
            title,
            short_title,
            icon,
            children,
        } => RestructureNode::SplitView {
            orientation: match orientation {
                LlmSplitOrientation::Vertical => SplitOrientation::Vertical,
                LlmSplitOrientation::Horizontal => SplitOrientation::Horizontal,
            },
            title: title.trim().to_string(),
            short_title: normalize_optional(short_title),
            icon: icon.and_then(|value| crate::naming::normalize_icon(&value)),
            children: children
                .into_iter()
                .map(|child| convert_node(child, terminal_pane_ids))
                .collect(),
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
    use std::path::PathBuf;

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
            current_icon: None,
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
            r#"{"children":[{"kind":"group","title":"Auth Refactor Work","short_title":"Auth","icon":"🔐","children":[{"kind":"pane","id":1,"title":"Backend Agent","short_title":null,"icon":"🔧"},{"kind":"pane","id":2,"title":"Frontend Shell","short_title":"Frontend","icon":"🖥️"}]}]}"#,
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
            RestructureNode::Pane { id, title, short_title, .. }
                if *id == NodeId(1) && title == "Backend Agent" && short_title.is_none()
        ));
    }

    #[test]
    fn restructure_preserves_an_existing_leaf_icon_over_a_model_recommendation() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"Backend Agent","short_title":"Backend","icon":"🧪"}]}"#,
        );
        let mut contexts = vec![leaf(1, "shell-a")];
        contexts[0].current_icon = Some("🔧".to_string());

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert!(matches!(
            &plan.children[0],
            RestructureNode::Pane { icon: Some(icon), .. } if icon == "🔧"
        ));
    }

    #[test]
    fn a_terminal_panes_command_hint_is_prefixed_onto_title_and_short_title() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"Monitor Live Processes","short_title":"Process Monitor","icon":"📊","command_hint":"htop"}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert!(matches!(
            &plan.children[0],
            RestructureNode::Pane { title, short_title, .. }
                if title == "[htop] Monitor Live Processes"
                    && short_title.as_deref() == Some("[htop] Process Monitor")
        ));
    }

    #[test]
    fn an_empty_command_hint_leaves_a_terminal_panes_title_unprefixed() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"Idle Shell","short_title":null,"icon":"🐚","command_hint":""}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert!(matches!(
            &plan.children[0],
            RestructureNode::Pane { title, .. } if title == "Idle Shell"
        ));
    }

    #[test]
    fn a_command_hint_on_a_non_terminal_pane_is_ignored() {
        // The prompt asks the model to leave "command_hint" empty for
        // anything that isn't a plain-shell terminal, but the gate must not
        // rely on the model actually following that -- a mislabeled agent
        // pane must never get a "[cmd] " prefix either.
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"Claude Fixing Login Bug","short_title":null,"icon":"🤖","command_hint":"claude"}]}"#,
        );
        let mut contexts = vec![leaf(1, "agent-a")];
        contexts[0].kind_label = "Claude agent (working)".to_string();

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert!(matches!(
            &plan.children[0],
            RestructureNode::Pane { title, .. } if title == "Claude Fixing Login Bug"
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
            "```json\n{\"children\":[{\"kind\":\"pane\",\"id\":1,\"title\":\"A\",\"short_title\":null,\"icon\":\"📌\"}]}\n```",
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert_eq!(plan.children.len(), 1);
    }

    #[test]
    fn tolerates_a_response_wrapped_in_a_plain_code_fence() {
        let generator = FakeGenerator::new(
            "```\n{\"children\":[{\"kind\":\"pane\",\"id\":1,\"title\":\"A\",\"short_title\":null,\"icon\":\"📌\"}]}\n```",
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert_eq!(plan.children.len(), 1);
    }

    #[test]
    fn tolerates_prose_surrounding_the_json_object() {
        let generator = FakeGenerator::new(
            "Sure, here is the restructure plan:\n{\"children\":[{\"kind\":\"pane\",\"id\":1,\"title\":\"A\",\"short_title\":null,\"icon\":\"📌\"}]}\nLet me know if you need changes!",
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
            r#"{"children":[{"kind":"pane","id":1,"title":"A","short_title":null,"icon":"📌"}]}"#
                .to_string(),
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
    fn split_view_with_a_nested_group_child_is_rejected_by_the_core_apply_not_here() {
        // This module only checks the leaf-id-set invariant and title
        // non-emptiness; split-view-pane-only and capacity are
        // `Tree::apply_restructure`'s job (see ilium-core's tests), so a
        // split view containing a nested group parses fine here.
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"split_view","orientation":"vertical","title":"Split","short_title":null,"icon":"🪟","children":[{"kind":"group","title":"Nested","short_title":null,"icon":"📁","children":[{"kind":"pane","id":1,"title":"A","short_title":null,"icon":"📌"}]}]}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();
        assert_eq!(plan.children.len(), 1);
    }

    #[test]
    fn prompt_includes_every_item_and_the_json_output_example() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"A","short_title":null,"icon":"📌"}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        infer_restructure_plan(&generator, &contexts).unwrap();

        let prompt = generator.last_prompt.borrow().clone().unwrap();
        assert!(prompt.contains("<item id=\"1\""));
        assert!(prompt.contains("shell-a"));
        assert!(prompt.contains("$ cargo build"));
        assert!(prompt.contains("<output-example>"));
        assert!(prompt.contains("command_hint"));
        assert!(prompt.contains("kind=\"Plain shell\""));
    }

    #[test]
    fn prompt_includes_current_hierarchy_and_manual_or_llm_structure_sources() {
        let mut tree = Tree::new();
        let project = tree
            .add_project(PathBuf::from("/tmp/continuity-project"))
            .unwrap();
        let initial_group = tree.add_group(project, "Initial manual work").unwrap();
        let existing_pane = tree
            .add_pane(
                initial_group,
                "shell-a",
                ilium_core::PaneContentKind::Terminal,
            )
            .unwrap();
        tree.apply_project_restructure(
            project,
            RestructurePlan {
                children: vec![RestructureNode::Group {
                    title: "AI organized work".to_string(),
                    short_title: None,
                    icon: None,
                    children: vec![RestructureNode::Pane {
                        id: existing_pane,
                        title: "AI organized shell".to_string(),
                        short_title: None,
                        icon: None,
                    }],
                }],
            },
        )
        .unwrap();
        let llm_group = tree.children_of(project).unwrap()[0];
        let manual_pane = tree
            .add_pane(
                llm_group,
                "new manual shell",
                ilium_core::PaneContentKind::Terminal,
            )
            .unwrap();

        let structure = render_project_structure(&tree, project).unwrap();
        assert!(structure.contains(&format!("project id=\"{}\"", project.0)));
        assert!(structure.contains(&format!(
            "group id=\"{}\" title=\"AI organized work\" icon=\"\" source=\"LLM restructure\"",
            llm_group.0
        )));
        assert!(structure.contains(&format!(
            "pane id=\"{}\" title=\"AI organized shell\" icon=\"\" source=\"LLM restructure\"",
            existing_pane.0
        )));
        assert!(structure.contains(&format!(
            "pane id=\"{}\" title=\"new manual shell\" icon=\"\" source=\"manual\"",
            manual_pane.0
        )));

        let generator = FakeGenerator::new(format!(
            r#"{{"children":[{{"kind":"group","title":"AI organized work","short_title":null,"icon":"🔐","children":[{{"kind":"pane","id":{},"title":"AI organized shell","short_title":null,"icon":"🔧"}},{{"kind":"pane","id":{},"title":"new manual shell","short_title":null,"icon":"🖥️"}}]}}]}}"#,
            existing_pane.0, manual_pane.0
        ));
        let contexts = vec![
            leaf(existing_pane.0, "shell-a"),
            leaf(manual_pane.0, "shell-b"),
        ];

        infer_restructure_plan_with_structure(&generator, &contexts, &structure).unwrap();

        let prompt = generator.last_prompt.borrow().clone().unwrap();
        assert!(prompt.contains("Use it as context and preserve useful continuity"));
        assert!(prompt.contains("do not reproduce it mechanically"));
        assert!(prompt.contains("source=\"manual\""));
        assert!(prompt.contains("source=\"LLM restructure\""));
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
            current_icon: None,
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
