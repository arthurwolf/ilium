# 19. Stop core actions with hooks: quiet hours

## 19. Stop core actions with hooks: quiet hours

This step writes the handler side of extension points. The plugin is `example.quiet-hours`. It adds three handlers to core plugins. A **veto** handler on `ilium.core.notifications/before-notify` refuses low-urgency notices between 22:00 and 07:00. A **veto** handler on `ilium.core.text-triggers/before-fire` refuses office rules outside 09:00 to 18:00. A **transform** handler on `ilium.core.smart-copy/after-copy` removes secret keys from copied text. The three handlers show what a hook can change, how a handler fails safely, and why one of the grants is high risk.

You start a fresh plugin. You have finished tutorials 1 to 9 and read [Extension points](/pages/api-extension-points/). The points of the core plugins are listed in [Reference: core extension points](/pages/reference-core-extension-points/), and [Extending core plugins](/pages/extending-core-plugins/) compares the five ways to work with a core feature. [Tutorial 18](/pages/tutorial-18-add-an-extension-point/) shows the owner side. This page does not repeat the reference tables.

Tutorial plugin: `example.quiet-hours`, version 0.1.0, API `ilium:plugin@0.1.0`. Rust and TypeScript behave the same in every step.

### You will need

- Rust: stable 1.90 or later with the `wasm32-wasip2` target, and the crates `ilium-plugin-sdk`, `serde_json` and `regex-lite`. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`.
- Three grants: `extensions.contribute:ilium.core.notifications/before-notify`, `extensions.contribute:ilium.core.smart-copy/after-copy` and `extensions.contribute:ilium.core.text-triggers/before-fire`. The last one is high risk, because that point declares `risk: "high"`. A trigger sends text into a terminal, so a handler there can stop a command from reaching an agent.
- A scratch folder for package inspection: `/media/arthur/tmp/quiet-hours/` with a `.scratch-reaper` file in it.

### The surface this step uses

| Area | Name | Meaning |
|---|---|---|
| Manifest | `contributes.extensions[]` | Entries with `point`, `id`, `priority` and `handler`. |
| Manifest | `exports: ["handler", "hook-handler"]` | A package with a hook extension must list `hook-handler`, or the install fails with `manifest.exports-mismatch`. |
| Export | `hook-handler` | Rust: `HookHandler::handle_hook(&mut self, call: HookCall) -> HookReply`. TypeScript: `handleHook(call)` in `definePlugin`. |
| Call | `HookCall` | `point`, `extension` (the `handler` name), `call` (a number for log lines) and `payload_json`. |
| Replies | `pass`, `replace(json)`, `refuse(reason)`, `answer(json)` | Allowed by mode: `transform` allows `replace`, `veto` allows `refuse`, `first` allows `answer`. |
| Core points | `before-notify` (veto), `before-fire` (veto, high), `after-copy` (transform) | Payload fields are in the reference page and in the code below. |

### The flow

The core plugin runs the chain. Your handler sees one payload and answers with one reply. The kernel stops the chain at the first refusal.

<figure>
<svg viewBox="0 0 480 300" role="img" aria-label="Sequence of a vetoed notice">
  <title>A veto chain</title>
  <desc>A core plugin raises a notice. The registry calls the quiet hours handler within 20 milliseconds. The handler refuses. The registry returns the refusal to the core plugin, which drops the notice.</desc>
  <defs>
    <marker id="t19-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="10" y="8" width="100" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="60" y="25" font-size="11" text-anchor="middle" fill="currentColor">Notifications</text>
  <rect x="190" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="235" y="25" font-size="11" text-anchor="middle" fill="currentColor">Registry</text>
  <rect x="360" y="8" width="110" height="26" rx="4" fill="none" stroke="currentColor"/>
  <text x="415" y="25" font-size="11" text-anchor="middle" fill="currentColor">Quiet hours</text>
  <line x1="60" y1="34" x2="60" y2="290" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="235" y1="34" x2="235" y2="290" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="415" y1="34" x2="415" y2="290" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="60" y1="62" x2="235" y2="62" stroke="currentColor" marker-end="url(#t19-arrow)"/>
  <text x="148" y="56" font-size="10" text-anchor="middle" fill="currentColor">run-hook(before-notify)</text>
  <line x1="235" y1="100" x2="415" y2="100" stroke="currentColor" marker-end="url(#t19-arrow)"/>
  <text x="325" y="94" font-size="10" text-anchor="middle" fill="currentColor">handle-hook (20 ms)</text>
  <line x1="415" y1="140" x2="235" y2="140" stroke="currentColor" marker-end="url(#t19-arrow)"/>
  <text x="325" y="134" font-size="10" text-anchor="middle" fill="currentColor">refuse("quiet hours")</text>
  <line x1="235" y1="190" x2="60" y2="190" stroke="currentColor" marker-end="url(#t19-arrow)"/>
  <text x="148" y="184" font-size="10" text-anchor="middle" fill="currentColor">refused: extension.vetoed</text>
  <text x="148" y="232" font-size="10" text-anchor="middle" fill="currentColor">notice is dropped</text>
  <line x1="415" y1="262" x2="235" y2="262" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#t19-arrow)"/>
  <text x="325" y="256" font-size="10" text-anchor="middle" fill="currentColor">timeout counts as pass</text>
</svg>
<figcaption>The dashed arrow is the fail-open path. A handler that is late, traps or errors counts as <code>pass</code>, and the notice goes through.</figcaption>
</figure>

### Step 1: the manifest

The package has one server side, three extensions and the `hook-handler` export. All three handlers run on the server, because all three points belong to server-side core plugins. A handler on the other side fails the install with `manifest.extension-side`.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.quiet-hours",
  "name": "Quiet hours",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "description": "Mutes low notices at night, holds office rules after hours and removes keys from copies.",
  "icon": "moon",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["hook-handler"]
  },
  "capabilities": {
    "server": [
      "extensions.contribute:ilium.core.notifications/before-notify",
      "extensions.contribute:ilium.core.text-triggers/before-fire",
      "extensions.contribute:ilium.core.smart-copy/after-copy"
    ]
  },
  "activation": ["startup"],
  "contributes": {
    "extensions": [
      { "point": "ilium.core.notifications/before-notify@0.1.0", "id": "quiet-night",
        "priority": 20, "handler": "quiet_night" },
      { "point": "ilium.core.text-triggers/before-fire@0.1.0", "id": "office-hours",
        "priority": 50, "handler": "office_hours" },
      { "point": "ilium.core.smart-copy/after-copy@0.1.0", "id": "strip-keys",
        "priority": 70, "handler": "strip_keys" }
    ]
  }
}
```

Points to notice:

- `exports` must list `hook-handler` next to `handler`. A package that lists hook extensions without it fails with `manifest.exports-mismatch`.
- `handler` is a name that your code dispatches on. It reaches you as `call.extension`. The `id` is for the user: it is shown on the Extensions tab and is unique per contributor and point.
- `priority` runs from 0 to 100. Lower runs first, and ties go in plugin id order. Quiet night runs at 20 so that it refuses before other handlers spend their time. The user can override the order, as step 5 shows.
- Each grant names one point. There is no wildcard, and no grant that covers every point. A wildcard fails with `manifest.grant-wildcard`.
- `@0.1.0` asks for a minimum version of the point. The core plugin's point is at that version or newer in the same major. Otherwise the extension is dormant.
- `activation: ["startup"]` keeps the plugin loaded so that the first call does not pay a start. A lazy plugin would start on the first hook call, and that call would count against its 20 ms.

### Step 2: the handlers

A hook handler is a plain function from one call to one reply. It runs synchronously on the contributor's own instance, within `deadline_ms` of the point (20 ms for these three). It may call other host functions that its grants allow, such as `clock`, but each call must finish inside the deadline. It cannot wait for a reply from another plugin.

Each handler does three things. It parses its payload and passes at once when the payload is not what it expected. It decides. It returns one reply. Parsing errors are `pass`, never `refuse`. A broken payload must not stop a notice.

The code keeps the time rules in two small pure functions, so the tests in step 6 can drive them with fixed times. Ilium gives plugins the clock in UTC. The sketch uses a fixed offset constant of 120 minutes. A real plugin reads the offset from a setting, as [tutorial 10](/pages/tutorial-10-settings-new-tab/) shows.

**Rust**

```rust
// src/lib.rs
use ilium_plugin_sdk::exports::{HookCall, HookHandler, HookReply};
use ilium_plugin_sdk::host::clock;
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use regex_lite::Regex;
use serde_json::{json, Value};

const UTC_OFFSET_MINUTES: u64 = 120;
const NIGHT_START: u64 = 22 * 60;
const NIGHT_END: u64 = 7 * 60;
const OFFICE_START: u64 = 9 * 60;
const OFFICE_END: u64 = 18 * 60;

#[ilium_plugin(side = "server", exports = ["hook-handler"])]
#[derive(Default)]
struct QuietHours {
    refused: u32,
}

impl Lifecycle for QuietHours {}

impl Handler for QuietHours {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

fn minute_of_day(now_unix_millis: u64) -> u64 {
    (now_unix_millis / 60_000 + UTC_OFFSET_MINUTES) % 1440
}

fn is_night(minute: u64) -> bool {
    minute >= NIGHT_START || minute < NIGHT_END
}

fn is_office(minute: u64) -> bool {
    (OFFICE_START..OFFICE_END).contains(&minute)
}

fn pass() -> HookReply {
    HookReply::Pass
}

impl HookHandler for QuietHours {
    fn handle_hook(&mut self, call: HookCall) -> HookReply {
        let payload: Value = match serde_json::from_str(&call.payload_json) {
            Ok(value) => value,
            Err(_) => return pass(),
        };
        let reply = match call.extension.as_str() {
            "quiet_night" => quiet_night(&payload, clock::now_unix_millis()),
            "office_hours" => office_hours(&payload, clock::now_unix_millis()),
            "strip_keys" => strip_keys(&payload),
            _ => pass(),
        };
        if matches!(reply, HookReply::Refuse(_)) {
            self.refused += 1;
            log::log(Level::Info, &format!("call {} refused ({} so far)", call.call, self.refused));
        }
        reply
    }
}

fn quiet_night(payload: &Value, now: u64) -> HookReply {
    if payload["urgency"] != "low" || !is_night(minute_of_day(now)) {
        return pass();
    }
    HookReply::Refuse("quiet hours until 07:00".to_string())
}

fn office_hours(payload: &Value, now: u64) -> HookReply {
    let is_office_rule = payload["rule"].as_str().is_some_and(|rule| rule.starts_with("office-"));
    if !is_office_rule || is_office(minute_of_day(now)) {
        return pass();
    }
    HookReply::Refuse("office rule outside 09:00 to 18:00".to_string())
}

fn strip_keys(payload: &Value) -> HookReply {
    let Some(text) = payload["text"].as_str() else { return pass() };
    let Ok(pattern) = Regex::new(r"sk-[A-Za-z0-9]{20,}") else { return pass() };
    let cleaned = pattern.replace_all(text, "[removed]");
    if cleaned == text {
        return pass();
    }
    HookReply::Replace(json!({ "text": cleaned }).to_string())
}
```

**TypeScript**

```ts
// src/index.ts
import { definePlugin, type HookCall, type HookReply } from '@ilium/plugin-sdk';
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import { log } from '@ilium/plugin-sdk/host/log';

const UTC_OFFSET_MINUTES = 120;
const NIGHT_START = 22 * 60;
const NIGHT_END = 7 * 60;
const OFFICE_START = 9 * 60;
const OFFICE_END = 18 * 60;

export function minuteOfDay(nowMillis: bigint): number {
  return (Number(nowMillis / 60000n) + UTC_OFFSET_MINUTES) % 1440;
}
const isNight = (minute: number): boolean => minute >= NIGHT_START || minute < NIGHT_END;
const isOffice = (minute: number): boolean => minute >= OFFICE_START && minute < OFFICE_END;

const pass = (): HookReply => ({ tag: 'pass' });
const refuse = (reason: string): HookReply => ({ tag: 'refuse', val: reason });
const replace = (payload: unknown): HookReply => ({ tag: 'replace', val: JSON.stringify(payload) });

function quietNight(payload: { urgency?: string }, now: bigint): HookReply {
  if (payload.urgency !== 'low' || !isNight(minuteOfDay(now))) return pass();
  return refuse('quiet hours until 07:00');
}

function officeHours(payload: { rule?: string }, now: bigint): HookReply {
  const isOfficeRule = typeof payload.rule === 'string' && payload.rule.startsWith('office-');
  if (!isOfficeRule || isOffice(minuteOfDay(now))) return pass();
  return refuse('office rule outside 09:00 to 18:00');
}

function stripKeys(payload: { text?: string }): HookReply {
  if (typeof payload.text !== 'string') return pass();
  const cleaned = payload.text.replace(/sk-[A-Za-z0-9]{20,}/g, '[removed]');
  return cleaned === payload.text ? pass() : replace({ text: cleaned });
}

let refused = 0;

export function onHook(call: HookCall): HookReply {
    let payload: Record<string, unknown>;
    try {
      payload = JSON.parse(call.payloadJson) as Record<string, unknown>;
    } catch {
      return pass();
    }
    const now = nowUnixMillis();
    let reply: HookReply;
    switch (call.extension) {
      case 'quiet_night': reply = quietNight(payload, now); break;
      case 'office_hours': reply = officeHours(payload, now); break;
      case 'strip_keys': reply = stripKeys(payload); break;
      default: reply = pass();
    }
    if (reply.tag === 'refuse') {
      refused += 1;
      log('info', `call ${call.call} refused (${refused} so far)`);
    }
    return reply;
}

export const { lifecycle, handler, hookHandler } = definePlugin({
  side: 'server',
  onEvents() {
    return [];
  },
  handleHook: onHook,
});
```

Points to notice:

- The reply constructors are tiny local functions. They keep the three handlers free of repeated enum syntax and make a wrong reply easy to see in review.
- `strip_keys` is a `transform` handler. It returns `Replace` with a complete payload that matches the point's `reply_schema` (an object with `text`). A reply that fails the schema counts as `pass` and is logged as `extension.schema-mismatch`.
- A reply that the mode does not allow also counts as `pass`. `Refuse` on `after-copy` would log `extension.reply-not-allowed` and change nothing.
- The handlers touch only their payload and the clock. They do no I/O, no file read and no service call. That keeps them far below 20 ms.
- The state is a counter. A handler keeps state only for its own use. It cannot see the owner's state or the other handlers.
- `refuse` carries a short reason. The owner sees it as `example.quiet-hours: quiet hours until 07:00`, and the user sees it in logs and notices. Do not put secrets in a reason.
- The regular expression is compiled for every call in the sketch. Compile it once in a `static` for a hot point. `after-copy` runs on every copy, so keep it cheap.

### Step 3: how a chain ends

The mode of the point decides what a chain does with your reply.

| Point | Mode | Your reply | Result for the core plugin |
|---|---|---|---|
| `before-notify` | veto | `refuse(reason)` | The chain stops. The completion is `refused` with code `extension.vetoed`. The notice is dropped. |
| `before-notify` | veto | `pass` | The next handler runs. When all pass, the notice goes on. |
| `before-fire` | veto | `refuse(reason)` | The trigger does not send its reply. The rule is logged as vetoed. |
| `after-copy` | transform | `replace(json)` | The next handler gets your payload as its input. The last payload becomes the copied text. |
| `after-copy` | transform | `pass` | The payload goes on unchanged. |

Three rules limit the damage that a handler can do.

1. **Fail open.** A timeout, a trap or an error counts as `pass`. A slow handler never blocks a notice. It is logged as `extension.handler-timeout`.
2. **Three strikes.** Three breaches in a row disable that extension until the plugin restarts. The Plugins screen shows `stopped after 3 timeouts` and a Reset button. The other two extensions of the package keep running.
3. **A 100 ms chain.** The whole chain has a cap of 100 ms. Handlers after the cap are skipped and logged, so the action goes on.

A handler runs in the contributor's own instance. A T1 plugin shares the Ilium process, so a call is cheap. A T2 or T3 plugin costs one helper round trip per call, so do not use a hook on a path hotter than about 100 calls per second.

### Step 4: grants, risk and the review dialog

Two of the grants are normal risk and one is high.

| Grant | Risk | Why |
|---|---|---|
| `extensions.contribute:ilium.core.notifications/before-notify` | normal | A handler can only hide a notice. |
| `extensions.contribute:ilium.core.smart-copy/after-copy` | normal | A handler can change copied text. It cannot see more than the text, its kind and the pane. |
| `extensions.contribute:ilium.core.text-triggers/before-fire` | **high** | A handler can stop a trigger from sending text into a terminal. The point declares `risk: "high"`. |

The review dialog shows the high-risk grant unchecked. If the user leaves it unchecked, the package installs, but the `office-hours` extension is not granted and does nothing. The other two extensions work. Check `ilium plugin grants example.quiet-hours` to see the state. The user can revoke a grant later with `ilium plugin grants example.quiet-hours --revoke extensions.contribute:ilium.core.text-triggers/before-fire`.

A veto hook is a power. Keep the reason honest, keep the rule narrow, and put the rule in the description so the user knows what the package does. A `high` risk point means that a bundled T1 plugin is also asked for the grant at its first activation.

### Step 5: priority and the user's order

Priority is the order you suggest. The user's order wins. On the owner's Extensions tab, each contributor has a toggle and a move up and down. If the user moves `quiet-night` below another veto handler, the new order applies from then on, and the priority in your manifest is ignored for that point. If the user turns a toggle off, the extension is not called, and its log line is `extension.disabled`.

Write handlers that work in any order. A veto handler is order-independent when each handler judges only its own rule. A transform handler is not: the output of one is the input of the next. Make a transform handler accept the payload shape of the point, not the shape of your own previous output, and always return a payload that fits `reply_schema`.

### Step 6: test the handlers

The fake host described in [Testing](/pages/testing/) delivers events and records submissions. `host.run_hook` drives a hook handler with a `HookCall` and returns the `HookReply`, and `FakeSession.install_core` adds a stub of the core plugin that owns the point. Test the pure time functions and the three handlers. The 20 ms deadline is enforced in component mode (`ilium plugin test --component`), not in native mode. Check it in the app with the log as well, as step 7 shows. See [Test harness reference](/pages/reference-test-harness/).

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const NIGHT: u64 = 24 * 3_600_000 - UTC_OFFSET_MINUTES * 60_000; // 00:00 local
    const NOON: u64 = 12 * 3_600_000 - UTC_OFFSET_MINUTES * 60_000;

    fn call(extension: &str, payload: serde_json::Value) -> HookCall {
        HookCall {
            point: "test/point".to_string(),
            extension: extension.to_string(),
            call: 1,
            payload_json: payload.to_string(),
        }
    }

    #[test]
    fn low_notice_is_refused_at_night() {
        let payload = json!({ "urgency": "low" });
        assert!(matches!(quiet_night(&payload, NIGHT), HookReply::Refuse(_)));
    }

    #[test]
    fn high_notice_is_never_refused() {
        let payload = json!({ "urgency": "high" });
        assert!(matches!(quiet_night(&payload, NIGHT), HookReply::Pass));
    }

    #[test]
    fn low_notice_passes_at_noon() {
        let payload = json!({ "urgency": "low" });
        assert!(matches!(quiet_night(&payload, NOON), HookReply::Pass));
    }

    #[test]
    fn office_rule_is_held_at_night_only() {
        let payload = json!({ "rule": "office-standup" });
        assert!(matches!(office_hours(&payload, NIGHT), HookReply::Refuse(_)));
        assert!(matches!(office_hours(&payload, NOON), HookReply::Pass));
        assert!(matches!(office_hours(&json!({ "rule": "retry" }), NIGHT), HookReply::Pass));
    }

    #[test]
    fn copied_key_is_removed() {
        let mut plugin = QuietHours::default();
        let reply = plugin.handle_hook(call("strip_keys", json!({ "text": "key sk-abcdefghijklmnopqrstu end" })));
        match reply {
            HookReply::Replace(text) => assert!(text.contains("[removed]")),
            _ => panic!("expected replace"),
        }
    }

    #[test]
    fn garbage_payload_and_unknown_handler_pass() {
        let mut plugin = QuietHours::default();
        let mut bad = call("quiet_night", json!({}));
        bad.payload_json = "not json".to_string();
        assert!(matches!(plugin.handle_hook(bad), HookReply::Pass));
        assert!(matches!(plugin.handle_hook(call("nobody", json!({}))), HookReply::Pass));
    }
}
```

**TypeScript**

```ts
// src/index.test.ts
import { expect, test } from 'bun:test';
import type { HookCall } from '@ilium/plugin-sdk';
import { minuteOfDay, onHook } from './index';

const MIDNIGHT_LOCAL = BigInt((24 * 60 - 120) * 60000); // 00:00 at offset 120
const NOON_LOCAL = BigInt((12 * 60 - 120) * 60000);

function call(extension: string, payload: unknown): HookCall {
  return { point: 'test/point', extension, call: 1n, payloadJson: JSON.stringify(payload) };
}

test('time of day uses the offset', () => {
  expect(minuteOfDay(MIDNIGHT_LOCAL)).toBe(0);
  expect(minuteOfDay(NOON_LOCAL)).toBe(720);
});

test('copied key is removed', () => {
  const reply = onHook(call('strip_keys', { text: 'key sk-abcdefghijklmnopqrstu end' }));
  expect(reply.tag).toBe('replace');
});

test('garbage payload and unknown handler pass', () => {
  const bad: HookCall = { ...call('quiet_night', {}), payloadJson: 'not json' };
  expect(onHook(bad).tag).toBe('pass');
  expect(onHook(call('nobody', {})).tag).toBe('pass');
});
```

Export `quietNight` and `officeHours` as well and add the same cases as in the Rust tests: a low notice at night is refused, a high notice never is, and an office rule is held at midnight but not at noon. Run `cargo test` or `bun test`. Then run the plugin against a fixture with `ilium plugin test . --fixture tests/fixtures/idle.json` to check that it activates and does nothing else.

The handler functions take the time as an argument for this reason. A test that reads the real clock would pass in the daytime and fail at night.

### Step 7: pack, install and see it work

Build and pack as in [tutorial 9](/pages/tutorial-09-package-and-share/): `ilium plugin build`, `ilium plugin pack`, then `ilium plugin keygen`, `ilium plugin sign` and `ilium plugin trust add` once. Install `dist/example.quiet-hours-0.1.0.iliumpkg`. An unsigned package is T3 and needs `--allow-unsigned`.

#### What you see

The review dialog groups the three extensions under **Adds to other plugins**. The high-risk line is unchecked and shown in the warning colour.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Review: example.quiet-hours 0.1.0            │
│                                              │
│ Adds to other plugins                        │
│   Notifications > Before notify              │
│     can stop a notice                        │
│   Smart Copy > After copy                    │
│     can change copied text                   │
│   Text triggers > Before fire                │
│     can stop a trigger from sending text     │
│                                              │
│ Grants                                       │
│   [x] extensions.contribute: before notify   │
│   [x] extensions.contribute: after copy      │
│<span class="t-warn">   [ ] extensions.contribute: fire   HIGH</span>     │
│                                              │
│   [allow]  [decline]                         │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The high-risk grant is unchecked until the user ticks it.</figcaption>
</figure>

On Settings > Notifications > Extensions the user sees the contributor in run order and can move it.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Notifications > Extensions                   │
│                                              │
│ Before notify (veto)                         │
│  1 [x] example.quiet-hours    quiet-night    │
│  2 [x] example.webhook-mute   mute-bots      │
│                                              │
│ Channels (collect)                           │
│    [x] example.chat-relay     relay          │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The arrows on a row move it. The order shown replaces the priority.</figcaption>
</figure>

At 00:10 local time a low notice reaches the hook. The log of the plugin shows the refusal. Run `ilium plugin logs example.quiet-hours --tail 5`:

<figure>
<pre class="term">
<span class="t-dim">2026-10-12 00:10:04 info example.quiet-hours</span>
<span class="t-ok">call 41 refused (1 so far)</span>
<span class="t-dim">2026-10-12 00:10:09 info example.quiet-hours</span>
<span class="t-ok">call 42 refused (2 so far)</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Each line is one refused notice. A passing call writes no line.</figcaption>
</figure>

If a handler became slow, for example by reading a file, the log would show the breach and the third one stops the extension:

<figure>
<pre class="term">
<span class="t-dim">2026-10-12 00:12:11 warn example.quiet-hours</span>
<span class="t-warn">deadline breach in hook-handler: limit 20 ms</span>
<span class="t-dim">2026-10-12 00:12:15 warn example.quiet-hours</span>
<span class="t-warn">deadline breach in hook-handler: limit 20 ms</span>
<span class="t-dim">2026-10-12 00:12:19 warn example.quiet-hours</span>
<span class="t-err">quiet-night stopped after 3 timeouts</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Notices went through during these breaches, because a timeout counts as pass. The user presses Reset to turn the extension on again.</figcaption>
</figure>

### Security and consent

| Concern | What the plugin does |
|---|---|
| What the handlers see | The payload of one call. `before-notify` shows the title and the pane, so the plugin sees what a notice says. Say so in the description. |
| What the handlers can change | A veto can hide a notice or hold a trigger. A transform can edit copied text. Nothing else. |
| High-risk grant | `before-fire` is high risk. The user opts in. A refused grant leaves the other two handlers working. |
| Reasons | A refusal reason is shown in logs. It holds no secret and no text from the payload. |
| Fail open | A bug in the plugin cannot silence the product. It can only fail to help. |
| Copy edits | `strip_keys` removes text. It never adds text. Test it, because a bad pattern can corrupt a copy. |

### Edge cases and errors: what can go wrong

| Code or symptom | Cause | Fix |
|---|---|---|
| `manifest.exports-mismatch` | Hook extensions without `hook-handler` in `exports`. | Add the export. |
| `manifest.extension-side` | The handler is declared on a different side from the point's calls. | Declare it on the side that owns the call. |
| `manifest.grant-missing-extension` | An extension has no matching `extensions.contribute` grant. | Add the exact grant. |
| `manifest.grant-wildcard` | A grant ends in `*`. | Name one point. |
| `manifest.extension-limit` | More than 64 extensions in one plugin. | Merge handlers. |
| Extension is dormant | The owner is missing, or the point version is older than `@0.1.0`. | Install the owner, or lower the version. |
| `extension.handler-timeout` | The handler took over 20 ms. It counts as pass. | Remove I/O. Precompile patterns. |
| `extension.disabled` | Three breaches, or the user turned it off. | Fix the cause, then Reset. |
| `extension.reply-not-allowed` | A reply that the mode does not allow, for example `refuse` on a transform. | Use the reply of the mode. |
| `extension.schema-mismatch` | A `replace` that fails the `reply_schema`. | Return a complete object. |
| `extension.vetoed` | Your own refusal, as the core plugin sees it. | Not an error of yours. |
| A refusal has no effect | Another handler runs first, or the extension is toggled off. | Check the Extensions tab. |
| Wrong hour | The clock is in UTC and the offset is wrong. | Fix the offset constant. |

### See also

- [Extension points](/pages/api-extension-points/) for hook modes, replies and the error codes.
- [Extending core plugins](/pages/extending-core-plugins/) for the quiet hours scenario and the other ways to extend.
- [Reference: core extension points](/pages/reference-core-extension-points/) for the payload of every core hook.
- [Core plugin: notifications](/pages/core-plugin-notifications/), [text triggers](/pages/core-plugin-text-triggers/) and [smart copy](/pages/core-plugin-smart-copy/).
- [Tutorial 18: offer an extension point](/pages/tutorial-18-add-an-extension-point/) for the owner side.
- [Tutorial 20: extend six core plugins](/pages/tutorial-20-extend-six-core-plugins/) for collect points.
- [Testing](/pages/testing/) and [Debugging](/pages/debugging/).

API checked against `ilium:plugin@0.1.0`.
