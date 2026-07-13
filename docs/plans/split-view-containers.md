# Split view containers

## Goal

Add persistent vertical and horizontal split views to ilium. A split view is
a tree container that displays zero to four existing panes in the right panel.
It can contain terminals, agent terminals, editors, and boards.

This plan deliberately covers design only. It does not authorize an
implementation change.

## User-visible behaviour

- A user can create a split view from a keyboard shortcut, the tree footer,
  or a relevant tree context menu.
- Creation first asks for an orientation: vertical or horizontal.
- An optional second dialog lists eligible existing panes with checkboxes. The
  selected panes are moved into the new split view atomically.
- Empty split views are valid. Selecting one shows a status-bar message that
  it needs panes added.
- A split with one pane renders that pane alone.
- A split with two panes renders equal side-by-side regions when vertical,
  or equal stacked regions when horizontal.
- A split with three panes renders three equal columns when vertical, or
  three equal rows when horizontal.
- A split with four panes always renders a 2 by 2 grid.
- Selecting a split view displays all of its panes. Selecting one of its
  children displays the same split and gives that child keyboard focus.

## Domain model

The server-owned `ilium-core::Tree` remains the one persistent ownership
model. Split membership must not be duplicated in client state.

Refactor group-like nodes into an explicit container abstraction:

```rust
enum NodeKind {
    Container(ContainerNode),
    Pane(PaneNode),
    Folder { path: PathBuf },
}

struct ContainerNode {
    kind: ContainerKind,
    children: Vec<NodeId>,
    expanded: bool,
}

enum ContainerKind {
    Group,
    SplitView { orientation: SplitOrientation },
}

enum SplitOrientation {
    Vertical,
    Horizontal,
}
```

`ContainerNode` owns the rules for accepting children and maximum capacity.
This makes the invariants explicit rather than scattering split-specific
branches across move, creation, rendering, and IPC code.

### Invariants

- A split view is a child of a normal group.
- A split view contains only `Pane` nodes; folders, groups, and nested split
  views are rejected.
- A split view contains at most four panes.
- A pane has one tree parent, so it cannot appear in two split views.
- Normal groups retain their current arbitrary-depth group/pane/folder
  semantics.
- Closing a non-empty split follows the existing non-empty group behaviour:
  confirmation, then removal of its child panes and associated PTYs.

### Core API

Add focused `Tree` operations and queries rather than exposing raw child-list
mutation:

- `create_split_view(parent_group, orientation, pane_ids)`
- `container_children(container_id)`
- `is_split_view(node_id)`
- `split_orientation(node_id)`
- container-aware `move_node`, `add_pane`, `remove_node`, and expansion
  operations

`create_split_view` validates every selected pane before mutating. It then
creates the split and moves the selected panes in their selected order. Any
validation failure leaves the tree unchanged.

Use typed errors for rejected operations, including an invalid split child,
capacity exceeded, invalid parent, and attempted duplicate/split-contained
pane selection.

## IPC and server

Add one atomic structural request:

```rust
CreateSplitView {
    parent_group: NodeId,
    orientation: SplitOrientation,
    pane_ids: Vec<NodeId>,
}
```

The server delegates this to `Tree::create_split_view`, then broadcasts a
full `TreeSnapshot` and schedules persistence through the existing snapshot
writer. It must not perform client-side-style partial moves.

Existing creation and reparent requests become container-aware:

- `NewPane` may target a split only while it has spare capacity.
- `ReparentNode` may move a pane into or out of a split, subject to domain
  validation.
- `NewGroup` and `NewFolder` may not target a split.

Persisting orientation and child order inside the tree snapshot makes restore
behave exactly like current group restore behaviour.

## Client presentation model

Replace the single-pane presentation assumption with a client-local target:

```rust
enum RightPanelTarget {
    Empty,
    Pane { pane_id: NodeId },
    SplitView {
        split_id: NodeId,
        active_pane_id: Option<NodeId>,
    },
}
```

The tree is still authoritative for which panes belong to the split. This
type owns only local presentation and keyboard focus.

- Selecting a normal pane sets `Pane { pane_id }`.
- Selecting a split sets `SplitView { active_pane_id: None }` and renders
  every child.
- Selecting a split child sets the parent split target and that child as
  active.
- A snapshot reconciliation step clears or replaces stale targets after a
  remote structural change or close.
- `SetPaneFocus` remains one pane at a time for the detection scheduler.

## Shared viewport layout

Create a pure client module, for example `ilium-client/src/split_layout.rs`.
It maps the current right-panel rectangle, orientation, and ordered pane IDs
to `PaneViewport` values:

```rust
struct PaneViewport {
    pane_id: NodeId,
    outer_area: Rect,
    content_area: Rect,
    slot_index: usize,
}
```

This must be the single geometry source for:

- rendering borders and active-slot chrome;
- terminal, editor, and board rendering;
- PTY row/column resize requests;
- editor toolbar/minimap/scrollbar placement;
- mouse hit-testing and terminal-relative mouse coordinates;
- selecting a slot on click.

Do not allow render, hit-testing, and resize code to independently recreate
split rectangles. The existing tree/pane mouse work demonstrates that such
geometry drift causes subtle input bugs.

Only displayed terminal panes receive a resize request, using the exact size
of their own viewport. Panes outside the selected view keep their prior PTY
size until displayed again.

## UI work

### Creation dialogs

Add two dedicated modal states:

1. Orientation selector: Vertical or Horizontal.
2. Optional membership selector: checkbox rows for eligible panes, including
   title, pane kind, and current tree path.

The second dialog excludes panes already contained in split views, permits an
empty selection, caps selection at four, and sends one `CreateSplitView`
request on confirmation. Server-side validation remains authoritative.

### Tree integration

- Render split views as expandable container rows with a distinct split and
  orientation icon.
- Allow split views to be renamed, moved between groups, expanded/collapsed,
  and closed using the same interaction patterns as groups.
- Permit creation or drag-and-drop of panes into a selected split when its
  capacity permits.
- Keep folder and group actions unavailable for split views when they would
  violate the domain rules.

### Input routing

- Keyboard input targets only the active split child.
- Clicking a viewport activates its pane, updates focus state, and forwards
  pointer coordinates relative to that viewport's content area.
- Editor and board interactions receive their individual `PaneViewport`, not
  the global current `UiLayout::pane_content_area`.
- The active viewport alone receives focused border styling.

## Implementation sequence

1. Add `ContainerNode`, `ContainerKind`, `SplitOrientation`, invariants, and
   exhaustive `ilium-core` tests.
2. Extend IPC protocol and server request handling; test atomic creation,
   broadcast, and persistence behaviour.
3. Add `RightPanelTarget` and snapshot reconciliation in the client.
4. Implement and test the pure `split_layout` viewport allocator.
5. Refactor pane rendering, resizing, input routing, and editor geometry to
   consume `PaneViewport`.
6. Add tree rendering and creation-dialog state/keyboard/mouse flows.
7. Add end-to-end PTY smoke coverage and complete workspace verification.

## Verification

- `ilium-core` unit tests: empty through four-member splits, valid and
  rejected moves, capacity, atomic creation, serde, and close behaviour.
- `ilium-ipc` protocol tests: frame round trips for split orientation and
  creation requests.
- `ilium-server` tests: valid request broadcasts/persists one snapshot;
  invalid requests change nothing.
- `ilium-client` tests: layout for zero through four panes in both
  orientations, target reconciliation, per-viewport hit-testing, and resize
  requests only for displayed terminal panes.
- Ratatui `TestBackend` tests: expected slot placement and active chrome.
- PTY smoke test: create a two-pane split, assert both real pane streams
  render, focus each slot, and verify separate input and resize routing.
- Final checks: `cargo fmt --check`, `cargo test --workspace`, and
  `cargo clippy --workspace --all-targets`.

## Edge cases

- Empty split selection and selection of an empty split.
- A split member closed, moved, or removed by another attached client.
- Capacity reached during a stale client request.
- A child pane selected from the tree after its split was collapsed.
- Tiny terminal dimensions and zero-sized inner areas.
- Switching between normal panes and split views during tree-width animation.
- Terminal mouse protocols, editor input, board input, and focus transitions
  inside individual split slots.
