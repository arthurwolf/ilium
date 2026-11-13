//! Feature tests of the core plugin `ilium.core.chatroom` (manual page core-plugin-chatroom.md).
//!
//! Features F-062 (chatroom) and F-064 (context-injection hook), the behaviour contract
//! chatroom-B01 to chatroom-B17, the manifest (COR-050), the extension points, services and topics,
//! the virtual row with its unread count, the tail-and-publish flow, the message view and composer,
//! and the ported tests of the baseline chatroom modules.

mod common;
mod context_output;
mod doc_page;
mod extension_points;
mod init_room;
mod locked_append;
mod manifest;
mod ports_server;
mod reader_cursor;
mod record_format;
mod send_command;
mod tail_publish;
mod unread_row;
mod view_render;
