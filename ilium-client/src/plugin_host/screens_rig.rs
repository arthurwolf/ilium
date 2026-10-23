//! Test-support rig that draws the user-admin screens (Plugins tab, Capabilities tab, Extensions
//! tab, recovery page, install dialogs, footer and notices) into a screen buffer and feeds keys
//! back. Skeleton: every method is `unimplemented!`; spec in `tests/plugin_screens/API_GAPS.md`.

/// Requirement family USR-014..USR-214: the state shown in a Plugins tab row.
#[derive(Clone, Debug, PartialEq)]
pub enum RowState {
    /// Running normally.
    On,
    /// Running but slow.
    OnSlow,
    /// Being restarted.
    Restarting,
    /// Stopped after failures.
    Failed,
    /// Switched off.
    Off,
    /// Waiting for the named owner plugin.
    WaitingFor(String),
    /// Blocked by system policy.
    BlockedByPolicy,
    /// Refused to load, with the reason.
    Refused(String),
}

/// Requirement family USR-014..USR-214: the client screens test rig.
#[derive(Debug)]
pub struct ScreensRig {
    _private: (),
}

impl ScreensRig {
    /// USR-014..USR-214: a rig with a screen of the given size.
    pub fn new(_columns: u16, _rows: u16) -> ScreensRig {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::new")
    }

    /// USR-014..USR-214 screen reads.
    pub fn screen_text(&self) -> String {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::screen_text")
    }

    /// USR-014..USR-214 screen reads.
    pub fn scrolled_text(&mut self) -> String {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::scrolled_text")
    }

    /// USR-014..USR-214 screen reads.
    pub fn footer_text(&self) -> String {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::footer_text")
    }

    /// USR-014..USR-214 screen reads.
    pub fn footer_notice_foreground(&self) -> Option<String> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::footer_notice_foreground")
    }

    /// USR-014..USR-214 screen reads.
    pub fn theme_danger_colour(&self) -> String {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::theme_danger_colour")
    }

    /// USR-014..USR-214 screen reads.
    pub fn last_notice(&self) -> Option<String> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::last_notice")
    }

    /// USR-014..USR-214 screen reads.
    pub fn advance_ms(&mut self, _ms: u64) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::advance_ms")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn press_key(&mut self, _key: char) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::press_key")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn press_key_named(&mut self, _name: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::press_key_named")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn press_space(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::press_space")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn press_enter(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::press_enter")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn press_escape(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::press_escape")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn press_prefix_then(&mut self, _key: char) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::press_prefix_then")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn move_selection_to(&mut self, _text: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::move_selection_to")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn select_plugin(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::select_plugin")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn activate(&mut self, _label: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::activate")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn confirm_yes(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::confirm_yes")
    }

    /// USR-014/USR-032/USR-041/USR-160 input.
    pub fn confirm_no(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::confirm_no")
    }

    /// USR-226: opens the Diagnostics "Build support bundle" dialog for a plugin.
    pub fn open_support_bundle_dialog(&mut self, _plugin_id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_support_bundle_dialog")
    }

    /// USR-226: the label of the button that Enter presses in the open dialog.
    pub fn default_button_label(&self) -> Option<String> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::default_button_label")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn install_plugin(&mut self, _id: &str, _tier: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::install_plugin")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn install_plugin_with_grants(&mut self, _id: &str, _grants: &[&str]) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::install_plugin_with_grants")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn install_plugin_that_traps_on_activate(&mut self, _id: &str, _name: &str) {
        unimplemented!(
            "client::plugin_host::screens_rig::ScreensRig::install_plugin_that_traps_on_activate"
        )
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn install_plugin_that_traps_in_render(&mut self, _id: &str, _name: &str, _view: &str) {
        unimplemented!(
            "client::plugin_host::screens_rig::ScreensRig::install_plugin_that_traps_in_render"
        )
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn install_plugin_that_claims_recovery_page(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::install_plugin_that_claims_recovery_page")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn install_owner_with_point(&mut self, _id: &str, _owner_name: &str, _point_name: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::install_owner_with_point")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn install_contributor(
        &mut self,
        _id: &str,
        _owner: &str,
        _point: &str,
        _hook_name: &str,
        _priority: i32,
    ) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::install_contributor")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn remove_plugin(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::remove_plugin")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn set_plugin_state_off(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::set_plugin_state_off")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn put_plugin_in_state(&mut self, _id: &str, _state: RowState) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::put_plugin_in_state")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn fail_plugin_three_times(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::fail_plugin_three_times")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn set_grant_state(&mut self, _id: &str, _grant: &str, _state: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::set_grant_state")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn set_policy_denied_grants(&mut self, _grants: &[&str]) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::set_policy_denied_grants")
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn write_system_policy_with_mode(&mut self, _toml: &str, _mode: u32) {
        unimplemented!(
            "client::plugin_host::screens_rig::ScreensRig::write_system_policy_with_mode"
        )
    }

    /// USR-014/USR-031/USR-161/USR-162 set-up.
    pub fn set_bundled_first_start(&mut self, _id: &str, _grants: &[&str]) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::set_bundled_first_start")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn start_client(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::start_client")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn start_client_safe_mode(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::start_client_safe_mode")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn stop_client(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::stop_client")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn restart_client(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::restart_client")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_settings(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_settings")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_plugins_tab(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_plugins_tab")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_plugin_details(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_plugin_details")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_details(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_details")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_capabilities_tab(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_capabilities_tab")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_recovery_page(&mut self) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_recovery_page")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_review_dialog(&mut self, _id: &str, _grants: &[&str]) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_review_dialog")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_trust_prompt(&mut self, _publisher: &str, _fingerprint: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_trust_prompt")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_key_change_prompt(&mut self, _id: &str, _old: &str, _new: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_key_change_prompt")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_replace_prompt(&mut self, _id: &str, _replaced: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_replace_prompt")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_extensions_owner_view(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::open_extensions_owner_view")
    }

    /// USR-022/USR-050/USR-120/USR-122 screens.
    pub fn open_extensions_contributor_view(&mut self, _id: &str) {
        unimplemented!(
            "client::plugin_host::screens_rig::ScreensRig::open_extensions_contributor_view"
        )
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn plugin_row_count(&self) -> usize {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_row_count")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn cli_list_text(&self) -> String {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::cli_list_text")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn plugin_state_word(&self, _id: &str) -> String {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_state_word")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn plugin_is_running(&self, _id: &str) -> bool {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_is_running")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn failure_count(&self, _id: &str) -> usize {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::failure_count")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn trust_answers(&self) -> Vec<bool> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::trust_answers")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn replace_answers(&self) -> Vec<bool> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::replace_answers")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn plain_wording(&self, _grant: &str) -> String {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plain_wording")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn grant_is_high(&self, _grant: &str) -> bool {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::grant_is_high")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn run_order_of_point(&self, _owner: &str, _point: &str) -> Vec<String> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::run_order_of_point")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn breach_extension(
        &mut self,
        _contributor: &str,
        _owner: &str,
        _point: &str,
        _times: u32,
    ) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::breach_extension")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn plugin_setting(&self, _id: &str, _key: &str) -> Option<String> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_setting")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn plugin_setting_project(&self, _id: &str, _key: &str) -> Option<String> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_setting_project")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn set_plugin_setting(&mut self, _id: &str, _key: &str, _value: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::set_plugin_setting")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn set_plugin_setting_project(&mut self, _id: &str, _key: &str, _value: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::set_plugin_setting_project")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn grants_file_bytes(&self) -> Vec<u8> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::grants_file_bytes")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn requested_editor_paths(&self) -> Vec<String> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::requested_editor_paths")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn restart_requested_in_safe_mode(&self) -> bool {
        unimplemented!(
            "client::plugin_host::screens_rig::ScreensRig::restart_requested_in_safe_mode"
        )
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn recovery_page_owner(&self) -> String {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::recovery_page_owner")
    }

    /// USR-032/USR-053/USR-080/USR-160/USR-163 read-back.
    pub fn last_install_error(&self) -> Option<String> {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::last_install_error")
    }

    /// USR-087/USR-088 plugin actions.
    pub fn plugin_reads_clipboard(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_reads_clipboard")
    }

    /// USR-087/USR-088 plugin actions.
    pub fn plugin_writes_clipboard(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_writes_clipboard")
    }

    /// USR-087/USR-088 plugin actions.
    pub fn plugin_toasts(&mut self, _id: &str, _text: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_toasts")
    }

    /// USR-087/USR-088 plugin actions.
    pub fn plugin_locks_input(&mut self, _id: &str) {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::plugin_locks_input")
    }

    /// USR-087/USR-088 plugin actions.
    pub fn pane_received_input(&self, _key: char) -> bool {
        unimplemented!("client::plugin_host::screens_rig::ScreensRig::pane_received_input")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// A 64-character lowercase hexadecimal fingerprint in groups of four (USR-120).
    const HEX_FINGERPRINT: &str =
        "0123 4567 89ab cdef 0123 4567 89ab cdef 0123 4567 89ab cdef 0123 4567 89ab cdef";

    /// REQ: USR-014
    #[test]
    fn first_start_prompt_stops_after_third_not_now() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        let prompt_screen = rig.screen_text();
        rig.confirm_no();
        rig.stop_client();
        rig.start_client();
        rig.confirm_no();
        rig.stop_client();
        rig.start_client();
        rig.confirm_no();
        rig.stop_client();
        rig.start_client();
        assert_ne!(rig.screen_text(), prompt_screen);
    }

    /// REQ: USR-022, USR-012
    #[test]
    fn review_dialog_starts_high_rows_unchecked_and_records_defaults() {
        let mut rig = ScreensRig::new(100, 30);
        let grants = ["http:api.example.com", "terminal.write"];
        assert!(!rig.grant_is_high("http:api.example.com"));
        assert!(rig.grant_is_high("terminal.write"));
        rig.open_review_dialog("ilium.test.review", &grants);
        rig.press_enter();
        let grants_file = String::from_utf8_lossy(&rig.grants_file_bytes()).into_owned();
        assert_eq!(grants_file.matches("granted").count(), 1);
        assert_eq!(grants_file.matches("declined").count(), 1);
    }

    /// REQ: USR-031
    #[test]
    fn safe_mode_labels_third_party_rows_off_with_reason() {
        let mut rig = ScreensRig::new(100, 30);
        rig.install_plugin("ilium.test.third", "T2");
        rig.start_client_safe_mode();
        assert!(rig.screen_text().contains("safe-mode"));
        assert!(rig
            .plugin_state_word("ilium.test.third")
            .to_lowercase()
            .contains("off"));
        assert!(!rig.plugin_is_running("ilium.test.third"));
    }

    /// REQ: USR-041
    #[test]
    fn plugins_tab_refuses_to_switch_off_core_plugin_and_says_why() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.open_plugins_tab();
        rig.select_plugin("ilium.core.tree");
        rig.press_space();
        assert!(rig.plugin_is_running("ilium.core.tree"));
        assert!(rig.last_notice().is_some_and(|text| !text.is_empty()));
    }

    /// REQ: USR-050, USR-051
    #[test]
    fn user_moved_run_order_wins_over_priority_and_survives_restart() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.install_owner_with_point("ilium.test.owner", "Owner", "greet");
        rig.install_contributor("ilium.test.a", "ilium.test.owner", "greet", "hook_a", 10);
        rig.install_contributor("ilium.test.b", "ilium.test.owner", "greet", "hook_b", 20);
        assert_eq!(
            rig.run_order_of_point("ilium.test.owner", "greet"),
            vec!["ilium.test.a".to_string(), "ilium.test.b".to_string()]
        );
        rig.open_extensions_owner_view("ilium.test.owner");
        rig.move_selection_to("ilium.test.b");
        rig.activate("Move up");
        let expected = vec!["ilium.test.b".to_string(), "ilium.test.a".to_string()];
        assert_eq!(
            rig.run_order_of_point("ilium.test.owner", "greet"),
            expected
        );
        rig.restart_client();
        assert_eq!(
            rig.run_order_of_point("ilium.test.owner", "greet"),
            expected
        );
    }

    /// REQ: USR-053
    #[test]
    fn three_breaches_stop_extension_with_notice_and_not_on_disk() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.install_owner_with_point("ilium.test.owner", "Owner", "greet");
        rig.install_contributor("ilium.test.c", "ilium.test.owner", "greet", "hook_c", 10);
        rig.breach_extension("ilium.test.c", "ilium.test.owner", "greet", 3);
        assert!(!rig
            .run_order_of_point("ilium.test.owner", "greet")
            .contains(&"ilium.test.c".to_string()));
        assert!(rig.last_notice().is_some());
        assert!(rig.screen_text().contains("Reset"));
        rig.restart_client();
        assert!(rig
            .run_order_of_point("ilium.test.owner", "greet")
            .contains(&"ilium.test.c".to_string()));
    }

    /// REQ: USR-080, USR-082
    #[test]
    fn capabilities_tab_shows_plain_wording_and_exact_grant_string() {
        let mut rig = ScreensRig::new(100, 30);
        rig.install_plugin_with_grants(
            "ilium.test.caps",
            &["terminal.write", "http:api.example.com"],
        );
        rig.open_capabilities_tab("ilium.test.caps");
        let wording = rig.plain_wording("http:api.example.com");
        assert!(!wording.is_empty());
        assert_ne!(wording, "http:api.example.com");
        assert!(rig.screen_text().contains(&wording));
        assert!(rig.screen_text().contains("http:api.example.com"));
        assert!(rig.grant_is_high("terminal.write"));
    }

    /// REQ: USR-087, USR-061
    #[test]
    fn clipboard_footer_indicator_lasts_three_seconds_and_needs_grant() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.install_plugin_with_grants("ilium.test.clip", &["clipboard.read"]);
        rig.install_plugin("ilium.test.noclip", "T2");
        let quiet_footer = rig.footer_text();
        rig.plugin_reads_clipboard("ilium.test.noclip");
        assert_eq!(rig.footer_text(), quiet_footer);
        rig.plugin_reads_clipboard("ilium.test.clip");
        assert_ne!(rig.footer_text(), quiet_footer);
        rig.advance_ms(3000);
        assert_eq!(rig.footer_text(), quiet_footer);
    }

    /// REQ: USR-088
    #[test]
    fn toasts_capped_at_three_per_window_and_input_lock_ends_after_two_seconds() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        rig.install_plugin("ilium.test.toaster", "T2");
        for index in 0..10 {
            rig.plugin_toasts("ilium.test.toaster", &format!("toast-{index}"));
        }
        assert_eq!(rig.screen_text().matches("toast-").count(), 3);
        rig.plugin_locks_input("ilium.test.toaster");
        assert!(!rig.pane_received_input('x'));
        rig.advance_ms(2000);
        assert!(rig.pane_received_input('x'));
    }

    /// REQ: USR-120
    #[test]
    fn declining_unknown_signing_key_installs_nothing() {
        let mut rig = ScreensRig::new(100, 30);
        rig.start_client();
        let rows_before = rig.plugin_row_count();
        rig.open_trust_prompt("Example Publisher", HEX_FINGERPRINT);
        assert!(rig.screen_text().contains(HEX_FINGERPRINT));
        rig.confirm_no();
        assert_eq!(rig.trust_answers(), vec![false]);
        assert_eq!(rig.plugin_row_count(), rows_before);
    }
}
