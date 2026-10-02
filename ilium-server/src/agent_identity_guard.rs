//! Fresh process evidence for agent-directed delivery. Cached detection alone
//! cannot authorize a write after an exit, PID reuse or in-place exec.

use ilium_detect::AgentIdentity;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

/// Refresh only this process, including its current arguments. A fresh System
/// avoids retaining stale names across exec on platforms that cache them.
/// This is a preflight observation, not an atomic promise that the process
/// cannot exit between inspection and the subsequent PTY admission.
pub(crate) fn matches_current_agent_identity(identity: &AgentIdentity) -> bool {
    if identity.pid == 0 || identity.started_at_unix_seconds == 0 {
        return false;
    }
    let process_id = Pid::from_u32(identity.pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[process_id]),
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cmd(UpdateKind::Always),
    );
    let Some(process) = system.process(process_id) else {
        return false;
    };
    observed_identity_matches(
        identity,
        process.pid().as_u32(),
        process.start_time(),
        process.exists()
            && !matches!(
                process.status(),
                ProcessStatus::Zombie | ProcessStatus::Dead
            ),
        &ilium_detect::identifying_process_names(process),
    )
}

fn observed_identity_matches(
    identity: &AgentIdentity,
    process_id: u32,
    started_at_unix_seconds: u64,
    exists: bool,
    names: &[String],
) -> bool {
    exists
        && process_id == identity.pid
        && identity.started_at_unix_seconds != 0
        && started_at_unix_seconds == identity.started_at_unix_seconds
        && !identity.process_name.is_empty()
        && names.iter().any(|name| name == &identity.process_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> AgentIdentity {
        AgentIdentity {
            class: ilium_core::AgentClass::Codex,
            pid: 123,
            started_at_unix_seconds: 456,
            process_name: "codex".into(),
            matched_signature: "codex".into(),
            process_tree_depth: 1,
        }
    }

    #[test]
    fn cached_identity_cannot_authorize_exit_reuse_or_exec() {
        let identity = identity();
        let names = vec!["codex".into()];
        assert!(observed_identity_matches(&identity, 123, 456, true, &names));
        assert!(!observed_identity_matches(
            &identity, 123, 456, false, &names
        ));
        assert!(!observed_identity_matches(
            &identity, 124, 456, true, &names
        ));
        assert!(!observed_identity_matches(
            &identity, 123, 457, true, &names
        ));
        assert!(!observed_identity_matches(
            &identity,
            123,
            456,
            true,
            &["sh".into()],
        ));
        let mut unknown = identity;
        unknown.started_at_unix_seconds = 0;
        assert!(!observed_identity_matches(&unknown, 123, 0, true, &names));
        assert!(!matches_current_agent_identity(&unknown));
    }

    #[test]
    fn a_fresh_real_process_matches_only_its_observed_birth_and_name() {
        let process_id = sysinfo::get_current_pid().unwrap();
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[process_id]),
            true,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_cmd(UpdateKind::Always),
        );
        let process = system.process(process_id).unwrap();
        let mut identity = identity();
        identity.pid = process_id.as_u32();
        identity.started_at_unix_seconds = process.start_time();
        identity.process_name = ilium_detect::identifying_process_names(process)[0].clone();
        assert!(matches_current_agent_identity(&identity));
        identity.started_at_unix_seconds += 1;
        assert!(!matches_current_agent_identity(&identity));
        identity.started_at_unix_seconds -= 1;
        identity.process_name = "a-different-executable".into();
        assert!(!matches_current_agent_identity(&identity));
    }
}
