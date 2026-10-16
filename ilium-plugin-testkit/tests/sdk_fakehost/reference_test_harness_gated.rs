//! Rows that need a plugin to call a host import or a session to bind a native type to an installed
//! package. Both are missing from the skeleton, so each module is behind a cfg feature that no
//! build enables yet; API_GAPS.md lists what the implementation phase must add and then the
//! features are declared and these modules compile.

/// Plugins that call host imports through `host()` (needs `HostApi: HostFs + HostHttp`).
#[cfg(feature = "plugin_host_calls")]
mod host_calls {
    use std::cell::RefCell;

    use ilium_plugin_sdk_core::host::{HttpRequest, HttpResponse};
    use ilium_plugin_sdk_core::types::{Activation, DeactivationReason, ErrorCode, Event, FsPath, PluginError, Submission};
    use ilium_plugin_sdk_core::{NativeEntry, host};
    use ilium_plugin_testkit::FakeHost;

    use crate::common::{client_batch, with_clean_env};

    thread_local! {
        static READ_RESULT: RefCell<Option<Result<Vec<u8>, PluginError>>> = const { RefCell::new(None) };
        static FETCH_RESULT: RefCell<Option<Result<u64, PluginError>>> = const { RefCell::new(None) };
    }

    #[derive(Default)]
    struct Prober;

    impl NativeEntry for Prober {
        const SIDE: &'static str = "server";
        const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];

        fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }

        fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
            None
        }

        fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
            let path = FsPath { scope: "project".to_string(), path: "a.txt".to_string() };
            let read = host().read(path, 0, 1024);
            READ_RESULT.with(|slot| *slot.borrow_mut() = Some(read));
            let request = HttpRequest {
                method: "GET".to_string(),
                url: "https://other.example.org/x".to_string(),
                headers: Vec::new(),
                body: None,
                timeout_millis: 1_000,
            };
            let fetch = host().fetch(request).map(|id| id.0);
            FETCH_RESULT.with(|slot| *slot.borrow_mut() = Some(fetch));
            Vec::new()
        }
    }

    fn probe(grants: &[&str]) -> (Result<Vec<u8>, PluginError>, Result<u64, PluginError>) {
        let mut host = FakeHost::new().grants(grants);
        host.activate::<Prober>();
        host.fs().write("project:a.txt", b"hi");
        host.http().expect(
            "GET",
            "https://api.example.com/x",
            HttpResponse { status: 200, headers: Vec::new(), body: Vec::new() },
        );
        host.deliver(client_batch(1));
        (
            READ_RESULT.with(|slot| slot.borrow_mut().take()).expect("read ran"),
            FETCH_RESULT.with(|slot| slot.borrow_mut().take()).expect("fetch ran"),
        )
    }

    /// REQ: DEV-455
    #[test]
    fn dev_455_a_read_outside_the_granted_scope_returns_not_permitted() {
        with_clean_env(|| {
            let (read, _) = probe(&["fs.read:session"]);
            assert_eq!(read.expect_err("scope not granted").code, ErrorCode::NotPermitted);
        });
    }

    /// REQ: DEV-455
    #[test]
    fn dev_455_a_read_inside_the_granted_scope_returns_the_bytes() {
        with_clean_env(|| {
            let (read, _) = probe(&["fs.read:project"]);
            assert_eq!(read.expect("scope granted"), b"hi".to_vec());
        });
    }

    /// REQ: DEV-463
    #[test]
    fn dev_463_a_request_to_a_host_outside_the_grant_returns_not_permitted() {
        with_clean_env(|| {
            let (_, fetch) = probe(&["http:api.example.com"]);
            assert_eq!(fetch.expect_err("other.example.org is not granted").code, ErrorCode::NotPermitted);
        });
    }

    /// REQ: DEV-463
    #[test]
    fn dev_463_a_wildcard_pattern_covers_subdomains_only() {
        with_clean_env(|| {
            let (_, fetch) = probe(&["http:*.example.org"]);
            assert_eq!(fetch.as_ref().err().map(|error| format!("{:?}", error.code)), None, "other.example.org matches *.example.org");
            let (_, fetch) = probe(&["http:*.example.com"]);
            assert_eq!(fetch.expect_err("different domain").code, ErrorCode::NotPermitted);
        });
    }
}

/// Sessions that bind a native plugin type to an installed package (needs `FakeSession::install_native`).
#[cfg(feature = "plugin_session")]
mod session_routing {
    use ilium_plugin_sdk_core::events;
    use ilium_plugin_testkit::session::FakeSession;

    use crate::common::{Spy, spy, spy_reset, spy_script, temp_dir, with_clean_env, write_file, manifest_text};

    fn package(label: &str, id: &str, extra: &str) -> std::path::PathBuf {
        let dir = temp_dir(label);
        let text = manifest_text(id, &[], &[]).replacen("\n}\n", &format!(",\n  {extra}\n}}\n"), 1);
        write_file(&dir, "ilium-plugin.json", &text);
        dir
    }

    /// REQ: DEV-480
    #[test]
    fn dev_480_a_service_call_reaches_the_provider_that_services_provide_names() {
        with_clean_env(|| {
            spy_reset();
            let provider = package("s480p", "example.provider", r#""services": { "provide": ["example.provider/status"] }"#);
            let caller = package("s480c", "example.caller", r#""services": { "use": ["example.provider/status"] }"#);
            let mut session = FakeSession::new();
            session.install_native::<Spy>(&provider).install_native::<Spy>(&caller).start();
            let reply = session.call_service("example.caller", "example.provider/status", "*", "{}");
            let reply = reply.expect("the provider named by services.provide answers");
            assert!(
                matches!(reply, ilium_plugin_sdk_core::types::ServiceReply::Done(_) | ilium_plugin_sdk_core::types::ServiceReply::Deferred),
                "reply was {reply:?}"
            );
            assert!(!spy().batches.is_empty(), "the provider handled the call");
            let missing = session.call_service("example.caller", "example.unprovided/status", "*", "{}");
            assert_eq!(missing.expect_err("no provider").code, ilium_plugin_sdk_core::types::ErrorCode::NotFound);
        });
    }

    /// REQ: DEV-481
    #[test]
    fn dev_481_a_message_from_the_server_side_reaches_the_client_side_of_the_same_plugin() {
        with_clean_env(|| {
            spy_reset();
            let dir = package("s481", "example.both", r#""x-note": "two sides""#);
            let mut session = FakeSession::new();
            session.install_native::<Spy>(&dir).start();
            use ilium_plugin_sdk_core::types::{Event, Intent, MessageSpec, MessageTarget, PluginId, PluginSide, Submission};
            spy_script(vec![vec![Submission {
                intent: Intent::Message(MessageSpec {
                    to: MessageTarget { plugin: PluginId("example.both".to_string()), side: PluginSide::Client, client: None },
                    channel: "note".to_string(),
                    payload: b"hello".to_vec(),
                }),
                correlation: 1,
                dedup_key: None,
            }]]);
            session.server("example.both").deliver(vec![events::command("send", &[])]);
            session.pump().expect("settles");
            let received = spy()
                .batches
                .iter()
                .flatten()
                .filter_map(|event| match event {
                    Event::Message(message) => Some(message.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(received.len(), 1, "exactly one message arrives");
            assert_eq!(received[0].channel, "note");
            assert_eq!(received[0].payload, b"hello".to_vec());
            assert_eq!(received[0].from, PluginId("example.both".to_string()));
            assert_eq!(received[0].from_side, PluginSide::Server);
        });
    }

    /// REQ: DEV-482
    #[test]
    fn dev_482_a_hook_point_calls_its_contributors_in_priority_order() {
        with_clean_env(|| {
            spy_reset();
            let first = package("s482a", "example.first", r#""x-priority": 10"#);
            let second = package("s482b", "example.second", r#""x-priority": 20"#);
            let mut session = FakeSession::new();
            session
                .install_core("ilium.core.tree")
                .install_native::<Spy>(&first)
                .install_native::<Spy>(&second)
                .start();
            session.pump().expect("settles");
            let order: Vec<u64> = spy().activations.iter().map(|a| a.instance).collect();
            assert_eq!(order, vec![1, 1]);
        });
    }
}
