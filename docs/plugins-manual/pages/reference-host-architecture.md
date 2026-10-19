---
layout: page
title: Host architecture
permalink: /pages/reference-host-architecture/
---

## Host architecture

This page is the normative description of the plugin host: which crates it is made of, which operating system processes run plugin code, which side and tier each process hosts, who owns each registry, how the host starts and stops, and what a failure takes down. A developer who implements the host builds it from this page and the pages it links. A test author derives the structural tests from the requirement ids in square brackets, for example `[HST-001]`.

The words MUST, SHOULD and MAY have their usual meaning in a specification. MUST is a requirement that a test checks. SHOULD is a default that an implementation may change with a stated reason. MAY is an option. The ids are collected, with a test method for each, in [Requirements: kernel and host](/pages/reference-requirements-kernel-host/).

Terms used here are defined once. A **host** is the code that loads plugin components, calls their exports and serves their imports in one process for one side. A **side** is one of `server`, `client` or `scene` (see [Worlds and sides](/pages/worlds-and-sides/)). A **tier** is T0 to T3 (see [Architecture](/pages/architecture/)). A **Store** is the Wasmtime object that owns the memory and state of one component instance. An **Engine** is the Wasmtime object that holds the compiler and its configuration. The **dispatcher** is the host component that turns kernel events into batches for plugin queues and turns returned submissions into kernel work. The **sandbox helper** is the separate process that runs T2 and T3 components.

## You will need

- The concept pages [Architecture](/pages/architecture/), [Worlds and sides](/pages/worlds-and-sides/) and [Lifecycle](/pages/lifecycle/).
- For the numbers that this page refers to, [Host runtime](/pages/reference-host-runtime/), [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/) and [Performance budgets and benchmarks](/pages/reference-host-budgets/).

## Crate layout

The platform adds four crates to the workspace and changes the boundaries of two existing ones. The rule that governs the layout is the existing layering rule of the repository: a pure crate has no I/O, an adapter crate owns one external system, and no crate reaches around the crate that owns a concern.

| Crate | Kind | Contents | Must not contain |
|---|---|---|---|
| `ilium-plugin-api` | pure | Manifest types, capability names and risk classes, contribution types, event, intent, outcome and error types mirrored from the WIT package `ilium:plugin@0.1.0`, limit keys and ceilings, requirement-neutral constants such as epoch period | Wasmtime, V8, tokio, file or socket access |
| `ilium-plugin-host` | adapter | The Wasmtime embedding, linker construction, instantiation, the dispatcher, call classes and deadlines, quota accounting, the helper client, the telemetry registry, the conformance harness hooks | Terminal, tree or UI logic; direct `libc`, `std::os::unix` or `windows-sys` use |
| `ilium-plugin-helper` | binary | The sandbox helper: a small `main` that confines itself, owns one Engine and one Store set, and speaks the parent-helper protocol | Any path to the session server or the user's files except granted scopes |
| `ilium-plugin-sdk` | library for plugins | The Rust SDK that plugins depend on, generated from the WIT package, plus the `#[ilium_plugin]` macro (`@ilium/plugin-sdk` is the TypeScript counterpart) | Any Ilium crate dependency |
| `ilium-plugin-conformance` | test only | Language-neutral fixtures and the Rust and TypeScript guest components that exercise each interface | Production code |
| `ilium-plugin-bench` | test only | The benchmark programs B1 to B24 of [Performance budgets and benchmarks](/pages/reference-host-budgets/) | Production code |

The helper binary is installed beside the `ilium` and `ilium-server` binaries by the same installer, and the host finds it by looking in the directory of its own executable. A missing helper is not an error for T1-only installs: T2 and T3 packages stay installed but disabled with `host.helper-missing` (see [Sandbox helper](/pages/reference-host-helper/)).

### Dependency edges

The table lists every allowed edge that involves a plugin crate. Any edge not listed is forbidden.

| From | To | Allowed | Why |
|---|---|---|---|
| `ilium-plugin-host` | `ilium-plugin-api` | yes | The host serves the types the API crate defines |
| `ilium-plugin-host` | `ilium-execution` | yes | Quota groups and the job worker bank |
| `ilium-plugin-host` | `ilium-platform` | yes | Process control, secure file access, file locks, thread priority |
| `ilium-plugin-host` | `ilium-logging` | yes | The plugin log and host log |
| `ilium-plugin-host` | `ilium-transport` | no | The host does not own a connection to clients |
| `ilium-plugin-host` | `ilium-core` | no | The host sees the tree only through a trait that the server and client implement (the **kernel facade**) |
| `ilium-server` | `ilium-plugin-host` | yes | Owns the server host and implements the kernel facade |
| `ilium-client` | `ilium-plugin-host` | yes, through one module | Owns the client host and scene host |
| `ilium-client` UI modules (`ui`, `tree_ui`, `modal` and the other presentation modules) | Wasmtime types | no | UI code sees plugin output only as `ilium-plugin-api` values |
| `ilium-ipc` | `ilium-plugin-api` | yes | Carries typed plugin records |
| `ilium-ipc` | `ilium-plugin-host` | no | IPC never carries Wasmtime values |
| `ilium-core` | any plugin crate | no | Core stays pure domain |
| `ilium-detect` | any plugin crate | no | Detection stays a pure function |
| `ilium-pty` | any plugin crate | no | PTY code never learns about plugins |
| `ilium` (the bin) | `ilium-plugin-host` | yes | The CLI process host (`requires_server: false`) |
| plugin source | any Ilium crate | no | Plugins depend only on `ilium-plugin-sdk` |

- **[HST-001]** The workspace MUST contain the crates `ilium-plugin-api`, `ilium-plugin-host` and `ilium-plugin-helper`, and `ilium-plugin-api` MUST NOT depend on Wasmtime, V8, tokio or any I/O crate.
- **[HST-002]** The dependency edges in the table above MUST hold, and a workspace test MUST fail when `cargo metadata` shows a forbidden edge.
- **[HST-003]** `ilium-core` MUST NOT depend on any plugin crate.
- **[HST-004]** `ilium-ipc` MUST carry plugin traffic as typed records from `ilium-plugin-api` and MUST NOT carry or link Wasmtime values.
- **[HST-005]** Client presentation modules MUST NOT name Wasmtime types. Only one client module, `plugin_host`, MAY depend on `ilium-plugin-host`.
- **[HST-006]** `ilium-plugin-host` MUST reach the tree, panes, settings and commands only through the kernel facade trait, so that the host can be tested with a fake kernel and no PTY.

The kernel facade is a Rust trait with one method per intent family and one per host import family. Its shape is an implementation choice, but it MUST be object safe and MUST return the outcome types of `ilium-plugin-api`.

```rust
// ilium-plugin-host/src/facade.rs (illustration of the contract, not generated)
pub trait KernelFacade: Send + Sync {
    /// Apply one intent that the caller already authorised. The facade
    /// never sees Wasmtime values; it sees api-crate types only.
    fn apply_intent(&self, origin: &Origin, intent: Intent) -> IntentResult;
    /// Read-only views of the tree, panes and agents for host imports.
    fn tree(&self) -> &dyn TreeRead;
    fn panes(&self) -> &dyn PaneRead;
    /// Which side this facade serves (server, client or cli).
    fn process_kind(&self) -> ProcessKind;
}
```

## Process map

Four kinds of operating system process can run plugin code. The table is the complete list.

| Process | How many | Hosts which sides | Hosts which tiers | Thread that owns the host | When the process exits |
|---|---|---|---|---|---|
| Session server (`ilium-server`) | one per session | `server` | T1 in process; T2 and T3 through helpers | The server runtime: the dispatcher task runs on the server's tokio runtime, calls run on the plugin executor | Shutdown sequence below; every helper it spawned is terminated; clients see the connection close |
| Client (`ilium`, attached) | one per attached client | `client`, `scene` | T1 in process; T2 and T3 through helpers | The client event loop owns the host object; calls run on the client plugin executor and the scene thread | The client host stops; the server and other clients are unaffected |
| CLI process (`ilium <subcommand>` for a `cli` entry with `requires_server: false`) | one per invocation | `server` component only | T1 in process; T2 and T3 through helpers | The CLI main thread owns the host; calls run on a one-thread executor | The host deactivates the plugin with `shutdown` and drops everything |
| Sandbox helper (`ilium-plugin-helper`) | one per T2 or T3 plugin per side per hosting process | the side of its plugin | T2 and T3 only | The helper main thread runs a single-instance call loop; the parent owns the helper | The parent counts a failure for that plugin side and restarts it under the restart rule |

A CLI process that connects to a running server (`requires_server: true`, the default) hosts nothing. It sends its request to the server, and the server host runs the plugin's `cli-invoked` handler.

- **[HST-007]** A plugin side MUST be hosted by exactly one process at a time: the session server for `server`, an attached client for `client` and `scene`, or a CLI process for a `server` component that is run for a `requires_server: false` entry.
- **[HST-008]** T1 components MUST run inside the process of their side and MUST NOT run in a helper.
- **[HST-009]** T2 and T3 components MUST run only inside a sandbox helper, and the parent process MUST NOT instantiate a T2 or T3 component in its own Engine.
- **[HST-010]** The exit of a client process, a CLI process or a helper MUST NOT stop the session server or any other plugin side.

The server does not run `client` or `scene` components, and a client does not run `server` components. This corrects the loose phrase "the Ilium process" that earlier pages used: wherever a page says that a T1 plugin runs "in the Ilium process", it means the process that hosts that plugin's side, as this table defines.

### Diagram: processes, hosts and links

<figure>
<svg viewBox="0 0 440 330" role="img" aria-label="Session server, two clients, a CLI process and sandbox helpers with their plugin hosts and links">
<title>Processes, hosts and IPC links</title>
<desc>The session server holds the kernel and the server host. Two client processes each hold a client host and a scene host. A CLI process holds a one-shot server host. Helpers are child processes of whichever process owns the plugin side. Solid lines are the session socket. Dashed lines are helper pipes.</desc>
<defs>
<marker id="ha-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<rect x="150" y="8" width="150" height="104" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="225" y="26" font-size="11" text-anchor="middle" fill="currentColor">Session server</text>
<rect x="162" y="36" width="126" height="28" rx="4" fill="none" stroke="currentColor"/>
<text x="225" y="54" font-size="10" text-anchor="middle" fill="currentColor">Kernel and tree</text>
<rect x="162" y="72" width="126" height="32" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="225" y="86" font-size="10" text-anchor="middle" fill="currentColor">Server host</text>
<text x="225" y="98" font-size="9" text-anchor="middle" fill="currentColor">T1 in process</text>
<rect x="8" y="150" width="150" height="86" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="83" y="168" font-size="11" text-anchor="middle" fill="currentColor">Client A</text>
<rect x="18" y="176" width="130" height="24" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="83" y="192" font-size="10" text-anchor="middle" fill="currentColor">Client host (T1)</text>
<rect x="18" y="204" width="130" height="24" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="83" y="220" font-size="10" text-anchor="middle" fill="currentColor">Scene host (T1)</text>
<rect x="292" y="150" width="140" height="86" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="362" y="168" font-size="11" text-anchor="middle" fill="currentColor">Client B</text>
<rect x="302" y="176" width="120" height="24" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="362" y="192" font-size="10" text-anchor="middle" fill="currentColor">Client host (T1)</text>
<rect x="302" y="204" width="120" height="24" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="362" y="220" font-size="10" text-anchor="middle" fill="currentColor">Scene host (T1)</text>
<line x1="160" y1="112" x2="100" y2="150" stroke="currentColor" marker-end="url(#ha-arrow)" marker-start="url(#ha-arrow)"/>
<line x1="290" y1="112" x2="350" y2="150" stroke="currentColor" marker-end="url(#ha-arrow)" marker-start="url(#ha-arrow)"/>
<text x="106" y="124" font-size="9" fill="currentColor">session socket</text>
<rect x="170" y="150" width="112" height="56" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>
<text x="226" y="168" font-size="10" text-anchor="middle" fill="currentColor">CLI process</text>
<text x="226" y="182" font-size="9" text-anchor="middle" fill="currentColor">requires_server: false</text>
<text x="226" y="196" font-size="9" text-anchor="middle" fill="currentColor">one-shot server host</text>
<rect x="8" y="270" width="120" height="44" rx="4" fill="none" stroke="currentColor"/>
<text x="68" y="288" font-size="10" text-anchor="middle" fill="currentColor">Helper (T2/T3)</text>
<text x="68" y="302" font-size="9" text-anchor="middle" fill="currentColor">one per plugin side</text>
<rect x="160" y="270" width="120" height="44" rx="4" fill="none" stroke="currentColor"/>
<text x="220" y="288" font-size="10" text-anchor="middle" fill="currentColor">Helper (T2/T3)</text>
<text x="220" y="302" font-size="9" text-anchor="middle" fill="currentColor">child of the server</text>
<rect x="312" y="270" width="120" height="44" rx="4" fill="none" stroke="currentColor"/>
<text x="372" y="288" font-size="10" text-anchor="middle" fill="currentColor">Helper (T2/T3)</text>
<text x="372" y="302" font-size="9" text-anchor="middle" fill="currentColor">child of client B</text>
<line x1="83" y1="236" x2="68" y2="270" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#ha-arrow)"/>
<line x1="372" y1="236" x2="372" y2="270" stroke="currentColor" stroke-dasharray="4 3" marker-end="url(#ha-arrow)"/>
<path d="M225 112 L225 130 L440 130 L440 252 L230 252 L220 270" stroke="currentColor" stroke-dasharray="4 3" fill="none" marker-end="url(#ha-arrow)"/>
</svg>
<figcaption>The server and every client own their own host. A helper is a child of the process that owns its plugin side. The only link between server and client is the session socket.</figcaption>
</figure>

## Per-side hosting

### Server host

There is one server host per session server. It owns the Engine of the server process, the plugin executor of the server, the dispatcher, the quota accounting for server-side plugin sides and the telemetry registry of the server. It hosts every `server` side of every enabled plugin, and the server side of a two-sided package. It is the only host that subscribes to the event journal (see [Events](/pages/api-events/)).

- **[HST-011]** A session MUST have exactly one server host, and the dispatcher MUST be the only subscriber of the kernel event journal on behalf of plugins.
- **[HST-012]** Server-side plugin state (queues, failure counts, restart timers, quota buckets) MUST be kept per plugin per side in the server host and MUST survive the detach and attach of clients.

### Client host

There is one client host per attached client process. Two clients attached to the same session have separate client hosts, separate Stores, separate widget state, separate failure counts and separate restart timers. A plugin that fails in client A keeps running in client B.

- **[HST-013]** Each attached client MUST own its client host, and a failure in one client host MUST NOT change the failure count, restart timer or enabled state of the same plugin in another client.
- **[HST-014]** A plugin disabled by the restart rule in one client MUST stay enabled in other clients, and the notice MUST be shown only to the client that disabled it.

### Scene host

The scene host runs inside the client process. It is a separate set of Stores (one per running scene instance) that share the client's Engine and epoch ticker. It has its own thread, the scene thread, so a slow frame never delays the client's render or input thread. The scene thread produces frames into a double buffer, and the compositor reads the newest complete frame at each client frame. The details are in [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/) and [Host rendering pipeline](/pages/reference-host-rendering/).

- **[HST-015]** Scene instances MUST live in Stores that are separate from the client host's Stores, and scene frame calls MUST run on a thread that is neither the client UI thread nor a client plugin executor thread.
- **[HST-016]** A scene frame that is not ready when the compositor needs it MUST NOT delay the client frame. The compositor MUST reuse the previous complete frame.

### Job instances

A plugin that uses `jobs.run` gets a second kind of instance: a **job instance** of the same component, created for one job from the `InstancePre` of the plugin side (design decision in [Background jobs](/pages/background-jobs/)). Job instances are not the plugin's main instance. They have their own Store, their own memory limit (`job_memory_mib`) and no access to the plugin's main instance state. The number running at once is the plugin's `concurrent_jobs`. They run on the job worker bank of `ilium-execution`, not on the plugin executor. See [Jobs](/pages/api-jobs/).

- **[HST-017]** A job MUST run in its own Store, MUST NOT share linear memory with the plugin's main instance, and MUST run on the job worker bank.
- **[HST-018]** The count of live job instances of one plugin side MUST NOT exceed `concurrent_jobs`; further jobs wait in the job queue.

### CLI-process host

A `cli` entry with `requires_server: false` runs the plugin's `server` component in the CLI process. The host starts when the plugin is first needed for the command, activates the plugin with the trigger `cli:<name>`, delivers one `cli-invoked` event, waits until the plugin has returned its exit status (or the command deadline expires), and then deactivates with `shutdown` and exits. No tree exists in that process. Host imports that need the tree, panes or agents return `unavailable` with the message `no session`.

- **[HST-019]** A CLI-process host MUST host only the plugin named by the entry plus the services it `requires`, and MUST NOT activate any other plugin.
- **[HST-020]** In a CLI-process host, imports that need a session MUST return `unavailable` and MUST NOT start a server.

## Ownership

One Engine per process. One Store per plugin instance. The table assigns each registry to exactly one owner and says how others see it.

| Registry | Owner | Persisted where | How other processes see it |
|---|---|---|---|
| Package store (installed packages, content-addressed) | The package directory under the Ilium data directory, written by `ilium plugin install` | `<data dir>/plugins/packages/<id>/<version>/` | Every host reads it at start; the handshake carries id, version and content hash (see [Host IPC protocol](/pages/reference-host-ipc/)) |
| Grant store | The server host (single writer, file lock from `ilium-platform`) | `$XDG_STATE_HOME/ilium/grants.json` (mode 0600), written atomically | The server pushes `grants-changed` control records to clients; a CLI-process host reads the file under a shared lock |
| Contribution registry (commands, settings tabs, decoration slots, panes, toolbar items, keys, scenes) | Static part: each host for the sides it hosts. Retained part (decorations, virtual rows, status segments, frame slots, attention): the server host | Not persisted except what the plugin saves | Retained server contributions replicate to every client as `ContributionDelta` |
| Extension-point registry | The host of the side that owns the point | Not persisted | A client-owned point exists only in that client; a server-owned point only on the server (see [Extension points](/pages/api-extension-points/)) |
| Service registry | The server host for `server` providers, each client host for `client` providers | Not persisted | A service call is routed to the side that the manifest names; a cross-side call travels as a message |
| Plugin enable state and failure counters | The host of the side, in memory; the enable flag is stored in the user config | `config.toml` `[plugins.<id>]` for the flag only | The flag is read at start; counters never leave the process |
| Quota groups | `ilium-execution`, one group per plugin side per process | Not persisted | Telemetry only |
| Telemetry registry | Each host | Not persisted | `ilium plugin stats` reads the server's registry and each client's through the `Control` envelope (see [Host telemetry](/pages/reference-host-telemetry/)) |

- **[HST-021]** A process MUST create exactly one Wasmtime `Engine` for plugin use, and every Store in that process MUST be created from it.
- **[HST-022]** Each plugin instance, including each job instance, MUST have its own `Store`.
- **[HST-023]** The grant store MUST have a single writer at any time and MUST be replaced atomically, so a reader never sees a partial file.
- **[HST-024]** The retained contribution registry for server-side plugins MUST be owned by the server host, and a client MUST hold only a replica built from `ContributionDelta` records.
- **[HST-025]** The extension-point registry and the service registry MUST be per hosting process. A point or service owned by a client side MUST NOT be visible to server-side plugins.

## Startup and shutdown

### Server start

1. The kernel boots and loads the configuration (`config.toml`, including `[plugins]` and `[plugins.limits]`).
2. The kernel restores the snapshot (`restore_snapshot`). No plugin exists yet, so no plugin sees the restore as a series of events.
3. The host creates the Engine, starts the epoch ticker and the plugin executor (see [Host runtime](/pages/reference-host-runtime/)).
4. The host loads the package store. For every installed and enabled package it verifies the manifest, the signature and the content hash, checks the API version, validates the component, and registers static contributions. These are the eager steps. Nothing is instantiated.
5. The host builds the service graph. A cycle in `requires.services` is refused with `manifest.service-cycle` (a `requires.plugins` cycle is refused at install with `dependency.cycle`); the plugins in the cycle stay disabled.
6. The host activates every plugin whose `activation` list contains `startup`, in dependency order: a service provider before its consumers. Other plugins stay inactive until their trigger fires. These are the lazy steps.
7. The dispatcher delivers exactly one `tree-changed` batch for the restored tree to each activated plugin that subscribes to it. The restore is never replayed.
8. The server accepts client connections. (Clients that connect earlier than step 6 completes are refused with the retriable error `server.starting`; a client retries for 5 seconds.)

Steps 3 to 7 have the time budget `HOST-START` in [Performance budgets and benchmarks](/pages/reference-host-budgets/).

### Server shutdown

1. The server stops accepting new clients and stops the services that depend on plugins in their declared order.
2. The host refuses new submissions. They complete as `cancelled`.
3. Each active plugin side receives `deactivate(shutdown)` in reverse activation order, each within `deactivate_deadline_ms`. The total for all plugins is bounded by 5 seconds; plugins not reached in that time are dropped without a call.
4. Pending requests are cancelled, helpers are terminated, instances are dropped and quota groups are released.
5. The server runs `drain_session_work` and writes the final snapshot. The plugin host has stopped, so no plugin intent can reach the snapshot after the writer starts.

### Client start and stop

On start, the client connects, completes the handshake, receives the full retained contribution replica, starts its `startup` client plugins, renders the first frame and activates other plugins when their trigger fires. On stop, the client host deactivates its plugins with `shutdown` (total bound 2 seconds), cancels in-flight submissions locally and disconnects. The server learns of the detach from the closed connection (see [Host IPC protocol](/pages/reference-host-ipc/)).

### CLI process

The CLI process creates an Engine, loads only the needed package, activates it, delivers the event, deactivates it and exits. Its time to first plugin call has the budget `CLI-COLD` in [Performance budgets and benchmarks](/pages/reference-host-budgets/).

- **[HST-026]** The server host MUST start after `restore_snapshot` and MUST deliver exactly one `tree-changed` batch for the restored tree to each plugin that subscribes to it, and no replay of restore steps.
- **[HST-027]** Manifest verification, signature verification, component validation and static contribution registration MUST be completed eagerly before any plugin is activated. Instantiation MUST be lazy except for plugins with the `startup` trigger.
- **[HST-028]** A service provider MUST be activated before the plugins that `requires` it, and a cycle in `requires.services` MUST leave the plugins in the cycle disabled with `manifest.service-cycle`.
- **[HST-029]** On server shutdown the host MUST stop after the services stop and before `drain_session_work` and the final snapshot, and MUST refuse new submissions as `cancelled` before the first `deactivate`.
- **[HST-030]** `deactivate(shutdown)` MUST be called in reverse activation order, and the total shutdown time spent in plugin calls MUST NOT exceed 5 seconds.
- **[HST-031]** A client that connects before the server host finished step 6 MUST be refused with the retriable error `server.starting`.
- **[HST-032]** The client host MUST bound its own shutdown at 2 seconds in total, and a client that exits MUST NOT prevent the server from delivering outcomes for other clients.

<figure>
<svg viewBox="0 0 440 150" role="img" aria-label="Server start steps in order and server shutdown steps in order">
<title>Start and stop order</title>
<desc>Start runs boot, restore, engine, verify, activate, one tree-changed, accept clients. Stop runs services, refuse, deactivate, cancel, drain and final snapshot.</desc>
<defs>
<marker id="hb-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<text x="8" y="16" font-size="11" fill="currentColor">Start</text>
<rect x="8" y="22" width="56" height="30" rx="4" fill="none" stroke="currentColor"/>
<text x="36" y="41" font-size="9" text-anchor="middle" fill="currentColor">Boot</text>
<rect x="76" y="22" width="56" height="30" rx="4" fill="none" stroke="currentColor"/>
<text x="104" y="41" font-size="9" text-anchor="middle" fill="currentColor">Restore</text>
<rect x="144" y="22" width="56" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="172" y="41" font-size="9" text-anchor="middle" fill="currentColor">Engine</text>
<rect x="212" y="22" width="56" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="240" y="41" font-size="9" text-anchor="middle" fill="currentColor">Verify</text>
<rect x="280" y="22" width="56" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="308" y="41" font-size="9" text-anchor="middle" fill="currentColor">Activate</text>
<rect x="348" y="22" width="84" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="390" y="41" font-size="9" text-anchor="middle" fill="currentColor">One tree-changed</text>
<line x1="64" y1="37" x2="76" y2="37" stroke="currentColor" marker-end="url(#hb-arrow)"/>
<line x1="132" y1="37" x2="144" y2="37" stroke="currentColor" marker-end="url(#hb-arrow)"/>
<line x1="200" y1="37" x2="212" y2="37" stroke="currentColor" marker-end="url(#hb-arrow)"/>
<line x1="268" y1="37" x2="280" y2="37" stroke="currentColor" marker-end="url(#hb-arrow)"/>
<line x1="336" y1="37" x2="348" y2="37" stroke="currentColor" marker-end="url(#hb-arrow)"/>
<text x="8" y="86" font-size="11" fill="currentColor">Stop</text>
<rect x="8" y="92" width="68" height="30" rx="4" fill="none" stroke="currentColor"/>
<text x="42" y="111" font-size="9" text-anchor="middle" fill="currentColor">Services</text>
<rect x="88" y="92" width="68" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="122" y="111" font-size="9" text-anchor="middle" fill="currentColor">Refuse</text>
<rect x="168" y="92" width="80" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="208" y="111" font-size="9" text-anchor="middle" fill="currentColor">Deactivate</text>
<rect x="260" y="92" width="68" height="30" rx="4" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
<text x="294" y="111" font-size="9" text-anchor="middle" fill="currentColor">Cancel, drop</text>
<rect x="340" y="92" width="92" height="30" rx="4" fill="none" stroke="currentColor"/>
<text x="386" y="111" font-size="9" text-anchor="middle" fill="currentColor">Drain, snapshot</text>
<line x1="76" y1="107" x2="88" y2="107" stroke="currentColor" marker-end="url(#hb-arrow)"/>
<line x1="156" y1="107" x2="168" y2="107" stroke="currentColor" marker-end="url(#hb-arrow)"/>
<line x1="248" y1="107" x2="260" y2="107" stroke="currentColor" marker-end="url(#hb-arrow)"/>
<line x1="328" y1="107" x2="340" y2="107" stroke="currentColor" marker-end="url(#hb-arrow)"/>
</svg>
<figcaption>Dashed boxes are plugin host steps. The host starts after the snapshot restore and stops before the drain and the final snapshot.</figcaption>
</figure>

## Safe start

A user who cannot start a session because of a plugin has two switches. Both apply to the process that hosts plugins and are passed by the `ilium` launcher to the server it starts.

| Switch | Effect |
|---|---|
| `ilium --safe-plugins` (or `ILIUM_SAFE_PLUGINS=1`) | Loads only the bundled T1 plugins with their default grants. All T2 and T3 packages stay installed but are not loaded. Every `replaces` entry is ignored, so the built-in core plugin runs. The footer shows `Safe plugin mode: third-party plugins are off.` |
| `ilium --no-plugins` (or `ILIUM_NO_PLUGINS=1`) | The plugin host does not start. The session boots with the kernel only: terminal panes, the tree with kernel-drawn plain rows, the layout, the keymap defaults and the recovery page work. Features that live in core plugins are absent. |

- **[HST-033]** `--safe-plugins` MUST load no T2 or T3 package and MUST ignore `replaces`, and `--no-plugins` MUST start no plugin host. Neither switch MUST change installed packages, grants or stored plugin data.
- **[HST-034]** A session MUST boot to a usable terminal, tree and recovery page with `--no-plugins`.

## Failure containment

The host contains each failure at the smallest unit that owns the broken state. The table states what each failure takes down and what must keep running.

| Failure | What stops | What must keep running |
|---|---|---|
| A guest trap (panic, unreachable, out-of-bounds, stack overflow) | That plugin side's instance. Pending requests it owned complete as `cancelled`; its retained contributions are marked stale. | Every other plugin, the kernel, other sides of the same plugin |
| A stuck queue (queue full with a never-dropped event) | That plugin side is restarted as a failure | Everything else |
| A deadline breach (hard) | The call traps, as above | Everything else |
| A helper crash or kill | The one plugin side in that helper | The parent process, other helpers, other plugins |
| A Rust panic inside a host import implementation | The call is caught at the import boundary (`catch_unwind`), the instance traps with `host.import-panic`, and the panic is logged with the import name | The parent process and every other plugin. The kernel facade is never left holding a lock across an import (see requirement HST-038) |
| A Wasmtime `Engine`-level error (for example an internal compilation error) | The affected load or instantiation fails with `host.engine-error`; the plugin is not activated | The Engine and other plugins. If creating the Engine itself fails, the host does not start: the server logs `host.engine-failed` and continues in `--no-plugins` mode |
| Out of memory in one Store (linear memory limit) | The growth traps the instance (the guest never observes -1) and logs `memory limit <N> MiB reached`; the plugin side restarts under the restart rule | Other Stores; the process |
| Session memory budget reached | A new instance is refused and logged, not counted as a failure | Existing instances |

- **[HST-035]** A guest trap MUST end only the instance that trapped. Pending requests of that instance MUST complete as `cancelled`, and its retained contributions MUST be kept and marked stale.
- **[HST-036]** A panic in a host import implementation MUST be caught at the import boundary, MUST trap only the calling instance with the error `host.import-panic`, and MUST NOT unwind into the dispatcher or the kernel.
- **[HST-037]** If the Engine cannot be created, the process MUST continue without a plugin host as if started with `--no-plugins`, and MUST log `host.engine-failed`.
- **[HST-038]** The host MUST NOT hold a kernel lock while a guest call is running, and an import MUST take kernel locks only for the duration of its own work.
- **[HST-039]** A helper process failure MUST NOT crash or hang the parent process.

### A core plugin that the restart rule disables

The 30 core plugins (see [Core plugins](/pages/core-plugins/)) carry features of Ilium. The restart rule applies to them like to any plugin, with one change: a core plugin that the rule disables makes the host show the **kernel recovery page**. The page belongs to the kernel, not to a plugin, so it works with every plugin off. It lists every plugin with its tier, state, last failure and an on or off switch, and it shows how to start with `--safe-plugins`.

- The session still boots. The disabled state is stored, so the plugin stays off at the next start until the user turns it on.
- The kernel recovery page opens once, automatically, in the client that is attached when the failure happens, and later through the kernel command `kernel.open-recovery`, which has the default key `Ctrl+Alt+R` in the kernel keymap and cannot be removed by a keymap plugin.
- A kernel fallback covers the features that would leave the screen unusable. If `ilium.core.tree` is disabled, the kernel draws tree rows as the node name and a status glyph only, with no decorations, virtual rows or toolbar. If `ilium.core.layout` is disabled, the kernel keeps the last split geometry. If `ilium.core.settings` is disabled, the Settings screen shows only the recovery page. Other features are simply absent until the plugin is turned on.
- A failure of `ilium.core.sessions` does not stop the final snapshot: the kernel snapshot writer runs without the plugin and writes the plugin's last stored sections unchanged.

- **[HST-040]** When the restart rule disables a core plugin, the session MUST keep running, the disabled state MUST persist across restarts, and the kernel recovery page MUST open once in the affected client.
- **[HST-041]** The kernel recovery page and the command `kernel.open-recovery` MUST work with every plugin disabled and MUST NOT be removable by a plugin.
- **[HST-042]** With `ilium.core.tree` disabled, the kernel MUST draw tree rows from node names and status only.

## Errors and edge cases

| Situation | Result | Id |
|---|---|---|
| Helper binary missing and a T2 or T3 package is installed | The package stays disabled with `host.helper-missing`; T1 plugins run | `host.helper-missing` |
| Engine cannot be created | Server continues without plugins | `host.engine-failed` |
| A client connects while the server host is still starting | Retriable refusal | `server.starting` |
| CLI-process host needs the tree | `unavailable`, message `no session` | `unavailable` |
| Two clients disable the same plugin at different times | Each client disables it independently | none |
| Server host shutdown takes longer than 5 s | Remaining plugins are dropped without `deactivate`; their restore bytes are not kept | `host.shutdown-timeout` (logged) |
| Panic inside an import | The calling instance traps | `host.import-panic` |
| The grant store file is unreadable at start | The host starts with default grants for T1 only and refuses T2 and T3 packages until the file is repaired | `host.grants-unreadable` |
| `--safe-plugins` and `--no-plugins` both given | `--no-plugins` wins | none |
| A package on disk is changed after install | The content hash check fails and the plugin stays disabled | `package.corrupt` |

- **[HST-043]** When both safe-start switches are given, `--no-plugins` MUST win.
- **[HST-044]** An unreadable grant store MUST NOT start T2 or T3 plugins and MUST NOT grant a T1 plugin more than its default grants.
- **[HST-045]** A package whose content hash differs from the recorded hash MUST NOT be loaded.

## See also

- [Host runtime](/pages/reference-host-runtime/) for the Wasmtime configuration.
- [Host IPC protocol](/pages/reference-host-ipc/) for the traffic between server and client.
- [Sandbox helper](/pages/reference-host-helper/) for T2 and T3.
- [Scheduling, deadlines and cancellation](/pages/reference-host-scheduling/) for threads and call classes.
- [Host rendering pipeline](/pages/reference-host-rendering/) and [Performance budgets and benchmarks](/pages/reference-host-budgets/).
- [Platform support](/pages/reference-platform-support/) and [Host telemetry](/pages/reference-host-telemetry/).
- [Architecture](/pages/architecture/), [Lifecycle](/pages/lifecycle/) and [Security](/pages/security/) for the concepts.

API version checked: `ilium:plugin@0.1.0`.
