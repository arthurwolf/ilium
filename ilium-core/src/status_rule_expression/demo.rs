//! Named pane situations shared by the Settings preview, the documentation
//! and the tests. Each situation carries complete facts, so the client can
//! show what every rule set does to it, and the tests can prove that the
//! shipped defaults and every catalogued condition are exercised.

use super::{Observation, PaneFacts, ScreenPattern, SupplementaryFacts};
use crate::agent_recovery::{AgentAvailability, AgentExitOutcome, AgentProcessKey, AgentRecovery};
use crate::{
    AgentClass, AgentState, AgentTurn, GoalState, PaneProgress, PaneStatus, ProgressAttention,
    ProgressMonitorHealth, ProgressTaskReport, ProgressTaskStatus, ShellOutputPhase,
};

/// One synthetic pane, labelled as such; never a real pane's data.
#[derive(Clone, Debug)]
pub struct DemoSituation {
    pub name: &'static str,
    pub status: PaneStatus,
    pub progress: Vec<PaneProgress>,
    pub scheduled_input: bool,
    pub shell_output: Option<ShellOutputPhase>,
    pub activity_stale: Option<bool>,
    pub supplementary: SupplementaryFacts,
    /// Synthetic visible screen text for screen-pattern conditions. The
    /// client owns regular expressions, so it performs the matching.
    pub screen: &'static str,
    /// Identifier of the shipped default rule this situation selects, or
    /// `None` when the defaults show nothing for it.
    pub default_rule: Option<&'static str>,
}

impl DemoSituation {
    /// Rule-engine facts for this situation; `screen_matches` is the
    /// client's regex evaluation over [`Self::screen`].
    pub fn facts<'a>(
        &'a self,
        screen_matches: &'a dyn Fn(&ScreenPattern) -> Observation,
    ) -> PaneFacts<'a> {
        PaneFacts {
            status: &self.status,
            progress: &self.progress,
            scheduled_input: self.scheduled_input,
            shell_output: self.shell_output,
            activity_stale: self.activity_stale,
            supplementary: self.supplementary,
            screen_matches,
        }
    }
}

fn agent(
    class: AgentClass,
    turn: AgentTurn,
    goal: Option<GoalState>,
    completion_unread: bool,
) -> PaneStatus {
    PaneStatus::Agent(AgentState {
        class,
        turn,
        goal,
        completion_unread,
    })
}

fn lost(availability: AgentAvailability) -> PaneStatus {
    PaneStatus::AgentUnavailable(Box::new(AgentRecovery {
        last_known_state: AgentState {
            class: AgentClass::Codex,
            turn: AgentTurn::Idle,
            goal: None,
            completion_unread: false,
        },
        process: AgentProcessKey {
            class: AgentClass::Codex,
            process_id: 4242,
            started_at_unix_seconds: 1_700_000_000,
        },
        availability,
        signal_name: None,
        session_id: None,
        last_prompt: None,
        previous_exact_prompt: None,
        latest_prompt_unavailable: false,
    }))
}

struct Monitor {
    status: ProgressTaskStatus,
    percent: f32,
    indeterminate: bool,
    eta_seconds: Option<u64>,
    health: ProgressMonitorHealth,
    attention: ProgressAttention,
}

impl Monitor {
    fn running(percent: f32) -> Self {
        Self {
            status: ProgressTaskStatus::Running,
            percent,
            indeterminate: false,
            eta_seconds: None,
            health: ProgressMonitorHealth::Healthy,
            attention: ProgressAttention::Acknowledged,
        }
    }

    fn build(self, monitor_id: u64) -> PaneProgress {
        PaneProgress {
            monitor_id,
            report: ProgressTaskReport {
                job_id: format!("demo-task-{monitor_id}"),
                status: self.status,
                percent: self.percent,
                message: "Demo task".to_owned(),
                details: String::new(),
                error: None,
                eta_seconds: self.eta_seconds,
                indeterminate: self.indeterminate,
            },
            monitor_health: self.health,
            last_observed_unix_millis: 0,
            timing: Default::default(),
            attention: self.attention,
        }
    }
}

fn failed_health() -> ProgressMonitorHealth {
    ProgressMonitorHealth::Failed {
        consecutive_failures: 3,
        last_error: "probe exited 1".to_owned(),
    }
}

/// The catalogue in display order. The first ten mirror the historical
/// Settings preview cards; the rest exercise the remaining default rules and
/// the conditions the first ten never reach.
pub fn demo_situations() -> Vec<DemoSituation> {
    use AgentClass::{Antigravity, Claude, Codex};
    use AgentTurn::{Idle, Settling, WaitingApproval, WaitingSubagents, Working};
    use ProgressTaskStatus::{Done, Error, NotStartedYet};

    let observed = SupplementaryFacts {
        frozen: Some(false),
        session_cost_cents: Some(40),
        bookmarked: Some(false),
        queued_prompts: Some(0),
        focused: Some(false),
        blocking_error: Some(false),
        session_error: Some(false),
        selected: Some(false),
        visible: Some(false),
        prompt_queue_held: Some(false),
        context_permille: Some(300),
    };
    let situation = |name, status, progress: Vec<PaneProgress>, default_rule| DemoSituation {
        name,
        status,
        progress,
        scheduled_input: false,
        shell_output: None,
        activity_stale: Some(false),
        supplementary: observed,
        screen: "$ claude\n> Working on the request…",
        default_rule,
    };
    let single = |monitor: Monitor| vec![monitor.build(1)];
    vec![
        situation(
            "Working",
            agent(Claude, Working, None, false),
            vec![],
            Some("working"),
        ),
        situation(
            "Needs approval",
            agent(Claude, WaitingApproval, None, false),
            vec![],
            Some("approval"),
        ),
        situation(
            "Goal paused",
            agent(Codex, Idle, Some(GoalState::Paused), false),
            vec![],
            Some("goal-paused"),
        ),
        situation(
            "Waiting for subagents",
            agent(Claude, WaitingSubagents, None, false),
            vec![],
            Some("subagents"),
        ),
        situation(
            "Idle",
            agent(Codex, Idle, None, false),
            vec![],
            Some("idle"),
        ),
        situation(
            "Parked, task 50%",
            agent(Claude, Idle, None, false),
            single(Monitor {
                eta_seconds: Some(120),
                ..Monitor::running(50.0)
            }),
            Some("parked-progress"),
        ),
        situation(
            "Task failed",
            agent(Claude, Working, None, false),
            single(Monitor {
                status: Error,
                health: ProgressMonitorHealth::Healthy,
                attention: ProgressAttention::Unread,
                ..Monitor::running(60.0)
            }),
            Some("task-failed"),
        ),
        situation(
            "Monitor lost",
            agent(Codex, Working, None, false),
            single(Monitor {
                health: failed_health(),
                ..Monitor::running(30.0)
            }),
            Some("monitor-lost"),
        ),
        situation(
            "Task done, unread",
            agent(Claude, Idle, None, false),
            single(Monitor {
                status: Done,
                percent: 100.0,
                attention: ProgressAttention::Unread,
                ..Monitor::running(100.0)
            }),
            Some("task-unread"),
        ),
        situation(
            "Finished, not seen",
            agent(Codex, Idle, None, true),
            vec![],
            Some("completion-unread"),
        ),
        DemoSituation {
            screen: "$ codex\n[process exited with code 1]",
            ..situation(
                "Agent lost",
                lost(AgentAvailability::Exited(AgentExitOutcome::ExitCode(1))),
                vec![],
                Some("agent-lost"),
            )
        },
        situation(
            "Agent exited cleanly",
            lost(AgentAvailability::Exited(AgentExitOutcome::ExitCode(0))),
            vec![],
            Some("agent-lost"),
        ),
        situation(
            "Agent left a shell",
            lost(AgentAvailability::ShellForeground),
            vec![],
            Some("agent-lost"),
        ),
        situation(
            "Agent unverified",
            lost(AgentAvailability::Unverified),
            vec![],
            Some("agent-lost"),
        ),
        situation(
            "Goal blocked",
            agent(Claude, Idle, Some(GoalState::Blocked), false),
            vec![],
            Some("goal-blocked"),
        ),
        situation(
            "Usage limit reached",
            agent(Codex, Idle, Some(GoalState::UsageLimited), false),
            vec![],
            Some("usage-limited"),
        ),
        situation(
            "Goal reached",
            agent(Antigravity, Idle, Some(GoalState::Reached), false),
            vec![],
            Some("goal-reached"),
        ),
        situation(
            "Settling",
            agent(Claude, Settling, None, false),
            vec![],
            Some("settling"),
        ),
        situation(
            "Working toward a goal",
            agent(Codex, Working, Some(GoalState::Active), false),
            vec![],
            Some("working-goal"),
        ),
        situation(
            "Goal active, agent idle",
            agent(Claude, Idle, Some(GoalState::Active), false),
            vec![],
            Some("goal-idle"),
        ),
        situation(
            "Parked, waiting for task",
            agent(Codex, Idle, None, false),
            single(Monitor {
                indeterminate: true,
                percent: 0.0,
                ..Monitor::running(0.0)
            }),
            Some("parked"),
        ),
        DemoSituation {
            scheduled_input: true,
            ..situation(
                "Prompt scheduled",
                agent(Claude, Idle, None, false),
                vec![],
                Some("scheduled"),
            )
        },
        DemoSituation {
            shell_output: Some(ShellOutputPhase::Fast),
            screen: "$ cargo build\n   Compiling ilium-core v0.1.0",
            ..situation(
                "Terminal busy",
                PaneStatus::PlainShell,
                vec![],
                Some("shell-busy"),
            )
        },
        DemoSituation {
            shell_output: Some(ShellOutputPhase::Slow),
            screen: "$ cargo test\ntest tree::moves ... ok",
            ..situation(
                "Terminal trickling",
                PaneStatus::PlainShell,
                vec![],
                Some("shell-busy"),
            )
        },
        DemoSituation {
            screen: "$ ",
            ..situation("Terminal quiet", PaneStatus::PlainShell, vec![], None)
        },
        situation(
            "Terminal task pending",
            PaneStatus::PlainShell,
            single(Monitor {
                status: NotStartedYet,
                percent: 0.0,
                ..Monitor::running(0.0)
            }),
            Some("pending"),
        ),
        DemoSituation {
            activity_stale: Some(true),
            ..situation(
                "Activity check stale",
                agent(Claude, Working, None, false),
                vec![],
                Some("working"),
            )
        },
        DemoSituation {
            supplementary: SupplementaryFacts {
                frozen: Some(true),
                session_cost_cents: Some(1_250),
                bookmarked: Some(true),
                queued_prompts: Some(2),
                focused: Some(true),
                selected: Some(true),
                visible: Some(true),
                prompt_queue_held: Some(true),
                context_permille: Some(920),
                ..observed
            },
            screen: "$ claude\nerror: failed to compile",
            ..situation(
                "Focused, frozen, costly, slow task",
                agent(Claude, Working, None, false),
                single(Monitor {
                    health: ProgressMonitorHealth::Degraded {
                        consecutive_failures: 1,
                        last_error: "timeout".to_owned(),
                    },
                    eta_seconds: Some(3_600),
                    ..Monitor::running(20.0)
                }),
                Some("working"),
            )
        },
        situation(
            "Two tasks, one failed",
            agent(Codex, Working, None, false),
            vec![
                Monitor::running(50.0).build(1),
                Monitor {
                    status: Error,
                    health: failed_health(),
                    attention: ProgressAttention::Unread,
                    ..Monitor::running(10.0)
                }
                .build(2),
            ],
            Some("task-failed"),
        ),
        situation(
            "Parked, monitor unsteady",
            agent(Claude, Idle, None, false),
            single(Monitor {
                health: ProgressMonitorHealth::Degraded {
                    consecutive_failures: 2,
                    last_error: "probe timed out".to_owned(),
                },
                ..Monitor::running(40.0)
            }),
            Some("parked-progress-unsteady"),
        ),
        DemoSituation {
            activity_stale: None,
            ..situation(
                "Custom agent, not yet checked",
                agent(AgentClass::Other("aider".to_owned()), Idle, None, false),
                vec![],
                Some("idle"),
            )
        },
        situation(
            "Task done, acknowledged",
            agent(Claude, Idle, None, false),
            single(Monitor {
                status: Done,
                percent: 100.0,
                ..Monitor::running(100.0)
            }),
            Some("idle"),
        ),
        DemoSituation {
            supplementary: SupplementaryFacts {
                blocking_error: Some(true),
                ..observed
            },
            ..situation(
                "Blocking error on a working agent",
                agent(Claude, Working, None, false),
                vec![],
                Some("blocking-error"),
            )
        },
        // A session-level error belongs to no pane, so the shipped rules
        // (which only test the pane's own error) still show the idle icon.
        DemoSituation {
            supplementary: SupplementaryFacts {
                session_error: Some(true),
                ..observed
            },
            ..situation(
                "Session error, pane unaffected",
                agent(Codex, Idle, None, false),
                vec![],
                Some("idle"),
            )
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::demo_situations;
    use crate::status_rule_expression::{
        condition_catalog, default_rules, first_match, Condition, Observation, PaneKind,
    };

    fn no_screen(_: &crate::status_rule_expression::ScreenPattern) -> Observation {
        Observation::Unknown
    }

    #[test]
    fn situations_have_unique_names_and_the_historical_ten_come_first() {
        let situations = demo_situations();
        assert!(situations.len() >= 18, "{}", situations.len());
        let names: HashSet<&str> = situations.iter().map(|situation| situation.name).collect();
        assert_eq!(names.len(), situations.len());
        assert_eq!(situations[0].name, "Working");
        assert_eq!(situations[1].name, "Needs approval");
        assert_eq!(situations[5].name, "Parked, task 50%");
        assert_eq!(situations[9].name, "Finished, not seen");
    }

    #[test]
    fn golden_table_each_situation_selects_its_recorded_default_rule() {
        let rules = default_rules();
        for situation in demo_situations() {
            let facts = situation.facts(&no_screen);
            let selected = first_match(&rules.rules, |condition| condition.observe(&facts))
                .map(|rule| rule.id.as_str());
            assert_eq!(
                selected, situation.default_rule,
                "situation {:?}",
                situation.name
            );
        }
    }

    #[test]
    fn every_enabled_default_rule_fires_for_some_situation() {
        let rules = default_rules();
        let fired: HashSet<&str> = demo_situations()
            .iter()
            .filter_map(|situation| situation.default_rule)
            .collect();
        for rule in rules.rules.iter().filter(|rule| rule.enabled) {
            assert!(
                fired.contains(rule.id.as_str()),
                "no demo situation reaches default rule {:?}",
                rule.id
            );
        }
    }

    #[test]
    fn every_catalogue_condition_is_observed_true_and_false_somewhere() {
        // Screen patterns need the client's regex engine; `always` is never
        // false; editors never get custom status icons, so their
        // kinds are only ever false here and the dirty flag is never observed.
        let skipped = [Condition::EditorDirty];
        let only_true = [Condition::Always];
        let never_true = [Condition::PaneKind(PaneKind::Editor)];
        let situations = demo_situations();
        for condition in condition_catalog() {
            if matches!(condition, Condition::ScreenMatches(_)) || skipped.contains(&condition) {
                continue;
            }
            let mut seen: HashMap<Observation, usize> = HashMap::new();
            for situation in &situations {
                *seen
                    .entry(condition.observe(&situation.facts(&no_screen)))
                    .or_default() += 1;
            }
            if !never_true.contains(&condition) {
                assert!(
                    seen.contains_key(&Observation::True),
                    "{condition:?} is never true: {seen:?}"
                );
            }
            if !only_true.contains(&condition) {
                assert!(
                    seen.contains_key(&Observation::False),
                    "{condition:?} is never false: {seen:?}"
                );
            }
        }
    }
}
