# Inference and privacy

Ilium works without any AI service: terminals, agent detection, notifications, worktrees and session storage never contact an LLM. A language-model "inference provider" is used only for background titling of tree entries, tree organization (restructure), project naming, optional Smart Copy suggestions, and the "Ask for update" prompt text you configure. This page explains how to choose and configure a provider, load and test models, control the token budget, and exactly what is sent where, so you can decide what is safe for your work.

## Contents

- [What uses AI](#what-uses-ai)
- [Providers](#providers)
- [Step by step: choosing a provider](#step-by-step-choosing-a-provider)
- [Loading models](#loading-models)
- [Test provider](#test-provider)
- [Restructure token budget](#restructure-token-budget)
- [Tree organization (restructure)](#tree-organization-restructure)
- [What is sent where](#what-is-sent-where)
- [Logging](#logging)
- [Privacy guidance](#privacy-guidance)
- [Configuration reference](#configuration-reference)
- [Troubleshooting](#troubleshooting)

## What uses AI

| Feature | Provider used | Can be turned off by |
| --- | --- | --- |
| Automatic entry titles (agents, terminals) | Selected inference provider | Clearing the retitle action from each event on the Triggers tab, or choosing "Skip" in guided setup |
| Tree organization (grouping, ordering, group names, icons) | Selected inference provider | Clearing restructure actions on the Triggers tab |
| Project naming (name and icon) | Selected inference provider | Naming the project yourself |
| Smart Copy extra copy targets | Selected inference provider | Not using Smart Copy suggestions; see [Smart Copy](smart-copy.md) |
| Voice assistant | OpenAI Realtime (separate key; not the inference provider) | Leaving voice control off; see [Voice](voice.md) |

New installs enable AI titles and tree organization through Kilo Gateway, but automatic requests stay paused until the first-run guided setup is finished. Choosing **Skip** in setup, or an `[onboarding]` state with AI disabled, keeps them off. Existing configurations keep the provider they already had.

## Providers

Open **Settings -> Inference** (`Ctrl+B :`, then the Inference tab). Five providers are available:

| Provider | Credential | Endpoint | Model | Model discovery |
| --- | --- | --- | --- | --- |
| Kilo Gateway (default) | none | built in | Chosen from Kilo's live free-model catalogue | Yes; fallback list if offline |
| Ollama (local) | none | `http://127.0.0.1:11434` | Type or choose an installed model | Yes |
| OpenAI-compatible | API key | `https://api.openai.com/v1` (editable) | Loaded catalogue or typed ID | Yes (needs a key) |
| Anthropic | API key | `https://api.anthropic.com` (editable) | Type the model name | No |
| OpenRouter | API key | `https://openrouter.ai/api/v1` | Type the model name; default `openrouter/free` | No |

Only the fields relevant to the selected provider are shown. Switching providers never discards another provider's saved endpoint, key or model, so you can switch back and forth.

Provider notes:

- **Kilo Gateway.** The default free model is `stepfun/step-3.7-flash:free`. Kilo's default free model was marked as permitting prompt training when last checked (2026-09-27). Review Kilo's [data and usage guidance](https://kilo.ai/docs/getting-started/using-kilo-for-free) before sending project text. The Kilo model row warns that requests may be used for training; that warning is real, not decorative.
- **Ollama.** Fully local: prompts go to the server at the URL you set. Use this when titles and organization must never leave your machine.
- **OpenAI-compatible.** Also works with other services that implement the same HTTP interface; set the URL accordingly. Official OpenAI uses `max_completion_tokens` with documented per-model maxima and omits the limit for unknown model IDs.
- **Anthropic.** Uses the Anthropic API directly. A blank URL resolves to the Anthropic default.
- **OpenRouter.** Routes to many hosted models; the default `openrouter/free` selects a free router.

Requests ask for the model's maximum output allowance when it is known; response length is controlled by the prompts, not by a small convenience cap. Each request has a 45-second timeout.

## Step by step: choosing a provider

1. Open Settings and select **Inference**.
2. On the **Provider** row, press Left or Right to cycle through the five providers.
3. Fill in the rows that appear:
   - Ollama: **URL** (if not the default), then **Load available models**, then choose a **Model**.
   - OpenAI-compatible: **URL**, **API key**, **Load available models**, **Model**.
   - Anthropic: **URL**, **API key**, **Model**.
   - OpenRouter: **API key**, **Model**.
   - Kilo Gateway: choose a **Model** with Left and Right.
4. Press `Enter` on a text row to edit it, type, and press `Enter` again. The editor stays open until the value has been written to disk; if writing fails your input stays with an error and `Enter` retries.
5. Select **Test provider** to verify the configuration (see below).
6. Optionally open the **Triggers** tab to decide which events cause AI requests.

API keys are masked in the interface (only the last four characters are shown) and are never exposed to the voice assistant's settings tool.

### OpenAI example

1. Select **OpenAI-compatible**.
2. Enter your API key.
3. Choose **Load available models**.
4. Use Left and Right on the **Model** row to select a discovered ID, or press `Enter` to type one exactly.
5. Run **Test provider**.

The catalogue shows every model your key exposes; some IDs belong to other APIs (embeddings, speech, images) rather than text chat, so use **Test provider** to confirm a choice.

## Loading models

**Load available models** appears for Ollama, Kilo Gateway and OpenAI-compatible (OpenAI needs a key). While it runs the button shows a spinner; on failure it reads **Retry model discovery**.

- The catalogue is requested with `GET <base>/models` using your credentials where required. Ilium returns sorted exact IDs and does not invent capabilities.
- Refreshing preserves your saved model even if it is not in the new list.
- Ilium keeps the last good catalogue, and results that arrive after you have changed the provider, URL or key are discarded, so an old request cannot overwrite newer settings.
- For Kilo Gateway, the catalogue is public and unauthenticated; stable fallback models are shown when discovery fails.
- Anthropic and OpenRouter have no discovery row: type the model name.

## Test provider

**Test provider** sends one harmless, fixed, synthetic request that asks the model to title and organize an invented work sample ("fix login validation, add a regression test, and update the release notes") and return JSON. It never includes your terminals, files or transcripts. Ilium validates the reply, retries once if the JSON is malformed, and previews the resulting tree with the elapsed time. Use it after changing provider, key, URL or model.

## Restructure token budget

Restructure prompts can be large (a whole project's tree plus context). Ilium limits the rendered input to **200,000 estimated input tokens** by default.

- Change it in **Settings -> Inference -> Restructure token budget**, or set `restructure_prompt_token_limit` under `[inference]` in `config.toml`. The value must be greater than zero.
- The estimate rounds up one token per four Unicode characters. The selected provider's real context limit still applies, so pick a budget your model can accept.
- The budget includes the instructions, the animation scene catalogue, protected layouts and any corrective retry feedback.
- The editor saves valid values automatically after 600 ms; `Enter` shares the same pending write rather than queueing a second one.

## Tree organization (restructure)

Restructure asks the provider to reorganize one project (or all projects) by current work: it may create groups, name them, choose short titles and a compact icon, and order entries, without losing or duplicating any pane.

How to run it:

1. Automatically, through the **Triggers** tab (startup restructures every project with un-restructured activity; finishing an agent restructures its own project).
2. Manually, from the tree toolbar **Restructure** button, a project row's Restructure action, or by voice ("restructure this project").
3. A restructure can be reverted for a project through the voice/control surface (`revert_project_restructure`).

Rules Ilium enforces on every reply:

- Every existing pane must appear exactly once; the model cannot invent, duplicate or omit entries.
- **Split views are protected.** A split's membership, order and orientation never change; the model may move a whole split into an ordinary group and retitle panes inside it.
- **Name-fixed groups** (you renamed them) keep title, short title and icon exactly.
- Manual organization is treated as useful context to preserve; earlier "LLM restructure" results are context too.
- Titles that cannot be justified by verified task evidence are not applied (see [Titles and instructions](titles-and-instructions.md)); structural grouping can still apply.
- Failures are contained by a retry breaker that backs off from one to thirty minutes and shows the failure in the restructure status line; a failing provider never blocks the rest of Ilium.
- Each restructure reply also carries ambient background recommendations for the project and its entries; they are validated and used only if animations are enabled to follow them.

Custom guidance: the **Organization**, **Shared naming and organization context** and **Entry naming** instructions are added to restructure requests, including corrective retries. See [Titles and instructions](titles-and-instructions.md).

## What is sent where

Only the selected provider receives inference requests, and only when an AI feature runs. Content sent depends on the feature:

| Feature | Contents of the request |
| --- | --- |
| Agent title | Chronological user-request history from the pane's verified agent session, supporting assistant/tool transcript, the live screen, the current title, the entry's ancestor path and nearby titles, your Entry naming and shared instructions |
| Terminal title | The terminal's scrollback (earliest and most recent stretches, with gaps marked), current title, hierarchy |
| Project name | The project path, the project root directory listing, the contents of `CLAUDE.md` and `README.md` (clipped), your Project naming and shared instructions |
| Restructure | Titles, filenames, hierarchy and relevant content of every entry, protected split views, fixed groups, the animation catalogue, and your instructions; limited by the token budget |
| Smart Copy | A frozen snapshot of the terminal screen text and a list of already detected targets, plus your Smart Copy preference |
| Test provider | A fixed synthetic prompt only |

Transcript and screen content is encoded as untrusted data in the prompt; instructions quoted inside it are not obeyed.

Where it goes:

| Provider | Destination |
| --- | --- |
| Kilo Gateway | Kilo's hosted service |
| Ollama | The URL you configured (default local machine) |
| OpenAI-compatible | The URL you configured (default OpenAI) |
| Anthropic | The URL you configured (default Anthropic) |
| OpenRouter | OpenRouter and the model provider it routes to |

Not sent to the inference provider: your API keys for other providers, the voice key, the contents of files you have not opened in a context listed above, or anything from panes while the related trigger is disabled.

Voice control is separate: when voice is on, microphone audio and text go to OpenAI's Realtime service. See [Voice](voice.md).

An advanced, hand-edited option under `[inference.kilo_gateway]` can route Kilo requests through proxies loaded from a MongoDB source; it is intentionally absent from Settings and the proxy records are never written to `config.toml`.

## Logging

File logging is **off by default** (Settings -> Debug -> File logging, or `[debug] file_logging_enabled`). When enabled, instrumented actions, errors and the complete text of LLM exchanges are written to a timestamped log, and the setting applies immediately to both the client and the detached server.

- Credential headers and URL parameters are redacted; binary audio is summarised. Other text is not scrubbed, so logs can contain project prompts, titles, transcript excerpts and request bodies.
- One log file is created per server lifetime, named `log-<local date and time>.txt`, in a private per-session directory. On Linux and macOS the root is `/tmp/.ilium-<uid>/logs/<session-id>/`; on Windows it is `%LOCALAPPDATA%\ilium\logs`. The `ILIUM_DEBUG_LOG_DIR` environment variable overrides the root.
- The five most recent logs per session are kept; older ones are pruned when the server starts.
- Treat logs as sensitive. Turn logging off when you are done investigating and delete files you do not need.
- The separate **Agent debug menu** (User Interface tab) keeps investigation-only agent history snapshots with extra CPU, memory and disk cost; it is not part of normal logging.

## Privacy guidance

- If a project contains secrets, regulated data or client confidentiality, choose **Ollama** (local) or disable the retitle and restructure triggers on the **Triggers** tab before opening such content.
- Kilo Gateway's free model may permit prompt training; use a paid or local provider for sensitive work.
- Prefer a restricted API key with a spending limit for hosted providers.
- Use **Test provider** (synthetic data only) to verify setup without exposing real work.
- Keep **File logging** off unless you are diagnosing a problem; delete logs afterwards.
- The local `/create_agent` HTTP API (see [Automation](automation.md)) is unrelated to inference but is also unauthenticated; keep it loopback-only.
- `config.toml` stores API keys in plain text. Protect the file with normal filesystem permissions and do not commit it. For voice, the `OPENAI_API_KEY` environment variable can be used instead of persisting a key.

## Configuration reference

```toml
[inference]
selected_provider = "kilo_gateway"   # kilo_gateway | ollama | open_ai | anthropic | open_router
title_style = "labeling"             # labeling | summarization
restructure_prompt_token_limit = 200000

[inference.kilo_gateway]
model = "stepfun/step-3.7-flash:free"

[inference.ollama]
base_url = "http://127.0.0.1:11434"
model = ""

[inference.openai]
base_url = "https://api.openai.com/v1"
api_key = ""
model = ""

[inference.anthropic]
base_url = "https://api.anthropic.com"
api_key = ""
model = ""

[inference.openrouter]
api_key = ""
model = "openrouter/free"

[inference.instructions]
entry_naming = ""
organization = ""
naming_and_organization = ""
project_naming = ""
smart_copy = ""
ask_for_update = ""
```

Provider names use snake_case as defined by the settings schema (`kilo_gateway`, `ollama`, `open_ai`, `anthropic`, `open_router`). The Voice instruction is stored as `[voice].custom_prompt`.

## Troubleshooting

- **HTTP 400 `context_length_exceeded`.** Lower the restructure token budget or choose a model with a larger context window.
- **Test provider fails with a JSON error.** The model did not return valid JSON; pick a stronger instruction-following model. Ilium retries once automatically.
- **Model list is empty or unusable.** For OpenAI-compatible endpoints confirm the URL ends in the API root (for example `/v1`) and the key has model-list access. Some IDs in the catalogue are not text-chat models.
- **Nothing is being titled.** Check that guided setup was finished (automatic requests stay paused until then), that a provider is configured, and that the relevant event on the Triggers tab has actions. Agent panes are only titled once a real task is verified; see [Titles and instructions](titles-and-instructions.md).
- **Restructure keeps failing.** The retry breaker backs off from one to thirty minutes. Fix the provider, then trigger a manual restructure.
- **Edits to a field do not save.** Disk write errors keep the editor open with a message; fix permissions and press `Enter`.

Related: [Settings](settings.md), [Titles and instructions](titles-and-instructions.md), [Smart Copy](smart-copy.md), [Voice](voice.md), [Getting started](getting-started.md).
