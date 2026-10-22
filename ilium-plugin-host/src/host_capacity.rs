//! Aggregate ceilings of the session server: installed plugins and active plugin sides
//! (reference-host-aggregate-limits.md, HST-510, HST-511). Skeleton: every function with
//! behaviour is `unimplemented!` until the implementation phase.

/// A refused install or activation. `reason` is the stable word, `host-capacity` when the
/// active ceiling is reached (HST-511).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityRefusal {
    pub reason: String,
}

/// The session server's plugin ledger with its aggregate ceilings (HST-510, HST-511).
#[derive(Debug, Default)]
pub struct CapacityHost;

impl CapacityHost {
    /// A ledger with the installed and active ceilings the policy resolved to.
    pub fn new(_installed_ceiling: usize, _active_ceiling: usize) -> CapacityHost {
        unimplemented!("host::host_capacity::CapacityHost::new")
    }

    /// Records an installed plugin; refused at the installed ceiling (HST-510).
    pub fn install(&mut self, _plugin_id: &str) -> Result<(), CapacityRefusal> {
        unimplemented!("host::host_capacity::CapacityHost::install")
    }

    /// Activates an installed plugin side; refused with `host-capacity` at the active ceiling
    /// and never evicts an active plugin to make room (HST-511).
    pub fn activate(&mut self, _plugin_id: &str) -> Result<(), CapacityRefusal> {
        unimplemented!("host::host_capacity::CapacityHost::activate")
    }

    /// Number of installed plugins.
    pub fn installed_count(&self) -> usize {
        unimplemented!("host::host_capacity::CapacityHost::installed_count")
    }

    /// Ids of the currently active plugins, in activation order.
    pub fn active_plugin_ids(&self) -> Vec<String> {
        unimplemented!("host::host_capacity::CapacityHost::active_plugin_ids")
    }

    /// The text `ilium plugin status` prints, including plugins left inactive (HST-511).
    pub fn status_listing(&self) -> String {
        unimplemented!("host::host_capacity::CapacityHost::status_listing")
    }
}

/// The ceilings a policy resolves to: an administrator may lower each value but never raise it
/// above 256 installed plugins and 128 active plugin sides (HST-510).
pub fn effective_ceilings(_requested_installed: usize, _requested_active: usize) -> (usize, usize) {
    unimplemented!("host::host_capacity::effective_ceilings")
}
