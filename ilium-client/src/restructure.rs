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
use std::path::{Path, PathBuf};

use ilium_core::{
    AgentActivity, AgentClass, NodeId, NodeKind, PaneStatus, RestructureNode, RestructurePlan,
    SplitOrientation, Tree,
};
use ilium_inference::{
    InferenceRequest, InferenceSettings, TitleStyle, UNKNOWN_MODEL_MAX_OUTPUT_TOKENS,
};
use serde::{Deserialize, Serialize};

use crate::app::PaneRuntime;

/// Provider settings currently do not retain a selected model's advertised
/// output limit, so the inference-wide unknown-model fallback applies. This
/// deliberately leaves response length control to the prompt instead of an
/// arbitrary convenience cap.
const RESTRUCTURE_MAX_TOKENS: u32 = UNKNOWN_MODEL_MAX_OUTPUT_TOKENS;

/// The complete rendered request must remain small enough that an inference
/// backend can spend its output allowance on the replacement tree rather than
/// consuming it interpreting copied terminal/transcript noise. The observed
/// failure recordings reached 180k characters with only ten items because a
/// line-count cap alone does not constrain giant JSON/tool-output lines.
const MAXIMUM_RESTRUCTURE_PROMPT_CHARACTERS: usize = 32_000;

/// Every leaf remains represented, but all leaf evidence together receives a
/// fixed budget. This prevents one verbose agent transcript from crowding out
/// the identities of the other items the model must reference exactly once.
const MAXIMUM_ITEM_EVIDENCE_CHARACTERS: usize = 12_000;

/// Current hierarchy is useful continuity context, but is inspiration rather
/// than a second source of truth; it therefore has an independent cap.
const MAXIMUM_STRUCTURE_EVIDENCE_CHARACTERS: usize = 4_000;

/// A single item's content gets enough room to retain both its opening and
/// newest activity, without allowing a small project to recreate the old
/// unbounded-prompt behavior.
const MAXIMUM_CONTENT_CHARACTERS_PER_ITEM: usize = 2_000;

/// How many lines of a content extract to keep from the start/end when it
/// exceeds `HEAD_LINES + TAIL_LINES` -- mirrors `terminal_naming`'s
/// character-based clip, but line-based here. Kept generous, same rationale
/// as `naming::LLM_CONTEXT_EDGE_CHARS`: a restructure decision covering an
/// entire project benefits from seeing enough of each item to actually judge
/// what it's doing, not just its first and last screenful.
const CONTEXT_HEAD_LINES: usize = 60;
const CONTEXT_TAIL_LINES: usize = 60;

const RESTRUCTURE_TEMPLATE: &str = ilium_prompts::naming::RESTRUCTURE;

/// One pane or folder's current identity and content, as sent to the LLM.
/// `agent_lookup` is intentionally excluded from the rendered prompt
/// (`#[serde(skip)]`). It carries each agent's launch directory across the
/// worker boundary so transcript verification uses that pane's cwd.
#[derive(Debug, Clone, Serialize)]
pub struct LeafContext {
    pub id: NodeId,
    pub kind_label: String,
    pub current_title: String,
    pub current_icon: Option<String>,
    pub is_name_fixed: bool,
    pub filename: Option<String>,
    pub content_extract: String,
    /// Compact identity of the local evidence available before the worker
    /// reads an agent transcript. It lets automatic retry policy recognize
    /// new visible work without putting disk I/O on the UI event loop.
    automatic_content_fingerprint: u64,
    #[serde(skip)]
    agent_lookup: Option<(AgentClass, String, PathBuf)>,
}

/// Exact user-owned split layout captured beside the restructure's leaf
/// evidence. Unlike the human-readable current-structure text, this typed
/// contract is never clipped and is used to validate the model reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedSplitViewContext {
    pub id: NodeId,
    pub orientation: SplitOrientation,
    pub current_title: String,
    pub ordered_pane_ids: Vec<NodeId>,
}

/// Sends one already-rendered prompt to the current inference provider and
/// returns its raw text reply, at `RESTRUCTURE_MAX_TOKENS` rather than the
/// single-title pipeline's default -- kept as its own small trait (instead
/// of reusing `crate::naming::PromptCompletionClient`) precisely so this
/// call can ask for a larger budget than every other naming call does.
pub trait RestructureCompletionClient {
    fn complete_restructure_prompt(&self, prompt: &str) -> anyhow::Result<String>;

    fn title_style(&self) -> TitleStyle {
        TitleStyle::Summarization
    }
}

impl RestructureCompletionClient for InferenceSettings {
    fn title_style(&self) -> TitleStyle {
        self.title_style
    }

    fn complete_restructure_prompt(&self, prompt: &str) -> anyhow::Result<String> {
        let request = InferenceRequest {
            system_prompt: ilium_prompts::naming::JSON_ONLY.to_string(),
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
            is_name_fixed: node.is_name_fixed,
            filename: None,
            content_extract: String::new(),
            automatic_content_fingerprint: 0,
            agent_lookup: None,
        };
        match (panes.get(&pane_id), status) {
            (Some(PaneRuntime::Terminal(view)), PaneStatus::Agent(agent))
                if agent_session_ids.contains_key(&pane_id) =>
            {
                context.automatic_content_fingerprint =
                    stable_restructure_fingerprint(&view.with_screen(|screen| screen.contents()));
                context.agent_lookup = tree.pane_cwd(pane_id).map(|cwd| {
                    (
                        agent.class.clone(),
                        agent_session_ids[&pane_id].clone(),
                        cwd.to_path_buf(),
                    )
                });
            }
            (Some(PaneRuntime::Terminal(view)), _) => {
                context.content_extract = clip_lines(&view.with_screen(|screen| screen.contents()));
                context.automatic_content_fingerprint =
                    stable_restructure_fingerprint(&context.content_extract);
            }
            (Some(PaneRuntime::Editor(editor)), _) => {
                context.filename = editor
                    .path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned());
                context.content_extract = clip_lines(&editor.textarea.lines().join("\n"));
                context.automatic_content_fingerprint =
                    stable_restructure_fingerprint(&context.content_extract);
            }
            (Some(PaneRuntime::Board(board)), _) => {
                context.content_extract =
                    clip_lines(&crate::board::board_text_extract(&board.columns));
                context.automatic_content_fingerprint =
                    stable_restructure_fingerprint(&context.content_extract);
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
                is_name_fixed: node.is_name_fixed,
                filename: None,
                content_extract: String::new(),
                automatic_content_fingerprint: 0,
                agent_lookup: None,
            });
        }
    }

    contexts
}

/// Produces a stable identity for the exact UI evidence that would be sent
/// to a project-restructure worker. Automatic failures retry when this
/// evidence or the hierarchy changes, rather than on every repeated trigger.
pub fn project_restructure_input_fingerprint(
    contexts: &[LeafContext],
    current_structure: &str,
) -> u64 {
    let mut fingerprint = FNV_OFFSET_BASIS;
    hash_restructure_value(&mut fingerprint, current_structure);
    for context in contexts {
        hash_restructure_value(&mut fingerprint, &context.id.0.to_string());
        hash_restructure_value(&mut fingerprint, &context.kind_label);
        hash_restructure_value(&mut fingerprint, &context.current_title);
        hash_restructure_value(
            &mut fingerprint,
            context.current_icon.as_deref().unwrap_or_default(),
        );
        hash_restructure_value(
            &mut fingerprint,
            context.filename.as_deref().unwrap_or_default(),
        );
        hash_restructure_value(&mut fingerprint, &context.content_extract);
        hash_restructure_value(
            &mut fingerprint,
            &context.automatic_content_fingerprint.to_string(),
        );
        if let Some((_, _, cwd)) = &context.agent_lookup {
            hash_restructure_value(&mut fingerprint, &cwd.to_string_lossy());
        }
    }
    fingerprint
}

/// Fixed FNV-1a parameters avoid randomized collection hashers in a retry
/// policy where equal UI evidence must compare equally across calls.
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x00000100000001b3;

fn stable_restructure_fingerprint(value: &str) -> u64 {
    let mut fingerprint = FNV_OFFSET_BASIS;
    hash_restructure_value(&mut fingerprint, value);
    fingerprint
}

/// Delimit each field by its byte length so concatenated values cannot create
/// an accidental same-byte representation.
fn hash_restructure_value(fingerprint: &mut u64, value: &str) {
    for byte in (value.len() as u64)
        .to_le_bytes()
        .into_iter()
        .chain(value.bytes())
    {
        *fingerprint ^= u64::from(byte);
        *fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
    }
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

/// Captures every split owned by one project in tree order. Splits are
/// separate from ordinary leaf context because their identity and ordered
/// membership are hard constraints, not content evidence the model may clip
/// or reinterpret.
pub fn gather_project_split_view_contexts(
    tree: &Tree,
    project_id: NodeId,
) -> anyhow::Result<Vec<ProtectedSplitViewContext>> {
    if !tree
        .get(project_id)
        .is_some_and(ilium_core::Node::is_project)
    {
        anyhow::bail!(ilium_prompts::naming::NAMING_RESTRUCTURE_PROJECT_PROJECT_ID_NO_LONGER_EXISTS);
    }

    let mut split_views = Vec::new();
    gather_split_view_contexts_recursive(tree, project_id, &mut split_views)?;
    Ok(split_views)
}

fn gather_split_view_contexts_recursive(
    tree: &Tree,
    parent_id: NodeId,
    split_views: &mut Vec<ProtectedSplitViewContext>,
) -> anyhow::Result<()> {
    for child_id in tree.children_of(parent_id)? {
        let child = tree
            .get(*child_id)
            .ok_or_else(|| anyhow::anyhow!(ilium_prompts::naming::NAMING_RESTRUCTURE_TREE_CHILD_CHILD_ID_NO_LONGER_EXISTS))?;
        if child.is_split_view() {
            let orientation = tree
                .split_orientation(*child_id)
                .ok_or_else(|| anyhow::anyhow!(ilium_prompts::naming::NAMING_RESTRUCTURE_SPLIT_VIEW_CHILD_ID_HAS_NO_ORIENTATION))?;
            let ordered_pane_ids = tree.children_of(*child_id)?.to_vec();
            if ordered_pane_ids
                .iter()
                .any(|pane_id| !tree.get(*pane_id).is_some_and(ilium_core::Node::is_pane))
            {
                anyhow::bail!(ilium_prompts::naming::NAMING_RESTRUCTURE_SPLIT_VIEW_CHILD_ID_CONTAINS_A_NON_PANE_CHILD);
            }
            split_views.push(ProtectedSplitViewContext {
                id: *child_id,
                orientation,
                current_title: child.name.clone(),
                ordered_pane_ids,
            });
            continue;
        }
        if child.is_container() {
            gather_split_view_contexts_recursive(tree, *child_id, split_views)?;
        }
    }
    Ok(())
}

/// Renders one project's exact current hierarchy for the restructure prompt.
/// It deliberately follows the tree's stored child order instead of deriving
/// a second, flattened representation, so the model can preserve meaningful
/// manual organization without being forced to copy it.
pub fn render_project_structure(tree: &Tree, project_id: NodeId) -> anyhow::Result<String> {
    let project = tree
        .get(project_id)
        .filter(|node| node.is_project())
        .ok_or_else(|| anyhow::anyhow!(ilium_prompts::naming::NAMING_RESTRUCTURE_PROJECT_PROJECT_ID_NO_LONGER_EXISTS))?;
    let mut lines = vec![ilium_prompts::render_value("naming/restructure/project-id-v0-title-v1-source", &serde_json::json!({"v0": format!("{}", project_id.0), "v1": format!("{}", project.name), "v2": format!("{}", project.structure_source.prompt_label())}))];
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
            .ok_or_else(|| anyhow::anyhow!(ilium_prompts::naming::NAMING_RESTRUCTURE_TREE_CHILD_CHILD_ID_NO_LONGER_EXISTS))?;
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
        lines.push(ilium_prompts::render_value("naming/restructure/v0-v1-id-v2-title-v3-icon-v4-source-v5-name-fixed", &serde_json::json!({"v0": format!("{}", "  ".repeat(depth)), "v1": format!("{}", kind), "v2": format!("{}", child.id.0), "v3": format!("{}", child.name), "v4": format!("{}", child.inferred_icon.as_deref().unwrap_or("")), "v5": format!("{}", child.structure_source.prompt_label()), "v6": format!("{}", child.is_name_fixed)})));
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
pub fn resolve_content_extracts(contexts: &mut [LeafContext], home: &Path) {
    for context in contexts.iter_mut() {
        let Some((class, session_id, cwd)) = context.agent_lookup.take() else {
            continue;
        };
        let entries = ilium_agent_session::TranscriptLocator::new(home, &cwd)
            .transcript_for_session(&class, &session_id)
            .and_then(|transcript| {
                crate::transcript_context::recent_transcript_entries(&class, &transcript.path).ok()
            });
        context.content_extract = match entries {
            Some(entries) if !entries.is_empty() => {
                clip_lines(&format_transcript_entries(&entries))
            }
            _ => ilium_prompts::naming::RESTRUCTURE_FRAGMENT_1.to_string(),
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
        PaneStatus::PlainShell => ilium_prompts::naming::NAMING_RESTRUCTURE_PLAIN_SHELL.to_string(),
        PaneStatus::Agent(agent) => {
            ilium_prompts::render_value("naming/restructure/v0-agent", &serde_json::json!({"v0": format!("{}", agent.class.label()), "v1": format!("{}", describe_activity(&agent.activity()))}))
        }
        PaneStatus::Editor { .. } => "Editor".to_string(),
        PaneStatus::Board => "Board".to_string(),
    }
}

fn describe_activity(activity: &AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Working => "working",
        AgentActivity::WaitingBackground => ilium_prompts::naming::NAMING_SESSION_NAMING_WAITING_ON_BACKGROUND_TASKS,
        AgentActivity::BackgroundTaskStillRunning => ilium_prompts::naming::NAMING_SESSION_NAMING_A_BACKGROUND_TASK_IS_STILL_FINISHING_UP,
        AgentActivity::WaitingApproval => ilium_prompts::naming::NAMING_RESTRUCTURE_WAITING_FOR_YOUR_APPROVAL,
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
    ilium_prompts::render_value("naming/restructure/v0", &serde_json::json!({"v0": format!("{}", head), "v1": format!("{}", tail)}))
}

#[derive(Serialize)]
struct RestructurePromptContext {
    title_instructions: &'static str,
    output_example: &'static str,
    items: Vec<PromptLeafContext>,
    current_structure: String,
    protected_split_views: Vec<PromptProtectedSplitViewContext>,
    retry_feedback: Option<String>,
}

#[derive(Serialize)]
struct PromptLeafContext {
    id: NodeId,
    kind_label: String,
    current_title: String,
    current_icon: Option<String>,
    is_name_fixed: bool,
    filename: Option<String>,
    content_extract: String,
}

#[derive(Serialize)]
struct PromptProtectedSplitViewContext {
    id: NodeId,
    orientation: &'static str,
    current_title: String,
    ordered_pane_ids: String,
}

impl RestructurePromptContext {
    fn new(
        title_style: TitleStyle,
        items: &[LeafContext],
        current_structure: &str,
        protected_split_views: &[ProtectedSplitViewContext],
        item_evidence_budget: usize,
        structure_evidence_budget: usize,
        retry_feedback: Option<&str>,
    ) -> Self {
        let evidence_budget_per_item = item_evidence_budget / items.len().max(1);
        Self {
            title_instructions: match title_style {
                TitleStyle::Labeling => crate::session_naming::LABEL_INSTRUCTIONS,
                TitleStyle::Summarization => ilium_prompts::naming::RESTRUCTURE_FRAGMENT_2,
            },
            output_example: match title_style {
                TitleStyle::Labeling => ilium_prompts::naming::RESTRUCTURE_FRAGMENT_3,
                TitleStyle::Summarization => ilium_prompts::naming::RESTRUCTURE_FRAGMENT_4,
            },
            items: items
                .iter()
                .map(|item| PromptLeafContext::from_leaf(item, evidence_budget_per_item))
                .collect(),
            current_structure: crate::naming::encode_untrusted_context(&clip_restructure_evidence(
                current_structure,
                structure_evidence_budget,
            )),
            protected_split_views: protected_split_views
                .iter()
                .map(PromptProtectedSplitViewContext::from)
                .collect(),
            retry_feedback: retry_feedback.map(|feedback| {
                crate::naming::encode_untrusted_context(&clip_restructure_evidence(feedback, 512))
            }),
        }
    }
}

impl From<&ProtectedSplitViewContext> for PromptProtectedSplitViewContext {
    fn from(split_view: &ProtectedSplitViewContext) -> Self {
        Self {
            id: split_view.id,
            orientation: match split_view.orientation {
                SplitOrientation::Vertical => "vertical",
                SplitOrientation::Horizontal => "horizontal",
            },
            current_title: crate::naming::encode_untrusted_context(&split_view.current_title),
            ordered_pane_ids: split_view
                .ordered_pane_ids
                .iter()
                .map(|pane_id| pane_id.0.to_string())
                .collect::<Vec<_>>()
                .join(","),
        }
    }
}

impl PromptLeafContext {
    fn from_leaf(item: &LeafContext, evidence_budget: usize) -> Self {
        // Preserve every leaf's identity before allocating the remaining room
        // to its volatile content. The model cannot produce a valid plan if it
        // loses an id/title, whereas a long transcript can be summarized.
        let title_budget = (evidence_budget / 4).clamp(32, 256);
        let kind_budget = (evidence_budget / 10).clamp(16, 96);
        let icon_budget = (evidence_budget / 20).clamp(8, 32);
        let filename_budget = (evidence_budget / 10).clamp(16, 128);
        let metadata_budget = title_budget
            .saturating_add(kind_budget)
            .saturating_add(icon_budget)
            .saturating_add(filename_budget);
        let content_budget = evidence_budget
            .saturating_sub(metadata_budget)
            .min(MAXIMUM_CONTENT_CHARACTERS_PER_ITEM);

        Self {
            id: item.id,
            kind_label: crate::naming::encode_untrusted_context(&clip_restructure_evidence(
                &item.kind_label,
                kind_budget,
            )),
            current_title: crate::naming::encode_untrusted_context(&clip_restructure_evidence(
                &item.current_title,
                title_budget,
            )),
            current_icon: item.current_icon.as_deref().map(|icon| {
                crate::naming::encode_untrusted_context(&clip_restructure_evidence(
                    icon,
                    icon_budget,
                ))
            }),
            is_name_fixed: item.is_name_fixed,
            filename: item.filename.as_deref().map(|filename| {
                crate::naming::encode_untrusted_context(&clip_restructure_evidence(
                    filename,
                    filename_budget,
                ))
            }),
            content_extract: crate::naming::encode_untrusted_context(&clip_restructure_evidence(
                &item.content_extract,
                content_budget,
            )),
        }
    }
}

/// Preserves the beginning and newest end of a value while keeping one prompt
/// field inside its caller-assigned share of the request budget.
fn clip_restructure_evidence(value: &str, maximum_characters: usize) -> String {
    let value = value.trim();
    let character_count = value.chars().count();
    if character_count <= maximum_characters {
        return value.to_string();
    }
    if maximum_characters == 0 {
        return String::new();
    }

    let omitted_marker = ilium_prompts::naming::RESTRUCTURE_FRAGMENT_5;
    let marker_characters = omitted_marker.chars().count();
    if maximum_characters <= marker_characters {
        return value.chars().take(maximum_characters).collect();
    }

    let retained_characters = maximum_characters - marker_characters;
    let head_characters = retained_characters / 2;
    let tail_characters = retained_characters - head_characters;
    let head: String = value.chars().take(head_characters).collect();
    let tail: String = value
        .chars()
        .skip(character_count - tail_characters)
        .collect();
    format!("{head}{omitted_marker}{tail}")
}

/// Renders one bounded prompt from the exact current project state. Encoding
/// can expand JSON/control characters, so render-and-measure rather than
/// assuming raw input character budgets map one-to-one onto wire size.
fn render_restructure_prompt(
    title_style: TitleStyle,
    items: &[LeafContext],
    current_structure: &str,
    protected_split_views: &[ProtectedSplitViewContext],
    retry_feedback: Option<&str>,
) -> anyhow::Result<String> {
    let mut item_evidence_budget = MAXIMUM_ITEM_EVIDENCE_CHARACTERS;
    let mut structure_evidence_budget = MAXIMUM_STRUCTURE_EVIDENCE_CHARACTERS;

    for _ in 0..12 {
        let prompt_context = RestructurePromptContext::new(
            title_style,
            items,
            current_structure,
            protected_split_views,
            item_evidence_budget,
            structure_evidence_budget,
            retry_feedback,
        );
        let prompt = ilium_prompts::render("naming/restructure", &prompt_context)?;
        if prompt.chars().count() <= MAXIMUM_RESTRUCTURE_PROMPT_CHARACTERS {
            return Ok(prompt);
        }

        item_evidence_budget = item_evidence_budget.saturating_mul(3) / 4;
        structure_evidence_budget = structure_evidence_budget.saturating_mul(3) / 4;
    }

    anyhow::bail!(
        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_PROMPT_EXCEEDED_THE_MAXIMUM_RESTRUCTURE_PROMPT_CHARACTERS_CHARACTER_SAFET
    )
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
    ExistingGroup {
        id: NodeId,
        #[serde(default)]
        children: Vec<LlmRestructureNode>,
    },
    SplitView {
        id: NodeId,
        #[serde(default)]
        children: Vec<LlmRestructureNode>,
    },
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
    infer_restructure_plan_with_protected_splits(
        generator,
        contexts,
        ilium_prompts::naming::NAMING_RESTRUCTURE_NO_PRIOR_STRUCTURE_AVAILABLE,
        &[],
    )
}

/// Infers a plan from the live item extracts plus the project's current
/// hierarchy. The separate structure argument keeps this pure and allows
/// callers/tests to make the exact LLM request observable.
pub fn infer_restructure_plan_with_structure<G: RestructureCompletionClient>(
    generator: &G,
    contexts: &[LeafContext],
    current_structure: &str,
) -> anyhow::Result<RestructurePlan> {
    infer_restructure_plan_with_protected_splits(generator, contexts, current_structure, &[])
}

/// Infers one project plan while treating the typed split-view snapshot as a
/// hard structural contract. The server validates the same rules against its
/// live tree; this client-side pass exists to give malformed model output
/// corrective retry feedback before it crosses IPC.
pub fn infer_restructure_plan_with_protected_splits<G: RestructureCompletionClient>(
    generator: &G,
    contexts: &[LeafContext],
    current_structure: &str,
    protected_split_views: &[ProtectedSplitViewContext],
) -> anyhow::Result<RestructurePlan> {
    if contexts.is_empty() {
        anyhow::bail!(ilium_prompts::naming::NAMING_RESTRUCTURE_NO_PANES_OR_FOLDERS_TO_RESTRUCTURE);
    }

    static NEXT_OPERATION_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let operation_id = NEXT_OPERATION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut last_parse_error = None;
    let mut retry_feedback = None;
    for attempt in 1..=RESTRUCTURE_MAX_ATTEMPTS {
        let prompt = render_restructure_prompt(
            generator.title_style(),
            contexts,
            current_structure,
            protected_split_views,
            retry_feedback.as_deref(),
        )?;
        tracing::info!(
            operation_id,
            attempt,
            prompt_characters = prompt.chars().count(),
            item_count = contexts.len(),
            is_corrective_retry = retry_feedback.is_some(),
            ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_INFERENCE_STARTED
        );
        // The Debug setting explicitly promises complete LLM evidence. This
        // logger writes only while that setting is on, so retain the exact
        // bounded request here instead of hiding it behind `RUST_LOG=debug`.
        tracing::info!(operation_id, attempt, prompt = %prompt, ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_INFERENCE_PROMPT);
        let response = match generator.complete_restructure_prompt(&prompt) {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(
                    operation_id,
                    attempt,
                    error_characters = error.to_string().chars().count(),
                    ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_INFERENCE_REQUEST_FAILED
                );
                tracing::debug!(operation_id, attempt, error = %error, error_debug = ?error, ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_INFERENCE_REQUEST_FAILURE_DETAILS);
                return Err(error);
            }
        };
        tracing::info!(
            operation_id,
            attempt,
            response_characters = response.chars().count(),
            ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_INFERENCE_RESPONSE_RECEIVED
        );
        tracing::info!(operation_id, attempt, response = %response, ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_INFERENCE_RESPONSE);

        match parse_restructure_response(
            &response,
            contexts,
            protected_split_views,
            generator.title_style(),
        ) {
            Ok(plan) => {
                tracing::info!(
                    operation_id,
                    attempt,
                    ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_INFERENCE_RESPONSE_PARSED
                );
                return Ok(plan);
            }
            Err(error) => {
                tracing::error!(
                    operation_id,
                    attempt,
                    error_characters = error.to_string().chars().count(),
                    ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_INFERENCE_RESPONSE_COULD_NOT_BE_PARSED
                );
                tracing::debug!(operation_id, attempt, error = %error, error_debug = ?error, response = %response, ilium_prompts::naming::NAMING_RESTRUCTURE_UNPARSEABLE_RESTRUCTURE_INFERENCE_RESPONSE);
                retry_feedback = Some(error.to_string());
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
/// the first byte that isn't `{`. The shared structured-output parser keeps
/// title and restructure inference on the same contract.
fn parse_restructure_response(
    response: &str,
    contexts: &[LeafContext],
    protected_split_views: &[ProtectedSplitViewContext],
    title_style: TitleStyle,
) -> anyhow::Result<RestructurePlan> {
    let candidate = crate::naming::parse_structured_json_object(response, "restructure")?;
    let mut parsed: LlmRestructurePlan = serde_json::from_value(candidate).map_err(|error| {
        anyhow::anyhow!(ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_HAD_THE_WRONG_JSON_SHAPE_ERROR)
    })?;

    let mut referenced = Vec::new();
    collect_referenced_ids(&parsed.children, &mut referenced);
    let mut referenced_set = HashSet::new();
    for id in &referenced {
        if !referenced_set.insert(*id) {
            anyhow::bail!(ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_ID_ID_MORE_THAN_ONCE);
        }
    }
    let expected_set: HashSet<NodeId> = contexts.iter().map(|context| context.id).collect();
    if referenced_set != expected_set {
        let mut missing: Vec<_> = expected_set.difference(&referenced_set).copied().collect();
        let mut unexpected: Vec<_> = referenced_set.difference(&expected_set).copied().collect();
        missing.sort_by_key(|id| id.0);
        unexpected.sort_by_key(|id| id.0);
        anyhow::bail!(
            ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_THE_WRONG_LEAF_SET_MISSING_MISSING_UNEXPECTED_UNEXPEC
        );
    }
    let expected_kinds: HashMap<NodeId, ExpectedLeafKind> = contexts
        .iter()
        .map(|context| {
            let kind = if context.kind_label == "Folder" {
                ExpectedLeafKind::Folder
            } else {
                ExpectedLeafKind::Pane
            };
            (context.id, kind)
        })
        .collect();
    let mut expected_split_views = HashMap::new();
    for split_view in protected_split_views {
        if expected_split_views
            .insert(split_view.id, split_view)
            .is_some()
        {
            anyhow::bail!(
                ilium_prompts::naming::NAMING_RESTRUCTURE_PROTECTED_SPLIT_VIEW_CONTEXT_DUPLICATED_ID,
                split_view.id
            );
        }
    }
    let mut referenced_split_views = HashSet::new();
    validate_model_contract(
        &parsed.children,
        &expected_kinds,
        &expected_split_views,
        &mut referenced_split_views,
        false,
        "children",
    )?;
    let expected_split_view_ids: HashSet<NodeId> = expected_split_views.keys().copied().collect();
    if referenced_split_views != expected_split_view_ids {
        let mut missing: Vec<NodeId> = expected_split_view_ids
            .difference(&referenced_split_views)
            .copied()
            .collect();
        let mut unexpected: Vec<NodeId> = referenced_split_views
            .difference(&expected_split_view_ids)
            .copied()
            .collect();
        missing.sort_unstable();
        unexpected.sort_unstable();
        anyhow::bail!(
            ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CHANGED_THE_PROTECTED_SPLIT_VIEW_SET_MISSING_MISSING_UNEXPECTED
        );
    }
    // A restructure may reorganize existing leaves, but it must never
    // arbitrarily rebrand them. Keep each already-persisted icon authoritative
    // even when a model returns a different suggestion for that same item.
    preserve_existing_leaf_icons(&mut parsed.children, contexts);
    normalize_generated_icons(&mut parsed.children);
    validate_titles(&parsed.children)?;
    let fixed_name_ids: HashSet<NodeId> = contexts
        .iter()
        .filter(|context| context.is_name_fixed)
        .map(|context| context.id)
        .collect();
    if title_style == TitleStyle::Labeling {
        validate_generated_label_bounds(&parsed.children, &fixed_name_ids)?;
    }

    // The "[cmd] " prefix rule is terminal-only (see the prompt's
    // "command_hint" instructions): a model that mislabels some other pane
    // kind with a command_hint must not have it applied, so this set --
    // not the model's own claim -- is what actually gates the prefix.
    let terminal_pane_ids: HashSet<NodeId> = contexts
        .iter()
        .filter(|context| context.kind_label == ilium_prompts::naming::NAMING_RESTRUCTURE_PLAIN_SHELL)
        .map(|context| context.id)
        .collect();
    Ok(RestructurePlan {
        children: parsed
            .children
            .into_iter()
            .map(|node| convert_node(node, &terminal_pane_ids, &fixed_name_ids, title_style))
            .collect(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedLeafKind {
    Pane,
    Folder,
}

fn validate_model_contract(
    nodes: &[LlmRestructureNode],
    expected_kinds: &HashMap<NodeId, ExpectedLeafKind>,
    expected_split_views: &HashMap<NodeId, &ProtectedSplitViewContext>,
    referenced_split_views: &mut HashSet<NodeId>,
    in_split_view: bool,
    path: &str,
) -> anyhow::Result<()> {
    for (index, node) in nodes.iter().enumerate() {
        let node_path = format!("{path}[{index}]");
        match node {
            LlmRestructureNode::Pane { id, .. } => {
                if expected_kinds.get(id) != Some(&ExpectedLeafKind::Pane) {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_CLAIMED_ID_WAS_A_PANE_BUT_THE_EXISTING_ITEM_IS_A_FOLDE
                    );
                }
            }
            LlmRestructureNode::Folder { id, .. } => {
                if in_split_view {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_FOLDER_ID_INSIDE_A_SPLIT_VIEW
                    );
                }
                if expected_kinds.get(id) != Some(&ExpectedLeafKind::Folder) {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_CLAIMED_ID_WAS_A_FOLDER_BUT_THE_EXISTING_ITEM_IS_A_PAN
                    );
                }
            }
            LlmRestructureNode::Group { children, .. } => {
                if in_split_view {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_A_GROUP_INSIDE_A_SPLIT_VIEW
                    );
                }
                validate_model_contract(
                    children,
                    expected_kinds,
                    expected_split_views,
                    referenced_split_views,
                    false,
                    &node_path,
                )?;
            }
            LlmRestructureNode::ExistingGroup { children, .. } => {
                if in_split_view {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_AN_EXISTING_GROUP_INSIDE_A_SPLIT_VIEW
                    );
                }
                validate_model_contract(
                    children,
                    expected_kinds,
                    expected_split_views,
                    referenced_split_views,
                    false,
                    &node_path,
                )?;
            }
            LlmRestructureNode::SplitView { id, children } => {
                if in_split_view {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_NESTED_A_SPLIT_VIEW_INSIDE_ANOTHER_SPLIT_VIEW
                    );
                }
                let Some(expected_split_view) = expected_split_views.get(id) else {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_INVENTED_UNKNOWN_SPLIT_VIEW_ID
                    );
                };
                if !referenced_split_views.insert(*id) {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_REFERENCED_PROTECTED_SPLIT_VIEW_ID_MORE_THAN_ONCE
                    );
                }
                let actual_pane_ids = children
                    .iter()
                    .map(|child| match child {
                        LlmRestructureNode::Pane { id, .. } => Ok(*id),
                        LlmRestructureNode::Folder { .. }
                        | LlmRestructureNode::Group { .. }
                        | LlmRestructureNode::ExistingGroup { .. }
                        | LlmRestructureNode::SplitView { .. } => anyhow::bail!(
                            ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_NODE_PATH_PLACED_A_NON_PANE_INSIDE_PROTECTED_SPLIT_VIEW_ID
                        ),
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                if actual_pane_ids != expected_split_view.ordered_pane_ids {
                    anyhow::bail!(
                        ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CHANGED_PROTECTED_SPLIT_VIEW_ID_PANE_ORDER_OR_MEMBERSHIP_EXPECTE,
                        expected_split_view.ordered_pane_ids,
                        actual_pane_ids,
                    );
                }
                validate_model_contract(
                    children,
                    expected_kinds,
                    expected_split_views,
                    referenced_split_views,
                    true,
                    &node_path,
                )?;
            }
        }
    }
    Ok(())
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
            | LlmRestructureNode::ExistingGroup { children, .. }
            | LlmRestructureNode::SplitView { children, .. } => {
                preserve_leaf_icons_recursive(children, existing_icons);
            }
        }
    }
}

fn normalize_generated_icons(nodes: &mut [LlmRestructureNode]) {
    for node in nodes {
        match node {
            LlmRestructureNode::Pane { icon, .. } | LlmRestructureNode::Folder { icon, .. } => {
                *icon = icon.as_deref().and_then(crate::naming::normalize_icon);
            }
            LlmRestructureNode::Group { icon, children, .. } => {
                *icon = icon.as_deref().and_then(crate::naming::normalize_icon);
                normalize_generated_icons(children);
            }
            LlmRestructureNode::ExistingGroup { children, .. } => {
                normalize_generated_icons(children);
            }
            LlmRestructureNode::SplitView { children, .. } => {
                normalize_generated_icons(children);
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
            | LlmRestructureNode::ExistingGroup { children, .. }
            | LlmRestructureNode::SplitView { children, .. } => {
                collect_referenced_ids(children, out)
            }
        }
    }
}

fn validate_titles(nodes: &[LlmRestructureNode]) -> anyhow::Result<()> {
    for node in nodes {
        match node {
            LlmRestructureNode::Pane {
                title, short_title, ..
            }
            | LlmRestructureNode::Folder {
                title, short_title, ..
            } => {
                validate_title_field(title)?;
                validate_optional_title_field(short_title)?;
            }
            LlmRestructureNode::Group {
                title,
                short_title,
                children,
                ..
            } => {
                validate_title_field(title)?;
                validate_optional_title_field(short_title)?;
                validate_titles(children)?;
            }
            LlmRestructureNode::ExistingGroup { children, .. } => {
                validate_titles(children)?;
            }
            LlmRestructureNode::SplitView { children, .. } => {
                validate_titles(children)?;
            }
        }
    }
    Ok(())
}

/// Label limits apply to AI-generated names, while user-owned names retain
/// their existing form even when they exceed those limits.
fn validate_generated_label_bounds(
    nodes: &[LlmRestructureNode],
    fixed_name_ids: &HashSet<NodeId>,
) -> anyhow::Result<()> {
    for node in nodes {
        match node {
            LlmRestructureNode::Pane {
                id,
                title,
                short_title,
                ..
            }
            | LlmRestructureNode::Folder {
                id,
                title,
                short_title,
                ..
            } => {
                if !fixed_name_ids.contains(id) {
                    validate_label_pair(title, short_title)?;
                }
            }
            LlmRestructureNode::Group {
                title,
                short_title,
                children,
                ..
            } => {
                validate_label_pair(title, short_title)?;
                validate_generated_label_bounds(children, fixed_name_ids)?;
            }
            LlmRestructureNode::ExistingGroup { children, .. }
            | LlmRestructureNode::SplitView { children, .. } => {
                validate_generated_label_bounds(children, fixed_name_ids)?;
            }
        }
    }
    Ok(())
}

fn validate_label_pair(title: &str, short_title: &Option<String>) -> anyhow::Result<()> {
    if crate::naming::normalize_word_bounded(title, 1, 7).is_none() {
        anyhow::bail!(
            ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_LABEL_TITLE_MUST_CONTAIN_1_TO_7_WORDS_AND_AT_MOST_64_CHARACTERS
        );
    }
    if short_title.as_deref().is_some_and(|short| {
        !short.trim().is_empty() && crate::naming::normalize_word_bounded(short, 1, 3).is_none()
    }) {
        anyhow::bail!(
            ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_SHORT_LABEL_MUST_CONTAIN_1_TO_3_WORDS_AND_AT_MOST_64_CHARACTERS
        );
    }
    Ok(())
}

/// Rejects a blank title, or one containing a control character (e.g. an
/// embedded terminal escape sequence), before it can reach
/// `ilium_core::Node::name` and be rendered verbatim in the tree UI.
fn validate_title_field(title: &str) -> anyhow::Result<()> {
    if title.trim().is_empty() {
        anyhow::bail!(ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_AN_EMPTY_TITLE);
    }
    if title.chars().any(char::is_control) {
        anyhow::bail!(ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_A_CONTROL_CHARACTER_IN_A_TITLE);
    }
    Ok(())
}

/// `short_title` is optional, and a present-but-blank value is dropped later
/// by `normalize_optional`, so only a control character is rejected here --
/// the same rule `validate_title_field` applies to `title`, covering the
/// field that ends up in `ilium_core::Node::short_name`. Without this check,
/// `title` was validated but `short_title` was not, letting a model response
/// smuggle an unvalidated control character into the tree UI through the
/// short-form field alone.
fn validate_optional_title_field(short_title: &Option<String>) -> anyhow::Result<()> {
    let Some(short_title) = short_title else {
        return Ok(());
    };
    if short_title.chars().any(char::is_control) {
        anyhow::bail!(ilium_prompts::naming::NAMING_RESTRUCTURE_RESTRUCTURE_RESPONSE_CONTAINED_A_CONTROL_CHARACTER_IN_A_SHORT_TITLE);
    }
    Ok(())
}

/// Converts one LLM-shaped node into the core plan type, threading
/// `terminal_pane_ids` down so the `Pane` arm can gate its "[cmd] " prefix
/// (see `parse_restructure_response`) on the item actually being a plain-shell
/// terminal rather than trusting the model's own `command_hint` placement.
fn convert_node(
    node: LlmRestructureNode,
    terminal_pane_ids: &HashSet<NodeId>,
    fixed_name_ids: &HashSet<NodeId>,
    title_style: TitleStyle,
) -> RestructureNode {
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
                    normalize_restructure_title(title, title_style, fixed_name_ids.contains(&id)),
                    command_hint.as_deref(),
                ),
                short_title: normalize_optional(short_title).map(|short| {
                    crate::naming::format_with_command_hint(
                        normalize_restructure_title(
                            short,
                            title_style,
                            fixed_name_ids.contains(&id),
                        ),
                        command_hint.as_deref(),
                    )
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
            title: normalize_restructure_title(title, title_style, fixed_name_ids.contains(&id)),
            short_title: normalize_optional(short_title).map(|short| {
                normalize_restructure_title(short, title_style, fixed_name_ids.contains(&id))
            }),
            icon: icon.and_then(|value| crate::naming::normalize_icon(&value)),
        },
        LlmRestructureNode::Group {
            title,
            short_title,
            icon,
            children,
        } => RestructureNode::Group {
            title: normalize_restructure_title(title, title_style, false),
            short_title: normalize_optional(short_title)
                .map(|short| normalize_restructure_title(short, title_style, false)),
            icon: icon.and_then(|value| crate::naming::normalize_icon(&value)),
            children: children
                .into_iter()
                .map(|child| convert_node(child, terminal_pane_ids, fixed_name_ids, title_style))
                .collect(),
        },
        LlmRestructureNode::ExistingGroup { id, children } => RestructureNode::ExistingGroup {
            id,
            children: children
                .into_iter()
                .map(|child| convert_node(child, terminal_pane_ids, fixed_name_ids, title_style))
                .collect(),
        },
        LlmRestructureNode::SplitView { id, children } => RestructureNode::ExistingSplitView {
            id,
            children: children
                .into_iter()
                .map(|child| convert_node(child, terminal_pane_ids, fixed_name_ids, title_style))
                .collect(),
        },
    }
}

fn normalize_restructure_title(
    title: impl AsRef<str>,
    style: TitleStyle,
    is_name_fixed: bool,
) -> String {
    let title = title.as_ref().trim();
    if style == TitleStyle::Labeling && !is_name_fixed {
        title.to_uppercase()
    } else {
        title.to_string()
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
        prompts: RefCell<Vec<String>>,
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
                prompts: RefCell::new(Vec::new()),
                responses: RefCell::new(responses.into_iter().collect()),
            }
        }
    }

    impl RestructureCompletionClient for FakeGenerator {
        fn complete_restructure_prompt(&self, prompt: &str) -> anyhow::Result<String> {
            self.calls.set(self.calls.get() + 1);
            *self.last_prompt.borrow_mut() = Some(prompt.to_string());
            self.prompts.borrow_mut().push(prompt.to_string());
            let mut responses = self.responses.borrow_mut();
            if responses.len() > 1 {
                return Ok(responses.remove(0));
            }
            Ok(responses[0].clone())
        }
    }

    struct LabelGenerator(FakeGenerator);

    impl RestructureCompletionClient for LabelGenerator {
        fn complete_restructure_prompt(&self, prompt: &str) -> anyhow::Result<String> {
            self.0.complete_restructure_prompt(prompt)
        }

        fn title_style(&self) -> TitleStyle {
            TitleStyle::Labeling
        }
    }

    fn leaf(id: u64, title: &str) -> LeafContext {
        LeafContext {
            id: NodeId(id),
            kind_label: "Plain shell".to_string(),
            current_title: title.to_string(),
            current_icon: None,
            is_name_fixed: false,
            filename: None,
            content_extract: "$ cargo build".to_string(),
            automatic_content_fingerprint: 0,
            agent_lookup: None,
        }
    }

    fn protected_split(
        id: u64,
        orientation: SplitOrientation,
        title: &str,
        ordered_pane_ids: &[u64],
    ) -> ProtectedSplitViewContext {
        ProtectedSplitViewContext {
            id: NodeId(id),
            orientation,
            current_title: title.to_string(),
            ordered_pane_ids: ordered_pane_ids.iter().copied().map(NodeId).collect(),
        }
    }

    #[test]
    fn project_input_fingerprint_changes_for_content_or_structure_changes() {
        let contexts = vec![leaf(1, "shell")];
        let original = project_restructure_input_fingerprint(&contexts, "project shell");
        assert_eq!(
            original,
            project_restructure_input_fingerprint(&contexts, "project shell")
        );

        let mut changed_content = contexts.clone();
        changed_content[0].automatic_content_fingerprint =
            stable_restructure_fingerprint("new work");
        assert_ne!(
            original,
            project_restructure_input_fingerprint(&changed_content, "project shell")
        );
        assert_ne!(
            original,
            project_restructure_input_fingerprint(&contexts, "project renamed")
        );
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
    fn valid_response_preserves_a_protected_split_and_retitles_its_panes() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"group","title":"Focused Work","short_title":"Focused","icon":"🎯","children":[{"kind":"split_view","id":10,"children":[{"kind":"pane","id":1,"title":"Backend Agent","short_title":"Backend","icon":"🔧","command_hint":""},{"kind":"pane","id":2,"title":"Frontend Shell","short_title":"Frontend","icon":"🖥️","command_hint":"npm run"}]}]}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a"), leaf(2, "shell-b")];
        let protected_split_views = vec![protected_split(
            10,
            SplitOrientation::Horizontal,
            "Pinned Work",
            &[1, 2],
        )];

        let plan = infer_restructure_plan_with_protected_splits(
            &generator,
            &contexts,
            "project hierarchy",
            &protected_split_views,
        )
        .unwrap();

        let RestructureNode::Group { children, .. } = &plan.children[0] else {
            panic!("expected an ordinary group around the protected split");
        };
        let RestructureNode::ExistingSplitView {
            id,
            children: split_children,
        } = &children[0]
        else {
            panic!("expected a protected split reference");
        };
        assert_eq!(*id, NodeId(10));
        assert!(matches!(
            &split_children[0],
            RestructureNode::Pane { id, title, .. }
                if *id == NodeId(1) && title == "Backend Agent"
        ));
        assert!(matches!(
            &split_children[1],
            RestructureNode::Pane { id, title, .. }
                if *id == NodeId(2) && title == "[npm run] Frontend Shell"
        ));

        let prompt = generator.last_prompt.borrow().clone().unwrap();
        assert!(prompt.contains(
            r#"<split-view id="10" orientation="horizontal" current-title="Pinned Work" ordered-pane-ids="1,2" />"#
        ));
        assert!(prompt.contains("Never invent, omit, duplicate, dissolve, or nest a split view"));
        assert!(
            prompt.contains(r#"{"kind":"split_view","id":<existing-split-id>,"children":[...]}"#)
        );
        assert!(!prompt.contains(r#"{"kind":"split_view","orientation":"vertical"|"horizontal""#));
    }

    #[test]
    fn missing_protected_split_is_rejected_with_corrective_feedback() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"A","short_title":null,"icon":"📌"},{"kind":"pane","id":2,"title":"B","short_title":null,"icon":"📌"}]}"#,
        );
        let contexts = vec![leaf(1, "a"), leaf(2, "b")];
        let protected_split_views = vec![protected_split(
            10,
            SplitOrientation::Vertical,
            "Pinned",
            &[1, 2],
        )];

        let result = infer_restructure_plan_with_protected_splits(
            &generator,
            &contexts,
            "project hierarchy",
            &protected_split_views,
        );

        assert!(result.is_err());
        assert_eq!(
            generator.calls.get(),
            u8::try_from(RESTRUCTURE_MAX_ATTEMPTS).unwrap()
        );
        let prompts = generator.prompts.borrow();
        assert!(prompts[1].contains("changed the protected split-view set"));
        assert!(prompts[1].contains("NodeId(10)"));
    }

    #[test]
    fn invented_or_reordered_protected_splits_are_rejected_before_apply() {
        let contexts = vec![leaf(1, "a"), leaf(2, "b")];
        let protected_split_views = vec![protected_split(
            10,
            SplitOrientation::Vertical,
            "Pinned",
            &[1, 2],
        )];

        let invented = FakeGenerator::new(
            r#"{"children":[{"kind":"split_view","id":99,"children":[{"kind":"pane","id":1,"title":"A","short_title":null,"icon":"📌"},{"kind":"pane","id":2,"title":"B","short_title":null,"icon":"📌"}]}]}"#,
        );
        let invented_result = infer_restructure_plan_with_protected_splits(
            &invented,
            &contexts,
            "project hierarchy",
            &protected_split_views,
        );
        assert!(invented_result.is_err());

        let reordered = FakeGenerator::new(
            r#"{"children":[{"kind":"split_view","id":10,"children":[{"kind":"pane","id":2,"title":"B","short_title":null,"icon":"📌"},{"kind":"pane","id":1,"title":"A","short_title":null,"icon":"📌"}]}]}"#,
        );
        let reordered_result = infer_restructure_plan_with_protected_splits(
            &reordered,
            &contexts,
            "project hierarchy",
            &protected_split_views,
        );
        assert!(reordered_result.is_err());
    }

    #[test]
    fn empty_protected_split_must_still_be_preserved() {
        let contexts = vec![leaf(1, "outside")];
        let protected_split_views = vec![protected_split(
            10,
            SplitOrientation::Vertical,
            "Empty Pinned Split",
            &[],
        )];
        let missing = FakeGenerator::new(
            r#"{"children":[{"kind":"pane","id":1,"title":"Outside","short_title":null,"icon":"📌"}]}"#,
        );

        assert!(infer_restructure_plan_with_protected_splits(
            &missing,
            &contexts,
            "project hierarchy",
            &protected_split_views,
        )
        .is_err());

        let preserved = FakeGenerator::new(
            r#"{"children":[{"kind":"split_view","id":10,"children":[]},{"kind":"pane","id":1,"title":"Outside","short_title":null,"icon":"📌"}]}"#,
        );
        let plan = infer_restructure_plan_with_protected_splits(
            &preserved,
            &contexts,
            "project hierarchy",
            &protected_split_views,
        )
        .unwrap();
        assert!(matches!(
            &plan.children[0],
            RestructureNode::ExistingSplitView { id, children }
                if *id == NodeId(10) && children.is_empty()
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
        let prompts = generator.prompts.borrow();
        assert!(!prompts[0].contains("<retry-feedback>"));
        assert!(prompts[1].contains("<retry-feedback>"));
        assert!(prompts[1].contains("prior answer was rejected"));
    }

    #[test]
    fn bounds_recording_sized_evidence_without_omitting_any_leaf_identity() {
        // The live failure recordings had only 5-10 items but individual
        // transcript/tool-output lines large enough to create 180k-character
        // prompts. Model that shape locally without putting private recorded
        // data into the repository.
        let mut contexts = (1..=10)
            .map(|id| leaf(id, &format!("Recorded Item {id}")))
            .collect::<Vec<_>>();
        for context in &mut contexts {
            context.content_extract = format!(
                "opening-evidence-{} {} newest-evidence-{}",
                context.id.0,
                "x".repeat(30_000),
                context.id.0,
            );
        }
        let response_children = contexts
            .iter()
            .map(|context| {
                format!(
                    r#"{{"kind":"pane","id":{},"title":"Recorded Work {}","short_title":"Recorded","icon":"📌"}}"#,
                    context.id.0, context.id.0
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let generator = FakeGenerator::new(format!(
            r#"{{"children":[{{"kind":"split_view","id":99,"children":[]}}, {response_children}]}}"#
        ));
        let structure = format!("structure-head {} structure-tail", "y".repeat(30_000));
        let protected_split_views = vec![protected_split(
            99,
            SplitOrientation::Vertical,
            "Never Clipped",
            &[],
        )];

        let plan = infer_restructure_plan_with_protected_splits(
            &generator,
            &contexts,
            &structure,
            &protected_split_views,
        )
        .expect("bounded recorded-like context should still produce a valid plan");

        assert_eq!(plan.children.len(), contexts.len() + 1);
        let prompt = generator
            .last_prompt
            .borrow()
            .clone()
            .expect("rendered prompt");
        assert!(prompt.chars().count() <= MAXIMUM_RESTRUCTURE_PROMPT_CHARACTERS);
        assert!(prompt.contains("structure-head"));
        assert!(prompt.contains("structure-tail"));
        assert!(prompt.contains(
            r#"<split-view id="99" orientation="vertical" current-title="Never Clipped" ordered-pane-ids="" />"#
        ));
        for context in &contexts {
            assert!(prompt.contains(&format!(r#"<item id="{}""#, context.id.0)));
            assert!(prompt.contains(&format!("opening-evidence-{}", context.id.0)));
            assert!(prompt.contains(&format!("newest-evidence-{}", context.id.0)));
        }
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
    fn split_view_with_a_nested_group_child_is_rejected_before_apply() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"split_view","id":10,"children":[{"kind":"group","title":"Nested","short_title":null,"icon":"📁","children":[{"kind":"pane","id":1,"title":"A","short_title":null,"icon":"📌"}]}]}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];
        let protected_split_views = vec![protected_split(
            10,
            SplitOrientation::Vertical,
            "Pinned",
            &[1],
        )];

        let result = infer_restructure_plan_with_protected_splits(
            &generator,
            &contexts,
            "project hierarchy",
            &protected_split_views,
        );

        assert!(result.is_err());
        assert_eq!(
            generator.calls.get(),
            u8::try_from(RESTRUCTURE_MAX_ATTEMPTS).unwrap()
        );
    }

    #[test]
    fn wrong_existing_leaf_kind_is_rejected_before_apply() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"folder","id":1,"title":"Misclassified Existing Terminal","short_title":null,"icon":"📁"}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        let result = infer_restructure_plan(&generator, &contexts);

        assert!(result.is_err());
    }

    #[test]
    fn missing_or_invalid_generated_icons_fall_back_without_discarding_the_plan() {
        let generator = FakeGenerator::new(
            r#"{"children":[{"kind":"group","title":"Grouped Terminal Work","short_title":null,"icon":"   ","children":[{"kind":"pane","id":1,"title":"Build Project Terminal","short_title":null,"icon":""}]}]}"#,
        );
        let contexts = vec![leaf(1, "shell-a")];

        let plan = infer_restructure_plan(&generator, &contexts).unwrap();

        assert!(matches!(
            &plan.children[0],
            RestructureNode::Group {
                icon: None,
                children,
                ..
            } if matches!(&children[0], RestructureNode::Pane { icon: None, .. })
        ));
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
        assert!(prompt
            .contains("must use at most 7 words; this is a maximum, not a target or a minimum"));
        assert!(prompt.contains("never add filler merely to make it longer"));
    }

    #[test]
    fn labeling_restructure_names_items_for_tree_retrieval() {
        let prompt = render_restructure_prompt(
            TitleStyle::Labeling,
            &[leaf(1, "Coding Session")],
            "Current project",
            &[],
            None,
        )
        .unwrap();

        assert!(prompt.contains("thing the user will look for again in a large tree"));
        assert!(prompt.contains("scope the user intends to return to"));
        assert!(prompt.contains("Rewrite an automatic activity summary"));
        assert!(prompt.contains("\"title\":\"LOGIN BUG\""));
        assert!(prompt.contains("Every name-fixed ordinary group must appear exactly once"));
        assert!(prompt.contains("Split views are user-created presentation layouts"));
        assert!(!prompt.contains("Backend Agent Fixing Login Bug"));
    }

    #[test]
    fn labeling_normalizes_generated_names_without_changing_fixed_names() {
        let generator = LabelGenerator(FakeGenerator::new(
            r#"{"children":[{"kind":"group","title":"related work","short_title":"work","icon":"📁","children":[{"kind":"pane","id":1,"title":"user's original mixed Case title with many words","short_title":"user's Case","icon":"📌","command_hint":""},{"kind":"pane","id":2,"title":"diagnose icon state","short_title":"icon state","icon":"🔧","command_hint":""}]}]}"#,
        ));
        let mut fixed = leaf(1, "user's original mixed Case title with many words");
        fixed.is_name_fixed = true;
        let plan = infer_restructure_plan(&generator, &[fixed, leaf(2, "Coding Session")]).unwrap();
        let RestructureNode::Group {
            title,
            short_title,
            children,
            ..
        } = &plan.children[0]
        else {
            panic!("expected generated group");
        };
        assert_eq!(title, "RELATED WORK");
        assert_eq!(short_title.as_deref(), Some("WORK"));
        let RestructureNode::Pane {
            title, short_title, ..
        } = &children[0]
        else {
            panic!("expected fixed pane");
        };
        assert_eq!(title, "user's original mixed Case title with many words");
        assert_eq!(short_title.as_deref(), Some("user's Case"));
        let RestructureNode::Pane {
            title, short_title, ..
        } = &children[1]
        else {
            panic!("expected generated pane");
        };
        assert_eq!(title, "DIAGNOSE ICON STATE");
        assert_eq!(short_title.as_deref(), Some("ICON STATE"));
    }

    #[test]
    fn labeling_retries_a_generated_label_beyond_its_word_limit() {
        let generator = LabelGenerator(FakeGenerator::sequence([
            r#"{"children":[{"kind":"pane","id":1,"title":"one two three four five six seven eight","short_title":"one two","icon":"📌","command_hint":""}]}"#.to_string(),
            r#"{"children":[{"kind":"pane","id":1,"title":"search issue","short_title":"search issue","icon":"📌","command_hint":""}]}"#.to_string(),
        ]));

        let plan = infer_restructure_plan(&generator, &[leaf(1, "Coding Session")]).unwrap();
        assert_eq!(generator.0.calls.get(), 2);
        let RestructureNode::Pane {
            title, short_title, ..
        } = &plan.children[0]
        else {
            panic!("expected generated pane");
        };
        assert_eq!(title, "SEARCH ISSUE");
        assert_eq!(short_title.as_deref(), Some("SEARCH ISSUE"));
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
        assert!(prompt.contains("source=\\\"manual\\\""));
        assert!(prompt.contains("source=\\\"LLM restructure\\\""));
    }

    #[test]
    fn protected_split_context_comes_from_exact_project_tree_order() {
        let mut tree = Tree::new();
        let project = tree
            .add_project(PathBuf::from("/tmp/split-context"))
            .unwrap();
        let group = tree.add_group(project, "work").unwrap();
        let first = tree
            .add_pane(group, "first", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(group, "second", ilium_core::PaneContentKind::Editor)
            .unwrap();
        let split_view = tree
            .create_split_view(
                group,
                "User Layout",
                SplitOrientation::Horizontal,
                &[second, first],
            )
            .unwrap();

        let contexts = gather_project_split_view_contexts(&tree, project).unwrap();

        assert_eq!(
            contexts,
            vec![ProtectedSplitViewContext {
                id: split_view,
                orientation: SplitOrientation::Horizontal,
                current_title: "User Layout".to_string(),
                ordered_pane_ids: vec![second, first],
            }]
        );
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
        let cwd = Path::new("/home/developer/projects/ilium");

        let mut contexts = vec![LeafContext {
            id: NodeId(1),
            kind_label: "Claude agent (working)".to_string(),
            current_title: "shell".to_string(),
            current_icon: None,
            is_name_fixed: false,
            filename: None,
            content_extract: String::new(),
            automatic_content_fingerprint: 0,
            agent_lookup: Some((
                AgentClass::Claude,
                "00000000-0000-4000-8000-000000000000".to_string(),
                cwd.to_path_buf(),
            )),
        }];

        resolve_content_extracts(&mut contexts, &home);

        assert_eq!(contexts[0].content_extract, ilium_prompts::naming::RESTRUCTURE_FRAGMENT_6);
        assert!(contexts[0].agent_lookup.is_none());
    }

    #[test]
    fn resolves_each_agent_transcript_against_its_own_cwd() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let project_cwd = directory.path().join("repo");
        let worktree_cwd = directory.path().join("repo.worktrees").join("agent-task");
        let transcript_dir = home.join(".codex/sessions/2026/09/26");
        std::fs::create_dir_all(&project_cwd).unwrap();
        std::fs::create_dir_all(&worktree_cwd).unwrap();
        std::fs::create_dir_all(&transcript_dir).unwrap();

        let sessions = [
            (
                "11111111-1111-4111-8111-111111111111",
                &project_cwd,
                "main checkout task",
            ),
            (
                "22222222-2222-4222-8222-222222222222",
                &worktree_cwd,
                "worktree task",
            ),
        ];
        let mut contexts = Vec::new();
        for (index, (session_id, cwd, prompt)) in sessions.into_iter().enumerate() {
            let transcript_path = transcript_dir.join(format!(
                "rollout-2026-09-26T12-00-0{index}-{session_id}.jsonl"
            ));
            let transcript = [
                serde_json::json!({
                    "type": "session_meta",
                    "payload": {"id": session_id, "cwd": cwd},
                }),
                serde_json::json!({
                    "type": "event_msg",
                    "payload": {"type": "user_message", "message": prompt},
                }),
            ]
            .into_iter()
            .map(|entry| entry.to_string())
            .collect::<Vec<_>>()
            .join("\n");
            std::fs::write(transcript_path, transcript).unwrap();
            let mut context = leaf(index as u64 + 1, prompt);
            context.agent_lookup =
                Some((AgentClass::Codex, session_id.to_string(), cwd.to_path_buf()));
            contexts.push(context);
        }

        resolve_content_extracts(&mut contexts, &home);

        assert!(contexts[0].content_extract.contains("main checkout task"));
        assert!(!contexts[0].content_extract.contains("worktree task"));
        assert!(contexts[1].content_extract.contains("worktree task"));
        assert!(!contexts[1].content_extract.contains("main checkout task"));
    }
}
