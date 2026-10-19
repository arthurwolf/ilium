//! Menu-provider harness: opens a menu over scripted providers with a manual clock
//! (HST-476..478, WIT:ilium:exports/menu-provider). Test support only.

use ilium_plugin_api::generated::exports::menu_provider::{MenuContext, MenuEntry};

/// What one menu opening drew and waited for (HST-476..478).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuOpening {
    /// Entry ids per drawn frame.
    pub frames: Vec<Vec<String>>,
    /// Clock reading when the menu opened.
    pub opened_at_ms: u64,
    /// (plugin, start ms) of every provider call.
    pub provider_calls: Vec<(String, u64)>,
    /// Plugins whose activation the opening waited for.
    pub activations_waited_for: Vec<String>,
}

/// The menu harness (HST-476..478).
#[derive(Debug)]
pub struct MenuHarness {
    _private: (),
}

impl MenuHarness {
    /// A fresh harness.
    pub fn new() -> MenuHarness {
        unimplemented!("host::testing::menu::MenuHarness::new")
    }

    /// A running provider that answers after a delay.
    pub fn add_provider(&mut self, _plugin: &str, _answer_after_ms: u32, _entries: Vec<MenuEntry>) {
        unimplemented!("host::testing::menu::MenuHarness::add_provider")
    }

    /// A stopped provider that needs activation first.
    pub fn add_stopped_provider(
        &mut self,
        _plugin: &str,
        _activation_ms: u32,
        _entries: Vec<MenuEntry>,
    ) {
        unimplemented!("host::testing::menu::MenuHarness::add_stopped_provider")
    }

    /// The static entries of the menu.
    pub fn static_entries(&mut self, _entries: Vec<MenuEntry>) {
        unimplemented!("host::testing::menu::MenuHarness::static_entries")
    }

    /// Opens the menu.
    pub fn open(&mut self, _context: MenuContext) -> MenuOpening {
        unimplemented!("host::testing::menu::MenuHarness::open")
    }

    /// Moves the manual clock.
    pub fn advance_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::menu::MenuHarness::advance_ms")
    }
}

impl Default for MenuHarness {
    fn default() -> MenuHarness {
        MenuHarness::new()
    }
}
