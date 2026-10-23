//! Devtools tests of the plugin CLI: check, lint, new, build, CI and documentation rules, plus the
//! debugging, trace and doctor files added by the second writer of this lane.

mod common;
mod dev_pipeline_debug_build;
mod dev_pipeline_deprecation;
mod dev_pipeline_dev_loop;
mod dev_pipeline_network;
mod dev_pipeline_test_harness;
mod dev_pipeline_toolchain;
mod guide_backwards_compatibility_test_matrix;
mod guide_ci_recipes;
mod guide_plugin_documentation;
mod guide_plugin_documentation_p2;
mod lint_support;
mod performance;
mod reference_check_and_lint_rules_check;
mod reference_check_and_lint_rules_lint;
mod reference_check_and_lint_rules_suppress;
mod reference_plugin_cli_build;
mod reference_project_templates;
