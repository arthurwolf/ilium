// Core-parity requirements from master (COR IDs), red until implemented.

/// REQ: COR-005
#[test]
fn cor_005_session_backups_copy_before_restore() {
    // Observable: with backups_enabled the server copies the snapshot before any restore, then at most every 30 minutes.
    panic!("COR-005 not implemented: session backups copy the snapshot before restore and then at most every 30 minutes");
}

/// REQ: COR-009
#[test]
fn cor_009_task_alerts_coalesce_per_kind() {
    // Observable: same-kind alerts on one pane arriving within task_coalesce_seconds merge into the first alert.
    panic!("COR-009 not implemented: notifications coalesce same-kind task alerts within task_coalesce_seconds");
}

/// REQ: COR-019
#[test]
fn cor_019_editor_refuses_oversized_documents() {
    // Observable: a file over 32 MiB or 262144 lines is refused with a visible reason; large documents open in Source view.
    panic!("COR-019 not implemented: editor refuses oversized files and opens large documents in Source view");
}

/// REQ: COR-025
#[test]
fn cor_025_compaction_threshold_and_cooldown() {
    // Observable: with Automatic on, the pane compacts at threshold_percent, below the agent's own trigger, and not before cooldown.
    panic!("COR-025 not implemented: automatic compaction triggers at threshold_percent and respects the cooldown");
}

/// REQ: COR-026
#[test]
fn cor_026_empty_groups_auto_removed() {
    // Observable: with auto_remove_empty_groups on, closing the last item removes the group and ancestors that become empty, never a project.
    panic!("COR-026 not implemented: empty groups and folders are removed up the ancestor chain, never projects");
}
