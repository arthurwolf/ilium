//! chatroom-B15 (reader cursor files) and the `--since-last-read` flow of F-064.

use crate::common::*;

/// sha256("user")[..32]
const USER_KEY_HASH: &str = "04f8996da763b7a969b1028ee3007569";
/// sha256("pane-1")[..32]
const PANE_ONE_HASH: &str = "370bd5d12a1b3b8ac706a5726b8b800e";
/// sha256("s1")[..32]
const SESSION_ONE_HASH: &str = "e8bc163c82eee18733288c7d4ac636db";

fn first_record_lines() -> Vec<String> {
    vec![
        record_line(
            "2026-10-12 09:14:02 +02:00",
            "fix-auth",
            "Claiming src/auth.rs until 10:00.",
        ),
        record_line("2026-10-12 09:15:40 +02:00", "review", "Tests pass."),
    ]
}

fn cursor_text(host: &mut ilium_plugin_sdk::testing::FakeHost, hash: &str) -> Option<String> {
    host.fs()
        .read(format!("project:.ilium/chat-readers/{hash}").as_str())
        .map(|bytes| String::from_utf8(bytes).expect("utf-8").trim().to_string())
}

/// REQ: chatroom-B15, F-064
#[test]
fn cursor_b15_reading_with_a_reader_key_writes_the_cursor_file_named_by_the_key_hash() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    assert!(cursor_text(&mut host, PANE_ONE_HASH).is_some());
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_the_file_name_is_the_first_32_hex_characters_of_the_key_sha256() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    assert_eq!(PANE_ONE_HASH.len(), 32);
    assert!(host
        .fs()
        .read(format!("project:.ilium/chat-readers/{}", &PANE_ONE_HASH[..31]).as_str())
        .is_none());
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_the_file_holds_the_sha256_of_timestamp_author_content_with_nul_terminators() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    // sha256("2026-10-12 09:15:40 +02:00\0review\0Tests pass.\0")
    assert_eq!(
        cursor_text(&mut host, PANE_ONE_HASH).expect("cursor"),
        "0ec26ffffa55fa42e267c05df29ef960ad2d8c4edeb1078314a52e076201def7"
    );
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_the_cursor_value_is_64_lowercase_hex_characters() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    let value = cursor_text(&mut host, PANE_ONE_HASH).expect("cursor");
    assert_eq!(value.len(), 64);
    assert!(value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_the_cursor_moves_to_the_newest_record_after_each_read() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    send(&mut host, "third");
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    let moved = cursor_text(&mut host, PANE_ONE_HASH).expect("cursor");
    assert_ne!(
        moved,
        "0ec26ffffa55fa42e267c05df29ef960ad2d8c4edeb1078314a52e076201def7"
    );
    assert_eq!(moved.len(), 64);
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_each_reader_has_its_own_file() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    assert!(cursor_text(&mut host, "0e7a5ae0e7df37895f37227c57de19fa").is_none());
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-2"],
    );
    assert!(cursor_text(&mut host, "0e7a5ae0e7df37895f37227c57de19fa").is_some());
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_the_reader_key_is_the_pane_id_when_no_reader_option_is_given() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    host.cli().set_env("ILIUM_PANE_ID", "pane-1");
    chat(&mut host, &["context", "--since-last-read"]);
    assert!(cursor_text(&mut host, PANE_ONE_HASH).is_some());
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_the_reader_option_wins_over_the_pane_id() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    host.cli().set_env("ILIUM_PANE_ID", "pane-2");
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    assert!(cursor_text(&mut host, PANE_ONE_HASH).is_some());
    assert!(cursor_text(&mut host, "0e7a5ae0e7df37895f37227c57de19fa").is_none());
}

/// REQ: chatroom-B15, F-064
#[test]
fn cursor_b15_without_option_or_pane_id_the_session_id_of_the_hook_input_is_the_reader() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    chat_stdin(
        &mut host,
        &["context", "--since-last-read"],
        br#"{"session_id":"s1","hook_event_name":"UserPromptSubmit"}"#,
    );
    assert!(cursor_text(&mut host, SESSION_ONE_HASH).is_some());
}

/// REQ: chatroom-B15, F-064
#[test]
fn cursor_b15_without_any_reader_key_the_command_behaves_like_plain_context() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    let plain = chat(&mut host, &["context"]);
    let tracked = chat(&mut host, &["context", "--since-last-read"]);
    assert_eq!(tracked.out, plain.out);
    assert!(host
        .fs()
        .read(format!("project:.ilium/chat-readers/{USER_KEY_HASH}").as_str())
        .is_none());
}

/// REQ: chatroom-B15, PORT:ilium-client/src/chatroom.rs::unread_context_prints_only_records_the_reader_has_not_seen
#[test]
fn cursor_b15_a_reader_sees_only_records_it_has_not_seen() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    let first = chat(
        &mut host,
        &[
            "context",
            "--since-last-read",
            "--reader",
            "pane-1",
            "--max-bytes",
            "2048",
        ],
    );
    assert!(first.out.contains("Claiming src/auth.rs") && first.out.contains("Tests pass."));
    assert!(first.out.starts_with("Ilium chatroom is enabled"));
    let unchanged = chat(
        &mut host,
        &[
            "context",
            "--since-last-read",
            "--reader",
            "pane-1",
            "--max-bytes",
            "2048",
        ],
    );
    assert_eq!(unchanged.out, "");
    assert_eq!(unchanged.code, Some(0));
    send(&mut host, "third claim");
    let next = chat(
        &mut host,
        &[
            "context",
            "--since-last-read",
            "--reader",
            "pane-1",
            "--max-bytes",
            "2048",
        ],
    );
    assert!(
        next.out.starts_with("New ilium chatroom messages"),
        "{}",
        next.out
    );
    assert!(next.out.contains("third claim"));
    assert!(!next.out.contains("Tests pass."));
}

/// REQ: chatroom-B15, PORT:ilium-client/src/chatroom.rs::unread_context_prints_only_records_the_reader_has_not_seen
#[test]
fn cursor_b15_another_reader_gets_the_full_form_and_a_session_start_resets_the_reader() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    send(&mut host, "third claim");
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    let other = chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-2"],
    );
    assert!(other.out.starts_with("Ilium chatroom is enabled"));
    assert!(other.out.contains("Tests pass.") && other.out.contains("third claim"));
    let restart = chat_stdin(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
        br#"{"hook_event_name":"SessionStart"}"#,
    );
    assert!(
        restart.out.starts_with("Ilium chatroom is enabled"),
        "{}",
        restart.out
    );
    assert!(restart.out.contains("Tests pass.") && restart.out.contains("third claim"));
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_a_cursor_whose_record_is_no_longer_in_the_room_gets_the_full_form() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    host.fs().write(
        format!("project:.ilium/chat-readers/{PANE_ONE_HASH}").as_str(),
        format!("{}\n", "ab".repeat(32)).as_bytes(),
    );
    let run = chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    assert!(
        run.out.starts_with("Ilium chatroom is enabled"),
        "{}",
        run.out
    );
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_a_malformed_cursor_file_is_treated_as_unread_everything() {
    let mut host = server_host();
    seed_room(&mut host, &first_record_lines());
    host.fs().write(
        format!("project:.ilium/chat-readers/{PANE_ONE_HASH}").as_str(),
        b"not a hash\n",
    );
    let run = chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    assert!(
        run.out.starts_with("Ilium chatroom is enabled"),
        "{}",
        run.out
    );
    assert_eq!(
        cursor_text(&mut host, PANE_ONE_HASH)
            .expect("rewritten")
            .len(),
        64
    );
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_an_empty_room_writes_no_cursor() {
    let mut host = server_host();
    seed_room(&mut host, &[]);
    chat(
        &mut host,
        &["context", "--since-last-read", "--reader", "pane-1"],
    );
    assert!(cursor_text(&mut host, PANE_ONE_HASH).is_none());
}

/// REQ: chatroom-B07, chatroom-B15
#[test]
fn cursor_b07_the_unread_scan_covers_the_last_2000_records_cursor_inside() {
    // 2001 records; the cursor sits on record 1, the oldest record inside the scan window.
    let mut host = host_with_records(2001);
    let fingerprint = cursor_of(1);
    host.fs().write(
        format!("project:.ilium/chat-readers/{PANE_ONE_HASH}").as_str(),
        format!("{fingerprint}\n").as_bytes(),
    );
    let run = chat(
        &mut host,
        &[
            "context",
            "--since-last-read",
            "--reader",
            "pane-1",
            "--limit",
            "40",
        ],
    );
    assert!(
        run.out.starts_with("New ilium chatroom messages"),
        "{}",
        &run.out[..run.out.len().min(120)]
    );
}

/// REQ: chatroom-B07, chatroom-B15
#[test]
fn cursor_b07_the_unread_scan_covers_the_last_2000_records_cursor_outside() {
    // The cursor sits on record 0, one record outside the scan window of 2000.
    let mut host = host_with_records(2001);
    let fingerprint = cursor_of(0);
    host.fs().write(
        format!("project:.ilium/chat-readers/{PANE_ONE_HASH}").as_str(),
        format!("{fingerprint}\n").as_bytes(),
    );
    let run = chat(
        &mut host,
        &[
            "context",
            "--since-last-read",
            "--reader",
            "pane-1",
            "--limit",
            "40",
        ],
    );
    assert!(
        run.out.starts_with("Ilium chatroom is enabled"),
        "{}",
        &run.out[..run.out.len().min(120)]
    );
}

/// REQ: chatroom-B07, chatroom-B15
#[test]
fn cursor_b07_the_unread_scan_with_exactly_2000_records_finds_the_oldest_cursor() {
    let mut host = host_with_records(2000);
    let fingerprint = cursor_of(0);
    host.fs().write(
        format!("project:.ilium/chat-readers/{PANE_ONE_HASH}").as_str(),
        format!("{fingerprint}\n").as_bytes(),
    );
    let run = chat(
        &mut host,
        &[
            "context",
            "--since-last-read",
            "--reader",
            "pane-1",
            "--limit",
            "40",
        ],
    );
    assert!(
        run.out.starts_with("New ilium chatroom messages"),
        "{}",
        &run.out[..run.out.len().min(120)]
    );
}

/// The fingerprint a cursor file holds for `numbered_line(n)`: sha256 of `timestamp\0author\0content\0`.
fn cursor_of(n: usize) -> String {
    sha256_hex(format!("{}\0agent\0record {n}\0", stamp(n)).as_bytes())
}

/// Minimal sha256 so the tests need no extra dependency.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (index, word) in block.chunks(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut v = state;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let choose = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let majority = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(majority);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for index in 0..8 {
            state[index] = state[index].wrapping_add(v[index]);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// REQ: chatroom-B15
#[test]
fn cursor_b15_the_helper_sha256_matches_a_known_digest() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(sha256_hex(b"user")[..32], *USER_KEY_HASH);
    assert_eq!(sha256_hex(b"pane-1")[..32], *PANE_ONE_HASH);
    assert_eq!(sha256_hex(b"s1")[..32], *SESSION_ONE_HASH);
}
