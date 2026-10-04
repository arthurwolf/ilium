# Smart Copy

Smart Copy turns what is on a terminal screen into things you can copy with one click: a URL, a file path, a code block, a table cell, a whole paragraph, an error message. It freezes the visible screen, outlines the regions it understands, and lets you click them to build a selection that goes to the clipboard.

There are two variants:

- **Smart Copy** (AI) is started from an agent pane's toolbar. It offers the regions Ilium detects itself and then asks your inference provider for more.
- **Smart Copy light** needs no model and no network. Hold a modifier key (Ctrl by default) over any terminal pane, click regions, release the key.

Both work on exactly what is on the screen at that moment. The text that reaches the clipboard is always read from the captured screen, never typed by a model: an AI suggestion can only point at screen cells.

This page also covers the other ways to copy from a terminal (right-click menu, the agent toolbar, mouse text selection) and how to transfer a screen from one terminal pane to another.

## Contents

- [Quick start](#quick-start)
- [Smart Copy light](#smart-copy-light)
- [Smart Copy (AI)](#smart-copy-ai)
- [Selecting regions](#selecting-regions)
  - [Multi-selection](#multi-selection)
  - [Nested and overlapping regions](#nested-and-overlapping-regions)
- [What Smart Copy recognises](#what-smart-copy-recognises)
- [Terminal right-click menu](#terminal-right-click-menu)
- [Other ways to copy](#other-ways-to-copy)
- [Screen transfer between panes](#screen-transfer-between-panes)
- [Settings](#settings)
- [Privacy](#privacy)
- [Troubleshooting](#troubleshooting)

Related pages: [Inference and privacy](inference-and-privacy.md) (choosing the provider), [Titles and instructions](titles-and-instructions.md) (Smart Copy preferences), [Agent monitoring](agent-monitoring.md) (the agent toolbar), [Panes and layout](panes-and-layout.md) (split views), [Settings](settings.md).

Demos: [Smart Copy](../../assets/demos/11-smart-copy.gif), [screen transfer](../../assets/demos/05-screen-transfer.gif). See [Demos](demos.md).

---

## Quick start

1. Hold **Ctrl** and move the pointer over a terminal pane. The screen freezes and regions light up under the pointer.
2. Keep holding Ctrl and **click** the URL, path, line or block you want. Click more to add them.
3. **Release Ctrl.** Everything you selected is copied; a short **Preview** shows what went to the clipboard.

For AI suggestions, click the **Smart copy** button in an agent pane's toolbar instead. See [Smart Copy (AI)](#smart-copy-ai).

---

## Smart Copy light

### Use it

1. Make sure it is enabled (Settings, **Terminal**, **Smart Copy light**; on by default) and note the key (**Smart Copy light key**: **Ctrl**, **Alt** or **Shift**; Ctrl by default).
2. Hold the key while the pointer is over a **terminal** pane. Editors and boards are not affected.
3. The pane freezes and the toolbar line reads `Smart copy light - N selected - click to add, release the key to copy`. Move the pointer to see the region under it.
4. Click regions to select them. A selected region stays inverted. Click it again to deselect it.
5. Release the key. If anything is selected, the selection is copied and a **Preview** dialog appears for one second, with a progress bar counting down. It shows up to 6 lines of up to 72 characters, then `... N more lines` if there is more, and a summary such as `2 selections - 87 characters copied`. If the clipboard cannot be written, the summary says `clipboard unavailable`. Release with nothing selected simply leaves.
6. To cancel without copying, press **Esc** (or `q`), or click **[ Exit ]** in the toolbar line.

The status bar reports `Copied N selection(s)` on success.

### How "release" is detected

Terminals report which modifier keys are held on every mouse event, but most do not report the key being **released**. Ilium therefore treats the first of these as the release:

- a key-release event for the modifier, on terminals that send one (the Kitty keyboard protocol);
- the first mouse event that no longer carries the modifier;
- the terminal window losing focus;
- about **1.5 seconds** after your last modified event, once something is selected, for terminals that can do none of the above.

If you hold the key still for more than 1.5 seconds with a selection and no mouse movement, the selection is finished and copied.

### Rules

- It only starts from plain pointer events (move, press, release, drag) with the modifier held. A wheel event with the modifier keeps its normal meaning (for example Ctrl+wheel).
- It does not start while a dialog or the first-run setup is open, or when another Smart Copy is already running.
- The captured screen is the last frame Ilium actually emitted for that pane. If it is not ready yet the status bar says "Smart Copy is waiting for terminal presentation acknowledgement" or "Preparing Smart Copy from the emitted terminal frame..." and nothing is selectable until the capture arrives.
- Light mode never calls an inference provider. The detected regions are the complete offer.

### Pick a key that your terminal passes through

Some terminal emulators or window managers keep Ctrl+click, Alt+click or Shift+click for themselves (Shift+click is commonly used for the terminal's own selection). If the freeze never happens, change **Smart Copy light key** to another modifier, or switch the feature off.

---

## Smart Copy (AI)

Use this when the target is not something the built-in detectors know, for example "the command the agent suggested", "the second paragraph" or "the diagram". It is available on **detected agent panes** through the agent toolbar.

### Start it

1. Focus an agent pane. The **agent toolbar** (a row of buttons at the top of the pane) must be visible; if it is hidden, click the hamburger icon on the pane border, or right-click the pane and choose **Show agent toolbar**.
2. Click **Smart copy** (the magnet icon, tooltip "Freeze the screen and discover semantic copy targets").
3. Ilium captures the screen and immediately outlines the regions it detects itself.
4. In the background it sends the screen to your inference provider and streams back additional regions. The toolbar line shows progress:

   `Smart copy - connecting|streaming|complete|failed - 4.2s - 12 selections - ~830 tokens`

   (`selections` is the number of regions offered so far; the token count is estimated until the provider reports an exact figure.) New AI regions flash briefly as they arrive.
5. Click regions as described in [Selecting regions](#selecting-regions). Unlike light mode, **each click copies immediately**: the status bar says `Added <label>; copied N selection(s)` or `Removed <label>; copied N selection(s)`.
6. Leave with **Esc**, `q` or **[ Exit ]**. Leaving cancels any request still in flight.

If the provider returns nothing usable, a small dialog explains: "Waiting for the model to answer...", "Response started; waiting for the first valid block...", "The model returned no valid selectable blocks." or "Request failed: ..." together with elapsed time, received characters and the number of rejected records. The regions Ilium detected itself remain selectable in all cases.

### What the model sees and returns

- It receives a JSON list of the screen's lines, each with an id and its words (with stable ids), plus a manifest of the regions already detected (label, kind and cell spans) so it does not repeat them. Text on the screen, including anything that looks like an instruction, is treated as untrusted data.
- It returns **JSON Lines**, one candidate per line: a short label, a kind, and one or more *parts*, each either whole line ids or an inclusive word range on a line. It is asked for the most useful additional targets first: missed URLs, commands, paragraphs, phrases, code, tables or cells and diagram regions.
- It never supplies text or coordinates. Ilium resolves every reference back to cells of the frozen screen, so the copied text is exactly what was displayed. A reference to an id that does not exist, an invalid range, an empty or whitespace-only result, an invalid kind or label, or a line over 64 KiB is rejected and counted as a rejected record; the rest of the stream continues.
- Bounds: at most 2048 candidates in total (up to 1024 of them from the built-in detectors), 128 parts per candidate, labels up to 80 characters and kinds up to 40.
- The request uses the provider's maximum output length; length is controlled by the prompt, not by a smaller cap.

### Provider and preferences

Smart Copy uses the inference provider and model you selected under Settings, **Inference** (see [Inference and privacy](inference-and-privacy.md)). Add your own selection preferences under Settings, **LLM Instructions**, **Smart Copy** (also available from the Inference tab); they are added to the system prompt in a `custom-instructions` section and nothing else is changed. Example: "Prefer complete shell commands over single words."

If no provider is configured, the request fails and the progress dialog says so. Smart Copy light and the built-in detections keep working.

---

## Selecting regions

Everything in Smart Copy is built from **regions**: spans of screen cells with a label and a kind. The hovered region is highlighted and named in the toolbar line.

| Action | Smart Copy light | Smart Copy (AI) |
| --- | --- | --- |
| Hover | Highlights the region under the pointer | Same |
| Click | Adds the region to the selection, or removes it if already selected | Same, and copies the whole selection immediately |
| Enter | Toggles the highlighted region | Same |
| Wheel up / down, Tab / Shift+Tab, Up / Down, `k` / `j` | Cycle through overlapping regions under the pointer | Same |
| Esc, `q`, **[ Exit ]** | Cancel without copying | Leave (anything already copied stays on the clipboard) |
| Release the modifier key | Copy and show Preview | (not applicable) |

### Multi-selection

- Each click **toggles** one region in a persistent selection. Selected regions stay inverted.
- Deselect a region by clicking it again. Deselecting the last one reports "Deselected <label>; selection is empty".
- The clipboard text is every selected region's text **in click order**, separated by one blank line. The status line shows how many are selected.
- In AI mode every click republishes the whole selection to the clipboard, so you can stop at any moment. In light mode the copy happens once, when you release the key.

### Nested and overlapping regions

Regions nest: a URL sits inside a line, which sits inside a paragraph, which may sit inside a code block. Ilium offers all of them:

- By default the **smallest** region under the pointer is the current one, so clicking a URL selects the URL, and clicking elsewhere on the same line selects the whole line.
- The toolbar line shows which one is current and how many overlap, for example `... - path src/main.rs - 1/3`.
- Use the **wheel**, **Tab / Shift+Tab** or **Up / Down** (`j` / `k`) to cycle to the next larger or alternative region before you click.

Hovering where no region exists shows "Move over a highlighted Smart Copy selection" when you click.

---

## What Smart Copy recognises

These are found without any model, in both variants. Matching works on the visible cells, including wide characters and combining marks, and some kinds are checked against the file system or other lines.

**Inline items**

| Kind | Examples and notes |
| --- | --- |
| URLs | `https://example.com/a?b=c`; the link text of hyperlinks as well |
| Git remotes | `git@github.com:owner/repo.git` |
| E-mail addresses | `name@example.com` |
| IP addresses | IPv4 and IPv6, with ports and prefix lengths |
| Endpoints | `host:port` |
| Domains | `example.com` |
| File paths | Absolute, `~/`, relative and bare names. Existing files and directories are **verified and labelled** by checking the pane's working directory, each of its ancestors and the project root |
| File locations | `src/main.rs:42:7` |
| Qualified names | Rust-style `a::b::C`, and each segment |
| Names | Type names, identifiers, constants, function calls |
| Identifiers | Hashes (including Git SHAs), UUIDs, hexadecimal numbers |
| Versions and packages | `1.2.3`, `package@1.2.3`, package specs |
| Colours | `#ff8800` and similar |
| Assignments | `KEY=value` (key and value separately), environment-style assignments |
| Flags | `--long-flag`, `-x` |
| Quoted text | Quoted strings |
| Fields | `key: value` (name and value separately) |
| Sentences | A sentence in prose |
| Times and dates | Timestamps, dates, times |
| Quantities | Measurements, amounts |
| Contact details | Phone numbers; postal addresses (street plus postcode and city, on one line or across up to three lines, in English, French, German and Romance-language layouts, plus P.O. boxes) |

**Styled text.** Text the program styled stands out even if no pattern recognises it: foreground-coloured runs (Codex prints file names and commands in blue), highlighted backgrounds, bold, italic and underlined text. Grey and default colours are ignored.

**Structures** (offered as a whole and, where it makes sense, by part)

| Structure | Parts offered |
| --- | --- |
| Fenced code block (```` ``` ```` or `~~~`) | The whole block with its fences, and the code inside |
| Indented code | The block |
| Table (header row, separator row, data rows) | The whole table, and each cell |
| Diff and patch output | The diff block |
| Diagnostics and tracebacks | The error block |
| Tree listings and box-drawn frames | The structure and its contents |
| Headings and sections | The heading, and the section under it |
| Paragraphs | The whole paragraph |
| Commands and inline code | The command text or the code span |
| The visible frame | The whole screen contents |

The exact set evolves with the detectors; anything not listed here can still be found by the AI variant, or selected as a plain line.

---

## Terminal right-click menu

Right-click a terminal pane. The menu is built from the exact text visible at the moment you clicked, so a screen that keeps changing cannot move the target. Entries appear only when they apply.

| Entry | Appears when | What it does |
| --- | --- | --- |
| **Show debug log** | The agent debug menu is enabled in Settings and the pane is an agent | Opens the agent debug log (see [Agent monitoring](agent-monitoring.md)) |
| **Hide agent toolbar** / **Show agent toolbar** | The pane is a detected agent | Flips the global agent-toolbar setting |
| **Copy path to history file** | The agent's history file is known | Copies the absolute path of the agent's JSONL history |
| **Copy selection** | You have an active mouse text selection | Copies it |
| **Copy last submitted prompt** | Ilium knows the exact last prompt | Copies the exact text you submitted |
| **Copy previous exact prompt (latest unavailable)** / **Last submitted prompt unavailable** | The latest prompt cannot be reconstructed exactly | Offers the older exact prompt, or tells you none is available |
| **Copy line to clipboard** | Always | The clicked screen line |
| **Copy visible terminal to clipboard** | Always | All visible text |
| **Copy full terminal history** | Always | The whole retained scrollback (see Settings, **Terminal**, **Scrollback budget**) |
| **Paste clipboard** | Always | Pastes the clipboard into the pane (bracketed paste if the program asked for it) |
| **Paste screen into _pane_ _direction_** | A neighbouring split pane is also a live terminal | See [Screen transfer](#screen-transfer-between-panes) |
| **Open externally** / **Open in editor** | The clicked cell is an allowed URL, or a path that exists right now | Opens it with the system handler, or in an Ilium editor beside the terminal |

Keys in the menu: Up / Down (`k` / `j`) move, Enter / Right (`l`) activate, Esc or `q` close.

---

## Other ways to copy

**Agent toolbar** (on detected agent panes; see [Agent monitoring](agent-monitoring.md)):

| Button | Effect |
| --- | --- |
| Copy screen | Copies the visible screen text to the clipboard. Entirely local; nothing is sent to the agent. |
| Smart copy | Starts [Smart Copy (AI)](#smart-copy-ai). |
| Copy last message | Sends the agent's own `/copy` command so the agent puts its last message on the clipboard. |
| Text selection | Turns mouse text selection on or off. |

**Mouse text selection.** While **Terminal text selection** is on (Settings, **User Interface**; default on), a left-button drag over a terminal pane highlights text locally instead of forwarding the drag to the program. Right-click and choose **Copy selection** to copy it. A plain click clears an empty selection, and scrolling the wheel drops the selection because its cell positions would no longer match. Turn the setting off when you want a foreground program (an agent menu, a full-screen app) to receive clicks and drags itself. Ctrl-drag is not claimed by Ilium's selection.

**Editor copy actions** (copy line, copy chapter, copy whole file) are in [Editors and boards](editors-and-boards.md#right-click-menu-on-a-line).

---

## Screen transfer between panes

Screen transfer pastes the **visible screen** of one terminal pane into another, so you can hand an agent the output of a shell next to it, or the other way round.

### Use it

1. Put two or more panes in a split view (see [Panes and layout](panes-and-layout.md)). Both must be live **terminal** panes, shells or agents. A border next to an editor or board offers no transfer.
2. On the separator between two such panes, a small arrow control appears on the edge of the **source** pane, pointing toward the **destination**. Panes that share an edge in a four-pane grid get controls for each direction; diagonal panes do not.
3. **Click the arrow.** The source pane's visible text is pasted into the destination pane. The status bar says "Screen pasted into _label_ _direction_".

Or right-click the source pane and choose **Paste screen into _label_ on the left / on the right / above / below**.

### Details

- What is pasted is the plain text of the source's current visible screen, as one paste. It is **not** submitted: Ilium does not add Enter, so you can edit before sending.
- The destination receives it as a paste (bracketed when the program asked for bracketed paste), and the destination view scrolls to the bottom first.
- If the destination input queue is full the paste is refused with "Terminal paste rejected before admission". If the destination stops being a terminal before the click lands, you see "The destination pane is no longer a terminal".
- The arrow glyphs can be changed under Settings, **Icons** (Screen transfer: paste left / right / up / down).

---

## Settings

| Setting | Where | Default | Notes |
| --- | --- | --- | --- |
| Smart Copy light | Settings, **Terminal** (`[terminal] smart_copy_light`) | On | Turns the held-key variant on or off. The AI variant is unaffected. |
| Smart Copy light key | Settings, **Terminal** (`[terminal] smart_copy_light_key`) | Ctrl | `"ctrl"`, `"alt"` or `"shift"`. |
| Smart Copy instructions | Settings, **LLM Instructions** or **Inference** | empty | Selection preferences for the AI variant; empty uses the built-in prompt. |
| Terminal text selection | Settings, **User Interface** | On | Mouse drag selection, separate from Smart Copy. |
| Smart copy toolbar icon, screen-transfer arrows | Settings, **Icons** | magnet; arrows | Cosmetic. |
| Inference provider and model | Settings, **Inference** | none until chosen | Needed for the AI variant only. |

---

## Privacy

- **Smart Copy light** and all built-in detections run entirely in Ilium on your machine. Nothing is sent anywhere.
- **Smart Copy (AI)** sends a snapshot of the **visible screen text** (numbered lines and words) and a list of already-detected regions to the inference provider you selected, together with your Smart Copy preferences. A screen can contain secrets, tokens or private output: close or clear a pane before using the AI variant if that matters, or use the light variant. Providers are described in [Inference and privacy](inference-and-privacy.md).
- Clipboard contents go only to your system clipboard.

---

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| Holding the key does nothing | **Smart Copy light** is off; the pointer is not over a terminal pane's content; a dialog is open; or your terminal or window manager captures that modifier. Try another **Smart Copy light key**. |
| The selection copies by itself after a pause | Your terminal does not report key release; Ilium finishes after about 1.5 s without a modified event. Keep clicking or moving with the key held. |
| Nothing was copied, Preview says "clipboard unavailable" | Ilium could not write the system clipboard (for example no clipboard service in a headless or remote session). Use a terminal that provides one, or copy through your terminal's own mechanism. |
| The **Smart copy** button is missing | The agent toolbar is hidden or the pane is not a detected agent. Show the toolbar (hamburger icon or right-click) or use the light variant. |
| AI mode shows "Request failed" | Provider not configured, a network or authentication error, or a model unable to stream; test the provider under Settings, Inference. Built-in regions still work. |
| A path is not highlighted as an existing file | It does not exist relative to the pane's working directory, its ancestors or the project root, so it is offered as plain text only. |
| Clicking copies the whole line, not the URL | Another region is current. Cycle with the wheel or Tab before clicking. |
| Text selection by dragging does not work in an agent menu | **Terminal text selection** claims the drag; turn it off in Settings or from the toolbar. |
