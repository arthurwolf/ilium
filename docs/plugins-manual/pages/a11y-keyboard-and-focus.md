---
layout: page
title: Keyboard operation and focus in plugin views
permalink: /pages/a11y-keyboard-and-focus/
---

## Keyboard operation and focus in plugin views

A plugin view is operated from the keyboard alone. The host owns focus, moves it with Tab and Shift+Tab, and draws the focus indicator. The plugin decides the order of the focusable widgets through the tree it returns, and it can test that order without a terminal. This page is the author's side of keyboard use: which keys the host keeps, how to make the focus order match the reading order, how to write a test that drives a view with keys alone, and what the host does when the focused widget disappears. The rules of focus itself are owned by [UI model](/pages/ui-model/#focus); the accessibility baseline is in [API: ui](/pages/api-ui/#accessibility-baseline); the full contract is in [Accessibility and theming](/pages/accessibility-and-theming/). This page does not repeat them. It adds the steps a plugin author follows to prove them.

### You will need

- **A view.** A client view with the `views` export, as described in [Accessibility and theming](/pages/accessibility-and-theming/#worked-example-a-checks-panel).
- **A test host.** The fake host of [Test harness](/pages/reference-test-harness/). It offers `host.render(target, columns, rows)`, `host.key(target, key)` and `host.send_action(...)`, and it returns a `Screen` with `text()` and `cell(row, column)`.
- **Screen reader mode.** Only for the checks in [Screen reader and localized text](/pages/a11y-screen-reader-and-localized-text/).

## Keys the host keeps

A plugin never reads the keyboard directly. Key presses reach the focused widget as actions, so a plugin cannot take a key the host needs for navigation.

| Key | Owner | What it does in a plugin view |
|---|---|---|
| Tab, Shift+Tab | Host | Moves focus along the focus order. Never trapped inside a view. |
| Enter, Space | Focused widget | Presses a button, flips a toggle, applies a choice, sends `submitted` from a text input. |
| Up, Down, Left, Right, PageUp, PageDown | Focused widget | Moves inside the widget: a list, a table, a slider, a chart point. |
| Escape | Host | Cancels a capture, clears a selection, closes the topmost popup or dialog, and returns focus to the session tree from a non-modal panel. |

The table covers the keys a plugin author meets most. The full order of Escape is in [UI model](/pages/ui-model/#escape). A plugin that binds its own command to a key must not bind Tab, Shift+Tab or Escape (ACC-009).

## Focus order: make it match the reading order

The host visits focusable nodes in two groups. Nodes with a `tab-index` of 1 or more come first, in ascending order. Then come the nodes with no `tab-index` or 0, in tree pre-order. A negative value takes a node out of Tab. The rule is in [UI model](/pages/ui-model/#focus-order) and [WID-018](/pages/reference-requirements-widgets/).

Most views need no `tab-index` at all. The tree order already is the reading order when the plugin builds the tree from top to bottom and from left to right. Set `tab-index` only when a control has to come first although it is drawn later.

<figure>
<pre class="term"><span class="t-dim">Plugin view, top to bottom</span>
  [ Search        ]   <span class="t-dim">no tab-index, pre-order 1</span>
  [ Only failing  ]   <span class="t-dim">no tab-index, pre-order 2</span>
  ( Run again )       <span class="t-dim">tab-index 1, visited first</span>
<span class="t-dim">Tab visits:</span> Run again, Search, Only failing, Run again ...
</pre>
<figcaption>A plugin that sets <code>tab-index</code> 1 on the Run again button moves it ahead of the two controls drawn above it. Without the value, Tab visits Search, Only failing and then Run again.</figcaption>
</figure>

Two cases need care. A node that carries `disabled-reason` stays in the focus order, so a keyboard user can land on it and hear the reason. A node with `hidden` set is skipped, so a control that the plugin hides must not be the only route to an action.

## Focus indicator

The host draws focus, and the plugin must not draw a second one. A focused widget differs from unfocused widgets by at least one attribute besides colour: reverse, bold or a `>` marker, as [USR-304](/pages/reference-requirements-user-admin/) requires. A plugin that adds its own text cursor, such as a `>` in front of a row, produces two markers, and the user cannot tell which one is focus.

The check uses the screen the test host returns. Read the cell of the focused label with `cell(row, column)` and assert that the attributes are not the same as an unfocused row.

## Worked example: a keyboard test for a settings-like view

The test below drives a view with keys only. It checks the order of the focusable widgets, the action of a button reached by keys, and the return of focus after a dialog closes. The view has a search field, a toggle, a button that opens a confirm dialog, and a close button. The snippets are for the Rust SDK. The TypeScript form uses `createFakeHost()` with the same method names in camel case.

```rust
use ilium_plugin_sdk::testing::FakeHost;

#[test]
fn keyboard_only_reaches_every_action() {
    let mut host = FakeHost::for_manifest("manifest.json").expect("manifest");
    let target = host.open_view("settings");
    host.render(target, 48, 16);

    // Tab from the first node must visit each focusable node once, in order.
    let order = host.tab_sequence(target, 4);
    assert_eq!(order, ["search", "only-failing", "run-again", "search"]);

    // Enter on the focused button sends the same action a click would send.
    host.focus(target, "run-again");
    host.key(target, "Enter");
    assert!(host.last_action(target).is_some_and(|action| action.key == "run-again"));

    // Escape closes the confirm dialog and returns focus to the button that opened it.
    host.key(target, "Esc");
    assert_eq!(host.focused_key(target), Some("run-again".to_string()));
}
```

The harness helpers `tab_sequence`, `focus`, `last_action` and `focused_key` are not in the harness table yet. The test above states the checks the author needs; the exact helper names and the spelling of key names (`Tab`, `Enter`, `Esc`) are proposed, owner to confirm. Until the harness offers a focus reader, the same checks run with `host.key` and a `Screen` read: each Tab must move the reverse or bold cell from one label to the next.

<div class="note">
<p>A key that the harness does not know is refused with an error. Use the spelling of the key table above. A test that sends an unknown key passes nothing, so check that the focus moved after each key.</p>
</div>

## When the focused widget disappears

A plugin often rebuilds its tree. When the widget that has focus is no longer in the new tree, the host must still put focus somewhere. The reconciliation rules keep the view state of a node that keeps its `key` ([HST-269](/pages/reference-host-rendering/#reconciliation-hit-testing-focus-and-scrolling)), but they do not say where focus goes when the key is gone. That is an open point in [Rendering pipeline for plugins](/pages/reference-host-rendering/).

Proposed, owner to confirm: the host moves focus to the next focusable node in the new tree after the removed one, in focus order. If there is none, it moves focus to the last focusable node before it. If the view has no focusable node, focus is cleared and the host sends nothing to the plugin. A plugin that needs focus on a particular control after a rebuild requests it with a focus request. The request is honoured only for a view the user has focused or opened ([HST-271](/pages/reference-host-rendering/)).

## Failure modes

| Situation | What the user sees |
|---|---|
| A focusable widget has no label | The view is refused with `ui.label-missing`, and the last good tree stays on screen. See [API: ui](/pages/api-ui/#accessibility-baseline). |
| A focusable node has a negative `tab-index` | Tab skips it. A click or a `set-focus` still reaches it. |
| The only action is inside a `draggable` row | Keyboard users cannot reach it. See ACC-010. |
| A plugin request to focus a view the user never opened | The request is ignored, and focus stays where it was. |
| A focused control is removed by a rebuild | Focus moves by the proposed rule above. |

## Requirements

- **[ACC-001]** A plugin view MUST offer every action that a click can start through a focusable widget, so that Tab, Enter, Space or the arrow keys of that widget reach the same action. Test: for each action of the view, drive it with keys only and assert that the `ui-action` event arrives with the same key.
- **[ACC-002]** A plugin SHOULD leave `tab-index` unset when the tree order already matches the reading order. A `tab-index` value MUST be set only where a control has to come before the order of the tree. Test: review the tree and list the nodes with a `tab-index`.
- **[ACC-003]** A plugin view MUST let Tab visit each focusable node exactly once in focus order, MUST return to the first node after the last, and MUST NOT contain an enabled focusable node that Tab does not reach. Test: send Tab once per focusable node and assert the sequence of keys.
- **[ACC-030]** When focus moves to a node inside a `scroll-view`, a scrollable `paragraph`, a `%list`, `table` or `tree-list`, the host MUST scroll that container so the focused node is fully visible before the next frame is drawn (proposed, owner to confirm).
- **[ACC-004]** A plugin MUST NOT make a `hidden` node the only way to an action. A `disabled-reason` MUST say why the control cannot be used. Test: focus each disabled node and assert that the reason appears in the screen text.
- **[ACC-005]** A plugin MUST NOT draw its own focus marker in the text of a view, such as a `>` or a reverse space in front of a row. Test: search the rendered rows for a focus glyph that the plugin wrote itself.
- **[ACC-006]** A plugin view MUST have exactly one focused widget at a time, and the focused widget MUST differ from the unfocused widgets by an attribute besides colour. Test: render the view at 48 columns, focus each widget in turn, and assert that one cell per step carries bold, underline or reverse.
- **[ACC-032]** A plugin that shows or hides content on `hover-enter` and `hover-leave` MUST show and hide the same content on `focus-gained` and `focus-lost` of the same node (proposed, owner to confirm).
- **[ACC-007]** When a new tree removes the focused key, the host MUST place focus by the rule of the proposed decision on this page, which the owner must confirm before the rule is tested. A plugin MUST NOT depend on another node taking focus in that case. Test: rebuild the tree without the focused key and assert the focused key of the new tree.
- **[ACC-029]** When a popup, dialog or panel opens without a focus request, the host MUST place focus on the first focusable node of its focus order, and a plugin MUST NOT require a click to put focus into a view it opened (proposed, owner to confirm).
- **[ACC-008]** A plugin that opens a dialog MUST test that Escape closes it and that focus returns to the widget that opened it. Test: open the dialog with a key, send Escape, and assert the focused key of the opener.
- **[ACC-034]** A plugin MUST NOT close a dialog that awaits a decision, activate one of its buttons or change its default button on a timer; such a dialog MUST close only on a user action or when the subject it asks about no longer exists, and then with `closed`, never with an `activated` the user did not send (proposed, owner to confirm).
- **[ACC-009]** A plugin MUST NOT bind Tab, Shift+Tab or Escape to a command, a keybinding or a key capture of its own. The host keeps these keys for focus and for closing. Test: review the `keybindings` of the manifest and the key captures of the view.
- **[ACC-035]** A plugin MUST NOT bind F1, Shift+F10 or the Menu key to a command, a keybinding or a key capture of its own, and MUST NOT capture the drag mark chord of the active keymap preset (`Ctrl+Space` for tmux, `m` for screen) while a `draggable` node has focus; the host keeps these keys for the tooltip, the context menu and the keyboard drag (proposed, owner to confirm).
- **[ACC-010]** A control that needs a drag, such as a `draggable` row or a `drop-target`, MUST have an action that does the same thing from the keyboard. Test: find the drop action of the view and drive it with keys only.
- **[ACC-011]** A plugin MUST NOT assume that a focus request moves focus to a view the user has not opened. When the user must act in that view, the plugin MUST also say so in the text of the view. Test: send a focus request to a view that was never focused and assert that focus is unchanged and the text still gives the instruction.

## Checklist before release

- Run the keyboard test of each view with keys only, at 48 columns and at 40 columns in `compact` density.
- Check that the first key press of each view reaches the first control that the user needs, and that no control needs a mouse.
- Check the focus indicator in both themes. The indicator must still be visible with the colours turned off.
- Read the rebuild path of each view: the tree that is returned after an action must keep a focusable widget focused, or the path must send the focus request that the plugin needs.

## See also

- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Screen reader and localized text](/pages/a11y-screen-reader-and-localized-text/)
- [Widget catalogue](/pages/widgets-index/)
- [Test harness](/pages/reference-test-harness/)

## Production additions (round 3)

- **[PRY-006]** The host MUST honour at most 1 plugin focus request per view per second while the user has pressed a key in the last 2 seconds, MUST drop the excess requests, and MUST NOT move focus into a view of a different plugin than the one holding focus, counting each dropped request in `plugin.focus.dropped`.

## Production additions (round 4)

- **[PRZ-003]** A toast or notice that carries an action MUST stay visible for at least 10 seconds or until the user dismisses it, MUST pause its countdown while it holds focus, and the host MUST keep the same action reachable from the notification history for at least 24 hours.
