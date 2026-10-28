# Testing plugins

## Testing plugins

This page is the how-to. The exact contract of the harness, with every method, error code and limit, is [Test harness reference](/pages/reference-test-harness/). If this page and the reference differ, the reference wins.

A plugin is tested against the same contract that the host runs it under. You write the test in Rust or in TypeScript. The test creates a fake host, grants the capabilities the plugin asks for, delivers events and checks the submissions that come back. For a view, the test renders the view into a grid of cells and compares the text. None of this needs a terminal, a window, a running Ilium or a real agent, so a test runs in milliseconds.

Ilium's conformance suite uses fixtures of this kind. Each WIT interface has its own corpus, and the Rust and the TypeScript guest of the suite must both pass a fixture before the interface is released. The fixtures ship with Ilium's test suite, not with user plugins. You can read them as examples and write fixtures for your own plugin in the same shape.

## You will need

- A Rust plugin with the `testing` feature of `ilium-plugin-sdk`, or a TypeScript plugin with `@ilium/plugin-sdk`.
- The capabilities of the plugin in its manifest. The test grants the same ones with `grant(...)`. A capability that the test does not grant is declined, as it is when the user says no.
- No WebAssembly target. The `testing` feature replaces the host calls with the fake host, so `cargo test` links the plugin code for the host target.
- Knowledge of [events and intents](/pages/events-and-intents/) and of [the UI model](/pages/ui-model/).

| Task | Rust | TypeScript |
|---|---|---|
| Harness | `ilium_plugin_sdk::testing::FakeHost` | `createFakeHost()` from `@ilium/plugin-sdk/testing` |
| Runner | `cargo test` | `bun test` against the source |
| Grant a capability | `.grant("tree.read")` | `.grant('tree.read')` |
| Start the plugin | `host.activate::<MyPlugin>()` | `host.activate({ lifecycle, handler })` |
| Deliver events | `host.deliver(events)` returns the submissions | `host.deliver(events)` |
| Render a view | `host.render(view, width, height)` | `host.render(view, width, height)` |

## Fixture files

A fixture is a JSON file with a `schema` key and the state keys that the test needs. An unknown key fails with `fixture.unknown-key`. It describes the state that the plugin reads and the events that the test delivers.

```json
{
  "schema": 1,
  "tree": [],
  "panes": [],
  "agents": [],
  "settings": {},
  "events": [
    {
      "tag": "agent-activity-changed",
      "val": { "node": 42, "previous": "idle", "current": "working", "revision": 1 }
    }
  ]
}
```

| Key | Holds |
|---|---|
| `schema` | Always 1 |
| `tree` | Node views that the tree returns to the plugin |
| `panes` | Pane information for terminal and basic panes |
| `agents` | Agent panes and their current activity |
| `settings` | Current values of the plugin's settings |
| `events` | Events that the test delivers, each written as `{"tag", "val"}` |

The full key list, with `grants`, `tier`, `side`, `clock`, `storage`, `files`, `http`, `processes`, `completions`, `lifecycle` and `expect_state`, is in [Test harness reference](/pages/reference-test-harness/#fixtures-and-expectation-files). `with_fixture(path)` loads the state keys into the fake host. `fixture_events()` returns the decoded `events` list, so the test can pass it to `deliver`. A fixture holds inputs only. The expected submissions and outcomes are asserted in test code, where the test names them.

## The fake host

A fake host runs one plugin instance. It keeps the grants, the limits and the error codes of the real host. Its main methods are:

| Method | Returns | Meaning |
|---|---|---|
| `new()` | `FakeHost` | A host with no grants |
| `grant(name)` | `FakeHost` | Grants one capability, for example `tree.decorate` |
| `with_fixture(path)` | `FakeHost` | Loads the state and events of a fixture file |
| `activate::<P>()` | `PluginHandle` | Runs the activation of a Rust plugin type that is `Default`. Panics if activation fails. `try_activate` returns a `Result`. |
| `deliver(events)` | `Vec<Submission>` | Runs `on-events` once for the batch |
| `render(view, width, height)` | `Screen` | Runs `views.render` and draws the tree into cells |
| `complete(request_id, outcome)` | none | Completes a request that the plugin started |
| `settle()` | none | Delivers queued `request-completed` events until the queues are empty |
| `grant_now(name)`, `revoke(name)` | none | Change a grant after activation |
| `fixture_events()` | `Vec<Event>` | The events of the loaded fixture |
| `outcomes()` | `Vec<Outcome>` | Completions of the submissions, in submission order |
| `pending_requests()` | `Vec<RequestId>` | Requests the plugin started and the test has not completed |
| `logs()` | `Vec<LogLine>` | Lines that the plugin wrote to its log |

In TypeScript the same methods use camelCase names, and `activateModule(path)` imports a fresh copy of the plugin module for each test. Outcomes are queued as `request-completed` events, and `settle()` delivers them. `FakeHost::for_manifest(path)` takes the grants, side and tier from the manifest. `Screen` has `row(index)`, which returns the text of one row, and `cell(row, column)`, which returns the symbol and the style of one cell.

## Worked example: a handler test

The plugin below sets a badge on an agent's node when the agent starts working. It is the server side of the Rust SDK described on [the Rust SDK page](/pages/rust-sdk/). The tests follow it in the same file. The TypeScript version follows the Rust one.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct Hello;

impl Lifecycle for Hello {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for Hello {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) = event
            else {
                continue;
            };
            if current != AgentActivity::Working {
                continue;
            }
            submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                node,
                slot: "hello".into(),
                content: vec![Span::plain("hi")],
                icon: None,
                sort_key: None,
                persist: false,
            })));
        }
        submissions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::testing::FakeHost;

    fn became(current: AgentActivity) -> Event {
        Event::AgentActivityChanged(AgentActivityEvent {
            node: 42,
            previous: AgentActivity::Idle,
            current,
            revision: 1,
        })
    }

    fn host_with_grants() -> FakeHost {
        FakeHost::new().grant("agent.read").grant("tree.decorate")
    }

    #[test]
    fn working_agent_gets_a_badge() {
        let mut host = host_with_grants();
        host.activate::<Hello>();
        let submissions = host.deliver(vec![became(AgentActivity::Working)]);
        assert_eq!(submissions.len(), 1);
        assert!(matches!(submissions[0].intent, Intent::SetDecoration(_)));
    }

    #[test]
    fn idle_agent_gets_no_badge() {
        let mut host = host_with_grants();
        host.activate::<Hello>();
        assert!(host.deliver(vec![became(AgentActivity::Idle)]).is_empty());
    }

    #[test]
    fn fixture_events_produce_one_badge() {
        let mut host = host_with_grants().with_fixture("tests/fixtures/agent-working.json");
        host.activate::<Hello>();
        let events = host.fixture_events();
        assert_eq!(host.deliver(events).len(), 1);
    }
}
```

Run the tests from the plugin directory with `cargo test badge`. The filter selects the tests whose names contain `badge`.

**TypeScript**

The TypeScript plugin behaves the same way. Its handler is exported as the pair that `definePlugin` returns, and the test runs it with `bun test`.

```typescript
// src/plugin.ts
import { definePlugin } from '@ilium/plugin-sdk';
import { span, type Event, type Submission } from '@ilium/plugin-sdk/types';

let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'agent-activity-changed') continue;
      if (event.val.current !== 'working') continue;
      submissions.push({
        intent: {
          tag: 'set-decoration',
          val: {
            node: event.val.node,
            slot: 'hello',
            content: [span('hi')],
            icon: undefined,
            sortKey: undefined,
            persist: false,
          },
        },
        correlation: nextCorrelation++,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
```

```typescript
// src/plugin.test.ts
import { expect, test } from 'bun:test';
import type { Event } from '@ilium/plugin-sdk/types';
import { createFakeHost } from '@ilium/plugin-sdk/testing';
import { handler, lifecycle } from './plugin';

function activity(current: 'working' | 'idle'): Event {
  return {
    tag: 'agent-activity-changed',
    val: { node: 42n, previous: 'idle', current, revision: 1n },
  };
}

test('working agent gets a badge', async () => {
  const host = createFakeHost().grant('agent.read').grant('tree.decorate');
  await host.activate({ lifecycle, handler });
  const submissions = host.deliver([activity('working')]);
  expect(submissions.length).toBe(1);
  expect(submissions[0].intent.tag).toBe('set-decoration');
});

test('idle agent gets no badge', async () => {
  const host = createFakeHost().grant('agent.read').grant('tree.decorate');
  await host.activate({ lifecycle, handler });
  expect(host.deliver([activity('idle')])).toEqual([]);
});
```

## Render tests

A render test draws a view on the headless host and compares the text of the rows. The host draws the tree that `views.render` returns into a buffer of cells, so the test reads exactly the text and the styles that a user would see. Borders are the rounded `╭╮╰╯` corners, and a `leader-row` shows its `…` leaders, so the expected strings contain the same characters as the real Ilium.

**Rust**

```rust
#[test]
fn notes_panel_lists_open_counts() {
    let mut host = FakeHost::new()
        .grant("tree.read")
        .with_fixture("tests/fixtures/notes-panel.json");
    host.activate::<NotesPane>();
    let screen = host.render(ViewTarget::Pane(42), 40, 4);
    assert_eq!(screen.row(1), "│ claude-review …………………………… [2 open] │");
}
```

**TypeScript**

```typescript
import { expect, test } from 'bun:test';
import { createFakeHost } from '@ilium/plugin-sdk/testing';
import { handler, lifecycle, views } from './plugin';

test('notes panel lists open counts', async () => {
  const host = createFakeHost().grant('tree.read').withFixture('tests/fixtures/notes-panel.json');
  await host.activate({ lifecycle, handler, views });
  const screen = host.render({ tag: 'pane', val: 42n }, 40, 4);
  expect(screen.row(1)).toBe('│ claude-review …………………………… [2 open] │');
});
```

When a row differs, the assertion prints the expected and the actual text. The render below shows such a failure for a notes panel where the count of open notes is missing.

<figure>
<pre class="term">
<span class="t-prompt">$</span> cargo test notes_panel_lists_open_counts
<span class="t-err">assertion `left == right` failed</span>
  left: <span class="t-err">"│ claude-review …………………………… [0 open] │"</span>
 right: <span class="t-dim">"│ claude-review …………………………… [2 open] │"</span>
<span class="t-err">test result: FAILED. 0 passed; 1 failed</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A render assertion failure: the count of open notes is wrong in the row.</figcaption>
</figure>

Render tests check the text and the styles of the cells. They do not show how the output looks in a real terminal. Fonts, colour contrast and animation still need a person to look at the result.

## Testing views with the extended widgets

The extended widgets are drawn by the same fake host, so a test of a `check-list`, a `meter`, a `notice`, a `progress-row` or a `dialog` reads their rows the same way. Three rules keep these tests stable:

- Assert the characters the user sees (`[x]`, `█`, `▶ RUNNING`), not the widget props. The props are the plugin's business, and the render is the contract.
- A widget that animates, such as `spinner`, shows its first frame in the fake host, and one static frame when the test sets reduced motion. Do not assert later frames.
- Give every widget a `key`, so a test can send an action to it by name.

The render below is what a member picker draws when one of three items is checked.

<figure>
<pre class="term">╭  ≡ ● · Members ────────────────────────╮
│ <span class="t-dim">Pick the agents to include</span>             │
│ [x] claude: refactor                   │
│ [ ] codex: tests                       │
│ [ ] codex: docs <span class="t-dim">(frozen)</span>               │
╰────────────────────────────────────────╯</pre>
<figcaption>Rendered at 48 columns. Illustrative. A check-list inside a block. The first item is checked.</figcaption>
</figure>

**Rust**

```rust
#[test]
fn picking_a_member_updates_the_checklist() {
    let mut host = FakeHost::new().grant("panes.read");
    host.activate::<MemberPicker>();
    let first = host.render(ViewTarget::Pane(7), 40, 6);
    assert_eq!(first.row(2), "│ [x] claude: refactor                   │");

    host.deliver(vec![action("members", ActionKind::Changed, Some(ActionValue::ItemState("p2".into(), TriState::On)))]);
    let second = host.render(ViewTarget::Pane(7), 40, 6);
    assert_eq!(second.row(3), "│ [x] codex: tests                       │");
}

fn action(key: &str, kind: ActionKind, value: Option<ActionValue>) -> Event {
    Event::UiAction(UiAction {
        view: ViewTarget::Pane(7),
        key: key.into(),
        action_id: key.into(),
        kind,
        value,
    })
}
```

**TypeScript**

```typescript
function action(key: string, kind: ActionKind, value?: ActionValue): Event {
  return { tag: 'ui-action', val: { view: { tag: 'pane', val: 7n }, key, actionId: key, kind, value } };
}

test('picking a member updates the checklist', async () => {
  const host = createFakeHost().grant('panes.read');
  await host.activate({ lifecycle, handler, views });
  expect(host.render(view, 40, 6).row(2)).toBe('│ [x] claude: refactor                   │');

  host.deliver([action('members', 'changed', { tag: 'item-state', val: ['p2', 'on'] })]);
  expect(host.render(view, 40, 6).row(3)).toBe('│ [x] codex: tests                       │');
});
```

## Testing pointer and keyboard input

A node with `pointer-events` sends `pointer` actions with a `view-pointer` value: the kind (`down`, `up`, `move`, `drag`, wheel kinds), the button, the modifiers, the cell inside the widget, the position inside the cell from 0 to 255 (128 when the terminal reports whole cells), and the click count. A test builds that value and delivers it like any other action. The same pattern covers `hover-enter`, `hover-leave`, `drag-over` and `resized`.

**Rust**

```rust
#[test]
fn double_click_on_a_cell_opens_the_file() {
    let mut host = FakeHost::new().grant("fs.read:project");
    host.activate::<Explorer>();
    let pointer = ViewPointer {
        kind: PointerKind::Down,
        button: PointerButton::Left,
        modifiers: PointerModifiers::empty(),
        column: 4,
        row: 2,
        sub_x: 128,
        sub_y: 128,
        clicks: 2,
    };
    let submissions = host.deliver(vec![action("files", ActionKind::Pointer, Some(ActionValue::Pointer(pointer)))]);
    assert!(matches!(submissions[0].intent, Intent::EditorOpen(_)));
}
```

**TypeScript**

```typescript
test('double click on a cell opens the file', async () => {
  const host = createFakeHost().grant('fs.read:project');
  await host.activate({ lifecycle, handler, views });
  const pointer = {
    kind: 'down', button: 'left',
    modifiers: { shift: false, control: false, alt: false },
    column: 4, row: 2, subX: 128, subY: 128, clicks: 2,
  } as const;
  const submissions = host.deliver([action('files', 'pointer', { tag: 'pointer', val: pointer })]);
  expect(submissions[0].intent.tag).toBe('editor-open');
});
```

Keyboard input is tested the same way, one level up: send the `ui-action` that the widget would send after the key (`changed`, `submitted`, `activated`), because the host, not the plugin, handles the key itself. The key table of each widget is in [Widget catalogue](/pages/widgets-index/).

## Golden renders

A golden render is a text file that holds the expected rows of a whole view. The test draws the view, joins the rows with line breaks, and compares the result with the file. A mismatch prints the first differing row. `assert_golden` (`assertGolden` in TypeScript) reads the file, converts line endings and prints the diff; `ILIUM_PLUGIN_UPDATE_GOLDEN=1` rewrites it. Keep golden files next to the fixtures, for example `tests/golden/files-panel.txt`, and review each change to a golden file like a change to code.

**Rust**

```rust
fn screen_text(screen: &Screen, height: u16) -> String {
    (0..height).map(|index| screen.row(index)).collect::<Vec<_>>().join("\n")
}

#[test]
fn files_panel_matches_golden() {
    let mut host = FakeHost::new().with_fixture("tests/fixtures/files.json");
    host.activate::<Explorer>();
    let drawn = screen_text(&host.render(ViewTarget::Pane(42), 40, 6), 6);
    assert_golden("tests/golden/files-panel.txt", &drawn);
}
```

**TypeScript**

```typescript
function screenText(screen: Screen, height: number): string {
  return Array.from({ length: height }, (_, index) => screen.row(index)).join('\n');
}

test('files panel matches golden', async () => {
  const host = createFakeHost().withFixture('tests/fixtures/files.json');
  await host.activate({ lifecycle, handler, views });
  const drawn = screenText(host.render({ tag: 'pane', val: 42n }, 40, 6), 6);
  assertGolden('tests/golden/files-panel.txt', drawn);
});
```

<figure>
<pre class="term">╭  ≡ ● · Files ──────────────────────────╮
│ README.md ………………………………………………………… <span class="t-dim">2 KiB</span> │
│ CHANGELOG.md ………………………………………………… <span class="t-dim">9 KiB</span> │
│ <span class="t-dim">Esc close · Enter open</span>                 │
╰────────────────────────────────────────╯</pre>
<figcaption>Rendered at 48 columns. Illustrative. The content of a golden file for a small file panel: rounded border, title icons, leader dots and a hint line. The test compares it character by character.</figcaption>
</figure>

Golden files hold the Unicode characters of the real UI. Never replace `…`, `╭`, `▶` or `⠋` with ASCII stand-ins in a golden file: the test would then pass while the real render differs. The stable-glyph mode is a user setting, and a test for it sets the fake host's icon mode and keeps a second golden file.

## Testing requests, jobs and services

A call that returns a request id, such as `fs.read-all`, `jobs.start`, `services.call`, `catalogs.query-async` or `geocode.search`, is tested by completing the request by hand: the test calls `host.complete(request_id, outcome)` with the outcome the real host would send, then delivers the `request-completed` event that follows. This covers `ok`, `cancelled` and each `failed(code)` without a network, a disk or a second plugin.

**Rust**

```rust
#[test]
fn failed_import_is_logged_not_trapped() {
    let mut host = FakeHost::new().grant("jobs.run");
    host.activate::<WorldImporter>();
    host.deliver(vec![command("com.example.worlds/import")]);
    let request = host.pending_requests()[0];
    host.complete(request, Outcome::Failed(PluginError::timed_out("import timed out")));
    assert!(host.logs().iter().any(|line| line.message.contains("import failed")));
}
```

**TypeScript**

```typescript
test('failed import is logged, not trapped', async () => {
  const host = createFakeHost().grant('jobs.run');
  await host.activate({ lifecycle, handler });
  host.deliver([command('com.example.worlds/import')]);
  const request = host.pendingRequests()[0];
  host.complete(request, { tag: 'failed', val: { code: 'timed-out', message: 'import timed out' } });
  expect(host.logs().some((line) => line.message.includes('import failed'))).toBe(true);
});
```

## Data flow of a handler test

The diagram shows the path of one handler test. Events go in, the fake host returns submissions, and the test asserts them.

<figure>
<svg viewBox="0 0 420 120" role="img" aria-label="A handler test: fixture events go into the fake host, submissions come out, and the test asserts them">
  <defs>
    <marker id="testing-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="8" y="20" width="120" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="68" y="37" font-size="11" text-anchor="middle" fill="currentColor">Fixture events</text>
  <text x="68" y="53" font-size="10" text-anchor="middle" fill="currentColor">events and state</text>
  <line x1="128" y1="42" x2="148" y2="42" stroke="currentColor" marker-end="url(#testing-arrow)"/>
  <rect x="150" y="20" width="120" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="37" font-size="11" text-anchor="middle" fill="currentColor">Fake host</text>
  <text x="210" y="53" font-size="10" text-anchor="middle" fill="currentColor">grants, no PTY</text>
  <line x1="270" y1="42" x2="290" y2="42" stroke="currentColor" marker-end="url(#testing-arrow)"/>
  <rect x="292" y="20" width="120" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="352" y="37" font-size="11" text-anchor="middle" fill="currentColor">Submissions</text>
  <text x="352" y="53" font-size="10" text-anchor="middle" fill="currentColor">intents, outcomes</text>
  <line x1="352" y1="64" x2="352" y2="82" stroke="currentColor" marker-end="url(#testing-arrow)"/>
  <rect x="292" y="84" width="120" height="28" rx="6" fill="none" stroke="currentColor"/>
  <text x="352" y="102" font-size="10" text-anchor="middle" fill="currentColor">assert_eq! / expect</text>
</svg>
<figcaption>The fixture feeds events into the fake host. The test checks the submissions that come back.</figcaption>
</figure>

## Running tests from the command line

`ilium plugin test` is the gate for fixtures. It runs every fixture against the built component, compares each expectation file, and exits 3 when one differs or the component traps. Run the project's own unit tests with `cargo test` or `bun test`, and put both in CI. The full flags are in [Plugin command-line reference](/pages/reference-plugin-cli/).

<figure>
<pre class="term">
<span class="t-prompt">$</span> ilium plugin test
agent-working.json   pass  12 ms
agent-idle.json      pass   9 ms
summary: 2 passed, 0 failed, 0 skipped (21 ms)
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Text output on a terminal: one line per fixture and a summary line. Piped output or `--json` prints JSONL records instead. The exit code is 0.</figcaption>
</figure>

The command compares the output of each fixture with its `.expect.json` file. A fixture without an expectation file only has to run without a trap. Expectation files use the match modes `exact`, `ordered` and `subset`, which [Test harness reference](/pages/reference-test-harness/) defines.

## Faking the world

The fake host also fakes the clock, the file system, processes, the network, storage and settings. Each fake is deterministic, and an unexpected call to a fake process or network fails the test. Register what the plugin may call before you deliver the events.

```rust
let mut host = FakeHost::for_manifest("manifest.json").unwrap();
host.processes().expect("git", &["status", "--short"]).returns("M src/lib.rs\n", 0);
host.fs().write("project:notes.md", b"# Notes");
host.activate::<Plugin>();
host.advance_clock(60_000);
host.settle();
```

```ts
const host = createFakeHost().forManifest('manifest.json');
host.processes().expect('git', ['status', '--short']).returns('M src/lib.rs\n', 0);
host.fs().write('project:notes.md', new TextEncoder().encode('# Notes'));
await host.activateModule('../src/server');
host.advanceClock(60_000);
await host.settle();
```

## Tests with more than one plugin

Use `FakeSession` when the behaviour crosses a boundary: two sides of one plugin, a service call between plugins, or a core extension point. `FakeHost` runs one plugin.

```rust
let mut session = FakeSession::new();
session.install("../provider").install("../consumer").start();
session.plugin("example.consumer").deliver(vec![ask_event()]);
session.pump();
assert_eq!(session.plugin("example.consumer").outcomes(), vec![Outcome::Ok]);
```

## Failure injection, replay and coverage

- `inject_trap(code)` and `inject_failure(import, error)` make the next call fail, so you can test the restart rule and error paths.
- `fuzz_run(seed, rounds)` delivers random valid event batches. A failure prints the seed.
- `record_to(path)` and `replay(path)` record and re-run a session.
- `ilium plugin test --coverage --min-coverage 80` also runs your unit tests under a coverage tool and exits 3 below the minimum.
- `ilium plugin test --conformance` runs the [conformance suite](/pages/reference-conformance-suite/).

## Test lanes

A plugin usually has three lanes of tests. Fixture and render tests run on the headless host. End-to-end checks run the installed package in a real Ilium.

<figure>
<svg viewBox="0 0 340 164" role="img" aria-label="Three test lanes for a plugin">
  <title>Test harness lanes</title>
  <desc>Three lanes: fixture tests run a batch of events through the handler and compare the submissions; render tests draw a view on the headless host and compare the cells; end-to-end tests run the package in the real host.</desc>
  <rect x="6" y="8" width="326" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="16" y="24" font-size="10" fill="currentColor">Fixture tests</text>
  <text x="16" y="42" font-size="10" fill="currentColor">events in, submissions out, Rust and TypeScript</text>
  <rect x="6" y="60" width="326" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="16" y="76" font-size="10" fill="currentColor">Render tests</text>
  <text x="16" y="94" font-size="10" fill="currentColor">headless host draws a view, cells compared</text>
  <rect x="6" y="112" width="326" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="16" y="128" font-size="10" fill="currentColor">End-to-end checks</text>
  <text x="16" y="146" font-size="10" fill="currentColor">installed package, real kernel, manual review</text>
</svg>
<figcaption>Fixture and render tests run on the headless host. End-to-end checks need a running Ilium and a person to judge the result.</figcaption>
</figure>

## Lifecycle tests

The fake host can drive the lifecycle of [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/) without a real server. These methods exist on `FakeHost` for contract `ilium:plugin@0.1.0`.

| Method | What it does |
|---|---|
| `deactivate(reason)` | Calls `deactivate` with a reason and returns the restore bytes. |
| `restart_with(bytes)` | Activates a new instance with `restored` set. |
| `inject_trap(code)` | Traps the instance with a trap code as a failure and applies the restart rule. |
| `advance_clock(ms)` | Moves the fixture clock, so back-off and windows pass at once. |
| `revoke(grant)`, `grant_now(grant)` | Fire the `revoked` and `granted` reasons. |
| `with_storage_snapshot(path)` | Starts with the storage of a JSON file. |
| `with_data_version(n)` | Sets the stored data version before activation. |
| `crash_after_journal_write()` | Traps after the next journal append is committed. |
| `failures()` | Returns the failure window of the plugin side. |

```rust
#[test]
fn third_failure_disables() {
    let mut host = FakeHost::for_manifest("manifest.json").unwrap();
    host.activate::<Plugin>();
    host.inject_trap("trap.panic");   // restarts after 1 s
    host.advance_clock(1_000);
    host.inject_trap("trap.panic");   // restarts after 5 s
    host.advance_clock(5_000);
    host.inject_trap("trap.panic");   // third failure
    assert_eq!(host.lifecycle_state(), LifecycleState::StoppedByFailure);
    assert_eq!(host.failures().len(), 3);
}
```

```ts
test("restore bytes survive an upgrade", async () => {
  const host = createFakeHost().forManifest("manifest.json");
  await host.activateModule("../src/server");
  const bytes = await host.deactivate("upgrade");
  await host.restartWith(bytes!);
  expect(host.restored()).toEqual(bytes);
});
```

In an `ilium plugin test` fixture file, the keys `lifecycle` (a list of steps such as `trap`, `advance`, `deactivate`) and `expect_state` run the same steps.

### Host conformance checklist

A host implementation passes when each line holds. Each line cites the requirement that defines it.

- Back-off is 1 s then 5 s and the third failure disables (LIF-025 to LIF-042).
- Activation order follows dependencies (LIF-045 to LIF-059).
- Package and instance transitions match the tables (LIF-001 to LIF-020).
- Install, update and rollback follow the pipeline (LIF-065 to LIF-094).
- Migration runs once, with backup (LIF-100 to LIF-116).
- Health and deadline bounds hold (LIF-122 to LIF-138).
- Capacity refusals do not count as failures (LIF-141).
- The survival matrix holds (LIF-150 to LIF-157).

- [LIF-279] The fake host MUST offer the methods in the table, and its results MUST match the real host for the lines above. Test: run the same fixtures against both.

## Edge cases and error codes

A test should cover the refusal path as well as the success path. The fake host returns the same codes as the real host.

| Situation | What the test sees | Code and retry |
|---|---|---|
| The tree capability is declined or not requested | No tree events arrive. `get` returns none and `find` returns an empty list. The plugin log gets one warn line that names `tree.read`. | No error code |
| An intent needs a grant that the test did not give | The offending submission is `refused(not-permitted)`. The other submissions of the batch are `refused(cancelled)` with the message `batch refused: submission <index>`. | `not-permitted`, no retry |
| A submission is malformed, such as a missing dedup key for `send-input` | The whole batch is refused. | `invalid-argument`, fix the call |
| A decoration uses a slot that the manifest does not declare | The submission is `refused(invalid-argument)`. | `invalid-argument`, fix the call |
| A decoration targets a node that no longer exists | The submission is `refused(not-found)`. | `not-found`, no retry |
| The decoration quota is full | The submission is `refused(quota-exceeded)`. | `quota-exceeded`, free resources first |
| A restored pane refuses input | The submission is `refused(pane-not-ready)`. | `pane-not-ready`, retry after `tree-changed` |

A panic in a Rust test or a thrown exception in a TypeScript test fails that test. In the real host the same failure is a trap. The host writes it to the plugin log as `trap in <export>` with the reason. See [debugging](/pages/debugging/) for how to read the trap entry.

## What the user sees

Tests run on the developer's machine only. `cargo test` reports each test with `ok` or `FAILED`, and it prints the values of a failed assertion. `ilium plugin test` prints the JSON lines shown above. The end user sees none of this. A user sees the result only when the built package runs in Ilium, for example as a badge on a node in the tree or as a panel row with the right text.

## Advice

- Write one fixture for each event that the plugin handles, and one for each error that it expects.
- Keep fixtures small. A fixture that checks one behaviour is easy to read when it fails.
- Grant exactly the capabilities in the manifest. A test that grants more than the manifest asks for hides a missing entry.
- Test the refusal path too: a missing grant should produce `not-permitted`, and the plugin should hide the feature without a trap.
- Check text in render tests, and check the look by hand in a real terminal.

## See also

- [Debugging](/pages/debugging/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Capabilities](/pages/capabilities/)
- [Rust SDK](/pages/rust-sdk/)
- [TypeScript SDK](/pages/typescript-sdk/)
- [Tutorial 1: the agent badge](/pages/tutorial-01-agent-badge/)
- [Versioning](/pages/versioning/)

API version checked: `ilium:plugin@0.1.0`.

### Security

Security tests and fuzz targets are listed in [Security testing](/pages/reference-security-testing/).
