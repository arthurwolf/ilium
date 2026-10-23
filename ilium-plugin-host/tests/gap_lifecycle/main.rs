//! Gap lane `lifecycle`: requirement tests for the lifecycle-ops rows that had no test yet
//! (LIF-306..318, LIF-700..729 from reference-requirements-lifecycle-ops.md).
//!
//! One module per manual page: guide-operator-upgrades (LIF-306..308), reference-trust-and-supply-chain
//! (LIF-309..311), core-plugin-diagnostics (LIF-312..314), reference-persistence-and-restart
//! (LIF-317..318), reference-plugin-lifecycle-states (LIF-700, 701, 703, 705, 706, 718, 719, 720, 724),
//! lifecycle (LIF-702, 704, 721, 723, 729), reference-install-update-uninstall (LIF-707, 708, 711,
//! 713, 716, 717, 725, 726) and reference-install-and-update (LIF-727).
//!
//! `support` holds the shared helpers. The tests drive the `ilium_plugin_host::testing::ops` rig,
//! so run them with `--features testing`, as the lifecycle_ops lane does. Tests are expected to
//! fail or panic until the rig and the host behaviour exist.

mod core_plugin_diagnostics;
mod guide_operator_upgrades;
mod lifecycle;
mod reference_install_and_update;
mod reference_install_update_uninstall;
mod reference_persistence_and_restart;
mod reference_plugin_lifecycle_states;
mod reference_trust_and_supply_chain;
mod support;
