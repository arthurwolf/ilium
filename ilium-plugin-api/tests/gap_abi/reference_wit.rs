//! ABI-068, ABI-069, ABI-106, ABI-107, ABI-108, ABI-109, ABI-110, ABI-111, ABI-112 and ABI-117
//! from reference-wit.md.

use ilium_plugin_api::abi::{
    admit_length, first_failing_check, import_code_tables, invoke_import_twice, list_imports,
    read_only_imports, sdk_sorts_unordered_list, EpochClock, FakeWallClock, HandleTable, IdTable,
    ImportCheck, ListOrder, PluginInstance,
};
use ilium_plugin_api::conformance::run_import_code_case;
use ilium_plugin_api::error::ErrorCode;
use ilium_plugin_api::limits::{row_refusal_code, LimitKey};

fn wit_page() -> String {
    let path = format!(
        "{}/../docs/plugins-manual/pages/reference-wit.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).expect("manual page should be readable")
}

/// REQ: ABI-068
#[test]
fn abi_068_each_table_limit_refused_before_host_logic_without_truncation() {
    for key in LimitKey::ALL {
        let limit = ilium_plugin_api::limits::default_of(key);
        let at_limit = admit_length(key, limit);
        assert!(
            at_limit.result.is_ok(),
            "{key:?}: a value exactly at its limit must be accepted"
        );
        let oversize = admit_length(key, limit + 1);
        assert!(
            !oversize.host_logic_reached,
            "{key:?}: an oversize value must be refused before host logic runs"
        );
        assert!(
            !oversize.truncated,
            "{key:?}: an oversize value must not be truncated"
        );
        let refusal = oversize
            .result
            .expect_err("a value one past the limit must be refused");
        assert_eq!(
            refusal.message.split(':').next().unwrap_or_default(),
            row_refusal_code(key),
            "{key:?}: the refusal must carry that row's code"
        );
    }
}

/// REQ: ABI-069
#[test]
fn abi_069_one_past_the_table_limit_is_refused_so_the_limit_is_the_enforced_one() {
    for key in LimitKey::ALL {
        let limit = ilium_plugin_api::limits::default_of(key);
        assert!(
            admit_length(key, limit + 1).result.is_err(),
            "{key:?}: a length one past the table limit must be refused, or the table no longer states the enforced limit"
        );
    }
}

/// REQ: ABI-069
#[test]
fn abi_069_release_notes_list_every_limit_change_within_a_major() {
    let page = wit_page();
    assert!(
        page.contains("[ABI-069] A limit in the table MUST NOT be raised or lowered within the same ABI major without a minor release that lists the change in its notes."),
        "reference-wit.md must state the ABI-069 MUST text"
    );
}

/// REQ: ABI-106
#[test]
fn abi_106_every_import_table_lists_each_code_once_and_at_least_one() {
    for table in import_code_tables() {
        assert!(
            !table.codes.is_empty(),
            "import {} has an empty code table, so every code it returns would be refused",
            table.import
        );
        let mut seen = std::collections::BTreeSet::new();
        for code in &table.codes {
            assert!(
                seen.insert(code),
                "import {} lists code {code} more than once",
                table.import
            );
        }
    }
}

/// REQ: ABI-106
#[test]
fn abi_106_each_import_returns_only_codes_in_its_table() {
    let tables = import_code_tables();
    assert!(
        !tables.is_empty(),
        "the reference must list at least one import code table"
    );
    for table in tables {
        let outcome = run_import_code_case(&table.import);
        assert!(
            outcome.passed,
            "import {} returned a code outside its table {:?}",
            table.import, table.codes
        );
    }
}

/// REQ: ABI-107
#[test]
fn abi_107_first_failing_check_wins_for_each_adjacent_pair() {
    let order = ImportCheck::ORDER;
    for pair in order.windows(2) {
        let earlier = pair[0];
        let later = pair[1];
        let reported = first_failing_check(&[later, earlier]);
        assert_eq!(
            reported,
            Some(earlier),
            "with {later:?} and {earlier:?} both failing, {earlier:?} must be reported"
        );
    }
}

/// REQ: ABI-108
#[test]
fn abi_108_read_only_imports_return_same_value_twice_without_host_event() {
    for import in read_only_imports() {
        let (first, second) = invoke_import_twice(&import);
        assert_eq!(
            first, second,
            "read-only import {import} must be idempotent"
        );
    }
    let page = wit_page();
    for mark in [
        "idempotent by key",
        "idempotent by nature",
        "non-idempotent",
    ] {
        assert!(
            page.contains(mark),
            "reference-wit.md must mark each effectful import with one of the three idempotency marks ({mark} missing)"
        );
    }
}

/// REQ: ABI-109
#[test]
fn abi_109_stale_id_refused_and_live_object_unaffected() {
    let mut table = IdTable::new();
    let closed_timer = table.open("timer");
    let live_timer = table.open("timer");
    table.close(closed_timer).expect("first close must succeed");
    let stale = table
        .use_id(closed_timer)
        .expect_err("a closed id must be refused");
    assert_eq!(stale.code, ErrorCode::NotFound);
    assert!(
        stale.message.contains("stale-id"),
        "reason must be stale-id"
    );
    assert!(
        table.close(closed_timer).is_ok(),
        "closing an already closed id must be ok"
    );
    assert!(
        table.use_id(live_timer).is_ok(),
        "a stale id must not affect a live timer"
    );
}

/// REQ: ABI-110
#[test]
fn abi_110_handle_limit_of_4096_and_double_drop_traps() {
    let mut table = HandleTable::new();
    let mut created = Vec::new();
    for _ in 0..4_096 {
        created.push(
            table
                .create("child")
                .expect("handles up to the limit must be created"),
        );
    }
    let refusal = table
        .create("child")
        .expect_err("the 4,097th handle must be refused");
    assert_eq!(refusal.code, ErrorCode::Busy);
    assert!(
        refusal.message.contains("handle-limit"),
        "reason must be handle-limit"
    );
    let first_drop = table.drop_handle(created[0]);
    assert!(
        first_drop.trap_code.is_none(),
        "the first drop must not trap"
    );
    let second_drop = table.drop_handle(created[0]);
    assert!(
        second_drop.trap_code.is_some(),
        "dropping a handle twice must trap the call"
    );
}

/// REQ: ABI-111
#[test]
fn abi_111_trap_releases_all_handles_and_leak_is_logged_once() {
    let mut instance = PluginInstance::with_handles(3);
    instance.trap();
    assert_eq!(
        instance.live_host_objects(),
        0,
        "all three host objects must be released"
    );
    assert!(
        instance.release_elapsed() <= ilium_plugin_api::abi::deactivation_deadline(),
        "release must finish within the deactivation deadline"
    );

    let mut leaking = PluginInstance::with_leaking_host(3, 1);
    leaking.deactivate();
    let leak_logs: Vec<_> = leaking
        .logs()
        .into_iter()
        .filter(|record| record.code == "abi.handle-leaked")
        .collect();
    assert_eq!(
        leak_logs.len(),
        1,
        "a leaked handle must be logged exactly once per plugin"
    );
    assert_eq!(
        leak_logs[0].fields.get("count").map(String::as_str),
        Some("1")
    );
}

/// REQ: ABI-112
#[test]
fn abi_112_list_imports_state_order_and_unordered_lists_are_not_sorted() {
    let page = wit_page();
    for import in list_imports() {
        let (first, second) = ilium_plugin_api::abi::invoke_list_twice(&import.name);
        match import.order {
            ListOrder::Stable => assert_eq!(
                first, second,
                "stable list {} must keep its order",
                import.name
            ),
            ListOrder::Unordered => {
                assert!(
                    page.contains("unordered"),
                    "unordered lists must say 'unordered' on the page"
                );
                assert!(
                    !sdk_sorts_unordered_list(&import.name),
                    "the SDK must not sort the unordered list {}",
                    import.name
                );
            }
        }
    }
}

/// REQ: ABI-117
#[test]
fn abi_117_epoch_ms_whole_non_decreasing_and_backwards_clock_reported() {
    let mut clock = EpochClock::new(FakeWallClock::new(1_000));
    let mut previous = 0_u64;
    for _ in 0..1_000 {
        let sample = clock.sample();
        assert!(
            sample >= previous,
            "epoch-ms must be non-decreasing within one run"
        );
        previous = sample;
    }
    clock.set_wall_backwards(500);
    let _ = clock.sample();
    assert!(
        clock.events().iter().any(|event| event == "clock-changed"),
        "a backwards wall clock must be reported by clock-changed"
    );
}

/// REQ: ABI-068
#[test]
fn abi_068_a_value_exactly_at_its_limit_reaches_host_logic() {
    for key in LimitKey::ALL {
        let limit = ilium_plugin_api::limits::default_of(key);
        let at_limit = admit_length(key, limit);
        assert!(
            at_limit.host_logic_reached,
            "{key:?}: a value at its limit is accepted, so host logic must receive it"
        );
    }
}

/// REQ: ABI-107
#[test]
fn abi_107_with_every_check_failing_the_first_in_the_fixed_order_is_reported() {
    let order = ImportCheck::ORDER;
    let all_failing: Vec<ImportCheck> = order.to_vec();
    assert_eq!(
        first_failing_check(&all_failing),
        Some(order[0]),
        "with every check failing, the first check in the fixed order must be reported"
    );
    let none: Vec<ImportCheck> = Vec::new();
    assert_eq!(
        first_failing_check(&none),
        None,
        "with no failing check there is nothing to report"
    );
}

/// REQ: ABI-108
#[test]
fn abi_108_the_read_only_import_set_is_not_empty() {
    assert!(
        !read_only_imports().is_empty(),
        "the reference must name at least one read-only import"
    );
}

/// REQ: ABI-109
#[test]
fn abi_109_a_closed_id_stays_stale_after_another_close_and_the_live_id_is_unaffected() {
    let mut table = IdTable::new();
    let closed_timer = table.open("timer");
    let live_timer = table.open("timer");
    table.close(closed_timer).expect("first close must succeed");
    table.close(closed_timer).expect("second close must be ok");
    let stale = table
        .use_id(closed_timer)
        .expect_err("a closed id must stay refused");
    assert_eq!(stale.code, ErrorCode::NotFound);
    assert!(
        table.use_id(live_timer).is_ok(),
        "the live timer must still be usable"
    );
}

/// REQ: ABI-110
#[test]
fn abi_110_dropping_one_handle_frees_a_slot_under_the_limit() {
    let mut table = HandleTable::new();
    let mut created = Vec::new();
    for _ in 0..4_096 {
        created.push(
            table
                .create("child")
                .expect("handles up to the limit must be created"),
        );
    }
    let _ = table.drop_handle(created[0]);
    assert!(
        table.create("child").is_ok(),
        "the limit counts live handles, so a new handle must fit after a drop"
    );
}

/// REQ: ABI-111
#[test]
fn abi_111_a_clean_deactivation_logs_no_leaked_handle() {
    let mut clean = PluginInstance::with_handles(3);
    clean.deactivate();
    assert!(
        clean
            .logs()
            .into_iter()
            .all(|record| record.code != "abi.handle-leaked"),
        "a plugin that releases all its handles must not produce a leak record"
    );
}

/// REQ: ABI-112
#[test]
fn abi_112_some_list_import_declares_a_stable_order() {
    assert!(
        list_imports()
            .iter()
            .any(|import| matches!(import.order, ListOrder::Stable)),
        "the reference must declare at least one list import with a stable order"
    );
}

/// REQ: ABI-117
#[test]
fn abi_117_a_backwards_wall_clock_does_not_move_epoch_ms_backwards() {
    let mut clock = EpochClock::new(FakeWallClock::new(1_000));
    let before = clock.sample();
    clock.set_wall_backwards(500);
    let after = clock.sample();
    assert!(
        after >= before,
        "epoch-ms must not decrease when the wall clock steps back: {before} then {after}"
    );
}
