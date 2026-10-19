# 12. Your first CLI subcommand

## 12. Your first CLI subcommand

This tutorial adds a new word to the `ilium` command line. After it, a user can type `ilium hello --name Ada` and get a line of JSON back. The plugin is tiny on purpose. It shows the whole path of a command line call: the manifest, the event, the output and the exit code.

This tutorial is standalone. It starts a fresh plugin, so you do not need the earlier tutorials. You only need to have finished [Your first plugin in five minutes](/pages/getting-started-first-plugin/).

A few words first, because the page uses them often.

- A **subcommand** is a word after `ilium`, such as `hello` in `ilium hello`.
- A **manifest** is the file `manifest.json`. It tells Ilium what the plugin is and what it adds.
- A **grant** is a permission. The user allows it in the install dialog. Without it, the host refuses the matching calls.
- An **event** is a message from Ilium to the plugin. A **handler** is the plugin function that reads events.
- An **exit code** is a number that a command leaves behind when it ends. `0` means success. Any other number means a problem.

Tutorial plugin: `example.hello-cli`. Languages: Rust (target `wasm32-wasip2`) and TypeScript. Every listing is given in both.

### You will need

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/) finished. Nothing else.
- One grant, on the server side: `cli.contribute`. It takes no argument. If the user declines it, the subcommand is hidden and cannot run.
- Rust only: add the crate `serde_json` to `Cargo.toml`. It builds the JSON text.
- Create the project with the same command as in the first plugin page, with a new id:

```bash
ilium plugin new hello-cli --id example.hello-cli --lang rust --side server --name "Hello CLI"
```

Use `--lang typescript` for the TypeScript version.

### What this plugin does

The plugin owns one subcommand, `hello`. When the user runs it, Ilium sends the plugin a `cli-invoked` event. The plugin writes one JSON line and then ends the call with an exit code.

- `ilium hello --name Ada` prints `{"type":"result","greeting":"Hello, Ada!"}` and exits with `0`.
- `ilium hello` with no name prints an error record and exits with `2`.

### The surface this step uses

| Name | Kind | Fields and types | Meaning |
|---|---|---|---|
| `cli-invoked` | event | `invocation: u64`, `name: string`, `argv: list<string>`, `cwd: string` | A user ran `ilium <name>`. `argv` holds the words after the name, as the shell passed them. |
| `cli-cancelled` | event | `invocation: u64` | The caller left before the call ended. Later writes fail with `not-found`. |
| `write-line` | host function | `invocation`, `jsonl: string` | Writes one JSON object and a newline to standard output. Only for `output: "jsonl"`. |
| `write-stderr` | host function | `invocation`, `text: string` | Writes plain text to the error stream. It never mixes with the JSON on standard output. |
| `finish` | host function | `invocation`, `exit_code: u8` | Ends the call. The number becomes the exit code of `ilium`. |
| `contributes.cli` | manifest key | `name`, `summary`, `arguments_schema`, `output`, `stdin`, `requires_server`, `argument_groups` | Declares the subcommand. |
| `cli.contribute` | grant | none | Lets the plugin own one subcommand. Server side only. |

Standard output is where a command prints its normal result. The error stream, called standard error, is a second channel for messages about problems. A script that reads the first channel never sees the second.

### Manifest

The `cli` key holds one object. This is the whole manifest for the smallest version.

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.hello-cli",
  "name": "Hello CLI",
  "version": "0.1.0",
  "publisher": "Example Developer",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "capabilities": {
    "server": ["cli.contribute"]
  },
  "contributes": {
    "cli": {
      "name": "hello",
      "summary": "Print a greeting as JSON lines",
      "arguments_schema": {
        "type": "object",
        "properties": {
          "name": { "type": "string", "description": "Who to greet" }
        }
      },
      "output": "jsonl",
      "stdin": "none",
      "requires_server": false
    }
  }
}
```

The keys that matter here:

| Key | Value here | Meaning |
|---|---|---|
| `name` | `hello` | The word after `ilium`. It must match `[a-z][a-z0-9-]{0,31}`. |
| `summary` | one line | Shown in `ilium --help`. |
| `arguments_schema` | an object | Documents the arguments for help. The host does not check the arguments against it. |
| `output` | `jsonl` | Every output line is one JSON object with a string `type` field. The other value is `text`. |
| `stdin` | `none` | The command reads no standard input. |
| `requires_server` | `false` | See the next section. |

### Does the command need a server?

Ilium runs a background program called the server. It holds your panes and your plugins. The `ilium` command you type is a separate short program that talks to the server.

With `requires_server: true`, which is the default, `ilium hello` needs that server. If none runs, Ilium starts one first.

With `requires_server: false`, the command also works when no server runs. In that case the `ilium` program starts your plugin inside itself, in a small temporary host. That host is smaller than a real server:

- There are no panes, so `tree.*` calls return empty results.
- Storage and settings are read from disk.
- No other plugin runs, and the plugin has no client side.
- The temporary host ends when you call `finish`.

Choose `false` for a command that only needs its own arguments, such as this one. Choose `true` when the command must see live panes or talk to other plugins.

### The smallest plugin

First the smallest version that works. It reads `--name`, and uses the word `world` when there is none. It writes one result line and finishes.

**Rust**

```rust
// src/lib.rs. The smallest hello command.
use ilium_plugin_sdk::host::cli;
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::CliInvokedEvent;

#[ilium_plugin(side = "server")]
#[derive(Default)]
struct HelloCli;

impl Lifecycle for HelloCli {}

impl Handler for HelloCli {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::CliInvoked(CliInvokedEvent { invocation, name, argv, .. }) = event else {
                continue;
            };
            if name == "hello" {
                run_hello(invocation, &argv);
            }
        }
        Vec::new()
    }
}

/// The word after `--name`, if there is one.
fn find_name(argv: &[String]) -> Option<&str> {
    let position = argv.iter().position(|word| word == "--name")?;
    argv.get(position + 1).map(String::as_str)
}

fn run_hello(invocation: u64, argv: &[String]) {
    let name = find_name(argv).unwrap_or("world");
    let record = serde_json::json!({
        "type": "result",
        "greeting": format!("Hello, {name}!"),
    });
    let _ = cli::write_line(invocation, &record.to_string());
    cli::finish(invocation, 0);
}
```

**TypeScript**

```ts
// src/server.ts. The smallest hello command.
import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import * as cli from '@ilium/plugin-sdk/host/cli';

/** The word after `--name`, if there is one. */
function findName(argv: string[]): string | undefined {
  const position = argv.indexOf('--name');
  return position === -1 ? undefined : argv[position + 1];
}

function runHello(invocation: bigint, argv: string[]): void {
  const name = findName(argv) ?? 'world';
  cli.writeLine(invocation, JSON.stringify({ type: 'result', greeting: `Hello, ${name}!` }));
  cli.finish(invocation, 0);
}

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag === 'cli-invoked' && event.val.name === 'hello') {
        runHello(event.val.invocation, event.val.argv);
      }
    }
    return [];
  },
});
```

### Line by line

Read the code from the top.

1. The handler gets a batch of events. A **batch** is a list of events that arrive together. The plugin loops over it and ignores every event that is not `cli-invoked`.
2. The check `name == "hello"` matters. A plugin may receive `cli-invoked` for its own subcommand only, but the check keeps the code correct if you add a second subcommand later.
3. `invocation` is a number that names this one call. Every write and the final `finish` must carry it. Two users can run `ilium hello` at the same moment, and the number keeps their outputs apart. In TypeScript every `u64` is a `bigint`, which is why `invocation` has that type.
4. `argv` is the list of words the user typed after `hello`. For `ilium hello --name Ada` it is `["--name", "Ada"]`. The host does not parse it for you. The plugin finds `--name` itself.
5. `write_line` sends one JSON object. The text must be a single line and must have a string field called `type`. Agents and scripts read these lines, and `type` tells them what each line is. This tutorial uses `result`.
6. `finish` ends the call. Always call it, on every path. A call that never finishes leaves the caller waiting with no time limit.

The handler returns an empty list of submissions because the command does its work through host functions, not through intents.

### One improvement: a required name

Greeting `world` hides a mistake. If the user forgets the name, the better answer is an error and a non-zero exit code, so a script can notice. This version reports the problem on both channels. It writes an `error` record on standard output for programs, and a short sentence on standard error for people. Then it ends with exit code `2`.

By convention, `2` means the user called the command wrongly, and `1` means the command tried and failed. Scripts can tell the two apart.

The version also accepts `--name=Ada` and refuses an empty name. The pure function `parse_name` makes this easy to test.

**Rust**

```rust
/// The name from `--name <text>` or `--name=<text>`.
/// Returns a short message when the argument is missing or empty.
pub fn parse_name(argv: &[String]) -> Result<String, &'static str> {
    let mut words = argv.iter();
    while let Some(word) = words.next() {
        let value = if word == "--name" {
            words.next().map(String::as_str)
        } else {
            word.strip_prefix("--name=")
        };
        if let Some(value) = value {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err("the name is empty");
            }
            return Ok(trimmed.to_string());
        }
    }
    Err("the argument --name is missing")
}

fn run_hello(invocation: u64, argv: &[String]) {
    match parse_name(argv) {
        Ok(name) => {
            let record = serde_json::json!({
                "type": "result",
                "greeting": format!("Hello, {name}!"),
            });
            let _ = cli::write_line(invocation, &record.to_string());
            cli::finish(invocation, 0);
        }
        Err(reason) => {
            let record = serde_json::json!({
                "type": "error",
                "code": "usage",
                "message": reason,
            });
            let _ = cli::write_line(invocation, &record.to_string());
            let _ = cli::write_stderr(invocation, &format!("hello: {reason}. Try: ilium hello --name Ada"));
            cli::finish(invocation, 2);
        }
    }
}
```

**TypeScript**

```ts
/** The name from `--name <text>` or `--name=<text>`, or a message when it is missing. */
export function parseName(argv: string[]): { name: string } | { reason: string } {
  for (let index = 0; index < argv.length; index++) {
    const word = argv[index];
    const value = word === '--name'
      ? argv[index + 1]
      : word.startsWith('--name=') ? word.slice('--name='.length) : undefined;
    if (value === undefined) continue;
    const trimmed = value.trim();
    return trimmed === '' ? { reason: 'the name is empty' } : { name: trimmed };
  }
  return { reason: 'the argument --name is missing' };
}

function runHello(invocation: bigint, argv: string[]): void {
  const parsed = parseName(argv);
  if ('name' in parsed) {
    cli.writeLine(invocation, JSON.stringify({ type: 'result', greeting: `Hello, ${parsed.name}!` }));
    cli.finish(invocation, 0);
    return;
  }
  cli.writeLine(invocation, JSON.stringify({ type: 'error', code: 'usage', message: parsed.reason }));
  cli.writeStderr(invocation, `hello: ${parsed.reason}. Try: ilium hello --name Ada`);
  cli.finish(invocation, 2);
}
```

Also tell the schema about the rule. Change `arguments_schema` to add `"required": ["name"]`. The host does not enforce it, but `ilium --help` and agents read it, so it documents the rule.

```json
"arguments_schema": {
  "type": "object",
  "properties": { "name": { "type": "string", "description": "Who to greet" } },
  "required": ["name"]
}
```

### Points to notice

- **The host does not validate `argv`.** `arguments_schema` is documentation. Your code decides what a valid call is.
- **Every JSON line needs a string `type`.** `result` and `error` are the two types used here. A line without `type` is refused with `invalid-argument`.
- **Two channels, two audiences.** Standard output carries records for programs. Standard error carries sentences for people.
- **One `finish` per call.** Put it at the end of every branch, including the error branch.
- **The pure function is the test target.** `parse_name` needs no host, so a plain unit test can check it.

### Edge cases and errors

| Case | What happens | Code or result |
|---|---|---|
| `--name` is the last word with no value | `parse_name` returns the missing message | Exit code `2` |
| `--name ""` or only spaces | The name is empty | Exit code `2` |
| Name holds quotes or a newline | `serde_json` and `JSON.stringify` escape them, so the line stays valid JSON | Exit code `0` |
| Caller presses Ctrl+C early | `cli-cancelled` arrives, later writes fail | `not-found` |
| Plugin writes after `finish` | The write is refused | `not-found` |
| User declined `cli.contribute` | The subcommand is hidden from help | `ilium hello` prints `unknown command` |
| Plugin stops while a call is open | The call ends with exit code `1` and `plugin stopped` on standard error | Exit code `1` |

### Common mistakes

| Mistake | What you see | Fix |
|---|---|---|
| Declared `"output": "text"` and called `write-line` | `invalid-argument`, and the line is not sent | Use `write-text` for a text command, or set `output` to `jsonl`. |
| Wrote a JSON line without `type` | `invalid-argument` | Add a string `type` field. |
| Wrote a line longer than 64 KiB | `invalid-argument` | Split the output into several records. |
| Named the grant `cli.contribute:hello` | The install is refused with `manifest.grant-argument` | The grant takes no argument. |
| Chose a name that `ilium` or another plugin already uses | The install is refused with `manifest.cli-name-reserved` (kernel command) or `manifest.cli-name-taken` (another plugin) | Pick another subcommand name. |
| Wrote `"stdin": "sometimes"` | The install is refused with `manifest.cli-field` | Use `none`, `optional` or `required`. |
| Forgot `finish` on one branch | The caller waits with no time limit | End every branch with `finish`. |

### Test it

Test the pure function first. It needs no Ilium at all.

**Rust**

```rust
#[cfg(test)]
mod tests {
    use super::parse_name;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn reads_both_spellings() {
        assert_eq!(parse_name(&words(&["--name", "Ada"])), Ok("Ada".to_string()));
        assert_eq!(parse_name(&words(&["--name=Ada"])), Ok("Ada".to_string()));
    }

    #[test]
    fn rejects_missing_and_empty() {
        assert!(parse_name(&words(&[])).is_err());
        assert!(parse_name(&words(&["--name", "  "])).is_err());
    }
}
```

**TypeScript**

```ts
// src/server.test.ts. Run with: bun test
import { expect, test } from 'bun:test';
import { parseName } from './server';

test('reads both spellings', () => {
  expect(parseName(['--name', 'Ada'])).toEqual({ name: 'Ada' });
  expect(parseName(['--name=Ada'])).toEqual({ name: 'Ada' });
});

test('rejects missing and empty', () => {
  expect('reason' in parseName([])).toBe(true);
  expect('reason' in parseName(['--name', '  '])).toBe(true);
});
```

Run the Rust tests with `cargo test parse_name`. Then run the plugin live, call it and read the exit code:

```bash
ilium plugin dev
ilium hello --name Ada
ilium hello; echo $?
ilium plugin logs example.hello-cli
```

### What you see

The first render shows a call that works. The second shows the missing argument. The third shows the help list.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium hello --name Ada
{"type":"result","greeting":"Hello, Ada!"}
<span class="t-prompt">$</span> echo $?
0
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative. A call with a name prints one result record and exits with 0.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium hello
{"type":"error","code":"usage","message":"the argument --name is missing"}
<span class="t-err">hello: the argument --name is missing. Try: ilium hello --name Ada</span>
<span class="t-prompt">$</span> echo $?
2
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative. The record goes to standard output and the sentence goes to standard error. A terminal shows both.</figcaption>
</figure>

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium hello 2&gt;/dev/null
{"type":"error","code":"usage","message":"the argument --name is missing"}
<span class="t-prompt">$</span> ilium --help
Commands:
  hello    Print a greeting as JSON lines
</pre>
<figcaption>Rendered at 80 columns or fewer. Illustrative. With standard error sent to /dev/null, only the record remains. The help list shows the summary from the manifest.</figcaption>
</figure>

### Try it yourself

1. Add a second argument `--shout` that turns the greeting into capitals. Check that `ilium hello --name Ada --shout` prints `HELLO, ADA!`.
2. Change `output` to `text`, replace `write-line` with `write-text`, and print the greeting as plain text. Notice that `write-line` now fails with `invalid-argument`.
3. Set `requires_server` to `true`, stop the Ilium server, and run the command. Ilium starts a server first. Compare with `false`, where no server starts.

### See also

- [Getting started: your first plugin](/pages/getting-started-first-plugin/) builds the plugin the first time.
- [cli API](/pages/api-cli/) lists every function, the pane selector and standard input.
- [Capability: cli.contribute](/pages/capability-cli-contribute/) covers the grant and its declined behaviour.
- [Manifest reference](/pages/reference-manifest/) lists the `cli` keys and the manifest error codes.
- [Testing](/pages/testing/) shows fakes and fixtures for plugins.
- [Debugging](/pages/debugging/) explains the plugin log and `ilium plugin logs`.

API checked against `ilium:plugin@0.1.0`.

**Next:** [Tutorial 13, keybindings, chords and when expressions](/pages/tutorial-13-keys-and-when/).
