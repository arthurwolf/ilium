//! reference-wit.md section 8: guest exports annotator, token-provider, catalog-provider,
//! virtual-rows-provider and service-provider, driven through ExtWorld::call_export (gap, see
//! API_GAPS.md).

use crate::support::*;

const GUEST: &str = "com.example.exports";

fn export_world() -> ExtWorld {
    let mut world = ExtWorld::new();
    let manifest = json!({
        "id": GUEST,
        "exports": ["annotator", "token-provider", "catalog-provider", "virtual-rows-provider", "service-provider"]
    });
    assert_eq!(
        diag_code(&world.install_on(Tier::T3, Side::Client, manifest, Vec::new())),
        "ok"
    );
    world.settle();
    world
}

fn lines(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("line {index}")).collect()
}

/// REQ: WIT:ilium:exports/annotator.annotate
#[test]
fn wit_annotator_annotate_returns_annotations_inside_the_given_lines() {
    let mut world = export_world();
    let reply = world.call_export(
        GUEST,
        ExportCall::Annotate {
            buffer: BufferId(1),
            revision: 7,
            first_line: 10,
            lines: lines(3),
        },
    );
    match reply {
        Ok(ExportReply::Annotations(list)) => {
            assert!(list.iter().all(|item| item.line >= 10
                && item.line < 13
                && item.first_column <= item.last_column));
        }
        other => panic!("expected annotations, got {other:?}"),
    }
}

/// REQ: WIT:ilium:exports/annotator.annotate
#[test]
fn wit_annotator_annotate_with_no_lines_returns_an_empty_list() {
    let mut world = export_world();
    let reply = world.call_export(
        GUEST,
        ExportCall::Annotate {
            buffer: BufferId(1),
            revision: 0,
            first_line: 0,
            lines: Vec::new(),
        },
    );
    assert!(matches!(reply, Ok(ExportReply::Annotations(ref list)) if list.is_empty()));
}

/// REQ: WIT:ilium:exports/token-provider.tokens
#[test]
fn wit_token_provider_tokens_stay_inside_their_lines_and_return_a_state() {
    let mut world = export_world();
    let reply = world.call_export(
        GUEST,
        ExportCall::Tokens {
            buffer: BufferId(1),
            first_line: 0,
            lines: lines(4),
            state_in: None,
        },
    );
    match reply {
        Ok(ExportReply::Tokens(tokens, _state)) => {
            assert!(tokens
                .iter()
                .all(|token| (token.line as usize) < 4 && token.length > 0));
        }
        other => panic!("expected tokens, got {other:?}"),
    }
}

/// REQ: WIT:ilium:exports/token-provider.tokens
#[test]
fn wit_token_provider_tokens_accept_the_state_of_the_previous_call() {
    let mut world = export_world();
    let first = world.call_export(
        GUEST,
        ExportCall::Tokens {
            buffer: BufferId(1),
            first_line: 0,
            lines: lines(2),
            state_in: None,
        },
    );
    let state = match first {
        Ok(ExportReply::Tokens(_, state)) => state,
        other => panic!("expected tokens, got {other:?}"),
    };
    let second = world.call_export(
        GUEST,
        ExportCall::Tokens {
            buffer: BufferId(1),
            first_line: 2,
            lines: lines(2),
            state_in: state,
        },
    );
    assert!(matches!(second, Ok(ExportReply::Tokens(_, _))));
}

/// REQ: WIT:ilium:exports/catalog-provider.query-catalog
#[test]
fn wit_catalog_provider_query_catalog_returns_a_page() {
    let mut world = export_world();
    let reply = world.call_export(
        GUEST,
        ExportCall::QueryCatalog(CatalogQuery {
            catalog: "themes".to_owned(),
            filter: String::new(),
            cursor: None,
            limit: 20,
        }),
    );
    assert!(matches!(reply, Ok(ExportReply::CatalogPage(Ok(_)))));
}

/// REQ: WIT:ilium:exports/catalog-provider.query-catalog
#[test]
fn wit_catalog_provider_query_catalog_error_return_is_a_plugin_error_not_a_trap() {
    let mut world = export_world();
    world.script_export(
        GUEST,
        "query-catalog",
        HandlerScript {
            elapsed_ms: 1,
            action: HandlerAction::ErrorReturn,
        },
    );
    let reply = world.call_export(
        GUEST,
        ExportCall::QueryCatalog(CatalogQuery {
            catalog: "themes".to_owned(),
            filter: "x".to_owned(),
            cursor: None,
            limit: 20,
        }),
    );
    assert!(
        matches!(reply, Ok(ExportReply::CatalogPage(Err(_)))),
        "{reply:?}"
    );
}

/// REQ: WIT:ilium:exports/virtual-rows-provider.list-virtual-children
#[test]
fn wit_virtual_rows_provider_returns_a_page_and_a_cursor_that_ends_with_none() {
    let mut world = export_world();
    let mut cursor: Option<String> = None;
    for _ in 0..50 {
        let reply = world.call_export(
            GUEST,
            ExportCall::ListVirtualChildren {
                row_kind: "files".to_owned(),
                parent: "root".to_owned(),
                cursor: cursor.clone(),
            },
        );
        match reply {
            Ok(ExportReply::VirtualChildren(Ok((_, next)))) => {
                if next.is_none() {
                    return;
                }
                cursor = next;
            }
            other => {
                panic!("expected a page, got {other:?}");
            }
        }
    }
    panic!("the cursor never ended within 50 pages");
}

/// REQ: WIT:ilium:exports/virtual-rows-provider.list-virtual-children
#[test]
fn wit_virtual_rows_provider_error_return_is_a_plugin_error() {
    let mut world = export_world();
    world.script_export(
        GUEST,
        "list-virtual-children",
        HandlerScript {
            elapsed_ms: 1,
            action: HandlerAction::ErrorReturn,
        },
    );
    let reply = world.call_export(
        GUEST,
        ExportCall::ListVirtualChildren {
            row_kind: "files".to_owned(),
            parent: "x".to_owned(),
            cursor: None,
        },
    );
    assert!(
        matches!(reply, Ok(ExportReply::VirtualChildren(Err(_)))),
        "{reply:?}"
    );
}

/// REQ: WIT:ilium:exports/service-provider.handle
#[test]
fn wit_service_provider_handle_returns_done_with_a_json_string() {
    let mut world = export_world();
    let call = ServiceCall {
        call: 1,
        service: SERVICE.to_owned(),
        method: "echo".to_owned(),
        payload_json: r#"{"text":"hi"}"#.to_owned(),
        caller: ilium_plugin_api::generated::types::PluginId("com.example.caller".to_owned()),
    };
    match world.call_export(GUEST, ExportCall::ServiceHandle(call)) {
        Ok(ExportReply::ServiceReply(ServiceReply::Done(text))) => {
            assert!(
                serde_json::from_str::<serde_json::Value>(&text).is_ok(),
                "{text}"
            );
        }
        other => panic!("expected done, got {other:?}"),
    }
}

/// REQ: WIT:ilium:exports/service-provider.handle
#[test]
fn wit_service_provider_handle_may_defer_and_answer_later() {
    let mut world = export_world();
    world.script_export(
        GUEST,
        "handle",
        HandlerScript {
            elapsed_ms: 1,
            action: HandlerAction::ServiceDeferred,
        },
    );
    let call = ServiceCall {
        call: 2,
        service: SERVICE.to_owned(),
        method: "echo".to_owned(),
        payload_json: "{}".to_owned(),
        caller: ilium_plugin_api::generated::types::PluginId("com.example.caller".to_owned()),
    };
    assert!(matches!(
        world.call_export(GUEST, ExportCall::ServiceHandle(call)),
        Ok(ExportReply::ServiceReply(ServiceReply::Deferred))
    ));
}

/// REQ: WIT:ilium:exports/hook-handler.handle-hook
#[test]
fn wit_hook_handler_error_return_is_distinct_from_a_trap() {
    let mut world = export_world();
    world.script_export(
        GUEST,
        "handle-hook",
        HandlerScript {
            elapsed_ms: 1,
            action: HandlerAction::ErrorReturn,
        },
    );
    let call = HookCall {
        call: 3,
        point: POINT.to_owned(),
        payload_json: "{}".to_owned(),
    };
    assert!(matches!(
        world.call_export(GUEST, ExportCall::Hook(call)),
        Ok(ExportReply::HookResult(Err(_)))
    ));
}

/// REQ: WIT:ilium:exports/menu-provider.menu-items
#[test]
fn wit_menu_provider_menu_items_returns_a_list_of_entries() {
    let mut world = export_world();
    let reply = world.call_export(
        GUEST,
        ExportCall::MenuItems(MenuContext {
            menu: "tree.row".to_owned(),
            ..MenuContext::default()
        }),
    );
    assert!(matches!(reply, Ok(ExportReply::MenuEntries(ref list)) if list.len() <= 64));
}

/// REQ: WIT:ilium:exports/hook-handler.handle-hook
#[test]
fn doc_wit_hook_handler_variant_has_pass_replace_refuse_and_answer() {
    let text = page("reference-wit.md");
    assert!(text
        .contains("variant hook-reply { pass, replace(string), refuse(string), answer(string) }"));
}
