//! Requirement tests for the round-two production gaps (one test per PRX requirement).
//!
//! Each test is red until its requirement is implemented: it panics with the requirement ID.

/// REQ: PRX-001
#[test]
fn prx_001_editor_unsaved_buffers_when_the_editor_plugin_is_disabled_cr() {
    // Observable: A caller without a file or network grant that passes a path or URL to the editor's service provider receives a refusal instead of the content, and `ilium plugin check` prints a warning for a provider method whose caller input selects a granted fs or net resource.
    panic!(
        "PRX-001 not implemented: A service provider MUST NOT widen the caller's authority: the host MUST NOT pass a caller-supplied path, URL or handle to a provider as the caller's authority, a provider's reply that carries file or network content MUST be labelled with the grant it was read under so the host can refuse delivery to a caller that lacks that grant, and `ilium plugin check` MUST warn when a provider's service method lets caller input select a granted fs or net resource."
    );
}

/// REQ: PRX-002
#[test]
fn prx_002_confused_deputy_through_plugin_services() {
    // Observable: When a caller without fs.read calls a provider service that reads a file under the provider's own fs grant, the reply is refused with a not-permitted result and the file content is not delivered, and `ilium plugin check` on a provider whose method opens a path taken from caller input prints a warning naming that method.
    panic!(
        "PRX-002 not implemented: A service provider MUST NOT widen the caller's authority: the host MUST NOT pass a caller-supplied path, URL or handle to a provider as the caller's authority, a provider's reply that carries file or network content MUST be labelled with the grant it was read under so the host can refuse delivery to a caller that lacks that grant, and `ilium plugin check` MUST warn when a provider's service method lets caller input select a granted fs or net resource."
    );
}

/// REQ: PRX-003
#[test]
fn prx_003_queued_items_during_core_plugin_disable_replace_and_rollback() {
    // Observable: After swapping a core plugin with N unrun queued items and one hook chain in progress, each of the N items runs exactly once and only on the new owner, and the in-progress chain ends with error core.owner-changed and is not started again.
    panic!(
        "PRX-003 not implemented: On disable, replacement or rollback of a core plugin, the host MUST hand every queued item that the old owner had not yet run (queued prompts, pending notices, scheduled sends, running monitors) to the new owner exactly once, MUST NOT run any item on both the old and the new owner, and MUST fail a hook chain that was in progress at the swap with `core.owner-changed` instead of restarting it on the new owner."
    );
}

/// REQ: PRX-004
#[test]
fn prx_004_unknown_or_unavailable_pane_kinds_in_persisted_sessions() {
    // Observable: After restoring a snapshot with a pane of a kind that has no active owner, the pane appears as a placeholder at the same tree position, the next snapshot contains that pane's stored state byte-identical to the original, and activating an owner for the kind replaces the placeholder with the real pane holding that state.
    panic!(
        "PRX-004 not implemented: When a snapshot contains a pane whose kind has no active owner, the host MUST restore it as a placeholder pane that keeps its tree position and stored state byte for byte, MUST write that state back unchanged in later snapshots, and MUST restore the real pane when an owner for the kind becomes active."
    );
}

/// REQ: PRX-005
#[test]
fn prx_005_registry_side_advisory_match_of_the_sbom_at_publish() {
    // Observable: Publishing a package whose SBOM lists a component with an unreviewed Critical advisory is refused with registry.known-vulnerable naming the component, advisory id and fixed version, even from a client with scanning disabled, and the same publish with a signed VEX not_affected statement succeeds.
    panic!(
        "PRX-005 not implemented: A registry that requires an SBOM MUST, on its own side and regardless of client flags, match every SBOM component against its advisory database and MUST refuse the publish with `registry.known-vulnerable` when a component has an unreviewed Critical advisory, unless the publisher attaches a signed VEX statement marking the advisory `not_affected`; the refusal MUST name the component, the advisory id and the fixed version."
    );
}

/// REQ: PRX-006
#[test]
fn prx_006_registry_data_durability_recovery_objective_and_restore_dril() {
    // Observable: After restoring from the second-failure-domain copy, the transparency log is byte-identical to the original, every earlier checkpoint and consistency proof verifies, and the copy is no more than 1 hour older than the loss point.
    panic!(
        "PRX-006 not implemented: A registry operator MUST keep a copy of the signed transparency log, all metadata documents and all blobs in a second failure domain with a recovery point of at most 1 hour, MUST run a restore drill at least once per quarter, and a restored log MUST be byte-identical to the lost one so that every previously issued checkpoint and consistency proof still verifies."
    );
}

/// REQ: PRX-007
#[test]
fn prx_007_layout_independent_default_key_chords() {
    // Observable: Running `ilium plugin lint` on a plugin whose default chord is shifted punctuation, `ctrl+plus` or Ctrl+Alt prints a warning for that chord, while a plugin with plain unshifted chords gets none, and every command of the plugin appears and runs from the command palette.
    panic!(
        "PRX-007 not implemented: A plugin's default chord for a command MUST be reachable without AltGr or dead keys on the US, German and French layouts, every command MUST also be reachable through the command palette so that a default chord is never the only route, and `ilium plugin lint` MUST warn on a default chord made from shifted punctuation, `ctrl+plus` or Ctrl+Alt."
    );
}

/// REQ: PRX-008
#[test]
fn prx_008_plugin_accessibility_declaration_in_manifest_and_registry_li() {
    // Observable: A plugin whose manifest declares a conformance level its fixture run does not meet makes `ilium plugin check` exit with failure, and `ilium plugin info` and the registry listing show the declared table for a plugin with one and the exact text `no accessibility declaration` for a plugin without one.
    panic!(
        "PRX-008 not implemented: A manifest SHOULD carry an `accessibility` table naming the conformance level for screen-reader mode, high contrast, reduced motion, 40-column compact use and the supported languages; `ilium plugin check` MUST fail a plugin whose declaration claims a level that its fixture run on the test host does not meet; and the registry listing and `ilium plugin info` MUST show the declaration or the text `no accessibility declaration`."
    );
}

/// REQ: PRX-009
#[test]
fn prx_009_rust_component_size_budget_and_size_reduction_build_step() {
    // Observable: A project generated from the Rust template contains a release profile with opt-level "s", lto true, codegen-units 1 and strip true, and building a component whose optimised size exceeds the documented threshold prints the warning build.component-large containing both the measured size and the threshold, while a smaller component prints no such warning.
    panic!(
        "PRX-009 not implemented: The Rust template MUST ship a release profile (`opt-level = 's'`, `lto = true`, `codegen-units = 1`, `strip = true`) and a documented post-link optimisation step, and `ilium plugin build` MUST warn `build.component-large` when the optimised component exceeds the documented size threshold, naming the measured size and the threshold."
    );
}

/// REQ: PRX-010
#[test]
fn prx_010_signature_algorithm_retirement_and_migration_process() {
    // Observable: After an algorithm is marked retired, a publish signed only with it is refused by the registry, while an already-installed package signed with it still verifies and shows a warning naming the algorithm; during the announced 12-month period packages signed with either algorithm verify.
    panic!(
        "PRX-010 not implemented: Adding or retiring a signature algorithm MUST be announced at least 12 months before enforcement; during that period the host MUST verify both algorithms; after retirement the registry MUST refuse new publishes signed only with the retired algorithm while the host still verifies already-installed packages signed with it and shows a warning naming the algorithm."
    );
}

/// REQ: PRX-011
#[test]
fn prx_011_security_fix_support_window_for_sdk_release_lines() {
    // Observable: For a plugin pinned to an SDK line whose documented support window has ended, `ilium plugin check` reports `lint.sdk-unsupported`, and for a line still inside its window it does not; a security fix release's notes and advisory entry list the affected SDK versions and the fixed version.
    panic!(
        "PRX-011 not implemented: Every SDK release line (`ilium-plugin-sdk` and `@ilium/plugin-sdk`) MUST receive security fixes for the documented support window after its successor ships; a fix release MUST name the affected SDK versions and the fixed version in its release notes and in the advisory feed, so that `ilium plugin check` flags a plugin pinned to an SDK line outside its window with `lint.sdk-unsupported`."
    );
}

/// REQ: PRX-012
#[test]
fn prx_012_locked_accessibility_settings_must_outrank_cli_flags_and_env() {
    // Observable: With a policy locking `[ui] contrast = "high"`, starting with `--contrast normal` yields effective contrast `high`, Settings shows `high` with its policy source, and the attach log contains the line `settings.locked`.
    panic!(
        "PRX-012 not implemented: A key in `settings.locked` MUST take precedence over the matching CLI flag and environment variable (`--screen-reader`, `--contrast`, `--reduced-motion`, `ILIUM_SCREEN_READER`, and their siblings), the locked value MUST be shown with its source in Settings, and a test with a policy that locks `[ui] contrast = 'high'` and a run with `--contrast normal` MUST end with the effective value `high` and the line `settings.locked` in the attach log."
    );
}

/// REQ: PRX-013
#[test]
fn prx_013_catch_up_notices_after_an_agent_detect_or_notifications_rest() {
    // Observable: After killing and restarting either plugin while N panes change to done or awaiting-approval, exactly one notice per such pane appears (none duplicating an already delivered pane and state), and the notice log contains a `notify.catch-up` entry whose count equals the number of notices raised.
    panic!(
        "PRX-013 not implemented: After `ilium.core.agent-detect` or `ilium.core.notifications` restarts following a crash, the host MUST raise exactly one catch-up notice for each pane whose activity changed to done or awaiting-approval during the outage, deduplicated against notices already delivered for the same pane and state, and MUST write `notify.catch-up` with the count to the notice log."
    );
}

/// REQ: PRX-014
#[test]
fn prx_014_measured_audit_volume_for_collector_and_disk_sizing() {
    // Observable: After running the reference workload for one hour, `ilium plugin metrics` lists both `ilium_audit_bytes_total` and `ilium_audit_records_total`, and the bytes per plugin-hour computed from them is at most the upper bound stated in the manual.
    panic!(
        "PRX-014 not implemented: The manual MUST state a measured upper bound for audit bytes per plugin-hour on the reference workload, and the host MUST expose `ilium_audit_bytes_total` and `ilium_audit_records_total` in `ilium plugin metrics` so an operator can verify the collector and disk budget against that bound."
    );
}

/// REQ: PRX-015
#[test]
fn prx_015_language_aware_number_formatting_for_user_visible_values() {
    // Observable: With the test host set to `.locale("de")`, a `number` text node holding 1234.5 renders the text `1.234,5`, and `ilium plugin lint` on a plugin that builds a number string by hand for a user-visible message reports a finding for it.
    panic!(
        "PRX-015 not implemented: The host MUST provide a `number` text node (or `ui.format-number`) that renders a value with the decimal mark, grouping and percent form of the effective UI language, a plugin MUST NOT hand-format a number shown to the user, the test host MUST render `1234.5` as `1.234,5` when `.locale('de')` is set, and `ilium plugin lint` MUST report a hand-built number string in a user-visible message."
    );
}
