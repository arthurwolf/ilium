---
layout: page
title: "23. Drive the fleet: a review swarm"
permalink: /pages/tutorial-23-review-swarm/
---

## 23. Drive the fleet: a review swarm

This step builds `example.swarm`, a plugin with a server side and a client side. One command starts a review swarm. The plugin creates N worktrees, starts one review agent in a new pane for each, sends them one shared instruction, shows progress, collects the diffs when the agents stop, and sends one notification with the result.

Core services are called as in [Extending core plugins](/pages/extending-core-plugins/). Their names are in [Reference: core extension points](/pages/reference-core-extension-points/). Use them from here. This page does not repeat those tables. [Tutorial 22](/pages/tutorial-22-versioned-service-with-fallbacks/) covers the failure rules of service calls.

You have finished tutorials 1 to 9 and read [Services](/pages/api-services/). Rust and TypeScript behave the same in every step.

### You will need

- Rust: stable 1.90 or later with the `wasm32-wasip2` target, and the crates `ilium-plugin-sdk` and `serde_json`. TypeScript: Bun 1.2 or later and `@ilium/plugin-sdk`.
- A Git repository with at least one commit, and an agent command on the path, such as `claude`.
- `"unstable": ["plugin-services"]`, because the plugin uses core services (dev mode, T2 and T3).

### The surface this step uses

| Surface | Used for |
|---|---|
| `ilium.core.worktrees/create` | One worktree per reviewer. |
| `Intent::CreatePane` and the grant `panes.create` | One terminal pane per reviewer. |
| `freeze-pane` and `terminate-pane-process` (grant `panes.lifecycle`) | Stop agents on cancel or at the deadline. |
| `ilium.core.fleet/broadcast` | One instruction to every reviewer. |
| `ilium.core.progress/report` | A progress bar for the run. |
| `ilium.core.notifications/notify` | The result. |
| Storage (always granted) | The run state, so a restart can resume. |
| `timers::at` and `timers::cancel_durable` | The deadline. |
| `jobs` (grant `jobs.run`) | Reading each diff off the main thread. |
| `dedup::key_for` and `Submission::keyed` | Each step happens once. |

### The permission summary

This plugin asks for a lot. Say so in its description and in this table. The user reads it in the review dialog.

| Grant | Risk | Why |
|---|---|---|
| `services.use` for four core services | normal | Worktrees, broadcast, progress, notifications. |
| `panes.create` | normal | Reviewer panes. |
| `fs.read:project` | normal | The pane's working folder is a worktree path. |
| `terminal.write` | high | The first instruction is typed into the agent's terminal at start. |
| `panes.lifecycle` | high | Stop agents on cancel and at the deadline. Server only. |
| `process.run:git` | high | The job runs `git diff` in a worktree. |
| `jobs.run` | normal | The diff job. |
| `panes.read` | normal | Learn that a reviewer pane has ended. |

### The flow

<figure>
<svg viewBox="0 0 480 330" role="img" aria-label="Sequence of a review swarm">
  <title>One review swarm run</title>
  <desc>The user starts the swarm. The plugin creates worktrees and panes, broadcasts an instruction, waits for the agents to stop, collects diffs with a job, and sends a notification.</desc>
  <defs>
    <marker id="t23-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L10,5 L0,10 z" fill="currentColor"/></marker>
  </defs>
  <rect x="10" y="8" width="70" height="26" rx="4" fill="none" stroke="currentColor"/> <text x="45" y="25" font-size="11" text-anchor="middle" fill="currentColor">User</text>
  <rect x="120" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/> <text x="165" y="25" font-size="11" text-anchor="middle" fill="currentColor">Swarm</text>
  <rect x="260" y="8" width="90" height="26" rx="4" fill="none" stroke="currentColor"/> <text x="305" y="25" font-size="11" text-anchor="middle" fill="currentColor">Core plugins</text>
  <rect x="390" y="8" width="80" height="26" rx="4" fill="none" stroke="currentColor"/> <text x="430" y="25" font-size="11" text-anchor="middle" fill="currentColor">Panes, jobs</text>
  <line x1="45" y1="34" x2="45" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="165" y1="34" x2="165" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="305" y1="34" x2="305" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="430" y1="34" x2="430" y2="320" stroke="currentColor" stroke-dasharray="3 4"/>
  <line x1="45" y1="56" x2="165" y2="56" stroke="currentColor" marker-end="url(#t23-arrow)"/> <text x="105" y="50" font-size="10" text-anchor="middle" fill="currentColor">start (N = 3)</text>
  <line x1="165" y1="88" x2="305" y2="88" stroke="currentColor" marker-end="url(#t23-arrow)"/> <text x="235" y="82" font-size="10" text-anchor="middle" fill="currentColor">worktrees/create x N</text>
  <line x1="165" y1="122" x2="430" y2="122" stroke="currentColor" marker-end="url(#t23-arrow)"/> <text x="298" y="116" font-size="10" text-anchor="middle" fill="currentColor">create-pane x N</text>
  <line x1="165" y1="156" x2="305" y2="156" stroke="currentColor" marker-end="url(#t23-arrow)"/> <text x="235" y="150" font-size="10" text-anchor="middle" fill="currentColor">fleet/broadcast</text>
  <line x1="165" y1="190" x2="305" y2="190" stroke="currentColor" stroke-dasharray="5 3" marker-end="url(#t23-arrow)"/> <text x="235" y="184" font-size="10" text-anchor="middle" fill="currentColor">progress/report</text>
  <line x1="430" y1="224" x2="165" y2="224" stroke="currentColor" marker-end="url(#t23-arrow)"/> <text x="298" y="218" font-size="10" text-anchor="middle" fill="currentColor">pane ended / deadline</text>
  <line x1="165" y1="258" x2="430" y2="258" stroke="currentColor" marker-end="url(#t23-arrow)"/> <text x="298" y="252" font-size="10" text-anchor="middle" fill="currentColor">job: git diff x N</text>
  <line x1="165" y1="296" x2="305" y2="296" stroke="currentColor" marker-end="url(#t23-arrow)"/> <text x="235" y="290" font-size="10" text-anchor="middle" fill="currentColor">notifications/notify</text>
</svg>
<figcaption>Every arrow out of the swarm is one step with its own dedup key. A restart resumes at the first step that did not finish.</figcaption>
</figure>

### Step 1: the manifest

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.swarm",
  "name": "Review swarm",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "description": "Starts N review agents in N worktrees, sends them one instruction and reports the diffs. Needs terminal write, pane control and git.",
  "icon": "users-group",
  "unstable": ["plugin-services"],
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": ["job-runner"]
  },
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": [] },
  "capabilities": {
    "server": [
      "services.use:ilium.core.worktrees/create",
      "services.use:ilium.core.fleet/broadcast",
      "services.use:ilium.core.progress/report",
      "services.use:ilium.core.notifications/notify",
      "panes.create",
      "panes.read",
      "panes.lifecycle",
      "fs.read:project",
      "terminal.write",
      "process.run:git",
      "jobs.run",
      "topics.subscribe:ilium.core.worktrees/changed"
    ],
    "client": ["topics.subscribe:example.swarm/status"]
  },
  "activation": ["startup"],
  "contributes": {
    "commands": [
      { "id": "example.swarm/start", "title": "Start a review swarm", "side": "server" },
      { "id": "example.swarm/cancel", "title": "Cancel the review swarm", "side": "server" }
    ],
    "jobs": [{ "kind": "example.swarm/diff", "deadline_ms": 60000, "memory_mib": 128 }],
    "topics": {
      "publishes": ["example.swarm/status"],
      "subscribes": ["ilium.core.worktrees/changed"]
    }
  }
}
```

The server holds all the logic and all the high-risk grants. The client side only shows the status topic, and it cannot hold `panes.lifecycle`.

### Step 2: the state, saved to storage

A run holds an id, a phase, a UTC deadline and one entry per reviewer (worktree path, pane node, `ended` flag, diff summary). The plugin saves it to storage after every step. A value is at most 1 MiB, so each diff summary is cut to 20 KiB.

Keep every decision in one pure function. It takes the run and one input, and returns the new run and a list of actions. The host calls live in a thin layer outside it. That split makes each phase testable without a host.

**Rust**

```rust
// src/plan.rs
use serde::{Deserialize, Serialize};

pub const MAX_REVIEWERS: usize = 8;
pub const DIFF_BYTES: usize = 20 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Phase { Creating, Working, Collecting, Done, Cancelled, Failed }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reviewer { pub worktree: Option<String>, pub pane: Option<u64>, pub ended: bool, pub diff: Option<String> }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run { pub run: String, pub phase: Phase, pub deadline: i64, pub reviewers: Vec<Reviewer> }

#[derive(Debug, PartialEq)]
pub enum Action { CreateWorktree(usize), CreatePane(usize), Broadcast, CollectDiff(usize), StopPane(u64), Notify(String) }

#[derive(Debug)]
pub enum Input {
    WorktreeReady(usize, String), PaneReady(usize, u64), PaneEnded(u64),
    DiffReady(usize, String), Deadline, Cancel, StepFailed(String),
}

/// A count the user typed, clamped to a safe range.
pub fn clamp_count(asked: i64) -> usize { asked.clamp(1, MAX_REVIEWERS as i64) as usize }

pub fn start(run: &str, count: usize, now: i64, window_ms: i64) -> (Run, Vec<Action>) {
    let blank = Reviewer { worktree: None, pane: None, ended: false, diff: None };
    let state = Run { run: run.into(), phase: Phase::Creating, deadline: now + window_ms, reviewers: vec![blank; count] };
    (state, (0..count).map(Action::CreateWorktree).collect())
}

fn stop_all(state: &Run) -> Vec<Action> {
    state.reviewers.iter().filter(|r| !r.ended).filter_map(|r| r.pane).map(Action::StopPane).collect()
}

/// One pure step. Any input after the run has finished changes nothing.
pub fn step(mut state: Run, input: Input) -> (Run, Vec<Action>) {
    if matches!(state.phase, Phase::Done | Phase::Cancelled | Phase::Failed) {
        return (state, vec![]);
    }
    let mut actions = vec![];
    let collect = |s: &Run| (0..s.reviewers.len()).map(Action::CollectDiff).collect::<Vec<_>>();
    match input {
        Input::WorktreeReady(i, path) => {
            if let Some(r) = state.reviewers.get_mut(i) { r.worktree = Some(path); actions.push(Action::CreatePane(i)); }
        }
        Input::PaneReady(i, node) => {
            if let Some(r) = state.reviewers.get_mut(i) { r.pane = Some(node); }
            if state.reviewers.iter().all(|r| r.pane.is_some()) { state.phase = Phase::Working; actions.push(Action::Broadcast); }
        }
        Input::PaneEnded(node) => {
            state.reviewers.iter_mut().filter(|r| r.pane == Some(node)).for_each(|r| r.ended = true);
            if state.reviewers.iter().all(|r| r.ended) { state.phase = Phase::Collecting; actions = collect(&state); }
        }
        Input::Deadline => {
            // Stop what still runs, then collect what the agents wrote.
            actions = stop_all(&state);
            state.reviewers.iter_mut().for_each(|r| r.ended = true);
            state.phase = Phase::Collecting;
            actions.extend(collect(&state));
        }
        Input::DiffReady(i, text) => {
            if let Some(r) = state.reviewers.get_mut(i) { r.diff = Some(text.chars().take(DIFF_BYTES).collect()); }
            if state.phase == Phase::Collecting && state.reviewers.iter().all(|r| r.diff.is_some()) {
                state.phase = Phase::Done;
                let changed = state.reviewers.iter().filter(|r| r.diff.as_deref().is_some_and(|d| !d.is_empty())).count();
                actions.push(Action::Notify(format!("Review swarm done: {changed} of {} reviewers changed files.", state.reviewers.len())));
            }
        }
        Input::Cancel => { actions = stop_all(&state); state.phase = Phase::Cancelled; }
        Input::StepFailed(reason) => {
            actions = stop_all(&state);
            state.phase = Phase::Failed;
            actions.push(Action::Notify(format!("Review swarm failed: {reason}")));
        }
    }
    (state, actions)
}
```

**TypeScript**

```ts
// src/plan.ts
export const MAX_REVIEWERS = 8;
export const DIFF_BYTES = 20 * 1024;

export type Phase = 'creating' | 'working' | 'collecting' | 'done' | 'cancelled' | 'failed';
export interface Reviewer { worktree?: string; pane?: number; ended: boolean; diff?: string }
export interface Run { run: string; phase: Phase; deadline: number; reviewers: Reviewer[] }

export type Action =
  | { kind: 'create-worktree' | 'create-pane' | 'collect-diff'; index: number }
  | { kind: 'broadcast' }
  | { kind: 'stop-pane'; node: number }
  | { kind: 'notify'; text: string };

export type Input =
  | { kind: 'worktree-ready'; index: number; path: string }
  | { kind: 'pane-ready'; index: number; node: number }
  | { kind: 'pane-ended'; node: number }
  | { kind: 'diff-ready'; index: number; text: string }
  | { kind: 'deadline' | 'cancel' }
  | { kind: 'step-failed'; reason: string };

/** A count the user typed, clamped to a safe range. */
export function clampCount(asked: number): number {
  return Math.min(MAX_REVIEWERS, Math.max(1, Math.trunc(asked) || 1));
}

export function start(run: string, count: number, now: number, windowMs: number): { state: Run; actions: Action[] } {
  const reviewers: Reviewer[] = Array.from({ length: count }, () => ({ ended: false }));
  const actions = reviewers.map((_, index): Action => ({ kind: 'create-worktree', index }));
  return { state: { run, phase: 'creating', deadline: now + windowMs, reviewers }, actions };
}

const FINISHED: Phase[] = ['done', 'cancelled', 'failed'];

/** One pure step. Any input after the run has finished changes nothing. */
export function step(old: Run, input: Input): { state: Run; actions: Action[] } {
  const state: Run = structuredClone(old);
  let actions: Action[] = [];
  if (FINISHED.includes(state.phase)) return { state, actions };
  const collect = (): Action[] => state.reviewers.map((_, index) => ({ kind: 'collect-diff', index }));
  const stopAll = (): Action[] =>
    state.reviewers.filter((r) => !r.ended && r.pane !== undefined).map((r) => ({ kind: 'stop-pane', node: r.pane! }));
  switch (input.kind) {
    case 'worktree-ready':
      if (state.reviewers[input.index]) {
        state.reviewers[input.index].worktree = input.path;
        actions.push({ kind: 'create-pane', index: input.index });
      }
      break;
    case 'pane-ready':
      if (state.reviewers[input.index]) state.reviewers[input.index].pane = input.node;
      if (state.reviewers.every((r) => r.pane !== undefined)) { state.phase = 'working'; actions.push({ kind: 'broadcast' }); }
      break;
    case 'pane-ended':
      state.reviewers.filter((r) => r.pane === input.node).forEach((r) => { r.ended = true; });
      if (state.reviewers.every((r) => r.ended)) { state.phase = 'collecting'; actions = collect(); }
      break;
    case 'deadline':
      // Stop what still runs, then collect what the agents wrote.
      actions = [...stopAll(), ...collect()];
      state.reviewers.forEach((r) => { r.ended = true; });
      state.phase = 'collecting';
      break;
    case 'diff-ready':
      if (state.reviewers[input.index]) state.reviewers[input.index].diff = input.text.slice(0, DIFF_BYTES);
      if (state.phase === 'collecting' && state.reviewers.every((r) => r.diff !== undefined)) {
        state.phase = 'done';
        const changed = state.reviewers.filter((r) => r.diff !== '').length;
        actions.push({ kind: 'notify', text: `Review swarm done: ${changed} of ${state.reviewers.length} reviewers changed files.` });
      }
      break;
    case 'cancel': actions = stopAll(); state.phase = 'cancelled'; break;
    case 'step-failed':
      actions = [...stopAll(), { kind: 'notify', text: `Review swarm failed: ${input.reason}` }];
      state.phase = 'failed';
      break;
  }
  return { state, actions };
}
```

### Step 3: the host calls

The thin layer turns each action into a host call. Every call that changes the world has a dedup key, or an id in its payload, so a repeat after a restart is harmless. A repeat inside the 10-minute window gives `duplicate`, and the layer treats that as success.

The payloads of the core services below are the shapes this page uses. Check the fields against [Reference: core extension points](/pages/reference-core-extension-points/) for the version you run. The functions `pane_intent` and `stop_intent` build the `CreatePaneSpec` (title `Review 1`, content kind `ilium.terminal`, the worktree as `cwd`, the command `claude`) and the terminate intent with a grace period.

**Rust**

```rust
// src/lib.rs
use std::collections::HashMap;

use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::host::{clock, jobs, services, storage, timers, topics};
use ilium_plugin_sdk::prelude::*;
use serde_json::{json, Value};
mod plan;
use plan::{Action, Input, Phase, Run};

const WINDOW_MS: i64 = 20 * 60_000;
enum Waiting { Worktree(usize), Service, Diff(usize) }

#[ilium_plugin(side = "server", exports = ["handler", "job-runner"])]
#[derive(Default)]
struct Swarm { run: Option<Run>, waiting: HashMap<RequestId, Waiting> }

impl Lifecycle for Swarm {
    fn activate(&mut self) {
        // Restart recovery: read the saved run. Step 5 says what happens next.
        let saved = storage::get("project", "run").ok().flatten();
        self.run = saved.and_then(|text| serde_json::from_str(&text).ok());
    }
}

impl Handler for Swarm {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            let input = match event {
                Event::CommandInvoked(c) if c.command == "example.swarm/start" => {
                    let args: Value = serde_json::from_str(&c.arguments_json).unwrap_or(Value::Null);
                    let now = clock::now_unix_millis();
                    let (state, actions) = plan::start(&format!("{now:x}"), plan::clamp_count(args["count"].as_i64().unwrap_or(3)), now, WINDOW_MS);
                    let _ = timers::at(state.deadline, "deadline");
                    self.run = Some(state);
                    self.perform(actions, &mut out);
                    None
                }
                Event::CommandInvoked(c) if c.command == "example.swarm/cancel" => Some(Input::Cancel),
                Event::DurableTimerFired { name, .. } if name == "deadline" => Some(Input::Deadline),
                Event::PaneLifecycleChanged(change) if change.ended => Some(Input::PaneEnded(change.node)),
                Event::RequestCompleted(done) => self.completed(done),
                _ => None,
            };
            if let (Some(input), Some(run)) = (input, self.run.take()) {
                let (next, actions) = plan::step(run, input);
                if matches!(next.phase, Phase::Done | Phase::Cancelled | Phase::Failed) {
                    let _ = timers::cancel_durable("deadline");
                }
                self.run = Some(next);
                self.perform(actions, &mut out);
            }
        }
        if let Some(run) = &self.run {
            let _ = storage::put("project", "run", &serde_json::to_string(run).unwrap_or_default());
            let _ = topics::publish("example.swarm/status", &json!({ "run": run.run, "phase": run.phase }).to_string());
        }
        out
    }
}

impl Swarm {
    fn call(&mut self, service: &str, payload: Value, why: Waiting) {
        if let Ok(id) = services::call_request(service, "send", &payload.to_string()) {
            self.waiting.insert(id, why);
        }
    }

    fn perform(&mut self, actions: Vec<Action>, out: &mut Vec<Submission>) {
        let Some(run) = self.run.clone() else { return };
        for action in actions {
            match action {
                Action::CreateWorktree(i) => self.call("ilium.core.worktrees/create",
                    json!({ "branch": format!("swarm/{}/{i}", run.run), "agent": "review" }), Waiting::Worktree(i)),
                Action::CreatePane(i) => out.push(Submission::keyed(
                    pane_intent(&run, i), dedup::key_for(&["swarm", &run.run, "pane", &i.to_string()]))),
                Action::Broadcast => self.call("ilium.core.fleet/broadcast",
                    json!({ "message": "Review the diff against main. List risks only.", "run": run.run }), Waiting::Service),
                Action::CollectDiff(i) => {
                    let path = run.reviewers[i].worktree.clone().unwrap_or_default();
                    if let Ok(id) = jobs::start_json("example.swarm/diff", &json!({ "path": path }).to_string()) {
                        self.waiting.insert(id, Waiting::Diff(i));
                    }
                }
                Action::StopPane(node) => out.push(Submission::keyed(
                    stop_intent(node), dedup::key_for(&["swarm", &run.run, "stop", &node.to_string()]))),
                Action::Notify(text) => self.call("ilium.core.notifications/notify",
                    json!({ "title": "Review swarm", "body": text, "urgency": "normal", "run": run.run }), Waiting::Service),
            }
        }
    }

    fn completed(&mut self, done: RequestResult) -> Option<Input> {
        match self.waiting.remove(&done.request)? {
            Waiting::Service => None,
            Waiting::Diff(i) => Some(Input::DiffReady(i, jobs::take_output(done.request).unwrap_or_default())),
            Waiting::Worktree(i) => match services::take_reply(done.request) {
                Ok(text) => {
                    let path = serde_json::from_str::<Value>(&text).ok()?["path"].as_str()?.to_string();
                    Some(Input::WorktreeReady(i, path))
                }
                Err(error) => Some(Input::StepFailed(error.code)),
            },
        }
    }
}
```

**TypeScript**

```ts
// src/server.ts
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { nowUnixMillis } from '@ilium/plugin-sdk/host/clock';
import { startJson, takeOutput } from '@ilium/plugin-sdk/host/jobs';
import { callRequest, takeReply } from '@ilium/plugin-sdk/host/services';
import { get, put } from '@ilium/plugin-sdk/host/storage';
import { at, cancelDurable } from '@ilium/plugin-sdk/host/timers';
import { publish } from '@ilium/plugin-sdk/host/topics';
import { clampCount, start, step, type Action, type Input, type Run } from './plan';
const WINDOW_MS = 20 * 60_000;
type Waiting = { kind: 'worktree' | 'diff'; index: number } | { kind: 'service' };

let run: Run | undefined;
const waiting = new Map<bigint, Waiting>();
let correlation = 1n;

function keyed(intent: Submission['intent'], ...parts: string[]): Submission {
  const dedupKey = parts.join(':').replace(/[^A-Za-z0-9._:/-]/g, '-').slice(0, 128);
  return { intent, correlation: correlation++, dedupKey };
}

function call(service: string, payload: object, why: Waiting): void {
  try { waiting.set(callRequest(service, 'send', JSON.stringify(payload)), why); } catch { /* the host logs it */ }
}

function perform(actions: Action[], out: Submission[]): void {
  if (!run) return;
  for (const a of actions) {
    if (a.kind === 'create-worktree') {
      call('ilium.core.worktrees/create', { branch: `swarm/${run.run}/${a.index}`, agent: 'review' }, { kind: 'worktree', index: a.index });
    } else if (a.kind === 'create-pane') {
      out.push(keyed(paneIntent(run, a.index), 'swarm', run.run, 'pane', String(a.index)));
    } else if (a.kind === 'broadcast') {
      call('ilium.core.fleet/broadcast', { message: 'Review the diff against main. List risks only.', run: run.run }, { kind: 'service' });
    } else if (a.kind === 'collect-diff') {
      try {
        const id = startJson('example.swarm/diff', JSON.stringify({ path: run.reviewers[a.index].worktree ?? '' }));
        waiting.set(id, { kind: 'diff', index: a.index });
      } catch { /* the host logs it */ }
    } else if (a.kind === 'stop-pane') {
      out.push(keyed(stopIntent(a.node), 'swarm', run.run, 'stop', String(a.node)));
    } else {
      call('ilium.core.notifications/notify', { title: 'Review swarm', body: a.text, urgency: 'normal', run: run.run }, { kind: 'service' });
    }
  }
}

function completed(request: bigint): Input | undefined {
  const entry = waiting.get(request);
  waiting.delete(request);
  if (!entry || entry.kind === 'service') return undefined;
  if (entry.kind === 'diff') return { kind: 'diff-ready', index: entry.index, text: takeOutput(request) ?? '' };
  try {
    const path = (JSON.parse(takeReply(request)) as { path?: string }).path;
    return path ? { kind: 'worktree-ready', index: entry.index, path } : { kind: 'step-failed', reason: 'no worktree path' };
  } catch (error) {
    return { kind: 'step-failed', reason: (error as { code?: string }).code ?? 'failed' };
  }
}

export const { lifecycle, handler, jobRunner } = definePlugin({
  side: 'server',
  activate() {
    // Restart recovery: read the saved run. Step 5 says what happens next.
    const saved = get('project', 'run');
    run = saved ? (JSON.parse(saved) as Run) : undefined;
  },
  onEvents(batch: Event[]): Submission[] {
    const out: Submission[] = [];
    for (const event of batch) {
      let input: Input | undefined;
      if (event.tag === 'command-invoked' && event.val.command === 'example.swarm/start') {
        const args = JSON.parse(event.val.argumentsJson) as { count?: number };
        const now = Number(nowUnixMillis());
        const started = start(now.toString(16), clampCount(args.count ?? 3), now, WINDOW_MS);
        run = started.state;
        at(BigInt(run.deadline), 'deadline');
        perform(started.actions, out);
      } else if (event.tag === 'command-invoked' && event.val.command === 'example.swarm/cancel') {
        input = { kind: 'cancel' };
      } else if (event.tag === 'durable-timer-fired' && event.val.name === 'deadline') {
        input = { kind: 'deadline' };
      } else if (event.tag === 'pane-lifecycle-changed' && event.val.ended) {
        input = { kind: 'pane-ended', node: Number(event.val.node) };
      } else if (event.tag === 'request-completed') {
        input = completed(event.val.request);
      }
      if (input && run) {
        const result = step(run, input);
        run = result.state;
        if (['done', 'cancelled', 'failed'].includes(run.phase)) cancelDurable('deadline');
        perform(result.actions, out);
      }
    }
    if (run) {
      put('project', 'run', JSON.stringify(run));
      publish('example.swarm/status', JSON.stringify({ run: run.run, phase: run.phase }));
    }
    return out;
  },
});
```

A restart loses `waiting`, the map from a request to its purpose. The saved run survives. Step 5 covers recovery.

### Step 4: the diff job

The job runs in its own instance with its own limits. It runs `git diff --stat` in one worktree, so the output stays short.

**Rust**

```rust
// src/job.rs
use ilium_plugin_sdk::host::process::{self, ProcessSpec};

impl JobRunner for Swarm {
    fn run(&mut self, job: JobCall) -> JobResult {
        let params: serde_json::Value = serde_json::from_str(&job.payload_json).unwrap_or_default();
        let path = params["path"].as_str().unwrap_or("");
        let args = ["-C", path, "diff", "--stat", "HEAD"].map(String::from).to_vec();
        match process::run(&ProcessSpec { command: "git".into(), args, timeout_millis: 20_000 }) {
            Ok(output) => JobResult::Done(output.stdout.chars().take(20 * 1024).collect()),
            Err(error) => JobResult::Failed(error.code),
        }
    }
}
```

**TypeScript**

```ts
// src/job.ts
import { run as runProcess } from '@ilium/plugin-sdk/host/process';

export function runJob(payloadJson: string): { tag: 'done' | 'failed'; val: string } {
  const { path = '' } = JSON.parse(payloadJson) as { path?: string };
  try {
    const args = ['-C', path, 'diff', '--stat', 'HEAD'];
    return { tag: 'done', val: runProcess({ command: 'git', args, timeoutMillis: 20_000 }).stdout.slice(0, 20 * 1024) };
  } catch (error) {
    return { tag: 'failed', val: (error as { code?: string }).code ?? 'failed' };
  }
}
```

### Step 5: restart, cancel and duplicates

Three events can cut a run short.

- **Ilium restarts.** `activate` reads the saved run. The `waiting` map is gone, so the listing does not match old replies. Resume by phase: in `creating` ask again for worktrees that have no path, in `working` wait for pane events and the durable deadline, in `collecting` start the diffs that are empty.
- **The user cancels.** `Cancel` stops every running pane with `terminate-pane-process` and a grace period, and the layer cancels the timer. Worktrees stay on disk for the user to inspect.
- **A second start.** Refuse the command while the saved phase is `creating`, `working` or `collecting`. One run at a time.

Recovery works because every action is safe to repeat. A pane intent has a dedup key, so a repeat inside 10 minutes gives `duplicate`. After that window create a pane only when the saved reviewer has none. The worktree call uses a fixed branch name, so a repeat should return the existing path. Check this against the worktrees plugin you run. Replies after a cancel are dropped, because `step` ignores every input in a finished run. The listing above restores the run but leaves the resume loop and the second-start guard to you.

### Step 6: test it

`FakeHost` has fake panes and jobs, and `FakeSession.install_core` adds stubs of the core plugins. Test the pure plan, then the pane and job calls with the fakes. Follow [Testing](/pages/testing/) and [Test harness reference](/pages/reference-test-harness/). Check the real core services in the app.

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::plan::*;

    fn working(count: usize) -> Run {
        let (mut state, _) = start("r1", count, 1_000, 60_000);
        for i in 0..count {
            state = step(state, Input::WorktreeReady(i, format!("/w/{i}"))).0;
            state = step(state, Input::PaneReady(i, 10 + i as u64)).0;
        }
        state
    }

    #[test]
    fn deadline_stops_only_running_panes_then_one_notification() {
        assert_eq!(clamp_count(500), MAX_REVIEWERS);
        let (state, _) = step(working(2), Input::PaneEnded(10));
        let (state, actions) = step(state, Input::Deadline);
        assert!(actions.contains(&Action::StopPane(11)) && !actions.contains(&Action::StopPane(10)));
        let (state, none) = step(state, Input::DiffReady(0, "a".into()));
        assert!(none.is_empty());
        let (state, done) = step(state, Input::DiffReady(1, "".into()));
        assert_eq!(state.phase, Phase::Done);
        assert_eq!(done, vec![Action::Notify("Review swarm done: 1 of 2 reviewers changed files.".into())]);
    }
}
```

**TypeScript**

```ts
// src/plan.test.ts
import { expect, test } from 'bun:test';
import { clampCount, MAX_REVIEWERS, start, step, type Run } from './plan';

function working(count: number): Run {
  let { state } = start('r1', count, 1000, 60000);
  for (let i = 0; i < count; i++) {
    state = step(state, { kind: 'worktree-ready', index: i, path: `/w/${i}` }).state;
    state = step(state, { kind: 'pane-ready', index: i, node: 10 + i }).state;
  }
  return state;
}

test('deadline stops only running panes, then one notification', () => {
  expect(clampCount(500)).toBe(MAX_REVIEWERS);
  const ended = step(working(2), { kind: 'pane-ended', node: 10 }).state;
  const result = step(ended, { kind: 'deadline' });
  expect(result.actions).toContainEqual({ kind: 'stop-pane', node: 11 });
  expect(result.actions).not.toContainEqual({ kind: 'stop-pane', node: 10 });
  const none = step(result.state, { kind: 'diff-ready', index: 0, text: 'a' });
  expect(none.actions).toEqual([]);
  const done = step(none.state, { kind: 'diff-ready', index: 1, text: '' });
  expect(done.state.phase).toBe('done');
  expect(done.actions).toEqual([{ kind: 'notify', text: 'Review swarm done: 1 of 2 reviewers changed files.' }]);
});

```

Run `cargo test` or `bun test`. Then check the live path with the steps in [Debugging](/pages/debugging/):

1. Pack and install as in [tutorial 9](/pages/tutorial-09-package-and-share/). Read the three high-risk rows before you confirm.
2. Run `Start a review swarm` with count 2 in a Git project. Two panes appear.
3. Run `Cancel the review swarm`, then read `ilium plugin logs example.swarm --tail 20`.

### What you see

The review dialog lists the three high-risk grants, unchecked.

<figure>
<pre class="term">
╭──────────────────────────────────────────────╮
│ Review: example.swarm 0.1.0                  │
│                                              │
│ Grants                                       │
│   [x] services.use (4 core services)         │
│   [x] panes.create                           │
│   [x] panes.read                             │
│   [x] fs.read:project                        │
│   [x] jobs.run                               │
│   [ ] terminal.write               HIGH      │
│   [ ] panes.lifecycle              HIGH      │
│   [ ] process.run:git              HIGH      │
│                                              │
│   [install]  [cancel]                        │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. HIGH rows start unchecked and the user ticks each one.</figcaption>
</figure>

### Edge cases and errors: what can go wrong

| Code or symptom | Cause | Fix |
|---|---|---|
| `not-permitted` on a service | A `services.use` grant is missing or revoked | Grant it. The run ends as failed with this code. |
| `not-found` on a service | The core plugin is off | Turn it on. The run ends as failed. |
| `refused` on `create-pane` | `panes.create` or `fs.read` is missing | Grant it. Check `ilium plugin grants example.swarm`. |
| `refused` on stop | `panes.lifecycle` was left off | Tick it, or stop the agents by hand. |
| `timed-out` on a service | A core plugin took over 30 s | The run fails. Start again. |
| Job `failed` | Git is not allowed or the path is gone | Check `process.run:git`. A failed diff gives an empty summary. |
| Restart in the middle | Ilium restarted | The saved run is read. Resume by phase (step 5). |

### See also

- [Extending core plugins](/pages/extending-core-plugins/) and [Reference: core extension points](/pages/reference-core-extension-points/) for the core services.
- [Services](/pages/api-services/), [tutorial 22](/pages/tutorial-22-versioned-service-with-fallbacks/), [Testing](/pages/testing/), [Debugging](/pages/debugging/) and [Manifest reference](/pages/reference-manifest/).

API checked against `ilium:plugin@0.1.0`.
