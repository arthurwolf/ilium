# Widget input map

## Widget input map

This page lists, for each of the 51 widgets of `ilium:plugin@0.1.0`, whether it takes focus, which keys and mouse actions it handles, which `ui-action` it sends and which state the host keeps under its key. The rules behind the columns are on the [UI model](/pages/ui-model/) page: the [focus order](/pages/ui-model/#focus-order), [Escape](/pages/ui-model/#escape) and the [action values](/pages/ui-model/#actions-back-to-the-plugin). Widgets without a key send nothing and keep no state. The word none means the widget does nothing for that column.

## The map

| Widget | Focus | Keys | Mouse | Sends | Host state |
|---|---|---|---|---|---|
| `layout` | no | none | none | none | none |
| `block` | no | none | none | none | none |
| `tabs` | yes | Left, Right, Home, End | click a title | selected(index) | tab(u32) |
| `popup` | no (children yes) | Esc closes | click outside closes a non-modal popup | closed(none) | none |
| `clear` | no | none | none | none | none |
| `scrollbar` | no | none | click track pages, drag thumb | scrolled(index) | none |
| `split` | only if resizable | Left, Right, Up, Down move the divider | drag divider | resized(number) | split(u16) |
| `separator` | no | none | none | none | none |
| `dialog` | no (buttons yes) | Tab, Enter, Esc | click a button | activated(text), closed | none |
| `scroll-view` | yes | arrows, PageUp, PageDown, Home, End | wheel, drag scrollbar | scrolled(index) | offset(u32) |
| `paragraph` | if scroll is set | arrows, PageUp, PageDown | wheel, click a span | scrolled, activated | offset(u32) |
| `markdown` | if it has a link or task item | Tab between links, Enter | click a link | activated(text) | none |
| `leader-row` | no | none | none | none | none |
| `key-hints` | no | none | click a hint runs its command on the host | none | none |
| `diff-view` | yes | arrows, n, p | wheel | none | offset(u32) |
| `%list` | yes | Up, Down, PageUp, PageDown, Home, End, Enter | click, double click, wheel | selected, activated, scrolled, context-requested | selected, offset, ids |
| `table` | yes | as %list | click row, click header sorts | selected, activated, scrolled | selected, offset, ids |
| `tree-list` | yes | Up, Down, Left collapses, Right expands, Enter | click, click arrow | selected, toggled, activated | ids, expanded |
| `menu` | yes | Up, Down, Enter, Esc | click entry | activated(text), closed | none |
| `check-list` | yes | Up, Down, Space | click item | toggled(item-state) | tri |
| `list-editor` | yes | Up, Down, Enter, Delete | click item | changed, selected | ids |
| `gauge` | no | none | none | none | none |
| `line-gauge` | no | none | none | none | none |
| `sparkline` | no | none | none | none | none |
| `bar-chart` | no | none | none | none | none |
| `chart` | only if focusable flag | Left, Right | click point | selected | none |
| `canvas` | no | none | none | none | none |
| `calendar` | no | none | none | none | none |
| `progress-row` | no | none | click | activated | none |
| `meter` | no | none | none | none | none |
| `minimap` | no | none | none | none | none |
| `button` | yes | Enter, Space | click | activated(none) | none |
| `toggle` | yes | Space, Enter | click | toggled(flag) | checked, tri |
| `choice` | yes | Left, Right step; Enter opens dialog | click | selected(index) or changed(text) | selected, text |
| `number` | yes | Up, Down, plus, minus, digits | click buttons, drag slider | changed(number) | number |
| `text-input` | yes | typing, Enter submits if submit-on-enter | click sets cursor | changed(text), submitted | text |
| `text-area` | yes | typing, Enter inserts a line | click sets cursor | changed(text) | text |
| `schema-form` | one stop per row | per row widget | per row widget | changed, filter-changed | per row |
| `key-capture` | yes | Enter starts capture; Esc cancels | click | changed(text), closed | text |
| `color-picker` | yes | Enter opens dialog | click | changed(text) | text |
| `path-field` | yes | typing, Enter opens dialog | click | changed(text) | text |
| `location-picker` | yes | arrows move, Enter confirms | click map | changed(geo) | geo |
| `badge` | no | none | none | none | none |
| `spinner` | no | none | none | none | none |
| `notice` | no | none | click close glyph | closed(none) | none |
| `icon` | no | none | none | none | none |
| `image` | no | none | none | none | none |
| `video` | no | none | none | none | none |
| `surface` | no | none | click, move | activated(cell), pointer | none |
| `terminal-view` | if interactive | all keys go to the terminal | click, wheel | pointer | none |
| `code-view` | yes | arrows, PageUp, PageDown | click, wheel | changed | offset(u32) |

## Rules for every row

- A widget takes focus only if it is listed as focusable here or has `flags.focusable`. A node with `disabled-reason` is never focused.
- `scrolled` carries the first visible row as `index(u32)` and is coalesced.
- Colour roles and stable-glyph substitutes of each widget are on its family page and on [Accessibility and theming](/pages/accessibility-and-theming/).
- The text-area `changed` delay is `debounce-ms`; unset it is 600 ms up to 10,000 bytes of text and 1,000 ms above.

## See also

- [UI model](/pages/ui-model/)
- [Widget catalogue](/pages/widgets-index/)
- [Input widgets](/pages/widgets-input/)
