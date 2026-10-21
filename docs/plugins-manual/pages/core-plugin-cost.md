---
layout: page
title: Cost
permalink: /pages/core-plugin-cost/
---

## Cost

The Cost plugin has the id `ilium.core.cost` and runs on both sides. It owns spend and usage: session statistics and the stats dot, spend and quota indicators, sorting the tree by cost, the price table, reset forecasts for plan quotas, and the Agent Cost and Reset planning settings. The server side reads transcripts, computes the figures and decorates the tree. The client side draws the statistics panel. The plugin also serves its figures to other plugins as the service `ilium.core.cost/usage`.

Reading an agent's own session files is done by the kernel and exposed as typed transcript facts. The plugin only turns those facts into money and quota percentages.

### What it covers

- Session statistics and the stats dot (see Agent monitoring, "The stats dot and session statistics").
- Spend and quota indicators, and the choice of a unit (see Agent cost, "What the numbers mean" and "Choose a unit").
- Sorting the tree by cost (see Agent cost, "Sorting the tree by cost").
- The price table (see Agent cost, "Prices and [cost.prices]").
- Reset planning, the reset forecasts (see Automation, "Reset forecasts", and Settings, "Reset planning").

### See it

<figure><img src="/assets/demos/06-cost-and-stats.gif" alt="The statistics panel shows spend over time, a bar per model and a table of sessions" loading="lazy"><figcaption>The session statistics panel and the spend indicators in the tree.</figcaption></figure>

### What the user sees

<figure>
<pre class="term"><span class="t-dim">╭ Session statistics ─────────────────────────╮</span>
<span class="t-dim">│</span> Spend, last 24 h   <span class="t-ok">▁▂▂▃▅▇▆▄▃▂▂▃</span>  <span class="t-hi">$6.12</span>    <span class="t-dim">  │</span>
<span class="t-dim">│</span>                                             <span class="t-dim">│</span>
<span class="t-dim">│</span> opus   <span class="t-ok">████████████</span>  $4.10                  <span class="t-dim">│</span>
<span class="t-dim">│</span> sonnet <span class="t-ok">█████</span>         $1.60                  <span class="t-dim">│</span>
<span class="t-dim">│</span> haiku  <span class="t-ok">█</span>             $0.42                  <span class="t-dim">│</span>
<span class="t-dim">│</span>                                             <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-hi">Session        Model   Tokens    Cost</span>       <span class="t-dim">│</span>
<span class="t-dim">│</span> refactor       opus    412k      $2.31      <span class="t-dim">│</span>
<span class="t-dim">│</span> tests          sonnet  188k      $0.77      <span class="t-dim">│</span>
<span class="t-dim">╰─────────────────────────────────────────────╯</span></pre>
<figcaption>Rendered at 48 columns. Illustrative: the block glyphs depend on the font. A sparkline of spend, a bar chart per model and a table of sessions.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-dim">╭ Quota ──────────────────────────────────────╮</span>
<span class="t-dim">│</span> 5-hour window  <span class="t-ok">██████████</span><span class="t-dim">░░░░░░░░░░</span>  <span class="t-hi">48%</span>    <span class="t-dim">│</span>
<span class="t-dim">│</span> Weekly         <span class="t-warn">████████████████</span><span class="t-dim">░░░░</span>  <span class="t-warn">81%</span>   <span class="t-dim"> │</span>
<span class="t-dim">│</span>                                             <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">Forecast: 5-hour window resets at 18:40</span>     <span class="t-dim">│</span>
<span class="t-dim">│</span> <span class="t-dim">Forecast: weekly window resets on Friday</span>    <span class="t-dim">│</span>
<span class="t-dim">╰─────────────────────────────────────────────╯</span></pre>
<figcaption>Rendered at 48 columns. Illustrative. Quota gauges for the 5-hour and weekly windows with a reset forecast line.</figcaption>
</figure>

### Manifest

This block is a fragment. It lists the contributions. The whole manifest is in [Complete manifest](#complete-manifest).

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.cost",
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0",
              "exports": ["service-provider"] },
  "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0",
              "exports": ["views"] },
  "contributes": {
    "decoration_slots": [ { "id": "cost", "placement": "badge", "priority": 40, "trailing": true } ],
    "tree_orderings": [ { "id": "by-cost", "title": "Cost, highest first", "slot": "cost" } ],
    "services": {
      "ilium.core.cost/usage": {
        "usage": {
          "params": { "type": "object", "properties": { "window": { "type": "string" } } },
          "result": { "type": "object", "properties": { "panes": { "type": "object" } },
                      "required": ["panes"] }
        }
      }
    },
    "settings": [
      { "kind": "section", "id": "cost-display", "tab": "agent-cost", "placement": "end", "title": "Display", "scope": "server",
        "rows": [
          { "type": "choice", "key": "unit", "title": "Unit", "default": "dollars",
            "options": [ { "value": "dollars", "title": "Dollars" },
                         { "value": "quota", "title": "Quota percent" } ] },
          { "type": "multi-choice", "key": "display", "title": "Display",
            "default": ["tree", "status"],
            "options": [ { "value": "tree", "title": "Tree rows" },
                         { "value": "status", "title": "Status bar" } ] }
        ] }
    ]
  },
  "capabilities": {
    "server": [
      "agent.read", "agent.transcript", "tree.decorate",
      "http:status.anthropic.com", "http:status.openai.com",
      "services.provide:ilium.core.cost/usage",
      "settings.contribute:agent-cost", "settings.write-kernel:agent-cost"
    ],
    "client": ["ui.panels"]
  }
}
```

### Complete manifest

This is the whole manifest of the plugin at `ilium:plugin@0.1.0`. The Manifest section above shows the contributions in detail; here the required keys, the sides, the `capabilities` per side and the `activation` triggers are all present. Activation follows [startup and lazy activation](/pages/reference-kernel-and-boot/#startup-and-lazy-activation-of-the-30-core-plugins). [COR-050]

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "ilium.core.cost",
  "name": "Cost",
  "version": "0.1.0",
  "publisher": "Ilium",
  "description": "The Cost plugin has the id ilium.core.cost and runs on both sides.",
  "license": "MIT",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": [
      "service-provider"
    ]
  },
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": [
      "views"
    ]
  },
  "capabilities": {
    "server": [
      "agent.read",
      "agent.transcript",
      "tree.decorate",
      "http:status.anthropic.com",
      "http:status.openai.com",
      "services.provide:ilium.core.cost/usage",
      "settings.contribute:agent-cost",
      "settings.write-kernel:agent-cost",
      "settings.write-kernel:reset-planning"
    ],
    "client": [
      "ui.panels"
    ]
  },
  "activation": [
    "startup",
    "view:cost.panel"
  ],
  "contributes": {
    "decoration_slots": [
      {
        "id": "cost",
        "placement": "badge",
        "priority": 40,
        "trailing": true
      }
    ],
    "tree_orderings": [
      {
        "id": "by-cost",
        "title": "Cost, highest first",
        "slot": "cost"
      }
    ],
    "services": {
      "ilium.core.cost/usage": {
        "usage": {
          "params": {
            "type": "object",
            "properties": {
              "window": {
                "type": "string"
              }
            }
          },
          "result": {
            "type": "object",
            "properties": {
              "panes": {
                "type": "object"
              }
            },
            "required": [
              "panes"
            ]
          }
        }
      }
    },
    "settings": [
      {
        "kind": "section",
        "id": "cost-display",
        "tab": "agent-cost",
        "placement": "end",
        "title": "Display",
        "scope": "server",
        "rows": [
          {
            "type": "choice",
            "key": "unit",
            "title": "Unit",
            "default": "dollars",
            "options": [
              {
                "value": "dollars",
                "title": "Dollars"
              },
              {
                "value": "quota",
                "title": "Quota percent"
              }
            ]
          },
          {
            "type": "multi-choice",
            "key": "display",
            "title": "Display",
            "default": [
              "tree",
              "status"
            ],
            "options": [
              {
                "value": "tree",
                "title": "Tree rows"
              },
              {
                "value": "status",
                "title": "Status bar"
              }
            ]
          }
        ]
      }
    ]
  },
  "limits_tier": "first-party"
}
```

### Kernel tabs this plugin owns

This plugin is the single owner of the following built-in Settings tabs. It holds `settings.write-kernel:<tab>` for each one (T1 only, one holder per tab, see [kernel settings](/pages/api-kernel-settings-and-keymap/#writing-a-kernel-tab)). The values stay in the same `config.toml` tables as before, so no user setting moves. [COR-030] [COR-033]

| Tab id | Label | Config table | Readers |
|---|---|---|---|
| `agent-cost` | Agent Cost | `[cost]` | Fleet, Tree |
| `reset-planning` | Reset planning | `[reset_planning]` | Notifications |

If the plugin is disabled, the tab stays in the rail with its stored values shown read-only and the notice "Cost is disabled. Values are kept." The kernel keeps reading the keys it consumes itself (listed in the Consumer column of the key table in [config and data migration](/pages/reference-config-and-data-migration/)). Behaviour that only this plugin provides is off. The exact result for this plugin is its row in [kernel and boot](/pages/reference-kernel-and-boot/). [COR-022]

### Samples

#### Session statistics

The client side draws spend over time, spend per model and a table of sessions. It uses the data widgets from [data display](/pages/widgets-data-display/): a `sparkline`, a `bar-chart` and a `table` ([collections](/pages/widgets-collections/)), inside a panel granted by [ui.panels](/pages/capability-ui-panels/). The server side computes the figures from the transcript facts ([agent transcript primitive](/pages/primitives-agent-transcript/)) and sends them to the client side.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::layout::Area;
use ilium_plugin_sdk::types::ui::{BarGroup, BarItem, TableColumn, TableRow, UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

impl Views for CostPanel {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let hourly = tree
            .sparkline(self.hourly_cents.iter().map(|cents| Some(*cents)).collect())
            .key("hourly");
        let bars = self
            .by_model
            .iter()
            .map(|(model, cents)| BarItem::new(*cents).label(model))
            .collect();
        let per_model = tree.bar_chart(vec![BarGroup::new("models", bars)]).key("models");
        let columns = vec![
            TableColumn::new("session", "Session").priority(9),
            TableColumn::new("model", "Model").priority(5),
            TableColumn::new("cost", "Cost").priority(9),
        ];
        let rows = self.sessions.iter().map(|session| {
            TableRow::new(vec![
                session.title.clone(),
                session.model.clone(),
                format!("${:.2}", session.cents as f64 / 100.0),
            ]).id(&session.id)
        }).collect();
        let table = tree.table(columns, rows).key("sessions");
        let root = tree.layout(Direction::Vertical, vec![Constraint::Length(1), Constraint::Length(6), Constraint::Fill(1)])
            .child(hourly).child(per_model).child(table);
        tree.finish(root)
    }
}
```

**TypeScript**

```ts
import { barChart, fill, layout, length, sparkline, table, tree } from '@ilium/plugin-sdk/ui/build';

export function render() {
  const bars = state.byModel.map(([model, cents]) => ({ value: cents, label: model }));
  const columns = [
    { id: 'session', title: 'Session', width: { tag: 'fill', val: 1 }, priority: 9 },
    { id: 'model', title: 'Model', width: { tag: 'length', val: 8 }, priority: 5 },
    { id: 'cost', title: 'Cost', width: { tag: 'length', val: 8 }, priority: 9 },
  ];
  const rows = state.sessions.map((session) => ({
    id: session.id,
    cells: [session.title, session.model, `$${(session.cents / 100).toFixed(2)}`],
  }));
  return tree(layout(
    { direction: 'vertical', constraints: [length(1), length(6), fill(1)] },
    [
      sparkline(state.hourlyCents, { key: 'hourly' }),
      barChart([{ label: 'models', bars }], { key: 'models' }),
      table(columns, rows, { key: 'sessions' }),
    ],
  ));
}
```

#### Quota gauges and spend history

One gauge per quota window, a thin line per pane, and a chart of recent days. The widgets are `gauge`, `line-gauge` and `chart` from [data display](/pages/widgets-data-display/). The daily totals live in project storage ([storage](/pages/api-storage/)), so the chart survives a restart.

**Rust**

```rust
use ilium_plugin_sdk::host::storage::{self, Bucket};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ui::{Axis, Dataset, GraphType, Marker};

fn save_day(day: u32, cents: u64) -> Result<(), PluginError> {
    storage::put(Bucket::Project, &format!("day/{day}"), &cents.to_le_bytes())
}

fn load_days() -> Vec<(f64, f64)> {
    storage::keys(Bucket::Project, "day/")
        .into_iter()
        .filter_map(|key| {
            let day: f64 = key.strip_prefix("day/")?.parse().ok()?;
            let bytes = storage::get(Bucket::Project, &key)?;
            let cents = u64::from_le_bytes(<[u8; 8]>::try_from(bytes).ok()?);
            Some((day, cents as f64 / 100.0))
        })
        .collect()
}

fn quota_widgets(tree: &mut Tree, windows: &[(String, f64)], pane_ratio: &[(String, f64)]) -> Vec<NodeRef> {
    let mut nodes = Vec::new();
    for (name, ratio) in windows {
        nodes.push(tree.gauge(*ratio).label(vec![Span::plain(&format!("{name} {:.0}%", ratio * 100.0))]).key(name).into_node());
    }
    for (pane, ratio) in pane_ratio {
        nodes.push(tree.line_gauge(*ratio).label(vec![Span::plain(pane)]).key(pane).into_node());
    }
    let history = tree
        .chart(
            vec![Dataset::new("cost per day", load_days()).marker(Marker::Braille).graph_type(GraphType::Line)],
            Axis { title: None, bounds: (0.0, 30.0), labels: vec![] },
            Axis { title: None, bounds: (0.0, 20.0), labels: vec![] },
        )
        .key("history");
    nodes.push(history.into_node());
    nodes
}
```

**TypeScript**

```ts
import * as storage from '@ilium/plugin-sdk/host/storage';
import { chart, gauge, lineGauge, span } from '@ilium/plugin-sdk/ui/build';

export function saveDay(day: number, cents: bigint): void {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, cents, true);
  storage.put('project', `day/${day}`, bytes);
}

export function loadDays(): Array<[number, number]> {
  return storage.keys('project', 'day/').flatMap((key) => {
    const bytes = storage.get('project', key);
    if (bytes === undefined || bytes.length !== 8) return [];
    const cents = new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(0, true);
    return [[Number(key.slice('day/'.length)), Number(cents) / 100] as [number, number]];
  });
}

export function quotaWidgets(windows: Array<[string, number]>, panes: Array<[string, number]>) {
  return [
    ...windows.map(([name, ratio]) => gauge(ratio, { key: name, label: [span(`${name} ${Math.round(ratio * 100)}%`)] })),
    ...panes.map(([pane, ratio]) => lineGauge(ratio, { key: pane, label: [span(pane)] })),
    chart(
      [{ name: 'cost per day', points: loadDays(), marker: 'braille', graphType: 'line' }],
      { bounds: [0, 30], labels: [] },
      { bounds: [0, 20], labels: [] },
      { key: 'history' },
    ),
  ];
}
```

#### Unit, display and sort by cost

Settings choose dollars or quota and what to display; the tree sorts by cost. The plugin declares a `choice` row for the unit and a `multi-choice` row for the display ([settings extension](/pages/settings-extension/), [settings row types](/pages/reference-settings-rows/)). It submits `set-decoration` with a `sort-key` taken from the spend ([tree and window chrome](/pages/api-tree-chrome/)).

**Rust**

```rust
use ilium_plugin_sdk::host::settings;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::intents::{Intent, SetDecorationSpec};

fn unit_is_dollars() -> bool {
    let values: serde_json::Value =
        serde_json::from_str(&settings::get_json()).unwrap_or(serde_json::Value::Null);
    values["unit"].as_str().unwrap_or("dollars") == "dollars"
}

fn cost_decoration(node: NodeId, cents: i64, quota_percent: u32) -> Submission {
    let text = if unit_is_dollars() {
        format!("${:.2}", cents as f64 / 100.0)
    } else {
        format!("{quota_percent}%")
    };
    Submission::new(Intent::SetDecoration(SetDecorationSpec {
        sort_key: Some(-cents), // ascending order puts the highest cost first
        trailing: true,
        ..SetDecorationSpec::text(node, "cost", text)
    }))
}
```

**TypeScript**

```ts
import { getJson } from '@ilium/plugin-sdk/host/settings';
import type { Submission } from '@ilium/plugin-sdk/types';

function unitIsDollars(): boolean {
  const values = JSON.parse(getJson()) as Record<string, unknown>;
  return (values['unit'] ?? 'dollars') === 'dollars';
}

export function costDecoration(node: bigint, cents: number, quotaPercent: number): Submission {
  const text = unitIsDollars() ? `$${(cents / 100).toFixed(2)}` : `${quotaPercent}%`;
  return {
    intent: {
      tag: 'set-decoration',
      val: { ...baseDecoration(node, 'cost', text), sortKey: BigInt(-cents), trailing: true },
    },
    correlation: 1n,
    dedupKey: undefined,
  };
}
```

#### Reset forecasts

Public status feeds predict when quotas reset. The plugin fetches the feed with `http.fetch` ([HTTP](/pages/api-http/), [http grant](/pages/capability-http/)) once an hour, using a durable timer ([durable timers](/pages/api-durable-timers/)), parses the reset times and stores the forecast. The completion arrives as `request-completed` ([events](/pages/api-events/)); the SDK future hides that event.

**Rust**

```rust
use ilium_plugin_sdk::host::http::{self, Header, HttpRequest};
use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::host::storage::{self, Bucket};
use ilium_plugin_sdk::prelude::*;

async fn refresh_forecast() {
    let request = HttpRequest {
        method: "GET".to_string(),
        url: "https://status.anthropic.com/api/v2/summary.json".to_string(),
        headers: vec![Header { name: "accept".into(), value: "application/json".into() }],
        body: None,
        timeout_millis: 5_000,
    };
    match http::fetch(request).await {
        Ok(response) if response.status == 200 => {
            if let Some(reset_at) = parse_reset_time(&response.body) {
                let _ = storage::put(Bucket::Global, "forecast/anthropic", &reset_at.to_le_bytes());
            }
        }
        Ok(response) => log::log(Level::Warn, &format!("status feed returned {}", response.status)),
        Err(error) => log::log(Level::Warn, &format!("status feed failed: {}", error.message)),
    }
}
```

**TypeScript**

```ts
import { fetch as httpFetch } from '@ilium/plugin-sdk/host/http';
import { log } from '@ilium/plugin-sdk/host/log';
import * as storage from '@ilium/plugin-sdk/host/storage';

export async function refreshForecast(): Promise<void> {
  try {
    const response = await httpFetch({
      method: 'GET',
      url: 'https://status.anthropic.com/api/v2/summary.json',
      headers: [{ name: 'accept', value: 'application/json' }],
      body: undefined,
      timeoutMillis: 5000,
    });
    if (response.status !== 200) {
      log('warn', `status feed returned ${response.status}`);
      return;
    }
    const resetAt = parseResetTime(response.body);
    if (resetAt !== undefined) {
      const bytes = new Uint8Array(8);
      new DataView(bytes.buffer).setBigUint64(0, BigInt(resetAt), true);
      storage.put('global', 'forecast/anthropic', bytes);
    }
  } catch (error) {
    log('warn', `status feed failed: ${String(error)}`);
  }
}
```

#### Serve usage to other plugins

Other plugins ask for spend per pane through a JSON service. The plugin implements `ServiceProvider` for `ilium.core.cost/usage` ([services API](/pages/api-services/), [services grant](/pages/capability-services/), [service catalogue](/pages/services/)). For cached figures it answers at once with `ServiceReply::Done`.

**Rust**

```rust
use ilium_plugin_sdk::exports::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
struct CostServer {
    /// Spend per pane in dollars, refreshed on every transcript change.
    by_pane: std::collections::BTreeMap<u64, f64>,
}

impl Lifecycle for CostServer {}

impl Handler for CostServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

impl ServiceProvider for CostServer {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        // The host has already checked the payload against the schema.
        let panes: serde_json::Map<String, serde_json::Value> = self
            .by_pane
            .iter()
            .map(|(pane, dollars)| (pane.to_string(), serde_json::json!(dollars)))
            .collect();
        ServiceReply::Done(serde_json::json!({ "panes": panes }).to_string())
    }
}
```

**TypeScript**

```ts
import { definePlugin, type ServiceCall, type ServiceReply } from '@ilium/plugin-sdk';

const byPane = new Map<bigint, number>(); // spend per pane in dollars

export const { lifecycle, handler, serviceProvider } = definePlugin({
  side: 'server',
  onEvents() {
    return [];
  },
  handle(_call: ServiceCall): ServiceReply {
    const panes: Record<string, number> = {};
    for (const [pane, dollars] of byPane) panes[pane.toString()] = dollars;
    return { tag: 'done', val: JSON.stringify({ panes }) };
  },
});
```

#### Model and price per pane

`agent-model-changed` tells the plugin which model a pane now runs. The plugin keeps the model per pane and uses its context window for the context gauge, so a switch to a larger model is priced correctly from the next turn.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::tree::AgentModel;

impl Handler for ModelPricing {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::AgentModelChanged(change) = event {
                self.models.insert(change.pane, change.model);
            }
        }
        Vec::new()
    }
}

fn context_window(&self, pane: NodeId) -> Option<u32> {
    self.models.get(&pane).and_then(|model: &AgentModel| model.context_window)
}
```

**TypeScript**

```ts
export function onEvents(events: Event[]): Submission[] {
  for (const event of events) {
    if (event.tag !== 'agent-model-changed') continue;
    models.set(event.val.pane, event.val.model);
  }
  return [];
}

function contextWindow(pane: string): number | undefined {
  return models.get(pane)?.contextWindow;
}
```

### Extension points

Other plugins change what `ilium.core.cost` does through the points below. The plugin owns each point (grant `extensions.provide:ilium.core.cost/<point>`). Contributors need `extensions.contribute:ilium.core.cost/<point>`. See [Extension points](/pages/api-extension-points/) for the mechanism and [Extending core plugins](/pages/extending-core-plugins/) for the five ways to work with a core feature.

| Point | Kind | What it does | Payload | Risk |
|---|---|---|---|---|
| `price-sources` | collect | Supply a price table for models the built-in table does not know. | entry: id, model_pattern, input_per_mtok, output_per_mtok, cache_read_per_mtok, currency | standard |
| `quota-sources` | collect | Supply a plan quota or remaining allowance for a provider. | entry: id, provider, service (a service the plugin provides) | standard |

**Services** (grant `services.use:<name>`):

- `ilium.core.cost/usage`: get: spend and tokens per pane, session or day

**Topics** (list under `contributes.topics.subscribes`):

- `ilium.core.cost/usage-changed`: { pane, total } when the total of a pane changes

**Replaceable:** yes. A replacement lists `"replaces": ["ilium.core.cost"]` and provides the whole surface (see [Extending core plugins](/pages/extending-core-plugins/#replacing-a-core-plugin)).

#### Example: Prices a local model at zero

A package adds this entry in its manifest. It needs no code.

```json
{
  "api": "ilium:plugin@0.1.0",
  "capabilities": { "server": ["extensions.contribute:ilium.core.cost/price-sources"] },
  "contributes": {
    "extensions": [
      {
        "point": "ilium.core.cost/price-sources",
        "id": "local-llm",
        "priority": 50,
        "entry": {
          "model_pattern": "qwen3-*",
          "input_per_mtok": 0,
          "output_per_mtok": 0,
          "currency": "USD"
        }
      }
    ]
  }
}
```

### Extension points this plugin owns

| Point | Side | Version | Deadline | Max | Dynamic |
|---|---|---|---|---|---|
| `ilium.core.cost/price-sources` | server | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |
| `ilium.core.cost/quota-sources` | server | 0.1.0 (default) | none (default) | 64 entries (default) | no (default) |

Side basis: ilium.core.cost/quota-sources is server because neither the manifest nor the page lists an extensions.provide grant or contribution for it on either side, so the side is taken from the owning logic: each entry names a service the plugin provides (ilium.core.cost/usage), which the server-side service provider serves, and the server side computes the quota percentages from transcript facts.

### ABI used

- [Data display widgets](/pages/widgets-data-display/) and [collections](/pages/widgets-collections/)
- [Agent transcript primitive](/pages/primitives-agent-transcript/) and [ui.panels](/pages/capability-ui-panels/)
- [Storage](/pages/api-storage/)
- [Settings extension](/pages/settings-extension/), [settings row types](/pages/reference-settings-rows/) and [tree and window chrome](/pages/api-tree-chrome/)
- [HTTP](/pages/api-http/), [http grant](/pages/capability-http/), [durable timers](/pages/api-durable-timers/) and [events](/pages/api-events/)
- [Services API](/pages/api-services/), [services grant](/pages/capability-services/) and [service catalogue](/pages/services/)
- [Agent events](/pages/primitives-agent/)

### See also

- [Fleet](/pages/core-plugin-fleet/), which calls the usage service to show cost in `ilium panes`.
- [Agent detection](/pages/core-plugin-agent-detect/), which supplies the agents that this plugin prices.
