# Voice control

Voice control lets you drive Ilium and prompt your agents by speaking. A live conversation with OpenAI's Realtime model listens to you, decides which Ilium action you meant (focus a pane, create an agent, type text into a terminal, search, change a setting and more), carries it out, and answers in a synthetic voice. You can also type sentences into the same conversation from the command line with `ilium voice say`. This page covers setup, the F8 key, every voice setting, what the assistant can do, how confirmations protect you from mistakes, typed input and its JSONL output, and troubleshooting audio.

Demo: [voice commands trigger Ilium actions and send a prompt to Claude](../../assets/demos/02-voice-control.gif).

## Contents

- [Requirements](#requirements)
- [Step by step: turning voice on](#step-by-step-turning-voice-on)
- [Using F8 and the voice control button](#using-f8-and-the-voice-control-button)
- [Voice settings](#voice-settings)
- [What you can ask for](#what-you-can-ask-for)
- [Confirmations and safety](#confirmations-and-safety)
- [Typed voice input: `ilium voice say`](#typed-voice-input-ilium-voice-say)
- [JSONL output](#jsonl-output)
- [Audio devices](#audio-devices)
- [Privacy and cost](#privacy-and-cost)
- [Configuration reference](#configuration-reference)
- [Troubleshooting](#troubleshooting)

## Requirements

- An **OpenAI API key** with access to the Realtime API. This is separate from the inference provider used for titles; see [Inference and privacy](inference-and-privacy.md).
- Network access to OpenAI.
- An **attached Ilium client**. The voice session runs inside the interactive client, which owns the microphone, the speaker, the provider connection and the tool executor. A detached server with no client cannot run voice.
- A working microphone and speaker for spoken input and output. Typed input through `ilium voice say` needs no microphone.

## Step by step: turning voice on

1. Open Settings (`Ctrl+B :`) and select **Voice control**.
2. Select **OpenAI API key**, press `Enter`, paste your key and press `Enter`. The key is masked afterwards. Alternatively set the `OPENAI_API_KEY` environment variable before starting Ilium; the tab then shows "Configured via OPENAI_API_KEY" and nothing is stored on disk.
3. Choose the **Model**, **Voice**, **Input device** and **Output device** (all optional; defaults work).
4. Turn **Voice control** on (or press `F8`). Ilium opens the audio devices and connects to OpenAI. The status moves from connecting to listening.
5. Speak a request, for example "what agents are running?".
6. Turn voice off with `F8` or the same toggle, or say "stop voice mode" (the assistant has a tool that ends and disables the session).

The first-run guided setup includes an optional voice step with a demonstration; reopen it with `ilium --onboarding`. See [Settings](settings.md#guided-setup-and-onboarding).

## Using F8 and the voice control button

`F8` is a global key: it works while any panel or dialog owns text input. Its behaviour depends on **Input mode**:

| Input mode | Press F8 | Release F8 |
| --- | --- | --- |
| Semantic VAD (default) | Toggles voice on or off | nothing |
| Push to talk | Turns voice on if it is off and starts recording | Stops recording and commits what you said |

A bottom-right voice control stays reachable above every panel and modal and shows the voice state; click it to toggle voice. Push to talk needs a terminal that reports key release events; semantic VAD does not.

States you may see: connecting, listening, recording, thinking and speaking.

## Voice settings

Open **Settings -> Voice control**. Every option is saved immediately to `[voice]` in `config.toml`.

| Option | Values | Default | Notes |
| --- | --- | --- | --- |
| Voice control | On / Off | Off | Starts or stops Ilium's owned microphone, Realtime session and speaker actor |
| OpenAI API key | Text, masked | empty | Or use `OPENAI_API_KEY` |
| Model | GPT Realtime 2.1, GPT Realtime 2.1 Mini | GPT Realtime 2.1 | API names `gpt-realtime-2.1` and `gpt-realtime-2.1-mini`; Mini is faster |
| Voice | Marin, Cedar, Alloy, Ash, Ballad, Coral, Echo, Sage, Shimmer, Verse | Marin | Changing it while audio is flowing may require a reconnect |
| Reasoning effort | Minimal, Low, Medium | Low | More effort helps multi-step tool selection; quality and latency are not guaranteed |
| Input mode | Semantic VAD, Push to talk | Semantic VAD | Semantic VAD is hands-free; push to talk records only while F8 is held |
| VAD eagerness | Auto, Low, Medium, High | Auto | How quickly a spoken turn is judged complete; higher means faster but may cut you off |
| Input device | System default or a discovered microphone | System default | |
| Output device | System default or a discovered speaker | System default | |
| Output volume | percentage | 80 | Local playback gain only; microphone input is unchanged |
| Confirm terminal submissions | On / Off | Off | See [Confirmations and safety](#confirmations-and-safety) |
| Pause media while active | On / Off | On | Pauses external media when voice starts and resumes it when voice stops |
| Custom prompt | Multi-line text | empty | Added to the voice system prompt (the Voice assistant instruction input) |
| Reconnect now | Button | | Restarts audio and the WebSocket using the saved settings |

Changing the model, voice, reasoning effort, input mode, VAD eagerness, devices, volume, key or custom prompt reconnects the voice session. Toggling **Confirm terminal submissions** does not interrupt a healthy connection.

Pause media: the option sends the same signal as a keyboard media key (on Linux through D-Bus). It is not a universal cross-platform guarantee; if it does nothing on your system, turn it off.

Custom prompt: use it for language, vocabulary and answer style. It refines the assistant and does not replace the built-in tool rules. The same text can be edited in **LLM Instructions**; see [Titles and instructions](titles-and-instructions.md).

## What you can ask for

The assistant works through a fixed set of tools; it can only do what these cover. In each case it resolves targets by name, path, id or the current focus.

| Area | Examples |
| --- | --- |
| State | "What agents are running?", "What is on the screen?" (compact or full state) |
| Interface | Focus the tree, focus a pane, next/previous pane, a pane in a direction, show a split, open Settings or a specific Settings tab, open search, open help, close an overlay |
| Tree | Create terminals, agents (Claude, Codex or Antigravity, optionally in a worktree with branch and base), command panes, editors, folders and projects, groups, boards, splits; rename, move up/down, reparent, close, expand/collapse, retitle, restructure one project or all projects, revert a project restructure |
| Terminal text | Send text to a terminal and press Enter (the default for "tell the agent..."), type text without Enter, press keys (Enter, Escape, Tab, arrows, Home/End, Page keys, Backspace, Delete, Space, Ctrl+C/D/L/Z), scroll up/down/to bottom |
| Scheduled and queued input | Schedule input for later; queue a prompt once, N times or forever; clear the prompt queue |
| Editors | Save, save as, insert text, replace the whole document, jump to a line, toggle rendered view, line numbers, minimap and autosave |
| Boards | Select and open cards and columns, add, update, rename, move and delete cards and columns, toggle checkboxes |
| Settings | Read, set and adjust settings, test the inference provider, refresh model lists, preview sound |
| Search | Query, step through results, open a result, close |
| Session | Detach, restart the client, restart the server, kill the session |

Example utterances:

- "Focus the first agent. Say hello to it."
- "Create a new Claude agent in a worktree called fix-login."
- "Open the Inference settings."
- "Queue the prompt 'run the tests' three times."
- "Stop voice mode."

API keys are never exposed to the settings tool.

## Confirmations and safety

Check the target before dictating text: speech recognition can mishear, and "tell the agent" goes to the currently active pane unless you name another.

By default an explicit request such as "send this to the agent" forwards the text and presses Enter immediately. Enable **Confirm terminal submissions** to add a safety step:

1. Ilium first types the text visibly into the pane without Enter.
2. The assistant asks whether to submit what is on screen, without reading the text aloud.
3. It presses Enter in that same pane only after you answer yes. Any other answer cancels the pending action.

With confirmation on, pressing Enter by voice, scheduling terminal input and queueing a prompt for automatic submission (once, several times or forever) also ask first, because queued delivery keeps submitting long after the request.

Some actions always require confirmation, whatever your settings:

- Running a shell command pane ("run this command").
- Replacing an editor's entire document.
- Closing a pane or group when closing would lose something or the target is pinned (the target is resolved once and fixed before asking, so changing focus while the question is pending cannot redirect the action).
- Deleting a board card or column (the exact item is named in the question).
- Killing the Ilium session and every pane, and restarting the detached server.

Detach and restarting the client do not need confirmation.

Destructive actions need a clear yes. A cancelled action reports "cancelled the pending action".

## Typed voice input: `ilium voice say`

`ilium voice say` types sentences into the live voice conversation as if you had spoken them. Each sentence becomes one turn, in order, interpreted by the same model and tools with the same confirmation policy. You can mix text freely with live microphone audio. It is useful for scripts, testing and quiet environments.

```sh
ilium voice say --start "what agents are running?"
printf '%s\n' "focus the first agent" "say hello to it" | ilium voice say -
```

Usage: `ilium voice say [OPTIONS] <SENTENCE>...`

| Option | Meaning | Default |
| --- | --- | --- |
| `<SENTENCE>...` | One or more sentences, in order (required). A lone `-` reads one sentence per non-empty line from standard input. Put `--` first when a sentence begins with a hyphen | |
| `--start` | If voice control is off, switch it on (and save that setting like F8), or restart a session that failed to start, instead of failing with `voice-off` | off |
| `--session-name <NAME>` | Session to address when not run from inside an Ilium pane | `default` |
| `--timeout-s <SECONDS>` | How long to wait for the voice session to accept the text (1 to 600). Starting voice opens audio devices and a connection first, so allow more time | 30 |
| `--cwd <DIR>` (global) | Project directory when not run from inside an Ilium pane | current directory |

Limits: at most 32 sentences per request, each at most 4,000 characters after trimming. Every sentence is trimmed; empty sentences and more than 32 sentences are rejected as `invalid-request`.

Targeting: run from inside an Ilium pane it addresses that pane's session; elsewhere pass `--cwd` and `--session-name`. The command never starts a server and needs an attached interactive client to host voice.

"Accepted" means the text is in the live voice session's queue. The provider sends no per-turn acknowledgement, so success does not prove the model has acted: check the target pane for the outcome.

## JSONL output

Standard output is JSONL, one object per line, each with a `type`. You get a `progress` record, then exactly one `result` or `error` record. Diagnostics for humans go to standard error. The process exits non-zero after an `error`.

Progress:

```json
{"type":"progress","command":"voice say","stage":"sending","request_id":1,"session":"default","socket":"/run/user/1000/ilium/...","session_source":"cwd","sentence_count":1,"start":true}
```

Result:

```json
{"type":"result","command":"voice say","ok":true,"request_id":1,"session":"default","sentence_count":1,"accepted_sentences":1,"voice_phase":"listening","started_voice":true,"delivery":"queued-to-voice-session"}
```

`voice_phase` is one of `connecting`, `listening`, `recording`, `thinking`, `speaking`. `started_voice` says whether `--start` switched voice on.

Error:

```json
{"type":"error","command":"voice say","ok":false,"request_id":1,"code":"voice-off","message":"...","hint":"pass --start to switch voice control on, or press F8 in the Ilium client"}
```

Error codes:

| Code | Meaning | Fix |
| --- | --- | --- |
| `invalid-request` | No sentences, an empty sentence, too many sentences, or one that is too long | Correct the input |
| `no-voice-client` | No interactive client is attached to the session | Attach with `ilium`; the voice session runs inside it |
| `voice-off` | Voice control is off | Pass `--start` or press F8 |
| `voice-unavailable` | Voice could not start | Fix voice settings (API key, audio devices) and retry |
| `client-unresponsive` | The attached client did not answer | Check the client is not frozen or suspended |
| `connection-failed` | The session's server is not reachable | Check `ilium ls` |
| `request-send-failed` | The connection closed before sending | Retry |
| `server-closed-connection` | Server closed before answering | An older server may not support the command; the hint explains |
| `timeout` | No answer within `--timeout-s` | Raise the timeout or check the client |
| `stdin-unreadable` | Reading `-` failed | Check the input stream |

An Ilium server started before `ilium voice say` existed cannot answer it; restart Ilium once so the current server is loaded. When several clients are attached, the request is offered to one client at a time, newest first.

## Audio devices

- Leave Input and Output on **System default** unless you have several devices. Open the device row and pick a discovered device by name.
- Device names are matched against what the operating system reports; if a saved device is unplugged, voice falls back as the system does or fails with `voice-unavailable` until you choose another.
- Linux uses ALSA, Windows uses WASAPI and macOS uses CoreAudio. On Linux, make sure the user can access the sound devices (for example through PipeWire or PulseAudio's ALSA bridge). On macOS, allow your terminal application to use the microphone in System Settings -> Privacy and Security.
- **Output volume** adjusts local playback gain only.
- If you use a headset, choose it for both input and output to reduce echo; the assistant interrupts its own speech when you start talking.
- Press **Reconnect now** after plugging in or changing a device.

## Privacy and cost

Voice sends microphone audio, your typed sentences and the assistant's tool results to OpenAI's Realtime service while voice is on, and bills your OpenAI account. Turn voice off when you are not using it. The assistant's tools read Ilium state (names, titles, screen text of panes you ask about); do not enable voice with sensitive content on screen unless you accept that. The API key is masked and never given to the assistant. Debug logging records only a summary of binary audio, but does record text exchanges; see [Inference and privacy](inference-and-privacy.md#logging).

## Configuration reference

```toml
[voice]
enabled = false
api_key = ""                       # or set OPENAI_API_KEY
model = "gpt-realtime-2.1"          # or "gpt-realtime-2.1-mini"
voice = "marin"                     # marin cedar alloy ash ballad coral echo sage shimmer verse
reasoning_effort = "low"            # minimal | low | medium
input_mode = "semantic_vad"         # semantic_vad | push_to_talk
vad_eagerness = "auto"              # auto | low | medium | high
# input_device_name = "..."
# output_device_name = "..."
output_volume_percent = 80
confirm_terminal_submissions = false
pause_media_while_active = true
custom_prompt = ""
```

## Troubleshooting

- **Voice will not start.** Check the API key (or `OPENAI_API_KEY`), your network and that the key can use the Realtime API; then press **Reconnect now**. `ilium voice say` reports `voice-unavailable` with the same advice.
- **`no-voice-client`.** Start or attach an interactive `ilium` client in that project and session.
- **F8 does nothing in push to talk.** Your terminal may not report key release. Use semantic VAD or a terminal with the kitty keyboard protocol or equivalent.
- **It keeps cutting me off or waits too long.** Lower or raise **VAD eagerness**.
- **It heard the wrong pane.** Name the target ("the second agent") and enable **Confirm terminal submissions**.
- **No sound.** Check **Output device** and **Output volume**; confirm the OS output is not muted.
- **No microphone input.** Check **Input device**, OS permissions and that no other app holds the device exclusively.
- **Media does not pause.** The option depends on a D-Bus media-key signal; disable it if unsupported.
- **A setting change interrupted me.** Changing runtime settings reconnects the session; only the confirmation option avoids that.
- **Text typed by `voice say` was not acted on.** "Accepted" only means queued; read the pane for the outcome and the assistant's reply.

Related: [Settings](settings.md), [Titles and instructions](titles-and-instructions.md), [CLI reference](cli-reference.md), [Automation](automation.md), [Getting started](getting-started.md).
