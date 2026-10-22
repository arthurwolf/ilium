---
layout: page
title: Plugin traffic over IPC
permalink: /pages/reference-host-ipc/
---

## Plugin traffic over IPC

Plugins run in the server and in each client. The two halves of one plugin talk through the same local connection that already carries pane frames. This page defines the messages, the handshake, the mirrored tree, how a client submission reaches the server and returns, what happens on reconnect, and how plugin traffic is kept from starving the terminal. Requirement ids are `[HST-100]` to `[HST-125]`.

## You will need

- [Host architecture](/pages/reference-host-architecture/) for the process map.
- [Requests and outcomes](/pages/requests-and-outcomes/) for submissions, outcomes and deduplication.
- [Messages and topics](/pages/api-messages-and-topics/) for plugin-to-plugin messages.

## Transport

The channel is the existing local connection of `ilium-transport` (a Unix domain socket, a named pipe on Windows). `ilium-ipc` frames each message as a length prefix followed by a `bincode` body. The frame limit is 64 MiB. This page adds no new connection and no new framing.

| Direction | Existing type | New variant | Carries |
|---|---|---|---|
| Client to server | `ClientRequest` | `ClientRequest::Plugin(PluginEnvelope)` | Forwarded submissions, plugin messages, replies to server-initiated client requests, control messages |
| Server to client | `ServerEvent` | `ServerEvent::Plugin(PluginEnvelope)` | Outcomes, plugin messages, server-sourced events, client requests, control messages |
| Server to client | `ServerEvent` | `ServerEvent::Contributions(ContributionDelta)` | Retained contributions that the client draws |
| Server to client | `ServerEvent` | `ServerEvent::Rejected(Rejection)` | A refused handshake or envelope, with a named error |

```rust
pub struct PluginEnvelope {
    pub plugin: PluginId,        // owner of the traffic
    pub seq: u64,                // per connection and direction, from 1
    pub body: EnvelopeBody,
}

pub enum EnvelopeBody {
    Submit(Submission),                // client to server
    Outcome(OutcomeRecord),            // server to client
    Message(PluginMessage),            // both directions
    ClientRequest(ClientRequestCall),  // server to client
    ClientReply(ClientReplyBody),      // client to server
    Event(SourcedEvent),               // server to client, events the client half subscribed to
    Control(ControlBody),              // Subscribe, Resync, GrantsChanged, Stats
}
```

There is no `HookCall` variant. A hook never crosses a process (see [Scheduling](/pages/reference-host-scheduling/)).

- **[HST-100]** Plugin traffic MUST use `ClientRequest::Plugin`, `ServerEvent::Plugin`, `ServerEvent::Contributions` and `ServerEvent::Rejected` on the existing connection, and MUST NOT open another connection or change the framing.
- **[HST-101]** `EnvelopeBody` MUST NOT contain a variant that carries a hook call or any guest call. Guest calls MUST NOT cross a process boundary.
- **[HST-102]** `seq` MUST increase by one for each envelope in one direction of one connection. A receiver MUST treat a gap as a protocol error and close the connection with `ipc.sequence-gap`.

## Handshake

`Attach` and `AttachInteractive` gain a field `hello`.

```rust
pub struct PluginHello {
    pub plugin_protocol: u16,         // PLUGIN_PROTOCOL_VERSION = 1
    pub contract: String,             // "0.1.0"
    pub installed: Vec<InstalledPlugin>,
}
pub struct InstalledPlugin { pub id: PluginId, pub version: String, pub content_hash: [u8; 32] }
```

`ilium-ipc` gains the constant `PLUGIN_PROTOCOL_VERSION: u16 = 1`. No such constant exists before the plugin system.

| Case | Server answer | Client behavior |
|---|---|---|
| `plugin_protocol` and `contract` equal | `Attached` with the server's plugin set and a full contribution snapshot | Runs normally |
| `plugin_protocol` differs, or `contract` differs | `Rejected(ipc.protocol-mismatch)`, then close | Prints "client and server speak different plugin protocols; start the matching client" and exits with code 76 |
| Same protocol, but a plugin differs in version or hash between client and server | `Attached`; both sides list the skew | Only the client half of that plugin is disabled with `plugin.version-skew` |
| A plugin is installed in the client but not in the server | `Attached` | The client half is disabled with `plugin.server-missing` |
| A plugin is installed in the server but not in the client | `Attached` | The server half runs; its client-owned contributions are absent and shown as "needs client update" in the health view |

Code 76 and the key for the recovery page are chosen by this contract; see [Host architecture](/pages/reference-host-architecture/).

- **[HST-103]** A client MUST send `hello` in every `Attach` and `AttachInteractive`. A server MUST answer a mismatch of `plugin_protocol` or `contract` with `Rejected(ipc.protocol-mismatch)` and close the connection.
- **[HST-104]** A client that receives `ipc.protocol-mismatch` MUST print the cause and exit with code 76 without drawing the interface.
- **[HST-105]** A plugin-set skew MUST disable only the client half of the affected plugin, with `plugin.version-skew` or `plugin.server-missing`. The server half and all other plugins MUST keep running.
- **[HST-106]** The protocol constant MUST be a `u16` named `PLUGIN_PROTOCOL_VERSION` in `ilium-ipc`, and MUST change only when the envelope or handshake layout changes.

<figure>
<svg viewBox="0 0 440 210" role="img" aria-label="Sequence of attach with plugin hello, snapshot, and a plugin-set skew">
<title>Attach sequence</title>
<desc>The client sends Attach with hello. The server checks protocol and contract. On a match the server sends Attached, the plugin set and a contribution snapshot. On a mismatch the server sends Rejected and the client exits with code 76.</desc>
<defs>
<marker id="ipc1-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<text x="70" y="16" font-size="11" text-anchor="middle" fill="currentColor">Client</text>
<text x="370" y="16" font-size="11" text-anchor="middle" fill="currentColor">Server</text>
<line x1="70" y1="24" x2="70" y2="200" stroke="currentColor" stroke-dasharray="4 4"/>
<line x1="370" y1="24" x2="370" y2="200" stroke="currentColor" stroke-dasharray="4 4"/>
<line x1="70" y1="44" x2="370" y2="44" stroke="currentColor" marker-end="url(#ipc1-arrow)"/>
<text x="220" y="38" font-size="10" text-anchor="middle" fill="currentColor">Attach + hello (protocol 1, 0.1.0, plugin set)</text>
<rect x="290" y="54" width="160" height="22" rx="4" fill="none" stroke="currentColor"/>
<text x="370" y="69" font-size="9" text-anchor="middle" fill="currentColor">compare protocol and contract</text>
<line x1="370" y1="96" x2="70" y2="96" stroke="currentColor" marker-end="url(#ipc1-arrow)"/>
<text x="220" y="90" font-size="10" text-anchor="middle" fill="currentColor">match: Attached + plugin set</text>
<line x1="370" y1="118" x2="70" y2="118" stroke="currentColor" marker-end="url(#ipc1-arrow)"/>
<text x="220" y="112" font-size="10" text-anchor="middle" fill="currentColor">Contributions snapshot (seq 1)</text>
<line x1="370" y1="152" x2="70" y2="152" stroke="currentColor" marker-end="url(#ipc1-arrow)"/>
<text x="220" y="146" font-size="10" text-anchor="middle" fill="currentColor">mismatch: Rejected(ipc.protocol-mismatch)</text>
<text x="76" y="176" font-size="10" fill="currentColor">exit 76</text>
</svg>
<figcaption>On a mismatch the client prints the cause and exits. On a match it receives the full snapshot before the first delta.</figcaption>
</figure>

## Contributions: the delta stream

Retained contributions that the client draws (decorations, status segments, frame slots, virtual rows, row actions, annotations that a server plugin owns) are replicated as a stream of **deltas**. The server owns the registry. Each client holds a mirror.

```rust
pub struct ContributionDelta {
    pub sequence: u64,             // per connection, from 1; snapshot has sequence 1
    pub kind: DeltaKind,           // Snapshot | Increment
    pub ops: Vec<ContributionOp>,  // Put(id, entry) | Remove(id) | Clear(plugin)
}
```

- On attach the server sends one `Snapshot` and then `Increment`s with consecutive `sequence`.
- A client that sees a `sequence` gap discards its mirror, sends `Control::Resync`, and the server answers with a new `Snapshot`.
- A delta is at most 4 MiB. A larger change is split into several increments.
- Increments are **coalescable**: when the writer queue is under pressure, several increments for the same id are merged into one.

- **[HST-107]** The first contribution message after attach MUST be a `Snapshot`, and every later one an `Increment` with the next `sequence`.
- **[HST-108]** A client MUST resync on a `sequence` gap, and the server MUST answer `Control::Resync` with a full `Snapshot`.
- **[HST-109]** A delta MUST NOT exceed 4 MiB.

## The mirrored tree

The client keeps a **mirror** of the tree: nodes, kinds, titles, status, attention and the metadata keys that plugins may read. The server sends tree changes as before, and a plugin half in the client reads the mirror.

The order matters for correctness. A plugin on the client submits an intent such as `create-pane`. The server applies it, the tree changes, and the client's plugin half expects to find the new node when it receives the completion.

- **[HST-110]** The server MUST send the tree change caused by a forwarded submission BEFORE the `request-completed` outcome of the same submission, on the same connection.
- **[HST-111]** The mirror MUST be updated before the client half's event queue receives the matching `tree-changed` event, and a `request-completed` MUST NOT be delivered to a plugin before the mirror shows the effect.

## Forwarded submissions

A client-hosted plugin cannot change the tree itself. Its intents are forwarded to the server in `Submit` envelopes. The sequence for one intent is as follows.

<figure>
<svg viewBox="0 0 440 230" role="img" aria-label="Sequence of a submission from a client plugin: submit, apply, tree change, outcome">
<title>Forwarded submission</title>
<desc>The client plugin returns a submission. The client forwards it as Submit. The server checks the grant, applies the effect, sends the tree change, then sends the outcome. The client delivers request-completed to the plugin.</desc>
<defs>
<marker id="ipc2-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<text x="50" y="16" font-size="11" text-anchor="middle" fill="currentColor">Plugin</text>
<text x="200" y="16" font-size="11" text-anchor="middle" fill="currentColor">Client</text>
<text x="380" y="16" font-size="11" text-anchor="middle" fill="currentColor">Server</text>
<line x1="50" y1="24" x2="50" y2="220" stroke="currentColor" stroke-dasharray="4 4"/>
<line x1="200" y1="24" x2="200" y2="220" stroke="currentColor" stroke-dasharray="4 4"/>
<line x1="380" y1="24" x2="380" y2="220" stroke="currentColor" stroke-dasharray="4 4"/>
<line x1="50" y1="44" x2="200" y2="44" stroke="currentColor" marker-end="url(#ipc2-arrow)"/>
<text x="125" y="38" font-size="9" text-anchor="middle" fill="currentColor">returns intent</text>
<line x1="200" y1="72" x2="380" y2="72" stroke="currentColor" marker-end="url(#ipc2-arrow)"/>
<text x="290" y="66" font-size="9" text-anchor="middle" fill="currentColor">Submit (dedup key)</text>
<rect x="320" y="82" width="120" height="22" rx="4" fill="none" stroke="currentColor"/>
<text x="380" y="97" font-size="9" text-anchor="middle" fill="currentColor">grant check, apply</text>
<line x1="380" y1="124" x2="200" y2="124" stroke="currentColor" marker-end="url(#ipc2-arrow)"/>
<text x="290" y="118" font-size="9" text-anchor="middle" fill="currentColor">tree change</text>
<line x1="380" y1="150" x2="200" y2="150" stroke="currentColor" marker-end="url(#ipc2-arrow)"/>
<text x="290" y="144" font-size="9" text-anchor="middle" fill="currentColor">Outcome (ok)</text>
<line x1="200" y1="180" x2="50" y2="180" stroke="currentColor" marker-end="url(#ipc2-arrow)"/>
<text x="125" y="174" font-size="9" text-anchor="middle" fill="currentColor">request-completed</text>
</svg>
<figcaption>The tree change is sent before the outcome, so the plugin finds the new node when it hears that the request completed.</figcaption>
</figure>

Deduplication works as in [Requests and outcomes](/pages/requests-and-outcomes/), with one change for forwarded submissions: the dedup scope is the pair (plugin, client id), and the server keeps the table for 10 minutes, measured on the wall clock from the accepted time that the journal persists (a key is free at exactly 600,000 ms). Two clients that submit the same key for the same plugin are two submissions.

- **[HST-112]** A client MUST forward each submission of a client-hosted plugin in a `Submit` envelope and MUST NOT apply tree or session effects itself.
- **[HST-113]** The dedup scope of a forwarded submission MUST be the pair (plugin id, client id), the window MUST be 10 minutes, and the table MUST be held by the server.
- **[HST-114]** The server MUST check the grant of the plugin at the time of application, not at the time of sending, and a submission whose grant was revoked in between MUST complete as `revoked`.
- **[HST-115]** The server MUST answer each `Submit` with exactly one `Outcome`.

## Reconnect and outcome replay

A client can lose the connection and attach again. The server keeps outcome records for a client id for a **grace period of 30 seconds**.

- On re-attach with the same client id and the last `seq` that the client saw, the server replays the outcome records with a higher `seq`, up to 1,024 records, and then sends a new contribution snapshot.
- The client keeps the submissions that have no outcome. After re-attach it sends them again with the same dedup keys. The server answers a duplicate from the table.
- After the grace period, the server discards the records. Submissions of that client whose effects the server has not applied complete as `cancelled` and nothing replays them.
- A client that sends a `Submit` while detached queues it, with a limit of 256 submissions. The 257th queued submission completes as `refused(busy)` at once. Submissions still queued when the client exits complete as `cancelled`.

- **[HST-116]** The server MUST keep outcome records for 30 seconds after a client connection closes, at most 1,024 records per client id.
- **[HST-117]** On re-attach within the grace period the server MUST replay outcomes with a higher `seq` than the client reports and MUST then send a contribution snapshot.
- **[HST-118]** After the grace period, unapplied submissions of that client MUST complete as `cancelled`.
- **[HST-119]** A detached client MUST queue at most 256 submissions and MUST complete later ones as `refused(busy)` at once. Submissions still queued when the client exits MUST complete as `cancelled`.

## Writer fairness

The server writes frames to each client through one writer. Pane frames (terminal output) are the priority, because the user sees them as typing latency. Plugin traffic gets a guaranteed share so that it cannot starve either.

| Rule | Value |
|---|---|
| Order | PTY frames first |
| Guaranteed plugin share | 1 plugin envelope for every 8 frames when both queues are non-empty |
| Plugin queue bound | 4 MiB or 4,096 records, whichever comes first |
| Overflow of the plugin queue | Coalescable increments are merged, then `Control::Resync` is flagged for that client |
| Hard cap | 16 MiB of queued plugin data disconnects the client with `ipc.client-too-slow` |

A client that reconnects after `ipc.client-too-slow` receives a snapshot. Pane output is never dropped by this rule.

- **[HST-120]** The writer MUST send PTY frames before plugin envelopes and MUST guarantee one plugin envelope for every eight frames while plugin data is queued.
- **[HST-121]** The plugin queue MUST be bounded at 4 MiB or 4,096 records. On overflow the server MUST merge coalescable increments first and flag a resync second.
- **[HST-122]** A client whose queued plugin data exceeds 16 MiB MUST be disconnected with `ipc.client-too-slow`.
- **[HST-123]** Plugin traffic MUST NOT delay a PTY frame by more than the time to write one plugin envelope.

## Payload limits

All caps are below the 64 MiB frame limit. The host checks them before encoding.

| Payload | Cap | Error |
|---|---|---|
| Plugin message body | 1 MiB | `invalid-argument` (the plugin call; see the limits table) |
| Topic payload | 64 KiB | `invalid-argument` (the plugin call; see the limits table) |
| `ui-tree` of a view | 1 MiB, 4 MiB with the T1 ceiling | `ipc.payload-too-large` |
| Surface write | 16 MiB | `ipc.payload-too-large` |
| Whole envelope | about 17 MiB | `ipc.payload-too-large` |
| Contribution delta | 4 MiB | `ipc.payload-too-large` |

An oversized item is refused at the sender. The sender's call returns `ipc.payload-too-large`, except a plugin message or topic over its cap: the plugin call returns `invalid-argument`, as in the limits table.

- **[HST-124]** The sender MUST check the caps above and refuse an oversized item with `ipc.payload-too-large` before it is encoded.
- **[HST-125]** A receiver MUST drop an envelope above 17 MiB and close the connection with `ipc.payload-too-large`.

## Errors

| Id | When | Effect |
|---|---|---|
| `ipc.protocol-mismatch` | Different `plugin_protocol` or `contract` | Rejected; client exits 76 |
| `ipc.sequence-gap` | A gap in envelope `seq` | Connection closed; client re-attaches |
| `ipc.payload-too-large` | A cap above is exceeded | Item refused; the sender's call returns this code |
| `ipc.client-too-slow` | More than 16 MiB queued | Client disconnected |
| `plugin.version-skew` | Client and server plugin differ | Client half disabled |
| `plugin.server-missing` | Plugin not installed on the server | Client half disabled |

## Visibility of coalesced and dropped plugin traffic

The queue rules bound plugin traffic by merging or dropping. Without a signal, an operator sees a plugin that appears to lag and cannot tell why.

- **[HST-522]** The host MUST count, per plugin and per client, the records that were coalesced, the resyncs that were flagged and the disconnects caused by an overloaded queue, and `ilium plugin status` MUST show these counts with the time of the latest occurrence.
- **[HST-523]** The host MUST write a log line `ipc.plugin-coalesced` naming the plugin and the kind of traffic at most once per minute per plugin and client, and when it disconnects a client for `ipc.client-too-slow` it MUST name the plugin with the largest queued share. The user interface MUST show a non-modal notice naming the plugin when its traffic has been coalesced for more than 10 s without a break.

## See also

- [Host scheduling](/pages/reference-host-scheduling/) for what happens on the client between messages.
- [Persistence and restart](/pages/reference-persistence-and-restart/) for state across restarts.
- [Requirements: kernel and host](/pages/reference-requirements-kernel-host/).

API version checked: `ilium:plugin@0.1.0`.
