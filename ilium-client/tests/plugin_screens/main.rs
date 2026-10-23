//! User and administrator lane, part 1 (ilium-client side): the screens a user sees while
//! managing plugins, driven through the screens rig and read back as screen text. One module
//! per surface. Red tests derived from docs/plugins-manual; every test carries a requirement tag.

mod accessible_dialogs;
mod diagnostics_dialog;
mod extensions_screen;
mod footer_and_notices;
mod install_dialogs;
mod plugins_tab;
mod recovery_page;
mod support;
mod usr_release_screen_reader;
