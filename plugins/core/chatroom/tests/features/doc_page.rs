//! Rules the page states in prose and tables: the behaviour table, the traceability rows and the page's
//! references (doc_ tests, they may pass before the implementation).

use crate::common::{page_text, traceability_text};

/// The cells of the row whose first cell is `id` in the behaviour contract table.
fn behaviour_row(id: &str) -> Vec<String> {
    let page = page_text();
    page.lines()
        .find(|line| line.starts_with(&format!("| {id} |")))
        .unwrap_or_else(|| panic!("row {id}"))
        .trim_matches('|')
        .split(" | ")
        .map(|cell| cell.trim().trim_matches('|').trim().to_string())
        .collect()
}

/// REQ: chatroom-B01
#[test]
fn doc_b01_record_line_row_names_the_exact_format() {
    let row = behaviour_row("chatroom-B01");
    assert_eq!(row[1], "Record line");
    assert!(row[2].contains("- YYYY-MM-DD HH:mm:ss +HH:MM"));
}

/// REQ: chatroom-B02
#[test]
fn doc_b02_escaping_row_lists_newline_tab_backslash_and_control_characters() {
    let row = behaviour_row("chatroom-B02");
    assert!(row[2].contains("`\\n`") && row[2].contains("`\\t`") && row[2].contains("`\\\\`"));
    assert!(row[2].contains("other control characters are removed"));
}

/// REQ: chatroom-B03
#[test]
fn doc_b03_limit_row_says_4000_characters() {
    assert!(behaviour_row("chatroom-B03")[2].contains("At most 4000 characters"));
}

/// REQ: chatroom-B04
#[test]
fn doc_b04_default_author_order_is_chatroom_author_agent_name_agent() {
    assert!(behaviour_row("chatroom-B04")[2].contains("`$ILIUM_CHATROOM_AUTHOR`, then `$AGENT_NAME`, then `agent`"));
}

/// REQ: chatroom-B05
#[test]
fn doc_b05_initial_file_row_names_title_marker_guidance_and_heading() {
    let row = behaviour_row("chatroom-B05");
    for needle in ["# ILIUM CHATROOM", "<!-- ilium-chatroom: v1 -->", "agent/coordination-guidance", "## Messages"] {
        assert!(row[2].contains(needle), "{needle}");
    }
}

/// REQ: chatroom-B06
#[test]
fn doc_b06_ignore_row_names_both_lines() {
    let row = behaviour_row("chatroom-B06");
    assert!(row[2].contains("/CHATROOM.md") && row[2].contains("/CHATROOM.archive.md"));
}

/// REQ: chatroom-B07
#[test]
fn doc_b07_limits_row_names_8_mib_200_messages_4_mib_and_2000_records() {
    let row = behaviour_row("chatroom-B07")[2].clone();
    for needle in ["8 MiB", "200 messages", "4 MiB", "2000 records"] {
        assert!(row.contains(needle), "{needle}");
    }
}

/// REQ: chatroom-B08
#[test]
fn doc_b08_composer_row_names_the_placeholder_and_the_empty_text() {
    let row = behaviour_row("chatroom-B08")[2].clone();
    assert!(row.contains("Write message - Enter sends"));
    assert!(row.contains("No messages yet. Type below to post as user."));
}

/// REQ: chatroom-B09
#[test]
fn doc_b09_rows_row_names_the_row_format_and_the_agents_list() {
    let row = behaviour_row("chatroom-B09")[2].clone();
    assert!(row.contains("<YYYY-MM-DD HH:mm:ss> <author> <content>"));
    assert!(row.contains("Agents in this project"));
}

/// REQ: chatroom-B10
#[test]
fn doc_b10_output_row_names_the_three_outputs_and_the_default_of_100() {
    let row = behaviour_row("chatroom-B10")[2].clone();
    assert!(row.contains("chatroom ready at <path>") && row.contains("chatroom message sent"));
    assert!(row.contains("default 100 records"));
}

/// REQ: chatroom-B11
#[test]
fn doc_b11_errors_row_lists_the_three_messages_with_exit_1() {
    let row = behaviour_row("chatroom-B11")[2].clone();
    assert!(row.contains("exit 1"));
    for needle in [
        "this project does not have a chatroom",
        "chatroom author and message must not be empty",
        "chatroom messages are limited to 4000 characters",
    ] {
        assert!(row.contains(needle), "{needle}");
    }
}

/// REQ: chatroom-B12
#[test]
fn doc_b12_hook_row_names_command_timeout_and_status() {
    let row = behaviour_row("chatroom-B12")[2].clone();
    assert!(row.contains("ilium chat context --limit 40 --since-last-read --max-bytes 2048"));
    assert!(row.contains("timeout 10 s") && row.contains("Checking ilium chatroom"));
}

/// REQ: chatroom-B13
#[test]
fn doc_b13_context_row_names_the_four_templates() {
    let row = behaviour_row("chatroom-B13")[2].clone();
    for needle in ["agent/chatroom-context", "agent/chatroom-empty-context", "agent/chatroom-omitted", "agent/chatroom-context-row"] {
        assert!(row.contains(needle), "{needle}");
    }
}

/// REQ: chatroom-B14
#[test]
fn doc_b14_cap_row_names_160_bytes_the_marker_and_the_factor_4() {
    let row = behaviour_row("chatroom-B14")[2].clone();
    assert!(row.contains("160 bytes") && row.contains("4 times the budget"));
}

/// REQ: chatroom-B15
#[test]
fn doc_b15_cursor_row_names_the_directory_the_hash_and_the_reader_keys() {
    let row = behaviour_row("chatroom-B15")[2].clone();
    assert!(row.contains(".ilium/chat-readers/<first 32 hex chars of sha256(reader key)>"));
    assert!(row.contains("timestamp\\0author\\0content\\0") || row.contains("timestamp\0author\0content\0"));
    assert!(row.contains("`--reader`, else `ILIUM_PANE_ID`"));
}

/// REQ: chatroom-B16
#[test]
fn doc_b16_unread_row_is_new_in_the_plugin_build_and_has_no_baseline_source() {
    let row = behaviour_row("chatroom-B16");
    assert!(row[2].contains("(N new)") && row[2].contains("`user`"));
    assert!(row[2].contains("not parity"));
    assert_eq!(row[3], "none");
}

/// REQ: chatroom-B17
#[test]
fn doc_b17_locked_append_row_and_the_fifty_fifty_test_sentence() {
    let page = page_text();
    assert!(behaviour_row("chatroom-B17")[2].contains("exclusive advisory lock"));
    assert!(page.contains("50 concurrent `ilium chat send` calls and 50 composer sends produce 100 records"));
}

/// REQ: F-062, F-064
#[test]
fn doc_f062_traceability_rows_name_chatroom_as_owner() {
    let text = traceability_text();
    for id in ["F-062", "F-064"] {
        let row = text.lines().find(|line| line.contains(id)).unwrap_or_else(|| panic!("row {id}"));
        assert!(row.contains("chatroom"), "{row}");
    }
}

/// REQ: F-064
#[test]
fn doc_f064_the_page_says_agent_setup_owns_the_hooks_and_blocks() {
    let page = page_text();
    assert!(page.contains("Agent setup owns the Setup tab, both instruction blocks and the hook entries."));
    assert!(page.contains("The plugin holds no `agent-config` grant"));
}

/// REQ: F-062
#[test]
fn doc_f062_the_page_covers_the_row_the_cli_the_view_and_the_context_output() {
    let page = page_text();
    for needle in [
        "The Chatroom row in the tree, with an unread count",
        "`ilium chat`, described in the CLI reference.",
        "The message view and the composer.",
        "The context-injection output of `ilium chat context`",
    ] {
        assert!(page.contains(needle), "{needle}");
    }
}
