# 17. A help topic and a custom sound

## 17. A help topic and a custom sound

This tutorial adds two small pieces of polish. The first is a **help topic**, a page of Markdown that ships inside the plugin and shows up in the Ilium help pane. The second is a **custom sound**, a short audio file that the plugin plays on request. The improvement adds a setting that lets the user mute the sound.

This tutorial is standalone. It starts a fresh plugin, so you do not need the earlier tutorials. You only need to have finished [Your first plugin in five minutes](/pages/getting-started-first-plugin/).

Some words first.

- A **help topic** is one Markdown file, listed in the manifest, that Ilium shows in its help pane.
- A **sound** is a short audio file in the package. The manifest gives it an id, such as `ding`.
- A **catalogue** is the list of sound ids that a plugin may play. It holds the bundled sound `chirping` and the sounds of the plugin itself.
- A **package** is the single file that holds the plugin: the manifest, the components and any asset files.
- A **command** is a named action that the user runs from the palette.
- An **intent** is a request the plugin hands back, such as "play this sound".
- A **grant** is a permission that the user allows at install.

Tutorial plugin: `example.chime`. Side: client, because sound and help belong to the client. Languages: Rust (target `wasm32-wasip2`) and TypeScript. Every listing is given in both.

### You will need

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/) finished. Nothing else.
- Three grants, all on the client side:

| Grant | What the plugin may do with it |
|---|---|
| `sound.play` | Send the `play-sound` intent. |
| `help.read` | Read the text of a help topic with `help.topic`. |
| `notify.toast` | Show a toast. |

- One sound file. Any short WAV file with PCM samples, or OGG Vorbis file, works. Put it at `sounds/ding.ogg` in the project. Keep it under a second, because it plays on every command.
- Create the project:

```bash
ilium plugin new chime --id example.chime --lang rust --side client --name "Chime"
```

Use `--lang typescript` for the TypeScript version.

### What this plugin does

- The command `Play the chime` plays the sound `ding`.
- The command `Show the chime help` reads the help topic and shows its first line in a toast.
- The help topic is listed in the help pane, so the user can also open it there.

### The surface this step uses

| Name | Kind | Fields and types | Meaning |
|---|---|---|---|
| `contributes.help` | manifest key | `id`, `file`, `title` | Declares a help topic. The file sits under `help/`. |
| `contributes.sounds` | manifest key | `id`, `file` | Declares a sound. The file sits under `sounds/`. |
| `help.topic` | host function | `topic(id) -> option<string>`, grant `help.read` | Returns the Markdown of a topic, or nothing. |
| `play-sound` | intent | the sound id, a string. Grant `sound.play`. | Asks the client to play a sound from the catalogue. |
| `sound.catalog` | host function | returns list of ids, always granted | The ids that this plugin may play. |
| `command-invoked` | event | `command`, `arguments-json`, `origin`, `context` | The user ran a command. |

### The files

Besides the manifest, this plugin has two asset files:

```text
help/chime.md
sounds/ding.ogg
```

The help file is plain Markdown. Start it with a heading, because the plugin shows the first line.

```markdown
# Chime

Chime plays a short sound when you run the command Play the chime.
Open Settings > Plugins to mute it.
```

### Manifest

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.chime",
  "name": "Chime",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "client": {
    "entry": "client.wasm",
    "world": "ilium:client/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "client": ["sound.play", "help.read", "notify.toast"]
  },
  "contributes": {
    "commands": [
      { "id": "play", "title": "Play the chime", "category": "Chime" },
      { "id": "help", "title": "Show the chime help", "category": "Chime" }
    ],
    "help": [
      { "id": "chime", "file": "help/chime.md", "title": "Chime" }
    ],
    "sounds": [
      { "id": "ding", "file": "sounds/ding.ogg" }
    ]
  }
}
```

The global id of the help topic is the plugin id, a slash and the local id: `example.chime/chime`. The sound id is only `ding`. Sound ids are plain, and each plugin sees only its own.

Two ids are protected. Ilium ships one bundled sound, `chirping`. A manifest may not reuse it, and may not repeat an id. Either mistake fails the package check with `manifest.sound-id-taken`.

### The smallest plugin

The handler waits for `command-invoked`. The first command returns a `play-sound` intent. The second reads the topic and returns a toast with its first line.

**Rust**

```rust
// src/lib.rs. Play a sound and read a help topic.
use ilium_plugin_sdk::host::help;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CommandInvokedEvent;
use ilium_plugin_sdk::types::intents::{Intent, ToastSpec};
use ilium_plugin_sdk::types::text::Tone;

const PLAY: &str = "example.chime/play";
const HELP: &str = "example.chime/help";
const TOPIC: &str = "example.chime/chime";

#[ilium_plugin(side = "client")]
#[derive(Default)]
struct Chime;

impl Lifecycle for Chime {}

/// The first line of a Markdown text, without the leading marks.
pub fn first_line(markdown: &str) -> String {
    markdown
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches('#')
        .trim()
        .to_string()
}

fn toast(title: String) -> Submission {
    Submission::new(Intent::Toast(ToastSpec {
        id: Some("chime".into()),
        tone: Tone::Info,
        title,
        body: None,
        actions: vec![],
        duration_ms: None,
    }))
}

impl Handler for Chime {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::CommandInvoked(CommandInvokedEvent { command, .. }) = event else {
                continue;
            };
            if command == PLAY {
                submissions.push(Submission::new(Intent::PlaySound("ding".into())));
            } else if command == HELP {
                let title = match help::topic(TOPIC) {
                    Some(markdown) => first_line(&markdown),
                    None => "The help topic is not installed".to_string(),
                };
                submissions.push(toast(title));
            }
        }
        submissions
    }
}
```

**TypeScript**

```ts
// src/client.ts. Play a sound and read a help topic.
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { topic } from '@ilium/plugin-sdk/host/help';

const PLAY = 'example.chime/play';
const HELP = 'example.chime/help';
const TOPIC = 'example.chime/chime';
let nextCorrelation = 1n;

/** The first line of a Markdown text, without the leading marks. */
export function firstLine(markdown: string): string {
  const line = markdown.split('\n')[0] ?? '';
  return line.replace(/^#+/, '').trim();
}

function submission(intent: Submission['intent']): Submission {
  return { intent, correlation: nextCorrelation++, dedupKey: undefined };
}

function toast(title: string): Submission {
  return submission({
    tag: 'toast',
    val: { id: 'chime', tone: 'info', title, body: undefined, actions: [], durationMs: undefined },
  });
}

export const { lifecycle, handler } = definePlugin({
  side: 'client',
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'command-invoked') continue;
      if (event.val.command === PLAY) {
        submissions.push(submission({ tag: 'play-sound', val: 'ding' }));
      } else if (event.val.command === HELP) {
        const markdown = topic(TOPIC);
        submissions.push(toast(markdown === undefined
          ? 'The help topic is not installed' : firstLine(markdown)));
      }
    }
    return submissions;
  },
});
```

### Line by line

1. `Intent::PlaySound("ding".into())` names the sound by its id from `contributes.sounds`. The plugin never opens the audio device.
2. `help::topic` takes the global topic id and returns the Markdown, or nothing when no installed plugin declares the id.
3. `first_line` is a pure function. It strips the leading `#` marks, so the toast reads `Chime` and not `# Chime`.

Ilium has no call that opens the help pane from code. This tutorial shows the first line in a toast instead. The user opens the full topic in the help pane. A plugin with a settings page can also point a row at the topic with the row key `help`.

### One improvement: let the user mute the sound

A sound that cannot be turned off is rude. Add a settings toggle `sound_enabled`. The plugin reads it before it plays.

Add the setting to the manifest. It is a tab with one section and one toggle row, as in [Tutorial 10](/pages/tutorial-10-settings-new-tab/).

```json
"settings": [
  {
    "kind": "tab",
    "id": "chime",
    "title": "Chime",
    "render": "rows",
    "sections": [
      {
        "id": "sound",
        "title": "Sound",
        "scope": "client",
        "rows": [
          { "type": "toggle", "key": "sound_enabled", "title": "Play the sound",
            "default": true,
            "description": "Turn this off to keep the chime silent.",
            "help": "example.chime/chime" }
        ]
      }
    ]
  }
]
```

The row key `help` links the row to the topic. The topic id must be declared under `contributes.help`, or the install fails with `manifest.settings-help`.

Now read the setting. The function `settings::get_json` returns the plugin values as one JSON object. A missing key means the default, which is on.

**Rust**

```rust
use ilium_plugin_sdk::host::settings;

/// True when the setting is on, or when it cannot be read.
pub fn sound_enabled(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|values| values["sound_enabled"].as_bool())
        .unwrap_or(true)
}

// In on_events, replace the PLAY branch with:
//     if sound_enabled(&settings::get_json()) {
//         submissions.push(Submission::new(Intent::PlaySound("ding".into())));
//     }
```

**TypeScript**

```ts
import { getJson } from '@ilium/plugin-sdk/host/settings';

/** True when the setting is on, or when it cannot be read. */
export function soundEnabled(json: string): boolean {
  try {
    const values: Record<string, unknown> = JSON.parse(json);
    return typeof values['sound_enabled'] === 'boolean' ? values['sound_enabled'] : true;
  } catch {
    return true;
  }
}

// In onEvents, replace the PLAY branch with:
//     if (soundEnabled(getJson())) submissions.push(submission({ tag: 'play-sound', val: 'ding' }));
```

The user can also take away the grant. Settings > Plugins > Capabilities lists `sound.play`, and the command `ilium plugin grants example.chime --revoke sound.play` revokes it. After that, every `play-sound` is refused with `not-permitted`.

### What the package check does

`ilium plugin pack` builds the package and writes the `files` inventory: each file with its length and a hash. The check at install compares every listed file with its entry.

| Problem | Error code |
|---|---|
| A file is in the package but not in `files` | `manifest.file-unlisted` |
| A file in `files` is missing | `manifest.file-missing` |
| The length does not match | `manifest.file-length` |
| The hash does not match | `manifest.file-hash` |
| A sound file is not WAV PCM or OGG Vorbis | package check fails |

### Points to notice

- **Each plugin has its own sounds.** The catalogue holds `chirping` and your own ids. Another plugin's id is refused with `not-found`.
- **The grant is separate from the setting.** The user can mute in the settings or revoke the grant.

### Edge cases and errors

| Case | What happens | Code or result |
|---|---|---|
| Sound id not in the catalogue | Nothing plays | `refused(not-found)` |
| Grant `sound.play` declined | Nothing plays | `refused(not-permitted)` |
| More than 4 sounds in a second | The extra sounds are dropped | `refused(rate-limited)` with `retry-after-ms` |
| Topic id unknown | `help.topic` returns nothing | No error |
| `help.read` declined | `help.topic` returns nothing, with a warning in the log | No error |

### Common mistakes

| Mistake | What you see | Fix |
|---|---|---|
| Used the id `chirping` in `sounds` | Install refused with `manifest.sound-id-taken` | Choose another id. |
| Pointed `file` at a path outside `sounds/` | The package check fails | Keep sound files under `sounds/`. |
| Played `example.chime/ding` | `refused(not-found)` | Use the plain id `ding`. |
| Read the topic with the plain id `chime` | `help.topic` returns nothing | Use the global id `example.chime/chime`. |

### Test it

The text functions are pure. Test them with plain tests. Test the sound command with the fake host.

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ilium_plugin_sdk::testing::FakeHost;

    #[test]
    fn first_line_drops_the_heading_marks() {
        assert_eq!(first_line("# Chime\n\nbody"), "Chime");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn the_play_command_plays_ding() {
        let mut host = FakeHost::new().grant("sound.play");
        host.activate::<Chime>();
        let submissions = host.deliver(vec![Event::CommandInvoked(CommandInvokedEvent {
            command: PLAY.into(),
            arguments_json: "{}".into(),
            origin: "palette".into(),
            context: Default::default(),
        })]);
        assert!(matches!(&submissions[0].intent, Intent::PlaySound(id) if id == "ding"));
    }
}
```

**TypeScript**

```ts
// src/client.test.ts. Run with: bun test
import { expect, test } from 'bun:test';
import { createFakeHost } from '@ilium/plugin-sdk/testing';
import { firstLine, handler, lifecycle } from './client';

test('first line drops the heading marks', () => {
  expect(firstLine('# Chime\n\nbody')).toBe('Chime');
  expect(firstLine('')).toBe('');
});

test('the play command plays ding', async () => {
  const host = createFakeHost().grant('sound.play');
  await host.activate({ lifecycle, handler });
  const submissions = host.deliver([
    { tag: 'command-invoked', val: { command: 'example.chime/play',
      argumentsJson: '{}', origin: 'palette', context: {} } },
  ]);
  expect(submissions[0].intent).toEqual({ tag: 'play-sound', val: 'ding' });
});
```

Run `cargo test` or `bun test`. Then run `ilium plugin pack`, install the package, and run both commands from the palette. A sound cannot be shown in a test, so listen for it.

### What you see

The palette lists both commands under their category.

<figure>
<pre class="term">╭  ≡ ● · Command palette ──────────────────────╮
│ <span class="t-prompt">&gt;</span> chime                                      │
│                                              │
│ <span class="t-dim">Chime</span>                                        │
│ <span class="t-hi">Play the chime</span>                               │
│ Show the chime help                          │
│                                              │
│ <span class="t-dim">Enter run · Esc close</span>                        │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The two commands of the plugin. Playing the sound has no picture, so the palette closes and you hear it.</figcaption>
</figure>

The help command shows the first line of the topic.

<figure>
<pre class="term">╭  ≡ ● · Chime ────────────────────────────────╮
│ ━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────│
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The toast that shows the first line of the help topic.</figcaption>
</figure>

### Try it yourself

1. Add a second sound `ding-low` to the manifest and a second command that plays it.
2. Call `sounds::catalog()` and show the number of ids in the toast.

### See also

- [Help and plugins API](/pages/api-help-and-plugins/) has `help.topic`, the topic ids and the settings link.
- [Capability: sound.play](/pages/capability-sound-play/) has the intent, the catalogue and the outcomes.
- [Packages](/pages/packages/) explains the file inventory and the package check.
- [Manifest reference](/pages/reference-manifest/) lists `help` and `sounds` and their errors.
- [Settings API](/pages/api-settings/) has `settings::get_json`.

API checked against `ilium:plugin@0.1.0`.
