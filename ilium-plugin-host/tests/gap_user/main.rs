//! Lane `user-gap`: requirement tests for the user-facing rows of `reference-requirements-user-admin`
//! (USR-250..USR-349) that no other lane covers. Covered IDs: USR-250, USR-251, USR-252, USR-254,
//! USR-255, USR-256, USR-259, USR-260, USR-300..USR-307, USR-310..USR-317, USR-318, USR-319,
//! USR-321, USR-322..USR-331, USR-332, USR-334..USR-337, USR-338, USR-339, USR-341, USR-343,
//! USR-344, USR-345, USR-346, USR-347, USR-348, USR-349.
//!
//! One module per manual page under `docs/plugins-manual/pages/`. `common` holds the shared
//! helpers. The tests drive the `ilium_plugin_host::testing::ops` rig and the user-facing APIs
//! named in the lane report. Run with `--features testing`.

mod accessibility_and_theming;
mod common;
mod core_plugin_diagnostics;
mod guide_enterprise_deployment;
mod guide_internationalisation;
mod guide_plugin_administration;
mod reference_grant_review_and_consent;
mod reference_install_and_update;
mod user_permissions_explained;
mod user_plugin_health;
mod user_plugin_security_and_privacy;
mod user_plugin_store;
mod user_privacy;
mod user_safe_mode_and_troubleshooting;
mod user_troubleshooting;
