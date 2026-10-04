//! Pure cooperative allocation accounting for owned domain/protocol data.
//! Capacities include unused storage. This declares owned allocations, not RSS
//! or an allocator-enforced bound. HashMap's private bucket layout is charged
//! conservatively by twice the public capacity rounded upward, plus controls.
use crate::animation_recommendation::*;
use crate::*;
use std::mem::{size_of, size_of_val};

pub trait AllocationSize {
    /// Heap allocations owned by this value; its inline size is counted by its container.
    fn heap_bytes(&self) -> usize;
    fn retained_bytes(&self) -> usize {
        size_of_val(self).saturating_add(self.heap_bytes())
    }
}
macro_rules! scalars { ($($kind:ty),*) => { $(impl AllocationSize for $kind { fn heap_bytes(&self)->usize { 0 } })* }; }
scalars!(
    bool,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64,
    ()
);
impl AllocationSize for String {
    fn heap_bytes(&self) -> usize {
        self.capacity()
    }
}
impl AllocationSize for PathBuf {
    fn heap_bytes(&self) -> usize {
        self.capacity()
    }
}
impl<T: AllocationSize> AllocationSize for Option<T> {
    fn heap_bytes(&self) -> usize {
        self.as_ref().map_or(0, AllocationSize::heap_bytes)
    }
}
impl<T: AllocationSize, E: AllocationSize> AllocationSize for Result<T, E> {
    fn heap_bytes(&self) -> usize {
        match self {
            Ok(v) => v.heap_bytes(),
            Err(e) => e.heap_bytes(),
        }
    }
}
impl<T: AllocationSize> AllocationSize for Box<T> {
    fn heap_bytes(&self) -> usize {
        size_of::<T>().saturating_add(self.as_ref().heap_bytes())
    }
}
impl<T: AllocationSize> AllocationSize for Vec<T> {
    fn heap_bytes(&self) -> usize {
        self.iter().fold(
            self.capacity().saturating_mul(size_of::<T>()),
            |sum, value| sum.saturating_add(value.heap_bytes()),
        )
    }
}
impl<A: AllocationSize, B: AllocationSize> AllocationSize for (A, B) {
    fn heap_bytes(&self) -> usize {
        self.0.heap_bytes().saturating_add(self.1.heap_bytes())
    }
}
impl<K: AllocationSize, V: AllocationSize> AllocationSize for HashMap<K, V> {
    fn heap_bytes(&self) -> usize {
        if self.capacity() == 0 {
            return 0;
        }
        let buckets = self
            .capacity()
            .saturating_mul(2)
            .checked_next_power_of_two()
            .unwrap_or(usize::MAX);
        self.iter().fold(
            buckets
                .saturating_mul(size_of::<(K, V)>().saturating_add(1))
                .saturating_add(64),
            |sum, (key, value)| {
                sum.saturating_add(key.heap_bytes())
                    .saturating_add(value.heap_bytes())
            },
        )
    }
}
impl AllocationSize for NodeId {
    fn heap_bytes(&self) -> usize {
        0
    }
}
impl AllocationSize for Node {
    fn heap_bytes(&self) -> usize {
        let Node {
            id,
            parent,
            name,
            short_name,
            inferred_icon,
            is_name_fixed,
            presentation_revision,
            is_bookmarked,
            activity_revision,
            last_restructure_activity_revision,
            last_focus_activity_revision,
            structure_source,
            kind,
            inferred_animation,
            animation_generation,
        } = self;
        0usize
            .saturating_add(id.heap_bytes())
            .saturating_add(parent.heap_bytes())
            .saturating_add(name.heap_bytes())
            .saturating_add(short_name.heap_bytes())
            .saturating_add(inferred_icon.heap_bytes())
            .saturating_add(is_name_fixed.heap_bytes())
            .saturating_add(presentation_revision.heap_bytes())
            .saturating_add(is_bookmarked.heap_bytes())
            .saturating_add(activity_revision.heap_bytes())
            .saturating_add(last_restructure_activity_revision.heap_bytes())
            .saturating_add(last_focus_activity_revision.heap_bytes())
            .saturating_add(structure_source.heap_bytes())
            .saturating_add(kind.heap_bytes())
            .saturating_add(inferred_animation.heap_bytes())
            .saturating_add(animation_generation.heap_bytes())
    }
}

impl AllocationSize for Tree {
    fn heap_bytes(&self) -> usize {
        let Tree { nodes, next_id } = self;
        nodes.heap_bytes().saturating_add(next_id.heap_bytes())
    }
}

impl AllocationSize for ContainerNode {
    fn heap_bytes(&self) -> usize {
        let ContainerNode {
            kind,
            children,
            expanded,
            locked_closed,
        } = self;
        0usize
            .saturating_add(kind.heap_bytes())
            .saturating_add(children.heap_bytes())
            .saturating_add(expanded.heap_bytes())
            .saturating_add(locked_closed.heap_bytes())
    }
}

impl AllocationSize for AgentState {
    fn heap_bytes(&self) -> usize {
        let AgentState {
            class,
            turn,
            goal,
            completion_unread,
        } = self;
        0usize
            .saturating_add(class.heap_bytes())
            .saturating_add(turn.heap_bytes())
            .saturating_add(goal.heap_bytes())
            .saturating_add(completion_unread.heap_bytes())
    }
}

impl AllocationSize for AgentProcessKey {
    fn heap_bytes(&self) -> usize {
        let AgentProcessKey {
            class,
            process_id,
            started_at_unix_seconds,
        } = self;
        0usize
            .saturating_add(class.heap_bytes())
            .saturating_add(process_id.heap_bytes())
            .saturating_add(started_at_unix_seconds.heap_bytes())
    }
}

impl AllocationSize for AgentRecovery {
    fn heap_bytes(&self) -> usize {
        let AgentRecovery {
            last_known_state,
            process,
            availability,
            signal_name,
            session_id,
            last_prompt,
            previous_exact_prompt,
            latest_prompt_unavailable,
        } = self;
        0usize
            .saturating_add(last_known_state.heap_bytes())
            .saturating_add(process.heap_bytes())
            .saturating_add(availability.heap_bytes())
            .saturating_add(signal_name.heap_bytes())
            .saturating_add(session_id.heap_bytes())
            .saturating_add(last_prompt.heap_bytes())
            .saturating_add(previous_exact_prompt.heap_bytes())
            .saturating_add(latest_prompt_unavailable.heap_bytes())
    }
}

impl AllocationSize for ScheduledPaneInput {
    fn heap_bytes(&self) -> usize {
        let ScheduledPaneInput {
            execute_at_unix_millis,
            text,
            send_enter,
        } = self;
        0usize
            .saturating_add(execute_at_unix_millis.heap_bytes())
            .saturating_add(text.heap_bytes())
            .saturating_add(send_enter.heap_bytes())
    }
}

impl AllocationSize for QueuedPrompt {
    fn heap_bytes(&self) -> usize {
        let QueuedPrompt {
            text,
            delivery,
            attempted_delivery,
        } = self;
        0usize
            .saturating_add(text.heap_bytes())
            .saturating_add(delivery.heap_bytes())
            .saturating_add(attempted_delivery.heap_bytes())
    }
}

impl AllocationSize for PaneWorkspace {
    fn heap_bytes(&self) -> usize {
        let PaneWorkspace {
            workspace_id,
            repo_common_dir,
            worktree_root,
            branch,
            base_ref,
            base_commit,
            created_by_ilium,
            created_at_unix,
        } = self;
        0usize
            .saturating_add(workspace_id.heap_bytes())
            .saturating_add(repo_common_dir.heap_bytes())
            .saturating_add(worktree_root.heap_bytes())
            .saturating_add(branch.heap_bytes())
            .saturating_add(base_ref.heap_bytes())
            .saturating_add(base_commit.heap_bytes())
            .saturating_add(created_by_ilium.heap_bytes())
            .saturating_add(created_at_unix.heap_bytes())
    }
}

impl AllocationSize for PaneProgress {
    fn heap_bytes(&self) -> usize {
        let PaneProgress {
            monitor_id,
            report,
            monitor_health,
            last_observed_unix_millis,
            attention,
        } = self;
        0usize
            .saturating_add(monitor_id.heap_bytes())
            .saturating_add(report.heap_bytes())
            .saturating_add(monitor_health.heap_bytes())
            .saturating_add(last_observed_unix_millis.heap_bytes())
            .saturating_add(attention.heap_bytes())
    }
}

impl AllocationSize for ProgressTaskReport {
    fn heap_bytes(&self) -> usize {
        let ProgressTaskReport {
            job_id,
            status,
            percent,
            message,
            details,
            error,
        } = self;
        0usize
            .saturating_add(job_id.heap_bytes())
            .saturating_add(status.heap_bytes())
            .saturating_add(percent.heap_bytes())
            .saturating_add(message.heap_bytes())
            .saturating_add(details.heap_bytes())
            .saturating_add(error.heap_bytes())
    }
}

impl AllocationSize for AnimationRecommendation {
    fn heap_bytes(&self) -> usize {
        let AnimationRecommendation {
            version,
            kind,
            resources,
            parameters,
        } = self;
        0usize
            .saturating_add(version.heap_bytes())
            .saturating_add(kind.heap_bytes())
            .saturating_add(resources.heap_bytes())
            .saturating_add(parameters.heap_bytes())
    }
}

impl AllocationSize for AnimationParameter {
    fn heap_bytes(&self) -> usize {
        let AnimationParameter { id, value } = self;
        0usize
            .saturating_add(id.heap_bytes())
            .saturating_add(value.heap_bytes())
    }
}
impl AllocationSize for AgentClass {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Claude => 0usize,
            Self::Codex => 0usize,
            Self::Antigravity => 0usize,
            Self::Other(value0) => 0usize.saturating_add(value0.heap_bytes()),
        }
    }
}

impl AllocationSize for PaneStatus {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::PlainShell => 0usize,
            Self::Agent(value0) => 0usize.saturating_add(value0.heap_bytes()),
            Self::Editor { dirty } => 0usize.saturating_add(dirty.heap_bytes()),
            Self::Board => 0usize,
            Self::AgentUnavailable(value0) => 0usize.saturating_add(value0.heap_bytes()),
        }
    }
}

impl AllocationSize for PaneContentKind {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Terminal => 0usize,
            Self::Editor => 0usize,
            Self::Board => 0usize,
        }
    }
}

impl AllocationSize for PaneTitleSource {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Automatic => 0usize,
            Self::UserSpecified => 0usize,
        }
    }
}

impl AllocationSize for StructureSource {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Manual => 0usize,
            Self::LlmRestructure => 0usize,
        }
    }
}

impl AllocationSize for ContainerKind {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Project { path } => 0usize.saturating_add(path.heap_bytes()),
            Self::Group => 0usize,
            Self::SplitView { orientation } => 0usize.saturating_add(orientation.heap_bytes()),
        }
    }
}

impl AllocationSize for SplitOrientation {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Vertical => 0usize,
            Self::Horizontal => 0usize,
        }
    }
}

impl AllocationSize for AgentTurn {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Working => 0usize,
            Self::WaitingApproval => 0usize,
            Self::WaitingSubagents => 0usize,
            Self::Settling => 0usize,
            Self::Idle => 0usize,
        }
    }
}

impl AllocationSize for GoalState {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Active => 0usize,
            Self::Paused => 0usize,
            Self::Blocked => 0usize,
            Self::UsageLimited => 0usize,
            Self::Reached => 0usize,
        }
    }
}

impl AllocationSize for AgentAvailability {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Unverified => 0usize,
            Self::ShellForeground => 0usize,
            Self::Exited(value0) => 0usize.saturating_add(value0.heap_bytes()),
        }
    }
}

impl AllocationSize for AgentExitOutcome {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::ExitCode(value0) => 0usize.saturating_add(value0.heap_bytes()),
            Self::Signal => 0usize,
            Self::Unknown => 0usize,
        }
    }
}

impl AllocationSize for PromptQueueDelivery {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Once => 0usize,
            Self::Times { remaining_runs } => 0usize.saturating_add(remaining_runs.heap_bytes()),
            Self::Forever => 0usize,
        }
    }
}

impl AllocationSize for ProgressTaskStatus {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::NotStartedYet => 0usize,
            Self::Running => 0usize,
            Self::Error => 0usize,
            Self::Done => 0usize,
        }
    }
}

impl AllocationSize for ProgressMonitorHealth {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Healthy => 0usize,
            Self::Degraded {
                consecutive_failures,
                last_error,
            } => 0usize
                .saturating_add(consecutive_failures.heap_bytes())
                .saturating_add(last_error.heap_bytes()),
            Self::Failed {
                consecutive_failures,
                last_error,
            } => 0usize
                .saturating_add(consecutive_failures.heap_bytes())
                .saturating_add(last_error.heap_bytes()),
        }
    }
}

impl AllocationSize for ProgressAttention {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Unread => 0usize,
            Self::Acknowledged => 0usize,
        }
    }
}

impl AllocationSize for ResourcePolicy {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Catalog => 0usize,
            Self::Authored => 0usize,
        }
    }
}

impl AllocationSize for AnimationValue {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Number(value0) => 0usize.saturating_add(value0.heap_bytes()),
            Self::Choice { index, label } => 0usize
                .saturating_add(index.heap_bytes())
                .saturating_add(label.heap_bytes()),
            Self::Bool(value0) => 0usize.saturating_add(value0.heap_bytes()),
        }
    }
}
impl AllocationSize for NodeActivityRevision {
    fn heap_bytes(&self) -> usize {
        let NodeActivityRevision {
            node_id,
            activity_revision,
        } = self;
        0usize
            .saturating_add(node_id.heap_bytes())
            .saturating_add(activity_revision.heap_bytes())
    }
}
impl AllocationSize for AgentActivity {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Working => 0usize,
            Self::WaitingBackground => 0usize,
            Self::BackgroundTaskStillRunning => 0usize,
            Self::WaitingApproval => 0usize,
            Self::Done => 0usize,
            Self::Idle => 0usize,
        }
    }
}

impl AllocationSize for NodeKind {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Container(value0) => 0usize.saturating_add(value0.heap_bytes()),
            Self::Pane {
                content,
                status,
                title_source,
                board_storage,
                scheduled_input,
                prompt_queue,
                last_prompt,
                progress,
                launch_cwd,
                workspace,
            } => 0usize
                .saturating_add(content.heap_bytes())
                .saturating_add(status.heap_bytes())
                .saturating_add(title_source.heap_bytes())
                .saturating_add(board_storage.heap_bytes())
                .saturating_add(scheduled_input.heap_bytes())
                .saturating_add(prompt_queue.heap_bytes())
                .saturating_add(last_prompt.heap_bytes())
                .saturating_add(progress.heap_bytes())
                .saturating_add(launch_cwd.heap_bytes())
                .saturating_add(workspace.heap_bytes()),
            Self::Folder {
                path,
                expanded,
                locked_closed,
            } => 0usize
                .saturating_add(path.heap_bytes())
                .saturating_add(expanded.heap_bytes())
                .saturating_add(locked_closed.heap_bytes()),
        }
    }
}
impl AllocationSize for BoardStorage {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Folder { path } => 0usize.saturating_add(path.heap_bytes()),
            Self::MarkdownFile { path } => 0usize.saturating_add(path.heap_bytes()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tree_storage_counts_unused_map_and_string_capacity() {
        let mut tree = Tree::new();
        let baseline = tree.retained_bytes();
        tree.nodes.reserve(512);
        let map_reserved = tree.retained_bytes();
        assert!(map_reserved > baseline + 512 * size_of::<Node>());
        tree.nodes
            .get_mut(&ROOT_ID)
            .expect("root")
            .name
            .reserve(65536);
        assert!(tree.retained_bytes() >= map_reserved + 65536);
    }
    #[test]
    fn nested_prompt_and_animation_capacity_is_counted_without_serialization() {
        let text = String::with_capacity(8192);
        let prompt = QueuedPrompt {
            text,
            delivery: PromptQueueDelivery::Once,
            attempted_delivery: false,
        };
        assert_eq!(prompt.heap_bytes(), 8192);
        let choice = AnimationValue::Choice {
            index: 0,
            label: String::with_capacity(4096),
        };
        assert_eq!(choice.heap_bytes(), 4096);
    }
}
