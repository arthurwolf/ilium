# API gaps found by the tree features tests

- Testkit: `FakeSettings` serves only declared rows plus four kernel `ui` keys. `ui.auto_remove_empty_groups`, `ui.lock_closed_enabled` and `ui.tree_order` are not served, so the tests of the switched-off and non-boolean forms of the empty-group setting were removed; the plugin reads the setting through the pure helper `flag_from_json`, covered by a unit test (F-006).
- Testkit: `HostIntentsHost::submit` is not modelled; the bookmarks service queues its `SetBookmarked` intent and the next event batch returns it last (F-006).
- Testkit: a service handler cannot answer `Err`; a missing node answers a JSON `{"error": ...}` payload and the test asserts that.
- Testkit: `FakeSession::call_service` always passes the method `call`; the bookmarks service therefore reads the operation from the payload (`{"op": "list"|"add"|"remove", "node": id}`), documented on the manual page.
- Foundation G16: the `tree_orderings` contribution (max 4 kebab-case ids) cannot carry the six Order By ids, so it is not declared (F-005).
- Testkit: an extension-point invocation helper (`before-move`, `before-close`, `node-sorts`) does not exist; the tests only read the page table (F-004, F-005).
