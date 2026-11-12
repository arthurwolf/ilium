//! Requirement tests for the production-completeness pages (one module per page).
//!
//! Every test is red until its requirement is implemented: it panics with the requirement ID.

#[path = "production_completeness/guide-pinning-the-sdk.rs"]
mod guide_pinning_the_sdk;
#[path = "production_completeness/reference-grant-expiry.rs"]
mod reference_grant_expiry;
#[path = "production_completeness/api-grant-refusal-and-changes.rs"]
mod api_grant_refusal_and_changes;
#[path = "production_completeness/reference-audit-grant-events.rs"]
mod reference_audit_grant_events;
#[path = "production_completeness/reference-grant-cli-unattended.rs"]
mod reference_grant_cli_unattended;
#[path = "production_completeness/reference-helper-orphan-and-server-death.rs"]
mod reference_helper_orphan_and_server_death;
#[path = "production_completeness/ops-out-of-band-kill-and-stuck-server.rs"]
mod ops_out_of_band_kill_and_stuck_server;
#[path = "production_completeness/reference-resource-limits-matrix.rs"]
mod reference_resource_limits_matrix;
#[path = "production_completeness/publish-registry-shutdown-and-migration.rs"]
mod publish_registry_shutdown_and_migration;
#[path = "production_completeness/user-reporting-a-plugin.rs"]
mod user_reporting_a_plugin;
#[path = "production_completeness/guide-air-gapped-transfer.rs"]
mod guide_air_gapped_transfer;
#[path = "production_completeness/reference-config-export-import.rs"]
mod reference_config_export_import;
#[path = "production_completeness/guide-policy-compliance-reporting.rs"]
mod guide_policy_compliance_reporting;
#[path = "production_completeness/guide-audit-collection.rs"]
mod guide_audit_collection;
#[path = "production_completeness/guide-bad-release-recall-playbook.rs"]
mod guide_bad_release_recall_playbook;
#[path = "production_completeness/ops-bad-release-fleet-response.rs"]
mod ops_bad_release_fleet_response;
#[path = "production_completeness/guide-bidirectional-text-and-control-characters.rs"]
mod guide_bidirectional_text_and_control_characters;
#[path = "production_completeness/guide-text-input-and-composition.rs"]
mod guide_text_input_and_composition;
#[path = "production_completeness/guide-colour-depth-and-no-color.rs"]
mod guide_colour_depth_and_no_color;
#[path = "production_completeness/a11y-non-text-panes.rs"]
mod a11y_non_text_panes;
#[path = "production_completeness/reference-language-resolution-and-coverage.rs"]
mod reference_language_resolution_and_coverage;
#[path = "production_completeness/governance-api-change-proposals.rs"]
mod governance_api_change_proposals;
#[path = "production_completeness/guide-ci-test-report-formats.rs"]
mod guide_ci_test_report_formats;
#[path = "production_completeness/reference-conformance-claims.rs"]
mod reference_conformance_claims;
#[path = "production_completeness/guide-plugin-test-strategy.rs"]
mod guide_plugin_test_strategy;
#[path = "production_completeness/round2.rs"]
mod round2;
#[path = "production_completeness/round3.rs"]
mod round3;
#[path = "production_completeness/round4.rs"]
mod round4;
#[path = "production_completeness/round5.rs"]
mod round5;
#[path = "production_completeness/round6.rs"]
mod round6;
#[path = "production_completeness/round7.rs"]
mod round7;
#[path = "production_completeness/round8.rs"]
mod round8;
