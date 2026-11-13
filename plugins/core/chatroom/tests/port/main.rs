//! Twins of the baseline chatroom tests that adapt to the plugin API (class adapt-to-plugin-api in
//! tools/test_port_map.tsv, target plugins/core/chatroom). Each twin asserts the same observable behaviour of
//! the original through FakeHost: the room file, `ilium chat`, the topic `chatroom`, the tree row and the view.

mod client_app;
mod client_chatroom;
mod client_ui;
mod common;
mod server_router;
