# Titles and custom instructions

Ilium can name the entries in your tree for you: agent panes, terminals and projects get short, scannable titles and icons, and the tree can be reorganized into groups. This page explains when automatic titles are applied, how the names you type yourself are protected, how to choose the title style, and how the seven optional custom instruction inputs let you steer wording, grouping, voice behaviour and more without touching the built-in prompts.

All AI-driven features need an inference provider; see [Inference and privacy](inference-and-privacy.md) for setup and for what each request contains.

## Contents

- [Automatic titles](#automatic-titles)
- [When a title is applied](#when-a-title-is-applied)
- [Manual names are protected](#manual-names-are-protected)
- [Title style](#title-style)
- [Triggering titles manually](#triggering-titles-manually)
- [Custom instructions](#custom-instructions)
- [The seven instruction inputs](#the-seven-instruction-inputs)
- [Editing instructions step by step](#editing-instructions-step-by-step)
- [Tree organization instructions](#tree-organization-instructions)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)

## Automatic titles

Every entry has a full title, a short title (used where space is tight) and an icon. For agents, terminals and projects Ilium can ask your inference provider to propose them.

- **Agent panes** are titled from the pane's own session history: the sequence of requests you have made to that agent.
- **Terminals** are titled from their scrollback; the earliest commands establish the terminal's long-term role and later commands only change it if the terminal is genuinely repurposed.
- **Projects** are named from the project path, root listing, `CLAUDE.md` and `README.md`: one or two words plus an icon.
- **Groups** created by tree organization receive a title, short title and icon from the same pass.

Titles describe the enduring subject you would search for later, not the latest step. A good title survives testing, debugging and completion steps; it changes only when the work genuinely changes scope.

## When a title is applied

Agent panes keep their existing names until Ilium can verify a real task from that pane's session history, or a task you submitted to its current agent session. Launching an agent, seeing its startup screen, or receiving a monitoring message does not establish a task. Specifically:

- A genuine request must exist in the pane's project-verified conversation, or be text you authored followed by Enter for that exact invocation.
- Startup screens, assistant output, goal bookkeeping, update requests, progress notifications and sibling panes' work never grant permission to title a pane.
- If the history is missing or cannot be verified, the existing name stays untouched. Tree organization may still move the pane into a group.
- Submitting exactly `/clear` to Claude or Codex discards that pane's detected session. Automatic title fields reset to `<new>` until the replacement session can be verified and titled; manually fixed names, short titles and icons remain unchanged.
- An eligibility check is made twice: the client when it captures the pane's state, and the server again when it applies the result. A stale or ineligible title is dropped; valid structural grouping can still apply.
- A delayed AI result cannot replace a newer manual name or a newer accepted AI title. Results are fenced by the pane's presentation revision and conversation identity, and a result that arrives after you renamed the pane is discarded.

Which events run titling at all is configured on the **Triggers** tab (see [Settings](settings.md#triggers)). By default titles are refreshed when an agent session becomes ready, when a prompt is received, when the agent starts working, when it waits on background work, when it finishes, and at every second submitted plain-shell command in a terminal.

## Manual names are protected

A name you enter through Rename (`Ctrl+B ,` by default, or the Rename hover control) is yours:

- The literal name, its short title and its icon are fixed together as one bundle and are never overwritten by automatic naming.
- The protection survives fresh conversations, session recovery and restart.
- A name-fixed group is preserved exactly by tree organization (title, short title and icon), even when it is moved or its children are regrouped. It is listed once as an existing group.
- Asking AI to retitle an automatically named pane produces another automatic name and follows the same task checks. Retitle does not replace a name you entered through Rename.
- Inferred titles stay automatic even after an explicit AI retitle; only a literal rename makes a title manual.
- Undo restores accepted presentation revisions and preserves later title changes; manual names remain fixed.

## Title style

Choose how automatic titles are worded in **Settings -> Titles** (a radio pair; click or use the keyboard):

| Style | What you get |
| --- | --- |
| Labeling (default) | A concise retrieval label in UPPERCASE: short label 1 to 3 words, long label 1 to 7 words, naming the durable object, problem or initiative. Cites your own recognisable words; avoids generic suffixes like "WORK" or "FIXES"; never includes secrets, commands, IDs, paths, logs or completion status |
| Summarization | A descriptive summary of what the session has generally been about, such as "Rework Web UI" or "Measure Music Share": short title 2 to 3 words, long title at most 7 words, not a play-by-play of the latest turn |

The style affects future automatic and requested AI titles only. Existing titles are not rewritten, and user-fixed titles remain yours. In `config.toml` the style is `title_style = "labeling"` or `"summarization"` under `[inference]`.

Labeling rules in short:

- Name the thing the user will look for again, at the scope they intend to return to.
- Prefer the user's words and repeated terms; an explicitly stated goal outweighs a technical theme inferred from the assistant's work.
- Use the ancestor path and nearby titles to judge scope and distinguish entries; omit redundant parent or project words only if the label is still recognisable on its own.
- An existing automatic title is evidence, not a constraint: keep it if it already works.

## Triggering titles manually

- **Retitle** control on an agent row (hover the row; the icon is configurable under Settings -> Icons).
- **Restructure** on the tree toolbar, or on a project row, to reorganize and retitle a project.
- Voice: ask the assistant to retitle an entry or restructure a project.

Manual requests follow the same task checks as automatic ones.

## Custom instructions

**Settings -> LLM Instructions** collects seven optional free-text inputs. The same values also appear in their feature tabs; both locations edit one stored value, so there is no copy to keep in sync.

Instructions are added to new requests and saved globally (not per project). Empty inputs use the built-in defaults, and the built-in prompts keep their required output format: your text refines the task, it cannot change the shape of the answer. Voice instructions refine the assistant's behaviour; the other inputs refine their specific task. Leading and trailing whitespace is trimmed, and the text is interpolated once into the prompt.

## The seven instruction inputs

| Input | Feature tab | Used by | Typical content |
| --- | --- | --- | --- |
| Voice assistant | Voice control | The voice assistant's system prompt | Language, vocabulary, response style |
| Entry naming | Titles | Titles and summaries of agent panes and terminals, and the titles produced during restructure | Preferred wording, terminology, language |
| Organization | Inference | Tree restructure | Grouping and ordering rules |
| Shared naming and organization context | Inference | Entry names, project names and tree organization | Vocabulary and preferences common to all three |
| Project naming | Inference | Project name and icon | Preferred language and abbreviations |
| Smart Copy | Inference | Smart Copy suggestions | Which exact-source copy targets to prioritise |
| Ask for update | Agent Monitoring | The "Ask for update" request sent to an agent | What a status update should emphasise |

Storage: the six non-voice inputs live under `[inference.instructions]` (`entry_naming`, `organization`, `naming_and_organization`, `project_naming`, `smart_copy`, `ask_for_update`); the voice input is `[voice].custom_prompt`.

## Editing instructions step by step

1. Open Settings (`Ctrl+B :`) and choose **LLM Instructions** (or the feature tab listed above).
2. In a feature tab, press `i` to focus the instruction inputs. In **LLM Instructions** they are all listed.
3. Select an input and press `Enter` (or click it). Each row shows the first part of the current text, or "Built-in default" when empty.
4. Type in the multi-line editor. Newlines are allowed.
5. Press `Ctrl+S` to apply. `Esc` cancels without changing anything.
6. To clear an input and return to the default, select it and press `Delete`.

New requests pick up the change immediately. Naming workers copy current settings when each request is dispatched, so a request already in flight uses the instructions it started with.

## Tree organization instructions

Restructure combines three inputs, in this order inside the prompt:

1. **Entry naming**, which also governs titles of entries and new groups chosen during a restructure.
2. **Organization**, the grouping and ordering rules.
3. **Shared naming and organization context**, common vocabulary.

They are included on the first request and on every corrective retry. Typical uses:

- "Group by customer first, then by feature."
- "Keep deployment and infrastructure terminals together in a group called OPS."
- "Never put agents and their terminals in different groups."

What instructions cannot do: override Ilium's structural rules. Split views stay protected, name-fixed groups stay exactly as you named them, every pane appears exactly once, and the reply must follow the required JSON shape. See [Inference and privacy](inference-and-privacy.md#tree-organization-restructure).

## Examples

Entry naming:

```text
Write titles in French. Keep product names such as "Ilium" in English.
```

Shared context:

```text
Our products are Atlas (billing), Beacon (alerts) and Cobalt (data pipeline).
Use these exact names; never abbreviate them.
```

Organization:

```text
Group work by product first. Put one-off experiments in a group called LAB.
```

Project naming:

```text
Use the repository's short codename, not its descriptive title.
```

Ask for update:

```text
Lead with blockers, then what changed since the last update, then next steps.
```

Voice assistant:

```text
Answer in short sentences. Always confirm which agent you are talking to.
```

## Troubleshooting

- **A pane keeps its old name.** No verified task exists yet, history is unavailable, or the name is manual. Submit a real request and use Retitle once the task can be verified. A manually named pane keeps its name even after Retitle; use Rename to change it yourself.
- **My own name was kept but the group moved.** Expected: name-fixed items keep their text; structure can still change.
- **Titles are in the wrong style.** Check Settings -> Titles. Existing titles change only when retitled.
- **An instruction seems ignored.** Instructions refine but cannot change the required format; make them specific and short. Confirm the correct input is used (Entry naming for titles, Organization for grouping).
- **No AI titles at all.** Check that guided setup is complete, a provider works (**Test provider**), and the Triggers tab has Retitle actions. See [Inference and privacy](inference-and-privacy.md#troubleshooting).
- **After `/clear` an automatic title shows `<new>`.** Normal until the new session has a verified request. Manual names stay unchanged.

Related: [Settings](settings.md), [Inference and privacy](inference-and-privacy.md), [Voice](voice.md), [Agent monitoring](agent-monitoring.md).
