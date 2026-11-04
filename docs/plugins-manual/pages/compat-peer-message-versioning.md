---
layout: page
title: Peer message and topic payload versions
permalink: /pages/compat-peer-message-versioning/
---

## Peer message and topic payload versions

Two plugins that talk to each other are updated on different days. The host carries the bytes of a `messages.peer` message and the JSON text of a topic event unchanged, and it does not read either of them ([messages.peer](/pages/capability-messages-peer/), [topics.subscribe](/pages/capability-topics-subscribe/)). The host therefore cannot tell a receiver which layout it holds. The publisher of the payload owns the version, and this page states the rules that every sender and receiver follows.

### You need

- [messages.peer](/pages/capability-messages-peer/) for the message shape and its outcomes.
- [Plugin to plugin calls](/pages/api-plugin-to-plugin/) for the grant rules that apply to the same pair of plugins.
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for the version bumps of the plugin itself.

### The version field

Each payload carries its format version in a place the receiver reads first. [COR-953]

| Kind | Payload type | Where the version sits | First version |
|---|---|---|---|
| `messages.peer` message | `list<u8>` | the first byte of the payload | 1 |
| Topic event | JSON text | the top-level member `schema_version` of the JSON object | 1 |

Raise the number by one for each incompatible change of the layout. Removing, renaming or retyping a field, or changing what a field means, is incompatible. Adding an optional field at the end is not, and it keeps the same number.

<div class="note">
<p>The version byte of a message is part of the layout, not a header that the host adds. A message that starts with a different byte is read as a different layout by the receiver, so a sender must never reuse a byte value for another meaning.</p>
</div>

### What a receiver does with a newer version

A receiver implements a set of versions, from 1 up to the highest one it knows. When a payload carries a higher version, the receiver does not guess at its layout. [COR-954]

| Version in the payload | What the receiver does |
|---|---|
| 1 up to the highest version it implements | Parses the payload with that layout. |
| Higher than the highest version it implements | Drops the payload, logs it at level `warn` with the sender id (or the publisher of the topic) and the version found, and does not call its handler. |

A message is one-way. The receiver cannot answer the sender, and the sender is not told. The `ok` outcome of `messages.peer` means that the host queued the message ([outcomes](/pages/capability-messages-peer/#outcomes)). It does not mean that the receiver parsed it, so a sender cannot detect a version mismatch from its outcome. A topic event has no reply at all.

### What a sender does with an older peer

A sender cannot read the release of its peer. The host does not expose it, and the peer may be installed on any release that the receiver's author still supports. So the order of the releases carries the rule. [COR-955]

1. Release the receiver that accepts the new version first.
2. Name, in the release notes of the sender, the oldest receiver release that accepts the new version.
3. Keep the old version in the sender until every receiver release that the sender still supports accepts the new one.

The sender may then emit the new version. A receiver keeps accepting the old version for as long as it lists the old layout as supported. The rule for how long that is follows the window in [Deprecation timeline and the window](/pages/compat-deprecation-timeline/).

### Worked example: an exporter and an importer

Plugin `example.export` sends an `export-done` message to plugin `example.import`. In version 1 of the layout, the payload is the byte `0x01` followed by the UTF-8 path. In version 2 the payload is the byte `0x02`, a four-byte checksum of the path, and then the path.

The upgrade runs in this order.

| Step | Release installed | Exporter emits | Importer accepts |
|---|---|---|---|
| Start | export 1.0, import 1.0 | `0x01` | `0x01` |
| 1 | import 1.1 (the receiver first) | `0x01` | `0x01` and `0x02` |
| 2 | export 1.1, still emitting `0x01` while import 1.0 may be installed | `0x01` | `0x01` and `0x02` |
| 3 | export 2.0, whose release notes name import 1.1 as the oldest importer it works with | `0x02` | `0x01` and `0x02` |

Step 3 is safe only because the release notes of export 2.0 name import 1.1. Nothing in the host enforces that. If a user keeps import 1.0 while export 2.0 is installed, the importer drops each `0x02` message with a `warn` log, and the user sees no error in the exporter. The author must say so in the release notes of export 2.0, and must test the pair as in the test below.

<figure>
<svg viewBox="0 0 320 130" role="img" aria-label="Upgrade order for a payload version: receiver first, then sender">
  <rect x="10" y="10" width="140" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="80" y="34" text-anchor="middle" font-size="11" fill="currentColor">1. Receiver accepts v2</text>
  <rect x="170" y="10" width="140" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="240" y="34" text-anchor="middle" font-size="11" fill="currentColor">2. Name oldest receiver</text>
  <rect x="10" y="80" width="300" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="160" y="104" text-anchor="middle" font-size="11" fill="currentColor">3. Sender emits v2 only after step 2</text>
  <line x1="80" y1="50" x2="80" y2="80" stroke="currentColor"/>
  <line x1="240" y1="50" x2="240" y2="80" stroke="currentColor"/>
</svg>
<figcaption>A new payload version reaches the receivers before any sender emits it.</figcaption>
</figure>

### Edge cases and errors

| Case | Result |
|---|---|
| Sender emits a version higher than every receiver's | The receiver drops the payload and logs a `warn`. The sender sees `ok` and no later event. |
| Receiver is older and cannot read the new version | Same as above. The receiver's author must prevent it with the order of releases. |
| Message payload over 1 MiB | `refused(invalid-argument)` at send ([messages.peer outcomes](/pages/capability-messages-peer/#outcomes)). |
| Topic payload over 64 KiB or not valid JSON | `invalid-argument` at publish ([topics](/pages/api-messages-and-topics/)). |
| Topic payload that is valid JSON but has no `schema_version` | Not covered by [COR-954]. See the open decisions below. |

### What the user sees

The user sees nothing in the dialogs of either plugin. A dropped payload is recorded only in the plugin log of the receiver, at level `warn`. The user can find it with `ilium plugin logs <id>`.

### Test

Build a fixture sender and receiver. The receiver implements versions 1 and 2. Send a version 1 message, a version 2 message and a version 3 message, and assert that the receiver parses the first two, logs one `warn` line for the third with the sender id and the version 3, and gives no reply to the sender. Repeat the same with a topic event whose `schema_version` is 1 and 3.

### See also

- [messages.peer](/pages/capability-messages-peer/) for the message capability and its outcomes
- [Topics](/pages/capability-topics-subscribe/) for the subscriber side of a topic
- [Plugin to plugin calls](/pages/api-plugin-to-plugin/) for the call that carries the grant check
- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for the bumps of the plugin itself
- [Deprecation timeline and the window](/pages/compat-deprecation-timeline/) for how long an old layout stays supported

### Open decisions

- A topic payload or a message with no version field: the default is that the receiver drops it and logs it at level `warn`, as for a newer version (proposed, owner to confirm).
- Whether the host should expose the release of a peer to the sender, so that step 3 of the upgrade order can be checked at run time: the default is no, the host does not read payloads or peer releases (proposed, owner to confirm).

API version checked: `ilium:plugin@0.1.0`.

## Requirement statements

- **[COR-953]** A plugin that sends a `messages.peer` message MUST start its payload with a one-byte format version, and a plugin that publishes a topic MUST give its JSON payload a top-level integer `schema_version` member; both counted from 1 and raised by one for each incompatible change of the layout.
- **[COR-954]** A receiver of a `messages.peer` message or a topic event whose version is higher than the highest version it implements MUST NOT parse the payload, MUST drop it, MUST log it at level `warn` with the sender id and the version found, and MUST NOT reply to the sender.
- **[COR-955]** A plugin that introduces a new payload version MUST release a receiver that accepts it before any sender emits it, MUST name in its release notes the oldest receiver release that accepts it, and MUST NOT emit the new version while a receiver release that it still supports rejects it.
