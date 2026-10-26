# ilium — architecture and design

This document covers the product design, process architecture, crate boundaries, agent-detection strategy, and milestone history. For installation and day-to-day use, see [`README.md`](README.md). For the rules that govern changes to this codebase, see [`CLAUDE.md`](CLAUDE.md).

ilium is built around two ideas tmux doesn't have:

1. **Tree-structured pane list.** Sessions are a tree of groups, split views, and panes (not a flat window/pane grid), shown in a left-side panel. The right panel renders either one pane or a persistent vertical/horizontal split containing up to four terminals, editors, or boards. Panes can be freely reordered and moved between containers.
2. **Agent awareness.** ilium periodically inspects each pane's content, detects whether it's running an AI coding agent (Claude Code, Codex CLI, Antigravity CLI, etc.), and shows whether that agent is *thinking* or *done* via icon + color in the tree — so a glance at the sidebar tells you which of your N agent sessions need attention.

Everything else (detach/reattach, persistent sessions, PTY handling, keybindings) follows tmux's model closely enough that tmux muscle memory should mostly transfer.

Detailed design chapters live in [`architecture/`](architecture/). This index keeps the existing section anchors.

## Prior art — why not just use X

[Detailed design](architecture/prior-art-why-not-just-use-x.md).

## Core concepts

[Detailed design](architecture/core-concepts.md).

## Form row leaders

[Detailed design](architecture/form-row-leaders.md).

## Ambient animation rendering

[Detailed design](architecture/ambient-animation-rendering.md).

## Process architecture

[Detailed design](architecture/process-architecture.md).

## Crate roles

[Detailed design](architecture/crate-roles.md).

## Worker and service execution

[Part 1](architecture/worker-and-service-execution-01.md), [Part 2](architecture/worker-and-service-execution-02.md), [Part 3](architecture/worker-and-service-execution-03.md), [Part 4](architecture/worker-and-service-execution-04.md), [Part 5](architecture/worker-and-service-execution-05.md).

## Ordered terminal ownership

[Detailed design](architecture/ordered-terminal-ownership.md).

## Agent worktrees

[Detailed design](architecture/agent-worktrees.md).

## Detection design

[Detailed design](architecture/detection-design.md).

## Scaling to hundreds of agents

[Detailed design](architecture/scaling-to-hundreds-of-agents.md).

## Key crates

[Detailed design](architecture/key-crates.md).

## Implementation plan

[Detailed design](architecture/implementation-plan.md).

## Compaction optimizer

[Detailed design](architecture/compaction-optimizer.md).

## Guided setup

[Detailed design](architecture/guided-setup.md).

## Attention queue

[Detailed design](architecture/attention-queue.md).

## Non-goals (for now)

[Detailed design](architecture/non-goals-for-now.md).

## Embedded application prompts

[Detailed design](architecture/embedded-application-prompts.md).

## Shared resource queues

`ilium-core::resource_queue` owns the pure, exclusive FIFO state machine, retry receipts, invocation fences, ownership generations and queue estimates. `ilium-server::resources` coordinates it through one user-wide catalog; project settings and managed instructions are projections, never competing authorities. `ilium-platform::resource_fs` owns durable local-filesystem locks, replacement, clocks and process birth proofs. Stable project identities travel with structured project metadata; explicit revision-bound rebinding reconciles directory moves. Bounded durable lifecycle events support detail history. Catalog paths are supplied explicitly by the server composition root so isolated tests cannot use real user data.

A reservation estimate never expires ownership. Dead owners remain occupied and need attention; uncertain invocation evidence blocks promotion. Guarded recovery separates revocation from an explicit clearance confirmation. Pending acquisition receipts survive restarts until acknowledged by an authenticated status or wait request. Native blocking waits use owned connection tasks and broadcast observations, with a bounded cross-project invocation proof before handoff. Resource request and attach workflows use heap-backed future boundaries so their state does not inflate ordinary connection stacks. Duplicate wait attachments have a finite per-reservation limit and share one presentation entry.

`ilium-ipc::resource` carries typed operations and snapshots. The CLI exposes `resource` and `ressource`; the client renders virtual resource rows, queue details, waiting badges and segmented wait progress. Optional grouping changes presentation only, preserving actual pane ancestry and input routing. `ilium-instructions` is the shared atomic writer for resource instructions and existing agent feature blocks. See [the resource guide](src/docs/resource-queues.md) for commands and hardware safety boundaries.
