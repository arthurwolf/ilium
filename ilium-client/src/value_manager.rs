//! Local branch disposition catalog; final removal remains explicitly confirmed.
use crate::app::{App, Mode};
impl App {
    pub(crate) fn begin_prune_branch_dialog(&mut self) {
        let result = match &self.mode {
            Mode::WorktreeManager(state) => {
                crate::value_dialog_host::ValueDialogHost::prune_branch(state)
            }
            _ => Err("No removal confirmation is open".into()),
        };
        match result {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }
}
