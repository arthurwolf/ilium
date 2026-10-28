//! api-messages-and-topics.md: topic publishing and subscribing, peer messages and queue
//! back-pressure (SVC-020..030).

use crate::support::*;

const PUB: &str = "com.example.pub";
const SUB: &str = "com.example.sub";
const TOPIC: &str = "com.example.pub/saved";

fn publisher_manifest(topics: Vec<&str>) -> Value {
    json!({"id": PUB, "contributes": {"topics": {"publishes": topics}}})
}

fn subscriber_manifest(id: &str, patterns: Vec<&str>, grant: bool) -> Value {
    let grants: Vec<String> = if grant {
        patterns
            .iter()
            .map(|pattern| format!("topics.subscribe:{pattern}"))
            .collect()
    } else {
        Vec::new()
    };
    json!({
        "id": id,
        "capabilities": {"server": grants},
        "contributes": {"topics": {"subscribes": patterns}}
    })
}

fn topic_world() -> ExtWorld {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            publisher_manifest(vec![TOPIC, "com.example.pub/other"]),
            Vec::new()
        )),
        "ok"
    );
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            subscriber_manifest(SUB, vec![TOPIC], true),
            Vec::new()
        )),
        "ok"
    );
    world.settle();
    world
}

fn received(world: &ExtWorld, plugin: &str) -> Vec<(String, String)> {
    world.topics_received(plugin)
}

// ---------------------------------------------------------------------------------------------
// SVC-020 .. SVC-023
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-020, WIT:ilium:host/topics.publish
#[test]
fn svc_020_publish_with_another_prefix_returns_not_permitted() {
    let mut world = topic_world();
    let error = err_of(world.publish(PUB, "other.x/y", "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
}

/// REQ: SVC-020
#[test]
fn svc_020_publish_with_own_prefix_but_no_slash_returns_not_permitted() {
    let mut world = topic_world();
    let error = err_of(world.publish(PUB, "com.example.pubx", "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
}

/// REQ: SVC-020
#[test]
fn svc_020_publish_with_a_longer_plugin_id_sharing_the_prefix_returns_not_permitted() {
    let mut world = topic_world();
    let error = err_of(world.publish(PUB, "com.example.pub2/saved", "{}"));
    assert_eq!(code_of(&error), ErrorCode::NotPermitted);
}

/// REQ: SVC-020
#[test]
fn svc_020_publish_of_a_declared_own_topic_is_accepted() {
    let mut world = topic_world();
    assert!(world.publish(PUB, TOPIC, "{}").is_ok());
}

/// REQ: SVC-021
#[test]
fn svc_021_publish_of_an_undeclared_own_topic_returns_invalid_argument() {
    let mut world = topic_world();
    let error = err_of(world.publish(PUB, "com.example.pub/undeclared", "{}"));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
}

/// REQ: SVC-022
#[test]
fn svc_022_payload_of_65_kib_returns_invalid_argument() {
    let mut world = topic_world();
    let payload = format!(r#"{{"a":"{}"}}"#, "x".repeat(65 * 1024));
    let error = err_of(world.publish(PUB, TOPIC, &payload));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
}

/// REQ: SVC-022
#[test]
fn svc_022_payload_of_exactly_64_kib_is_accepted() {
    let mut world = topic_world();
    let overhead = r#"{"a":""}"#.len();
    let payload = format!(r#"{{"a":"{}"}}"#, "x".repeat(KIB64 - overhead));
    assert_eq!(payload.len(), KIB64);
    assert!(world.publish(PUB, TOPIC, &payload).is_ok());
}

/// REQ: SVC-022
#[test]
fn svc_022_payload_of_64_kib_plus_one_byte_returns_invalid_argument() {
    let mut world = topic_world();
    let overhead = r#"{"a":""}"#.len();
    let payload = format!(r#"{{"a":"{}"}}"#, "x".repeat(KIB64 - overhead + 1));
    let error = err_of(world.publish(PUB, TOPIC, &payload));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
}

/// REQ: SVC-022
#[test]
fn svc_022_payload_that_is_not_json_returns_invalid_argument() {
    let mut world = topic_world();
    let error = err_of(world.publish(PUB, TOPIC, "{not json"));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
}

/// REQ: SVC-022
#[test]
fn svc_022_topic_name_of_128_characters_is_accepted_and_129_is_invalid() {
    let prefix = format!("{PUB}/");
    let name_128 = format!("{prefix}{}", "n".repeat(128 - prefix.len()));
    let name_129 = format!("{name_128}n");
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            publisher_manifest(vec![&name_128, &name_129]),
            Vec::new()
        )),
        "ok"
    );
    assert!(world.publish(PUB, &name_128, "{}").is_ok());
    let error = err_of(world.publish(PUB, &name_129, "{}"));
    assert_eq!(code_of(&error), ErrorCode::InvalidArgument);
}

/// REQ: SVC-023
#[test]
fn svc_023_one_hundred_publishes_in_one_second_are_accepted() {
    let mut world = topic_world();
    for index in 0..100 {
        assert!(world.publish(PUB, TOPIC, "{}").is_ok(), "publish {index}");
    }
}

/// REQ: SVC-023
#[test]
fn svc_023_publish_101_in_one_second_returns_rate_limited_with_retry_after() {
    let mut world = topic_world();
    for _ in 0..100 {
        let _ = world.publish(PUB, TOPIC, "{}");
    }
    let error = err_of(world.publish(PUB, TOPIC, "{}"));
    assert_eq!(code_of(&error), ErrorCode::RateLimited);
    assert!(error.retry_after_ms.is_some());
}

/// REQ: SVC-023
#[test]
fn svc_023_publishing_works_again_in_the_next_second() {
    let mut world = topic_world();
    for _ in 0..101 {
        let _ = world.publish(PUB, TOPIC, "{}");
    }
    world.advance_ms(1001);
    assert!(world.publish(PUB, TOPIC, "{}").is_ok());
}

/// REQ: SVC-023
#[test]
fn svc_023_limit_is_per_plugin() {
    let mut world = topic_world();
    let second = json!({"id": "com.example.pub2", "contributes": {"topics": {"publishes": ["com.example.pub2/t"]}}});
    assert_eq!(
        diag_code(&world.install(Tier::T3, second, Vec::new())),
        "ok"
    );
    for _ in 0..101 {
        let _ = world.publish(PUB, TOPIC, "{}");
    }
    assert!(world
        .publish("com.example.pub2", "com.example.pub2/t", "{}")
        .is_ok());
}

// ---------------------------------------------------------------------------------------------
// SVC-024 .. SVC-025
// ---------------------------------------------------------------------------------------------

fn install_subscriber(pattern: &str) -> Result<(), Diagnostic> {
    let mut world = ExtWorld::new();
    world.install(
        Tier::T3,
        subscriber_manifest(SUB, vec![pattern], true),
        Vec::new(),
    )
}

/// REQ: SVC-024
#[test]
fn svc_024_exact_topic_pattern_installs() {
    assert_eq!(
        diag_code(&install_subscriber("ilium.progress/finished")),
        "ok"
    );
}

/// REQ: SVC-024
#[test]
fn svc_024_prefix_slash_star_pattern_installs() {
    assert_eq!(diag_code(&install_subscriber("ilium.progress/*")), "ok");
}

/// REQ: SVC-024
#[test]
fn svc_024_bare_star_pattern_installs() {
    assert_eq!(diag_code(&install_subscriber("*")), "ok");
}

/// REQ: SVC-024
#[test]
fn svc_024_star_inside_a_word_fails_and_names_the_pattern() {
    let result = install_subscriber("a*b");
    match result {
        Err(diagnostic) => assert!(diagnostic.message.contains("a*b"), "{}", diagnostic.message),
        Ok(()) => panic!("a*b must be refused"),
    }
}

/// REQ: SVC-024
#[test]
fn svc_024_star_in_the_middle_of_a_path_fails() {
    // manual does not pin the manifest code, only "a manifest error naming the pattern"
    for pattern in ["ilium.*/finished", "*/finished", "ilium.progress/f*"] {
        match install_subscriber(pattern) {
            Err(diagnostic) => {
                assert!(
                    diagnostic.code.as_str().starts_with("manifest."),
                    "{pattern}: {}",
                    diagnostic.code.as_str()
                );
                assert!(
                    diagnostic.message.contains(pattern),
                    "{}",
                    diagnostic.message
                );
            }
            Ok(()) => panic!("{pattern} must be refused"),
        }
    }
}

proptest! {
    // REQ: SVC-024
    #[test]
    fn svc_024_pattern_grammar_accepts_exactly_exact_prefix_star_and_bare_star(
        head in "[a-z][a-z0-9]{0,6}", tail in "[a-z][a-z0-9]{0,6}", star_position in 0usize..4
    ) {
        let topic = format!("{head}.x/{tail}");
        let (pattern, valid) = match star_position {
            0 => (topic.clone(), true),
            1 => (format!("{head}.x/*"), true),
            2 => (format!("{head}.x/{tail}*"), false),
            _ => (format!("{head}*.x/{tail}"), false),
        };
        prop_assert_eq!(install_subscriber(&pattern).is_ok(), valid);
    }
}

/// REQ: SVC-025
#[test]
fn svc_025_subscriber_without_the_grant_receives_nothing_and_no_error() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(Tier::T3, publisher_manifest(vec![TOPIC]), Vec::new())),
        "ok"
    );
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            subscriber_manifest(SUB, vec![TOPIC], false),
            Vec::new()
        )),
        "ok"
    );
    assert!(world.publish(PUB, TOPIC, r#"{"a":1}"#).is_ok());
    world.settle();
    assert!(received(&world, SUB).is_empty());
    assert!(world.all_logs().iter().all(|line| line.level != "error"));
}

/// REQ: SVC-025
#[test]
fn svc_025_subscriber_with_the_grant_receives_the_event() {
    let mut world = topic_world();
    assert!(world.publish(PUB, TOPIC, r#"{"a":1}"#).is_ok());
    world.settle();
    assert_eq!(
        received(&world, SUB),
        vec![(TOPIC.to_owned(), r#"{"a":1}"#.to_owned())]
    );
}

/// REQ: SVC-025
#[test]
fn svc_025_prefix_pattern_matches_topics_at_any_depth() {
    let mut world = ExtWorld::new();
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            publisher_manifest(vec!["com.example.pub/a/b/c"]),
            Vec::new()
        )),
        "ok"
    );
    assert_eq!(
        diag_code(&world.install(
            Tier::T3,
            subscriber_manifest(SUB, vec!["com.example.pub/*"], true),
            Vec::new()
        )),
        "ok"
    );
    assert!(world.publish(PUB, "com.example.pub/a/b/c", "{}").is_ok());
    world.settle();
    assert_eq!(received(&world, SUB).len(), 1);
}

/// REQ: SVC-025
#[test]
fn svc_025_exact_pattern_does_not_match_another_topic() {
    let mut world = topic_world();
    assert!(world.publish(PUB, "com.example.pub/other", "{}").is_ok());
    world.settle();
    assert!(received(&world, SUB).is_empty());
}

/// REQ: SVC-025
#[test]
fn svc_025_publisher_does_not_receive_its_own_topic_without_subscribing() {
    let mut world = topic_world();
    assert!(world.publish(PUB, TOPIC, "{}").is_ok());
    world.settle();
    assert!(received(&world, PUB).is_empty());
}

// ---------------------------------------------------------------------------------------------
// SVC-026 queues
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-026
#[test]
fn svc_026_full_topic_queue_drops_the_oldest_and_counts_it() {
    let mut world = topic_world();
    let capacity = world.queue_capacity(SUB);
    world.block_delivery(SUB, true);
    for index in 0..(capacity + 5) {
        world.advance_ms(11);
        let _ = world.publish(PUB, TOPIC, &format!(r#"{{"i":{index}}}"#));
    }
    world.block_delivery(SUB, false);
    world.settle();
    let got = received(&world, SUB);
    assert_eq!(got.len(), capacity, "the queue holds exactly its capacity");
    assert_eq!(
        got[0].1, r#"{"i":5}"#,
        "the five oldest events were dropped"
    );
    assert_eq!(world.events_dropped(SUB).iter().sum::<u32>(), 5);
}

/// REQ: SVC-026
#[test]
fn svc_026_next_batch_after_a_drop_starts_with_events_dropped() {
    let mut world = topic_world();
    let capacity = world.queue_capacity(SUB);
    world.block_delivery(SUB, true);
    for index in 0..(capacity + 1) {
        world.advance_ms(11);
        let _ = world.publish(PUB, TOPIC, &format!(r#"{{"i":{index}}}"#));
    }
    world.block_delivery(SUB, false);
    world.settle();
    assert_eq!(
        world.first_event_of_next_batch(SUB),
        Some("events-dropped".to_owned())
    );
}

/// REQ: SVC-026
#[test]
fn svc_026_message_queue_never_drops_an_accepted_message() {
    let mut world = message_world(true);
    world.block_delivery(RECV, true);
    let mut accepted_payloads: Vec<Vec<u8>> = Vec::new();
    for index in 0..(world.queue_capacity(RECV) + 4) {
        let id = world.send_message(SENDER, RECV, "chan", format!("m{index}").as_bytes());
        world.settle();
        if let Ok(id) = id {
            if matches!(world.outcome(SENDER, id), Some(Outcome::Ok)) {
                accepted_payloads.push(format!("m{index}").into_bytes());
            }
        }
    }
    world.block_delivery(RECV, false);
    world.settle();
    assert!(
        !accepted_payloads.is_empty(),
        "at least one send is accepted"
    );
    // sends beyond capacity complete refused(busy); the receiver is not restarted (SVC-030)
    let delivered: Vec<Vec<u8>> = world
        .messages_received(RECV)
        .into_iter()
        .map(|(_, _, bytes)| bytes)
        .collect();
    assert_eq!(
        delivered, accepted_payloads,
        "every accepted message arrives, in order"
    );
}

// ---------------------------------------------------------------------------------------------
// SVC-027 .. SVC-030 messages
// ---------------------------------------------------------------------------------------------

const SENDER: &str = "com.example.sender";
const RECV: &str = "com.example.recv";

fn message_world(grant: bool) -> ExtWorld {
    let mut world = ExtWorld::new();
    let grants: Vec<String> = if grant {
        vec![format!("messages.peer:{RECV}")]
    } else {
        Vec::new()
    };
    let sender = json!({"id": SENDER, "capabilities": {"server": grants}});
    assert_eq!(
        diag_code(&world.install(Tier::T3, sender, Vec::new())),
        "ok"
    );
    let receiver = json!({"id": RECV, "accepts_messages_from": [SENDER]});
    assert_eq!(
        diag_code(&world.install(Tier::T3, receiver, Vec::new())),
        "ok"
    );
    world.settle();
    world
}

fn message_outcome(world: &mut ExtWorld, channel: &str, payload: &[u8]) -> Option<Outcome> {
    let id = world.send_message(SENDER, RECV, channel, payload);
    world.settle();
    match id {
        Ok(id) => world.outcome(SENDER, id),
        Err(error) => panic!("message was refused synchronously: {error:?}"),
    }
}

fn refused_code(outcome: &Option<Outcome>) -> ErrorCode {
    code_of(&refused_error(outcome))
}

/// REQ: SVC-027
#[test]
fn svc_027_message_without_messages_peer_completes_refused_not_permitted() {
    let mut world = message_world(false);
    let outcome = message_outcome(&mut world, "chan", b"x");
    assert_eq!(refused_code(&outcome), ErrorCode::NotPermitted);
}

/// REQ: SVC-027
#[test]
fn svc_027_message_with_the_grant_completes_ok() {
    let mut world = message_world(true);
    let outcome = message_outcome(&mut world, "chan", b"x");
    assert!(matches!(outcome, Some(Outcome::Ok)), "{outcome:?}");
}

/// REQ: SVC-027
#[test]
fn svc_027_message_to_a_disabled_plugin_completes_refused_not_found() {
    let mut world = message_world(true);
    world.set_enabled(RECV, false);
    let outcome = message_outcome(&mut world, "chan", b"x");
    assert_eq!(refused_code(&outcome), ErrorCode::NotFound);
}

/// REQ: SVC-027
#[test]
fn svc_027_message_to_a_plugin_that_is_not_installed_completes_refused_not_found() {
    let mut world = message_world(true);
    world.uninstall(RECV);
    let outcome = message_outcome(&mut world, "chan", b"x");
    assert_eq!(refused_code(&outcome), ErrorCode::NotFound);
}

/// REQ: SVC-028
#[test]
fn svc_028_channel_with_a_space_and_capitals_completes_refused_invalid_argument() {
    let mut world = message_world(true);
    let outcome = message_outcome(&mut world, "Bad Name", b"x");
    assert_eq!(refused_code(&outcome), ErrorCode::InvalidArgument);
}

/// REQ: SVC-028
#[test]
fn svc_028_channel_of_64_characters_is_accepted_and_65_is_refused() {
    let mut world = message_world(true);
    let ok = message_outcome(&mut world, &"a".repeat(64), b"x");
    assert!(matches!(ok, Some(Outcome::Ok)), "{ok:?}");
    let refused = message_outcome(&mut world, &"a".repeat(65), b"x");
    assert_eq!(refused_code(&refused), ErrorCode::InvalidArgument);
}

/// REQ: SVC-028
#[test]
fn svc_028_empty_channel_is_refused() {
    let mut world = message_world(true);
    let outcome = message_outcome(&mut world, "", b"x");
    assert_eq!(refused_code(&outcome), ErrorCode::InvalidArgument);
}

/// REQ: SVC-028
#[test]
fn svc_028_channel_with_dots_dashes_and_digits_is_accepted() {
    let mut world = message_world(true);
    let outcome = message_outcome(&mut world, "a.b-c.0", b"x");
    assert!(matches!(outcome, Some(Outcome::Ok)));
}

/// REQ: SVC-028
#[test]
fn svc_028_payload_of_1_mib_is_accepted_and_one_byte_more_is_refused() {
    let mut world = message_world(true);
    let ok = message_outcome(&mut world, "chan", &vec![7u8; MIB1]);
    assert!(matches!(ok, Some(Outcome::Ok)), "{ok:?}");
    let refused = message_outcome(&mut world, "chan", &vec![7u8; MIB1 + 1]);
    assert_eq!(refused_code(&refused), ErrorCode::InvalidArgument);
}

proptest! {
    // REQ: SVC-028
    #[test]
    fn svc_028_channel_grammar_is_a_to_z_0_to_9_dot_dash_up_to_64(channel in "[a-zA-Z0-9. _-]{0,70}") {
        let valid = !channel.is_empty()
            && channel.len() <= 64
            && channel.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '.' || ch == '-');
        let mut world = message_world(true);
        let outcome = message_outcome(&mut world, &channel, b"x");
        prop_assert_eq!(matches!(outcome, Some(Outcome::Ok)), valid);
    }
}

/// REQ: SVC-029
#[test]
fn svc_029_message_to_a_dormant_plugin_activates_it_then_delivers_after_activate() {
    let mut world = message_world(true);
    world.deactivate(RECV);
    assert!(!world.plugin_active(RECV));
    let outcome = message_outcome(&mut world, "chan", b"hello");
    assert!(matches!(outcome, Some(Outcome::Ok)), "{outcome:?}");
    assert!(world.plugin_active(RECV));
    assert_eq!(
        world
            .event_order(RECV)
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>(),
        vec!["activate", "message"]
    );
    assert_eq!(world.messages_received(RECV).len(), 1);
}

/// REQ: SVC-030, SVC-026
#[test]
fn svc_030_full_receiver_queue_completes_refused_busy_and_queues_nothing() {
    let mut world = message_world(true);
    world.fill_queue_with_never_dropped_events(RECV);
    let queued_before = world.queue_len(RECV);
    let outcome = message_outcome(&mut world, "chan", b"x");
    assert_eq!(refused_code(&outcome), ErrorCode::Busy);
    assert_eq!(world.queue_len(RECV), queued_before);
}

/// REQ: SVC-030, SVC-026
#[test]
fn svc_030_receiver_is_not_restarted_by_a_refused_message() {
    let mut world = message_world(true);
    world.fill_queue_with_never_dropped_events(RECV);
    let restarts_before = world.restart_count(RECV);
    let _ = message_outcome(&mut world, "chan", b"x");
    assert_eq!(world.restart_count(RECV), restarts_before);
    assert!(world.plugin_active(RECV));
}

/// REQ: SVC-030
#[test]
fn svc_030_a_message_accepted_earlier_is_still_delivered() {
    let mut world = message_world(true);
    world.block_delivery(RECV, true);
    let first = message_outcome(&mut world, "chan", b"first");
    assert!(matches!(first, Some(Outcome::Ok)));
    world.fill_queue_with_never_dropped_events(RECV);
    let second = message_outcome(&mut world, "chan", b"second");
    assert_eq!(refused_code(&second), ErrorCode::Busy);
    world.block_delivery(RECV, false);
    world.settle();
    let payloads: Vec<Vec<u8>> = world
        .messages_received(RECV)
        .into_iter()
        .map(|(_, _, payload)| payload)
        .collect();
    assert!(payloads.contains(&b"first".to_vec()));
    assert!(!payloads.contains(&b"second".to_vec()));
}

/// REQ: SVC-030
#[test]
fn svc_030_queue_with_a_drop_oldest_event_makes_room_for_a_message() {
    let mut world = message_world(true);
    world.fill_queue_with_drop_oldest_events(RECV);
    let outcome = message_outcome(&mut world, "chan", b"x");
    assert!(matches!(outcome, Some(Outcome::Ok)), "{outcome:?}");
}

/// REQ: SVC-026
#[test]
fn svc_026_never_dropped_event_other_than_message_on_a_full_queue_treats_the_plugin_as_stuck() {
    let mut world = message_world(true);
    world.fill_queue_with_never_dropped_events(RECV);
    let restarts_before = world.restart_count(RECV);
    world.deliver_never_dropped_event(RECV);
    world.settle();
    assert_eq!(world.restart_count(RECV), restarts_before + 1);
}

// ---------------------------------------------------------------------------------------------
// Doc test
// ---------------------------------------------------------------------------------------------

/// REQ: SVC-026, SVC-030
#[test]
fn doc_page_resolves_the_message_queue_rule_to_refused_busy_without_restart() {
    let text = page("api-messages-and-topics.md");
    assert!(text.contains("completes `refused(busy)`"));
    assert!(text.contains("The receiver is not restarted"));
}

/// REQ: SVC-029
#[test]
fn svc_029_second_message_to_the_woken_plugin_is_delivered_without_reactivating_it() {
    let mut world = message_world(true);
    world.deactivate(RECV);
    let _ = message_outcome(&mut world, "chan", b"first");
    let _ = message_outcome(&mut world, "chan", b"second");
    let activations = world
        .event_order(RECV)
        .iter()
        .filter(|event| event.to_string() == "activate")
        .count();
    assert_eq!(activations, 1);
    assert_eq!(world.messages_received(RECV).len(), 2);
}

/// REQ: WIT:ilium:host/topics.publish
#[test]
fn publish_to_the_second_declared_topic_is_accepted() {
    let mut world = topic_world();
    assert!(world.publish(PUB, "com.example.pub/other", "{}").is_ok());
}
