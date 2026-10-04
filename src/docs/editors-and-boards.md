# Editors and boards

Ilium panes are not only terminals. An **editor pane** edits a local text file, with syntax highlighting, a minimap, autosave and a rendered preview for Markdown. A **board pane** is a Kanban board whose cards live in ordinary Markdown files you choose. Both sit in the same project tree as your terminals and agents, can be split side by side with them, and are saved with the session layout.

This page explains how to open and use each, every related setting, how boards are stored on disk, and the limits you may meet with very large files.

## Contents

- [Editor panes](#editor-panes)
  - [Open a file](#open-a-file)
  - [The toolbar](#the-toolbar)
  - [Editing and saving](#editing-and-saving)
  - [Autosave](#autosave)
  - [Markdown: Source and Rendered views](#markdown-source-and-rendered-views)
  - [Right-click menu on a line](#right-click-menu-on-a-line)
  - [Editor settings](#editor-settings)
  - [Large files and source windows](#large-files-and-source-windows)
- [Kanban boards](#kanban-boards)
  - [Create a board](#create-a-board)
  - [Use a board](#use-a-board)
  - [How boards are stored](#how-boards-are-stored)
  - [Create a board from a Markdown file](#create-a-board-from-a-markdown-file)
  - [Board settings](#board-settings)
- [Keys at a glance](#keys-at-a-glance)
- [Troubleshooting](#troubleshooting)

Related pages: [Panes and layout](panes-and-layout.md) (split views mixing terminals, editors and boards), [Settings](settings.md), [Automation](automation.md) (create an agent from an editor line), [Smart Copy](smart-copy.md), [Session recovery](session-recovery.md).

Demos: [Markdown editor](../../assets/demos/19-markdown-editor.gif), [Kanban board](../../assets/demos/15-kanban.gif), [mixed splits](../../assets/demos/10-mixed-splits.gif). See [Demos](demos.md).

---

## Editor panes

### Open a file

An editor pane is always backed by a path. Ways to open one:

| Way | Steps |
| --- | --- |
| Leader key | Press the prefix (`Ctrl+B` by default) then `e`. A file picker opens. |
| Tree context menu | Right-click a project, group or empty tree space and choose **New editor here**. The same picker opens. |
| From a terminal | Right-click a file path or `file:line` shown in a terminal pane and choose **Open in editor**. The editor opens beside that terminal. The entry appears only when the clicked text resolves to a file that exists right now. |
| From another editor | Right-click a path in an editor line and choose **Open in editor**. |
| Workspace search | `Ctrl+B f` searches terminal history and open editor buffers (see [Panes and layout](panes-and-layout.md)). |

The file picker is a modal table with folder and file icons, size and modified columns (they collapse as the popup narrows). It starts in the originating pane's working directory.

| Key | Action |
| --- | --- |
| Up / Down, `k` / `j` | Move |
| Enter, Right, `l` | Open the folder or file |
| Left, Backspace, `h` | Go to the parent folder |
| `Ctrl+H` | Show or hide hidden files |
| Right-click a `.md` file | File actions, including **Create board from Markdown** |
| Esc | Cancel |

The mouse works too: click an entry to select and open it.

A file that does not exist yet opens as an empty buffer that is created on first save. A directory, or a path that cannot be read, is an error.

### The toolbar

Every editor pane has a toolbar above its text. It is click-driven; the leader-key equivalents are in [Keys at a glance](#keys-at-a-glance).

| Button | Meaning |
| --- | --- |
| **Source** / **Rendered** | Switch a Markdown file between editing and the rendered preview. Only shown for `.md` and `.markdown` files. |
| **Pixel headers** (checkbox) | Rendered view only: draw headings as pixel images (on) or as bold styled text (off). See [Rendered view](#markdown-source-and-rendered-views). |
| **Clip** / **Wrap** | How long lines are shown: clipped at the edge (scroll horizontally) or wrapped onto extra visual rows. This changes presentation only, never the file. |
| **# Lines: on/off** | Line-number gutter. |
| **Map: on/off** | Minimap column. |
| **Auto: on/off** | Autosave for this editor. |
| **Save** | Save. Highlighted when there are unsaved edits. |
| **Save As...** | Save to a new path; the pane retargets to it. |

Toolbar toggles apply to that pane only. The defaults for new panes are under [Editor settings](#editor-settings).

### Editing and saving

- **Typing.** In Source view the text area takes keyboard input. Keys that Ilium itself reserves (the prefix and the actions it binds) are handled by Ilium; the rest go to the text area, which uses the default key handling of the `ratatui-textarea` widget (arrow keys, Home/End, Page Up/Down, Backspace, Delete and its other standard editing chords).
- **Mouse.** Click to place the cursor, use the wheel to scroll, click the minimap to jump, and use the scrollbar.
- **Syntax highlighting.** Recognised file types are highlighted in Source view; colours follow the active theme.
- **Save.** `Ctrl+B S` (default), the **Save** button, or autosave. The editor tracks a dirty flag; the status bar and the pane close confirmation use it.
- **Closing with unsaved changes.** Closing the pane asks `"name" has unsaved changes. Close anyway?` with a Discard choice, so you cannot lose edits by accident.
- **Clipboard copies from the context menu use the live buffer**, so unsaved edits are included.

### Autosave

Autosave is **on by default**. After each modifying edit, Ilium waits for the configured delay (default 1000 ms) with no further edits and then writes the file. Every new edit restarts the wait, so a burst of typing produces one write. If you turn autosave off for a pane, nothing is written until you save.

Settings reload does not strand a dirty pane: an editor with unsaved edits keeps its pending save when you change autosave defaults.

### Markdown: Source and Rendered views

Markdown files (`.md`, `.markdown`) have two views:

- **Source**: plain text editing.
- **Rendered**: a read-only preview. The editor's cursor position in Source is kept for when you switch back. Typing does nothing in Rendered; switch back to Source to edit.

Switch with the toolbar, or `Ctrl+B v`. New Markdown files open in the view set by **Markdown default** (Source by default).

What Rendered shows:

- Headings, paragraphs, lists, block quotes, code blocks, tables and rules as styled text. Paragraphs reflow to the pane width and re-wrap on resize.
- Headings and standalone images are drawn as **pixel images** using the terminal graphics protocol your terminal supports (Kitty, Sixel or iTerm2), with a half-block fallback. Switch **Pixel headers** off for bold styled text headings if your terminal has no graphics support.
- Only a paragraph that is exactly one image is drawn as an image. An image inside running text is shown as a small placeholder.
- Images are resolved from **local files** relative to the Markdown file. Remote `http(s)` images are never fetched: opening a Markdown file never makes a network request, and such images show as text placeholders. There is no Mermaid or PDF support.

Task-list checkboxes (`- [ ] item`, `- [x] item`) can be **clicked in Source view** to flip them; the change is written to the buffer and marks it dirty.

If a document is too big to prepare (more than 32,768 lines, any line over 64 KiB, or more than 2 MiB of text), Rendered view reports "document exceeds preparation limits; showing source" and shows the source instead.

### Right-click menu on a line

Right-click a line in Source view for actions on that line:

| Entry | Meaning |
| --- | --- |
| **Copy line to clipboard** | The exact clicked physical line. |
| **Copy chapter to clipboard** | Markdown only, and only when the line is inside a heading section. Copies the raw source from that heading to just before the next heading of the same or a higher level. |
| **Copy entire file to clipboard** | The whole buffer, including unsaved edits. |
| **Create agent from line...** | Starts a Claude, Codex or Antigravity agent with a prompt built from the line. See [Automation](automation.md#create-an-agent-from-an-editor-line). |
| **Open in editor** / **Open externally** | Only when the clicked cell is a URL or an existing path. External open uses your system handler. |

### Editor settings

Settings (`Ctrl+B :`), **Editor** tab. These are the defaults for newly opened editors; the toolbar changes one pane.

| Setting | TOML key under `[editor]` | Default | Values |
| --- | --- | --- | --- |
| Long lines | `line_display` | Clip | `"clip"` or `"wrap"` |
| Line numbers | `show_line_numbers` | On | true / false |
| Minimap | `show_minimap` | On | true / false |
| Autosave | `autosave_enabled` | On | true / false |
| Autosave delay | `autosave_delay_ms` | 1000 | 250 to 5000; the Settings arrows step through 250, 500, 1000, 2000, 5000; a value typed in the file is clamped into the range |
| Markdown default | `markdown_rendered_by_default` | Source | true shows Rendered first |

The related leader-key bindings are remappable under Settings, **Keyboard** (see [Settings](settings.md)).

### Large files and source windows

Editors keep large files responsive by preparing what you see on background workers and painting from a bounded **window** of rows rather than rebuilding the whole file on every frame. A window is only ever a view onto the buffer: the pane never presents a partial window as if it were the complete file, and clipboard actions use the complete buffer.

Limits worth knowing:

| Limit | Value |
| --- | --- |
| Largest file loaded or saved | 32 MiB of text |
| Most lines in an editor buffer | 262,144 |
| Rendered Markdown preview | 32,768 lines, 64 KiB per line, 2 MiB total, otherwise Source is shown |
| Visible prepared panes | Four at a time (the split-view maximum) |

Opening something larger than the limits is refused with a message instead of freezing the interface.

---

## Kanban boards

A board pane shows columns of cards. Cards can be dragged between columns, edited in a side panel, and carry clickable task checkboxes. Everything is saved to your own Markdown files after every change, so boards diff cleanly in Git and can be edited with any other tool.

### Create a board

1. Press `Ctrl+B B`. The **New board** dialog opens.
2. **Board name**: the pane's name. Default `Board`.
3. **Storage** choose between:
   - **One Markdown file** (default): one file holds every column and card.
   - **Folder columns + Markdown cards**: a directory with one sub-folder per column and one file per card.
   Press `Ctrl+Space` to open the storage list.
4. **Storage path**: where to keep it. The default is `.ilium/boards/board.md` under the project root (the project that owns the group you are in). Relative paths resolve against the project root. Use **[ Browse path... ]** or `Ctrl+P` to pick with the file picker (for the folder kind the picker offers **Use Folder**).
5. Press Tab to move between fields; confirm with **Create board**.

A storage path that does not exist yet is created with three empty columns: **To do**, **Doing**, **Done**. A path that already exists is opened as it is (an existing empty folder simply opens with no columns; press `c` to add one). A board's storage can be open in only one pane per session: asking for one that is already open focuses the existing pane ("That board storage is already open"), which prevents two stale copies from overwriting each other.

### Use a board

Keyboard (board focused, no card editor open):

| Key | Action |
| --- | --- |
| Left / Right, `h` / `l` | Previous / next column |
| Up / Down, `k` / `j` | Move between the column header and its cards |
| Enter | Open the selected card in the details panel |
| `n` | New card in the selected column |
| `c` | New column |
| `e` | Rename the selected card or column |
| `d` | Delete the selected card or column (asks for confirmation) |
| Shift + arrows (or `h` `j` `k` `l`) | Move the selected card to the neighbouring column, or up/down within its column |
| `r` | Reload from storage (needed after the file was changed outside Ilium; see below) |

Mouse:

- Click a card to select it and open its details on the right (the keyboard equivalent is Enter).
- **Drag** a card to another place. The source and the insertion point are highlighted while you drag; releasing drops it there.
- Click a task checkbox (`[ ]` / `[x]`) in a card title to toggle it; the single character is written immediately.
- Use the wheel to scroll the board or the card body; horizontal scroll or the scrollbar moves across columns when they do not fit.

**Details panel.** The rightmost third shows the selected card's **title** and **notes** in two editable fields. Tab switches fields, typing edits, and **every change is saved immediately** (the status bar says "Saving card..."). Esc closes the panel.

Card titles are one line; column titles are one line and cannot be empty or contain path separators.

### How boards are stored

**One Markdown file** (default). Headings are columns and list items are cards:

```markdown
# Board

## To do
- Write the migration
  Notes for the card are indented by two spaces
  and may span several lines.
- [ ] Update the docs

## Doing
- [x] Review the API change

## Done
```

- Every ATX heading (`#`, `##`, ...) that owns content becomes a column; a lone level-one document title with no cards is ignored.
- Unordered list items (`-`, `*`, `+`) become cards, including nested task lists. Indentation beyond two spaces in a note is preserved.
- Fenced examples are skipped, so a code block containing `- item` does not become a card.
- Top-level prose between lists is not part of the board.
- When Ilium saves, it rewrites the file in its canonical form (`# Board`, then `## Column` headings and `- card` items), so extra prose and some formatting in a hand-written file are normalised. Keep other content in a separate file.
- Saving is guarded by a lock and by a check that the file still holds exactly what Ilium last read. If another program changed it, the save is refused with "changed outside this board; press r to reload before editing" instead of overwriting your edit.

**Folder columns + Markdown cards.**

```text
board/
  To do/
    001-write-the-migration.md
    002-update-the-docs.md
  Doing/
    001-review-the-api-change.md
  Done/
```

- Each immediate sub-folder is a column (its folder name is the column title). Files that are not folders are ignored.
- Each `.md` file in a column folder is a card. The title is the first line when it starts with `# `; otherwise the file name (hyphens become spaces). Everything after the title is the card's notes.
- Ilium names cards `NNN-slug.md` in column order (three-digit position, then a lower-case slug of the title, at most 80 characters) and renames them as cards move. Obsolete `.md` card files are removed after the new ones are written; other files and nested folders in a column are left alone. Writes are staged and replaced file by file so a failed save does not leave a half-written column.
- Limits: at most 8192 directory entries scanned, and 1 MiB of retained board content.

Both formats are read with bounded sizes (a single-file board is limited to 512 KiB).

### Create a board from a Markdown file

Any Markdown task list can become a board without being rewritten at creation:

- Right-click an **open Markdown editor** in the tree and choose **Create board from Markdown**. Unsaved edits are saved first so the board reads exactly what you see.
- Or, in the file picker, right-click a `.md` file.

The board opens as a **One Markdown file** board on that same file, in the same group. After that, edits made through the board are saved back in the canonical form described above.

### Board settings

Settings, **Kanban Board** tab, stored under `[kanban_board]`:

| Setting | TOML key | Default | Range |
| --- | --- | --- | --- |
| Card preview lines | `card_preview_lines` | 4 | 1 to 10 wrapped lines per card |
| Minimum column width | `minimum_column_width` | 45 | 10 to 80 cells; narrower viewports show whole columns behind a horizontal scrollbar |

Values outside the range are rejected when the file is read. Card previews are contiguous, with no redundant labels. Change the board and editor icons under Settings, **Icons** (see [Settings](settings.md)).

---

## Keys at a glance

Default leader keys (after the prefix, `Ctrl+B`). All of these are remappable under Settings, **Keyboard**:

| Keys | Action |
| --- | --- |
| `e` | New editor (opens the file picker) |
| `B` | New board |
| `S` | Save the focused editor |
| `v` | Toggle Source / Rendered for a focused Markdown editor |
| `N` | Toggle line numbers in the focused editor |
| `b` | Toggle the minimap in the focused editor |
| `a` | Toggle autosave in the focused editor |
| `f` | Search terminal history and open editor buffers |

---

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| Rendered view shows source and a "limits" message | The Markdown file exceeds the preview limits (lines, line length or total size). Edit in Source or split the file. |
| Remote images do not appear in Rendered | By design: only local images are drawn; remote images are never fetched. |
| Headings look like blocks or are missing | Your terminal may not support a graphics protocol. Turn **Pixel headers** off. |
| Edits are not saved | Autosave is off for this pane (check **Auto**) and you have not saved; use **Save** or `Ctrl+B S`. |
| Board will not save: "changed outside this board" | The Markdown file changed on disk after Ilium read it. Press `r` in the board to reload, then redo the edit. A reload is refused while writes are still pending. |
| Board creation says storage is already open | Another pane in this session owns that file or folder; focus that pane. |
| Folder board column is rejected | The folder name is empty, `.`/`..`, or contains a path separator or newline. |
| Board lost formatting from my hand-written file | Saving rewrites a single-file board in canonical form; keep non-board prose elsewhere. |
| "Create board from Markdown" is missing | The editor is not backed by a `.md`/`.markdown` file. |
