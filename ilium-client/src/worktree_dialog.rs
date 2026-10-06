//! Client-local state and shared draw/hit-test geometry for starting an agent
//! in a Git worktree. Git and filesystem validation remain server-owned; this
//! form uses one `RepoFacts` snapshot for immediate, advisory feedback.

use std::path::{Component, Path, PathBuf};

use crate::config::{GitClosePolicy, GitDefaultBase, GitSettings};
use crossterm::event::KeyCode;
use ilium_core::{
    slugify_branch, validate_branch_name, AgentProvider, BuiltinAgentProvider, NodeId,
};
use ilium_ipc::{RepoFacts, WorkspaceCreateSpec, WorkspaceCreateStage};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;
use ratatui_textarea::TextArea;

use crate::modal;
use crate::text_prompt::{self, TextPromptState};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeDialogMode {
    New,
    Existing,
}

/// A creation-time preference for the later pane-close flow. It is not part
/// of `WorkspaceCreateSpec`; the server only accepts the selected worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeClosePolicy {
    Keep,
    OfferRemovalWhenSafe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeDialogFocus {
    Prompt,
    Branch,
    Where,
    ExistingWorktree,
    Advanced,
    Provider,
    Base,
    Path,
    ClosePolicy,
    Create,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeDialogStatus {
    Loading,
    Ready,
    Creating(WorkspaceCreateStage),
    Error(String),
}

pub struct WorktreeDialogState {
    pub(crate) identity: std::sync::Arc<()>,
    pub project_id: NodeId,
    pub parent_group: NodeId,
    pub provider: BuiltinAgentProvider,
    pub mode: WorktreeDialogMode,
    pub focus: WorktreeDialogFocus,
    pub prompt: TextArea<'static>,
    pub branch: TextPromptState,
    pub base_ref: TextPromptState,
    pub path: TextPromptState,
    pub branch_is_auto: bool,
    pub path_is_auto: bool,
    pub advanced: bool,
    pub selected_existing: usize,
    pub close_policy: WorktreeClosePolicy,
    git_settings: GitSettings,
    pub facts: Option<RepoFacts>,
    pub status: WorktreeDialogStatus,
    pub(crate) facts_retention: Option<crate::connection::EventRetention>,
    pub(crate) status_retention: Option<crate::connection::EventRetention>,
    pub(crate) derivation_retention: Option<crate::connection::EventRetention>,
}

impl WorktreeDialogState {
    /// Opens immediately; the caller sends one `QueryRepoFacts` and applies
    /// its correlated result through `apply_facts` or `apply_facts_error`.
    pub fn new(
        project_id: NodeId,
        parent_group: NodeId,
        provider: BuiltinAgentProvider,
        mode: WorktreeDialogMode,
    ) -> Self {
        Self::new_with_git_settings(
            project_id,
            parent_group,
            provider,
            mode,
            GitSettings::default(),
        )
    }

    pub fn new_with_git_settings(
        project_id: NodeId,
        parent_group: NodeId,
        provider: BuiltinAgentProvider,
        mode: WorktreeDialogMode,
        git_settings: GitSettings,
    ) -> Self {
        let mut prompt = TextArea::default();
        prompt.set_cursor_line_style(Style::new());
        prompt.set_cursor_style(Style::new().fg(Color::Black).bg(Color::Cyan));
        Self {
            identity: std::sync::Arc::new(()),
            project_id,
            parent_group,
            provider,
            mode,
            focus: WorktreeDialogFocus::Prompt,
            prompt,
            branch: TextPromptState::new(format!("{}task", git_settings.branch_prefix)),
            base_ref: TextPromptState::new(""),
            path: TextPromptState::new(""),
            branch_is_auto: true,
            path_is_auto: true,
            advanced: false,
            selected_existing: 0,
            close_policy: match git_settings.default_close_policy {
                GitClosePolicy::Keep => WorktreeClosePolicy::Keep,
                GitClosePolicy::OfferRemovalWhenSafe => WorktreeClosePolicy::OfferRemovalWhenSafe,
            },
            git_settings,
            facts: None,
            status: WorktreeDialogStatus::Loading,
            facts_retention: None,
            status_retention: None,
            derivation_retention: None,
        }
    }

    /// Cooperative source-shaped budget for the copies created by apply_facts.
    /// Template placeholders are counted only in the authored template, never
    /// recursively inside server paths. Lossy UTF-8 expands at most threefold.
    pub(crate) fn facts_derivation_bytes(&self, facts: &RepoFacts) -> Option<usize> {
        let base = match self.git_settings.default_base {
            GitDefaultBase::Current => facts
                .current_branch
                .as_deref()
                .unwrap_or(&facts.default_base_ref),
            GitDefaultBase::DefaultBranch => &facts.default_base_ref,
        }
        .len();
        let prompt = self
            .prompt
            .lines()
            .iter()
            .try_fold(0usize, |sum, line| sum.checked_add(line.len()))?
            .checked_add(self.prompt.lines().len().saturating_sub(1))?;
        // Prompt join, prefix formatting, bounded48-byte slug, collision suffix,
        // and their concurrent old/new RawVec buffers.
        let mut bytes = base
            .checked_add(prompt)?
            .checked_add(
                self.git_settings
                    .branch_prefix
                    .len()
                    .checked_add(64)?
                    .checked_mul(8)?,
            )?
            .checked_add(4096)?;
        if let Some(main) = facts.worktrees.first() {
            let parent = main.path.parent().unwrap_or(&main.path);
            let name = main.path.file_name().map_or(7, |name| name.len());
            let replacements = [
                ("{repo_parent}", parent.as_os_str().len().checked_mul(3)?),
                ("{repo_name}", name.checked_mul(3)?),
                ("{project}", main.path.as_os_str().len().checked_mul(3)?),
                ("{branch_slug}", 48),
            ];
            let template = &self.git_settings.worktree_location_template;
            let mut literal = template.len();
            let mut replacement_bytes = 0usize;
            for (token, length) in replacements {
                let count = template.matches(token).count();
                literal = literal.checked_sub(count.checked_mul(token.len())?)?;
                replacement_bytes = replacement_bytes.checked_add(count.checked_mul(length)?)?;
            }
            let expanded = literal.checked_add(replacement_bytes)?;
            // Template initial buffer, cumulative old/new expanded buffer growth,
            // native separator copy, and TextPrompt display copy. Fallback joins
            // create three old/new path buffers with fixed suffix and bounded slug.
            bytes = bytes
                .checked_add(template.len())?
                .checked_add(expanded.checked_mul(8)?)?
                .checked_add(
                    main.path
                        .as_os_str()
                        .len()
                        .checked_add(128)?
                        .checked_mul(6)?,
                )?
                .checked_add(name.checked_mul(3)?)?;
        }
        Some(bytes)
    }

    pub fn apply_facts(&mut self, facts: RepoFacts) {
        let suggested_base = match self.git_settings.default_base {
            GitDefaultBase::Current => facts
                .current_branch
                .as_deref()
                .unwrap_or(&facts.default_base_ref),
            GitDefaultBase::DefaultBranch => &facts.default_base_ref,
        };
        self.base_ref = TextPromptState::new(suggested_base);
        self.facts = Some(facts);
        self.status = WorktreeDialogStatus::Ready;
        self.status_retention = None;
        self.facts_retention = None;
        self.refresh_auto_fields();
        self.derivation_retention = None;
    }

    pub fn apply_facts_error(&mut self, error: impl Into<String>) {
        self.facts = None;
        self.status = WorktreeDialogStatus::Error(error.into());
        self.facts_retention = None;
        self.status_retention = None;
    }

    pub fn set_stage(&mut self, stage: WorkspaceCreateStage) {
        self.status = WorktreeDialogStatus::Creating(stage);
        self.status_retention = None;
    }

    pub fn set_create_error(&mut self, error: impl Into<String>) {
        self.status = WorktreeDialogStatus::Error(error.into());
        self.status_retention = None;
    }

    pub fn prompt_text(&self) -> String {
        self.prompt.lines().join("\n")
    }

    pub fn set_prompt_text(&mut self, text: &str) {
        self.prompt = TextArea::from(text.lines().map(str::to_string).collect::<Vec<_>>());
        self.prompt.set_cursor_line_style(Style::new());
        self.prompt
            .set_cursor_style(Style::new().fg(Color::Black).bg(Color::Cyan));
        self.refresh_auto_fields();
    }

    /// Call after the `TextArea` receives a key or paste. A manually edited
    /// branch/path is never replaced by a later prompt change.
    pub fn refresh_auto_fields(&mut self) {
        if self.branch_is_auto {
            let preferred = format!(
                "{}{}",
                self.git_settings.branch_prefix,
                slugify_branch(&self.prompt_text())
            );
            let branch = self.unique_branch(&preferred);
            self.branch = TextPromptState::new(branch);
        }
        if self.path_is_auto {
            self.path = TextPromptState::new(self.default_path().to_string_lossy());
        }
    }

    pub fn use_auto_branch(&mut self) {
        self.branch_is_auto = true;
        self.refresh_auto_fields();
    }

    pub fn use_auto_path(&mut self) {
        self.path_is_auto = true;
        self.refresh_auto_fields();
    }

    pub fn edit_focused_text(&mut self, key: KeyCode) {
        let target = match self.focus {
            WorktreeDialogFocus::Branch => {
                self.branch_is_auto = false;
                Some(&mut self.branch)
            }
            WorktreeDialogFocus::Base if self.advanced => Some(&mut self.base_ref),
            WorktreeDialogFocus::Path if self.advanced => {
                self.path_is_auto = false;
                Some(&mut self.path)
            }
            _ => None,
        };
        if let Some(target) = target {
            let _ = text_prompt::handle_key(target, key);
            if self.focus == WorktreeDialogFocus::Branch && self.path_is_auto {
                self.path = TextPromptState::new(self.default_path().to_string_lossy());
            }
            self.clear_create_error();
        }
    }

    pub fn paste_focused_text(&mut self, pasted: &str) {
        for character in pasted.chars().filter(|character| !character.is_control()) {
            self.edit_focused_text(KeyCode::Char(character));
        }
    }

    pub fn set_mode(&mut self, mode: WorktreeDialogMode) {
        self.mode = mode;
        self.focus = WorktreeDialogFocus::Where;
        self.clear_create_error();
    }

    pub fn move_existing_selection(&mut self, direction: i32) {
        let count = self.facts.as_ref().map_or(0, |facts| facts.worktrees.len());
        if count > 0 {
            self.selected_existing = (self.selected_existing as i32 + direction.signum())
                .clamp(0, count as i32 - 1) as usize;
        }
    }

    pub fn step_provider(&mut self, direction: i32) {
        self.provider = self.provider.stepped(direction);
    }

    pub fn focus_next(&mut self) {
        let choices = self.focus_order();
        let index = choices
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(0);
        self.focus = choices[(index + 1) % choices.len()];
    }

    pub fn focus_previous(&mut self) {
        let choices = self.focus_order();
        let index = choices
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(0);
        self.focus = choices[(index + choices.len() - 1) % choices.len()];
    }

    fn focus_order(&self) -> Vec<WorktreeDialogFocus> {
        let mut choices = vec![WorktreeDialogFocus::Prompt];
        match self.mode {
            WorktreeDialogMode::New => choices.push(WorktreeDialogFocus::Branch),
            WorktreeDialogMode::Existing => choices.push(WorktreeDialogFocus::ExistingWorktree),
        }
        choices.extend([
            WorktreeDialogFocus::Where,
            WorktreeDialogFocus::Provider,
            WorktreeDialogFocus::Advanced,
        ]);
        if self.advanced {
            choices.extend([
                WorktreeDialogFocus::Base,
                WorktreeDialogFocus::Path,
                WorktreeDialogFocus::ClosePolicy,
            ]);
        }
        choices.extend([WorktreeDialogFocus::Create, WorktreeDialogFocus::Cancel]);
        choices
    }

    fn clear_create_error(&mut self) {
        if matches!(self.status, WorktreeDialogStatus::Error(_)) && self.facts.is_some() {
            self.status = WorktreeDialogStatus::Ready;
            self.status_retention = None;
        }
    }

    fn unique_branch(&self, preferred: &str) -> String {
        let Some(facts) = &self.facts else {
            return preferred.to_string();
        };
        let is_taken = |branch: &str| facts.local_branches.iter().any(|name| name == branch);
        if !is_taken(preferred) {
            return preferred.to_string();
        }
        for suffix in 2..=9999 {
            let candidate = format!("{preferred}-{suffix}");
            if !is_taken(&candidate) {
                return candidate;
            }
        }
        preferred.to_string()
    }

    /// Matches the server's sibling template using the main registered
    /// worktree, even when this project was opened inside a linked worktree.
    pub fn default_path(&self) -> PathBuf {
        let Some(main) = self
            .facts
            .as_ref()
            .and_then(|facts| facts.worktrees.first())
        else {
            return PathBuf::new();
        };
        let slug = slugify_branch(&self.branch.buf);
        let parent = main.path.parent().unwrap_or(&main.path);
        let name = main
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_string());
        let rendered = ilium_core::expand_worktree_path_template(
            &self.git_settings.worktree_location_template,
            &main.path,
            parent,
            &name,
            &slug,
        );
        let path = PathBuf::from(native_separators(&rendered.to_string_lossy()));
        if path.is_absolute() {
            path
        } else {
            main.path.join(".ilium").join("worktrees").join(slug)
        }
    }

    pub fn dirty_reminder(&self) -> Option<&'static str> {
        self.facts.as_ref().and_then(|facts| {
            (facts.source_dirty_count > 0)
                .then_some("Uncommitted changes in this checkout are not carried over.")
        })
    }

    pub fn preparation_reminders(&self) -> String {
        if self.mode != WorktreeDialogMode::New {
            return String::new();
        }
        let mut reminders = Vec::new();
        if let Some(dirty) = self.dirty_reminder() {
            reminders.push(dirty);
        }
        if self
            .facts
            .as_ref()
            .is_some_and(|facts| facts.has_gitmodules)
        {
            reminders
                .push("This repo has submodules; configure a setup command if they are needed.");
        }
        reminders.join("\n")
    }

    /// Returns a ready-to-send payload. The caller supplies the request id
    /// and preserves this dialog until correlated `WorkspaceCreated` arrives.
    pub fn validated_request(
        &self,
    ) -> Result<(BuiltinAgentProvider, WorkspaceCreateSpec, Option<String>), String> {
        if matches!(self.status, WorktreeDialogStatus::Creating(_)) {
            return Err("Agent creation is already in progress".into());
        }
        let facts = self.facts.as_ref().ok_or_else(|| match &self.status {
            WorktreeDialogStatus::Error(error) => error.clone(),
            _ => "Checking repository…".to_string(),
        })?;
        let spec = match self.mode {
            WorktreeDialogMode::New => {
                if self.git_settings.setup_command.len() > 8 * 1024
                    || self.git_settings.setup_command.contains('\0')
                {
                    return Err(
                        "Setup command must be at most 8192 bytes and contain no NUL".into(),
                    );
                }
                let branch = self.branch.buf.trim();
                validate_branch_name(branch).map_err(|error| format!("Branch: {error}"))?;
                if facts.local_branches.iter().any(|name| name == branch) {
                    return Err(format!("Branch {branch} already exists; choose another name or an existing worktree"));
                }
                let base_ref = self.base_ref.buf.trim();
                if base_ref.is_empty() {
                    return Err("Choose a base reference".into());
                }
                let path = PathBuf::from(self.path.buf.trim());
                validate_new_path(&path, facts)?;
                if self.git_settings.setup_command.trim().is_empty() {
                    WorkspaceCreateSpec::New {
                        branch: branch.to_string(),
                        base_ref: base_ref.to_string(),
                        path,
                    }
                } else {
                    WorkspaceCreateSpec::NewWithSetup {
                        branch: branch.to_string(),
                        base_ref: base_ref.to_string(),
                        path,
                        setup_command: self.git_settings.setup_command.clone(),
                    }
                }
            }
            WorktreeDialogMode::Existing => {
                let selected = facts
                    .worktrees
                    .get(self.selected_existing)
                    .ok_or("No existing worktree is available")?;
                if selected.occupied_pane_id.is_some() {
                    return Err(format!(
                        "{} already has a live agent pane",
                        selected.path.display()
                    ));
                }
                WorkspaceCreateSpec::Existing {
                    path: selected.path.clone(),
                }
            }
        };
        let prompt = self.prompt_text();
        let initial_input = (!prompt.trim().is_empty()).then_some(prompt);
        Ok((self.provider, spec, initial_input))
    }

    pub fn preview(&self) -> String {
        let Some(facts) = &self.facts else {
            return match &self.status {
                WorktreeDialogStatus::Error(error) => error.clone(),
                _ => "Checking repository…".into(),
            };
        };
        match self.mode {
            WorktreeDialogMode::New => {
                let base = self.base_ref.buf.trim();
                let short_commit = (base == facts.default_base_ref).then(|| {
                    facts
                        .default_base_commit
                        .chars()
                        .take(7)
                        .collect::<String>()
                });
                let commit = short_commit.map_or(String::new(), |hash| format!(" @ {hash}"));
                format!(
                    "Creates {} on new branch {} from {}{}",
                    self.path.buf, self.branch.buf, base, commit
                )
            }
            WorktreeDialogMode::Existing => {
                facts.worktrees.get(self.selected_existing).map_or_else(
                    || "Choose an existing worktree".into(),
                    |entry| {
                        format!(
                            "Starts in {} on {}{}{}",
                            entry.path.display(),
                            entry.branch.as_deref().unwrap_or("detached HEAD"),
                            if entry.is_dirty {
                                " · uncommitted changes"
                            } else {
                                ""
                            },
                            if entry.occupied_pane_id.is_some() {
                                " · agent already here"
                            } else {
                                ""
                            },
                        )
                    },
                )
            }
        }
    }
}

/// Location templates are written with `/` on every platform, while the
/// directory names substituted into them carry the platform's own separator.
/// On Windows that would leave a path like `C:\\repos\\app/.ilium/worktrees`
/// in an editable field; rewriting the template's slashes keeps one spelling.
fn native_separators(rendered: &str) -> String {
    replace_forward_slashes(rendered, std::path::MAIN_SEPARATOR)
}

fn replace_forward_slashes(text: &str, separator: char) -> String {
    if separator == '/' {
        return text.to_string();
    }
    text.replace('/', &separator.to_string())
}

fn validate_new_path(path: &Path, facts: &RepoFacts) -> Result<(), String> {
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err("Worktree path must be absolute and normalized".into());
    }
    if path.starts_with(&facts.repo_common_dir)
        || facts
            .worktrees
            .iter()
            .any(|entry| path.starts_with(&entry.path) || entry.path.starts_with(path))
    {
        return Err("Worktree path overlaps an existing checkout or Git metadata".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorktreeDialogLayout {
    pub popup: Rect,
    pub provider_row: Rect,
    pub prompt_area: Rect,
    pub branch_row: Rect,
    pub where_row: Rect,
    pub existing_list: Rect,
    pub advanced_row: Rect,
    pub base_row: Rect,
    pub path_row: Rect,
    pub close_policy_row: Rect,
    pub preview_row: Rect,
    pub reminder_row: Rect,
    pub error_row: Rect,
    pub button_row: Rect,
    pub create_button: Rect,
    pub cancel_button: Rect,
    pub hint_row: Rect,
}

pub fn dialog_layout(screen_area: Rect, state: &WorktreeDialogState) -> WorktreeDialogLayout {
    let existing_height = if state.mode == WorktreeDialogMode::Existing {
        3
    } else {
        0
    };
    let advanced_height = if state.advanced { 3 } else { 0 };
    let popup = modal::centered_fixed_rect(92, 18 + existing_height + advanced_height, screen_area);
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // provider
            Constraint::Length(3), // prompt
            Constraint::Length(1), // branch
            Constraint::Length(1), // where
            Constraint::Length(existing_height),
            Constraint::Length(1), // advanced switch
            Constraint::Length(if state.advanced { 1 } else { 0 }),
            Constraint::Length(if state.advanced { 1 } else { 0 }),
            Constraint::Length(if state.advanced { 1 } else { 0 }),
            Constraint::Min(1),    // preview
            Constraint::Length(2), // preparation reminders
            Constraint::Length(1), // error or progress
            Constraint::Length(1), // buttons
            Constraint::Length(1), // hint
        ])
        .split(inner);
    let buttons = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[12]);
    WorktreeDialogLayout {
        popup,
        provider_row: rows[0],
        prompt_area: rows[1],
        branch_row: rows[2],
        where_row: rows[3],
        existing_list: rows[4],
        advanced_row: rows[5],
        base_row: rows[6],
        path_row: rows[7],
        close_policy_row: rows[8],
        preview_row: rows[9],
        reminder_row: rows[10],
        error_row: rows[11],
        button_row: rows[12],
        create_button: buttons[0],
        cancel_button: buttons[1],
        hint_row: rows[13],
    }
}

/// Uses the exact rectangles rendered by `draw_dialog`; callers may then
/// handle a selected provider, worktree list row, or button as appropriate.
pub fn hit_test(
    state: &WorktreeDialogState,
    layout: &WorktreeDialogLayout,
    position: Position,
) -> Option<WorktreeDialogFocus> {
    if !layout.popup.contains(position) {
        return None;
    }
    let fields = [
        (layout.provider_row, WorktreeDialogFocus::Provider),
        (layout.prompt_area, WorktreeDialogFocus::Prompt),
        (layout.branch_row, WorktreeDialogFocus::Branch),
        (layout.where_row, WorktreeDialogFocus::Where),
        (layout.existing_list, WorktreeDialogFocus::ExistingWorktree),
        (layout.advanced_row, WorktreeDialogFocus::Advanced),
        (layout.base_row, WorktreeDialogFocus::Base),
        (layout.path_row, WorktreeDialogFocus::Path),
        (layout.close_policy_row, WorktreeDialogFocus::ClosePolicy),
        (layout.create_button, WorktreeDialogFocus::Create),
        (layout.cancel_button, WorktreeDialogFocus::Cancel),
    ];
    fields.into_iter().find_map(|(area, focus)| {
        if !area.contains(position) {
            return None;
        }
        match focus {
            WorktreeDialogFocus::Branch if state.mode == WorktreeDialogMode::Existing => None,
            WorktreeDialogFocus::ExistingWorktree if state.mode == WorktreeDialogMode::New => None,
            WorktreeDialogFocus::Base
            | WorktreeDialogFocus::Path
            | WorktreeDialogFocus::ClosePolicy
                if !state.advanced =>
            {
                None
            }
            _ => Some(focus),
        }
    })
}

pub fn existing_row_at(
    state: &WorktreeDialogState,
    layout: &WorktreeDialogLayout,
    position: Position,
) -> Option<usize> {
    if state.mode != WorktreeDialogMode::Existing || !layout.existing_list.contains(position) {
        return None;
    }
    let index = state.existing_window(layout).0 + usize::from(position.y - layout.existing_list.y);
    state
        .facts
        .as_ref()
        .and_then(|facts| (index < facts.worktrees.len()).then_some(index))
}

impl WorktreeDialogState {
    pub fn existing_window(&self, layout: &WorktreeDialogLayout) -> (usize, usize) {
        let count = self.facts.as_ref().map_or(0, |facts| facts.worktrees.len());
        modal::create_group_visible_window(
            self.selected_existing,
            count,
            usize::from(layout.existing_list.height),
        )
    }
}

pub fn draw_dialog(frame: &mut Frame, screen_area: Rect, state: &WorktreeDialogState) {
    let layout = dialog_layout(screen_area, state);
    frame.render_widget(Clear, layout.popup);
    frame.render_widget(
        theme::block(true).title(theme::chrome_title("New agent in a worktree")),
        layout.popup,
    );
    render_choice(
        frame,
        screen_area,
        state,
        crate::value_worktree::WorkspaceChoice::Provider,
    );
    let block = theme::block(state.focus == WorktreeDialogFocus::Prompt)
        .title(theme::chrome_title("Prompt · optional first message"));
    let prompt_inner = block.inner(layout.prompt_area);
    frame.render_widget(block, layout.prompt_area);
    frame.render_widget(&state.prompt, prompt_inner);
    if state.mode == WorktreeDialogMode::New {
        let auto = if state.branch_is_auto {
            " [auto]"
        } else {
            " [manual]"
        };
        draw_line(
            frame,
            layout.branch_row,
            format!("Branch  {}{auto}", state.branch.buf),
            state.focus == WorktreeDialogFocus::Branch,
        );
    } else {
        draw_line(
            frame,
            layout.branch_row,
            "Existing worktree · select below".into(),
            false,
        );
    }
    render_choice(
        frame,
        screen_area,
        state,
        crate::value_worktree::WorkspaceChoice::Where,
    );
    if state.mode == WorktreeDialogMode::Existing {
        if let Some(facts) = &state.facts {
            let (start, end) = state.existing_window(&layout);
            for (row, entry) in facts.worktrees[start..end].iter().enumerate() {
                let area = Rect::new(
                    layout.existing_list.x,
                    layout.existing_list.y + row as u16,
                    layout.existing_list.width,
                    1,
                );
                let occupied = if entry.occupied_pane_id.is_some() {
                    " · occupied"
                } else {
                    ""
                };
                draw_line(
                    frame,
                    area,
                    format!(
                        "{} {} · {}{}{}",
                        if start + row == state.selected_existing {
                            "›"
                        } else {
                            " "
                        },
                        entry.branch.as_deref().unwrap_or("detached"),
                        entry.path.display(),
                        if entry.is_dirty { " · dirty" } else { "" },
                        occupied
                    ),
                    state.focus == WorktreeDialogFocus::ExistingWorktree
                        && start + row == state.selected_existing,
                );
            }
        }
    }
    draw_line(
        frame,
        layout.advanced_row,
        format!("{} Advanced", if state.advanced { "⌄" } else { "›" }),
        state.focus == WorktreeDialogFocus::Advanced,
    );
    if state.advanced {
        draw_line(
            frame,
            layout.base_row,
            format!(
                "Agent {} · Base {}",
                state.provider.label(),
                state.base_ref.buf
            ),
            state.focus == WorktreeDialogFocus::Base,
        );
        draw_line(
            frame,
            layout.path_row,
            format!(
                "Path {}{}",
                state.path.buf,
                if state.path_is_auto { " [auto]" } else { "" }
            ),
            state.focus == WorktreeDialogFocus::Path,
        );
        render_choice(
            frame,
            screen_area,
            state,
            crate::value_worktree::WorkspaceChoice::ClosePolicy,
        );
    }
    frame.render_widget(
        Paragraph::new(state.preview())
            .wrap(Wrap { trim: true })
            .style(Style::new().fg(Color::Cyan)),
        layout.preview_row,
    );
    frame.render_widget(
        Paragraph::new(state.preparation_reminders()).style(Style::new().fg(Color::Yellow)),
        layout.reminder_row,
    );
    let status = match &state.status {
        WorktreeDialogStatus::Loading => "Checking repository…".to_string(),
        WorktreeDialogStatus::Ready => state.validated_request().err().unwrap_or_default(),
        WorktreeDialogStatus::Creating(stage) => match stage {
            WorkspaceCreateStage::CreatingWorktree => "Creating worktree…",
            WorkspaceCreateStage::Preparing => "Preparing files…",
            WorkspaceCreateStage::RunningSetup => "Running setup…",
            WorkspaceCreateStage::Starting => "Starting agent…",
        }
        .into(),
        WorktreeDialogStatus::Error(error) => error.clone(),
    };
    draw_line(frame, layout.error_row, status, false);
    frame.render_widget(
        Paragraph::new("[ Create agent ]")
            .alignment(Alignment::Center)
            .style(selected_style(state.focus == WorktreeDialogFocus::Create)),
        layout.create_button,
    );
    frame.render_widget(
        Paragraph::new("[ Cancel ]")
            .alignment(Alignment::Center)
            .style(selected_style(state.focus == WorktreeDialogFocus::Cancel)),
        layout.cancel_button,
    );
    frame.render_widget(
        Paragraph::new(if state.advanced && state.mode == WorktreeDialogMode::New {
            "Git checkout hooks and filters run on create · Esc cancel"
        } else {
            "Tab fields · arrows choose · Enter create · Ctrl+Enter create · Esc cancel"
        })
        .alignment(Alignment::Center)
        .style(Style::new().add_modifier(Modifier::DIM)),
        layout.hint_row,
    );
}

fn selected_style(selected: bool) -> Style {
    if selected {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    }
}

fn draw_line(frame: &mut Frame, area: Rect, text: String, selected: bool) {
    frame.render_widget(Paragraph::new(text).style(selected_style(selected)), area);
}

fn render_choice(
    frame: &mut Frame,
    screen: Rect,
    state: &WorktreeDialogState,
    field: crate::value_worktree::WorkspaceChoice,
) {
    let style = selected_style(state.focus == field.focus());
    field.control(screen, state).render(
        frame,
        crate::value_control::ControlStyles {
            label: style,
            value: style,
            button: style,
            disabled: Style::new().fg(Color::DarkGray),
            ..Default::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_ipc::{WorkspaceGitVersion, WorkspaceWorktreeFact};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Absolute on the platform running the test: a rooted path without a
    /// drive letter is relative on Windows, which `validate_new_path` rejects.
    fn projects_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\projects")
        } else {
            PathBuf::from("/projects")
        }
    }

    fn path_text(path: PathBuf) -> String {
        path.to_string_lossy().into_owned()
    }

    fn facts() -> RepoFacts {
        let acme = projects_root().join("acme");
        RepoFacts {
            repo_common_dir: acme.join(".git"),
            checkout_root: acme.clone(),
            project_subpath: PathBuf::new(),
            current_branch: Some("topic".into()),
            default_base_ref: "main".into(),
            default_base_commit: "abcdef123456".into(),
            local_branches: vec!["main".into(), "agent/task".into()],
            worktrees: vec![WorkspaceWorktreeFact {
                path: acme,
                branch: Some("topic".into()),
                created_by_ilium: false,
                is_dirty: true,
                occupied_pane_id: None,
            }],
            source_dirty_count: 2,
            main_dirty_count: 2,
            has_gitmodules: false,
            git_version: WorkspaceGitVersion {
                major: 2,
                minor: 45,
                patch: 0,
            },
        }
    }

    fn dialog(mode: WorktreeDialogMode) -> WorktreeDialogState {
        WorktreeDialogState::new(NodeId(1), NodeId(2), BuiltinAgentProvider::Claude, mode)
    }

    #[test]
    fn loads_facts_and_creates_collision_free_branch_in_main_sibling() {
        let mut state = dialog(WorktreeDialogMode::New);
        assert_eq!(
            state.validated_request().unwrap_err(),
            "Checking repository…"
        );
        state.apply_facts(facts());
        assert_eq!(state.branch.buf, "agent/task-2");
        assert_eq!(state.base_ref.buf, "topic");
        assert_eq!(
            state.path.buf,
            path_text(projects_root().join("acme.worktrees").join("agent-task-2"))
        );
        assert!(state.dirty_reminder().is_some());
        assert!(matches!(
            state.validated_request().unwrap().1,
            WorkspaceCreateSpec::New { .. }
        ));
    }

    #[test]
    fn default_path_preserves_inserted_placeholder_literals_in_repository_name() {
        let mut state = dialog(WorktreeDialogMode::New);
        let mut snapshot = facts();
        snapshot.worktrees[0].path = projects_root().join("{branch_slug}-repo");
        state.apply_facts(snapshot);
        assert_eq!(
            state.path.buf,
            path_text(
                projects_root()
                    .join("{branch_slug}-repo.worktrees")
                    .join("agent-task-2")
            )
        );
    }

    #[test]
    fn configured_setup_is_sent_with_new_worktree_only() {
        let settings = GitSettings {
            setup_command: "printf ready > setup-result".into(),
            ..GitSettings::default()
        };
        let mut state = WorktreeDialogState::new_with_git_settings(
            NodeId(1),
            NodeId(2),
            BuiltinAgentProvider::Claude,
            WorktreeDialogMode::New,
            settings,
        );
        state.apply_facts(facts());
        assert!(matches!(
            state.validated_request().unwrap().1,
            WorkspaceCreateSpec::NewWithSetup { setup_command, .. }
                if setup_command == "printf ready > setup-result"
        ));
    }

    #[test]
    fn oversized_setup_is_rejected_before_submission() {
        let settings = GitSettings {
            setup_command: "x".repeat(8 * 1024 + 1),
            ..GitSettings::default()
        };
        let mut state = WorktreeDialogState::new_with_git_settings(
            NodeId(1),
            NodeId(2),
            BuiltinAgentProvider::Claude,
            WorktreeDialogMode::New,
            settings,
        );
        state.apply_facts(facts());
        assert!(state.validated_request().unwrap_err().contains("8192"));
    }

    #[test]
    fn new_worktree_preview_warns_about_submodules_and_uncopied_changes() {
        let mut repository_facts = facts();
        repository_facts.has_gitmodules = true;
        let mut state = dialog(WorktreeDialogMode::New);
        state.apply_facts(repository_facts);
        let reminders = state.preparation_reminders();
        assert!(reminders.contains("Uncommitted changes"));
        assert!(reminders.contains("submodules"));
    }

    #[test]
    fn git_defaults_set_branch_base_path_and_close_policy_without_overwriting_manual_branch() {
        let settings = GitSettings {
            branch_prefix: "feature/".to_string(),
            worktree_location_template: "{project}/.ilium/worktrees/{branch_slug}".to_string(),
            default_base: GitDefaultBase::DefaultBranch,
            default_close_policy: GitClosePolicy::OfferRemovalWhenSafe,
            ..GitSettings::default()
        };
        let mut state = WorktreeDialogState::new_with_git_settings(
            NodeId(1),
            NodeId(2),
            BuiltinAgentProvider::Claude,
            WorktreeDialogMode::New,
            settings,
        );
        state.apply_facts(facts());
        assert_eq!(state.branch.buf, "feature/task");
        assert_eq!(state.base_ref.buf, "main");
        assert_eq!(
            state.path.buf,
            path_text(
                projects_root()
                    .join("acme")
                    .join(".ilium")
                    .join("worktrees")
                    .join("feature-task")
            )
        );
        assert_eq!(
            state.close_policy,
            WorktreeClosePolicy::OfferRemovalWhenSafe
        );

        state.branch = TextPromptState::new("custom/topic");
        state.branch_is_auto = false;
        state.set_prompt_text("another task");
        assert_eq!(state.branch.buf, "custom/topic");
    }

    #[test]
    fn manual_branch_survives_prompt_changes_and_duplicate_is_rejected() {
        let mut state = dialog(WorktreeDialogMode::New);
        state.apply_facts(facts());
        state.focus = WorktreeDialogFocus::Branch;
        state.branch = TextPromptState::new("agent/my-choice");
        state.edit_focused_text(KeyCode::End);
        state.set_prompt_text("fix login");
        assert_eq!(state.branch.buf, "agent/my-choice");
        state.branch = TextPromptState::new("main");
        assert!(state
            .validated_request()
            .unwrap_err()
            .contains("already exists"));
    }

    #[test]
    fn path_overlap_and_occupied_existing_are_blocked() {
        let mut state = dialog(WorktreeDialogMode::New);
        let mut snapshot = facts();
        snapshot.worktrees.push(WorkspaceWorktreeFact {
            path: projects_root().join("existing"),
            branch: Some("feature".into()),
            created_by_ilium: true,
            is_dirty: false,
            occupied_pane_id: Some(NodeId(9)),
        });
        state.apply_facts(snapshot);
        state.path = TextPromptState::new(path_text(projects_root().join("acme").join("nested")));
        assert!(state.validated_request().unwrap_err().contains("overlaps"));
        state.set_mode(WorktreeDialogMode::Existing);
        state.selected_existing = 1;
        assert!(state
            .validated_request()
            .unwrap_err()
            .contains("live agent"));
        state.selected_existing = 0;
        assert!(matches!(
            state.validated_request().unwrap().1,
            WorkspaceCreateSpec::Existing { .. }
        ));
    }

    #[test]
    fn template_slashes_become_the_platform_separator_without_touching_other_text() {
        assert_eq!(
            replace_forward_slashes(r"C:\repos\app/.ilium/worktrees/x", '\\'),
            r"C:\repos\app\.ilium\worktrees\x"
        );
        assert_eq!(
            replace_forward_slashes("/repos/app/.ilium", '/'),
            "/repos/app/.ilium"
        );
    }

    #[test]
    fn geometry_is_shared_for_draw_and_mouse_on_small_terminal() {
        let mut state = dialog(WorktreeDialogMode::Existing);
        state.apply_facts(facts());
        state.advanced = true;
        let screen = Rect::new(0, 0, 70, 24);
        let layout = dialog_layout(screen, &state);
        assert!(screen.contains(Position::new(layout.popup.x, layout.popup.y)));
        assert_eq!(
            hit_test(
                &state,
                &layout,
                Position::new(layout.where_row.x, layout.where_row.y)
            ),
            Some(WorktreeDialogFocus::Where)
        );
        assert_eq!(
            existing_row_at(
                &state,
                &layout,
                Position::new(layout.existing_list.x, layout.existing_list.y)
            ),
            Some(0)
        );
        let mut terminal = Terminal::new(TestBackend::new(70, 24)).unwrap();
        terminal
            .draw(|frame| draw_dialog(frame, frame.area(), &state))
            .unwrap();
    }
}
