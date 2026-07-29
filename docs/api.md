# HTTP API

Each running ilium server exposes a loopback-only HTTP API at
`http://127.0.0.1:8872`. It is intentionally bound to `127.0.0.1`, never a
network interface: this endpoint can start local coding agents and submit
their prompts.

## Configure the port

Open **Settings → API**, select **HTTP API port**, and enter a port from 1 to
65535. The change is saved immediately and takes effect when the detached
ilium server next starts; changing it does not restart a live server.

The same setting can be placed directly in
`~/.config/ilium/config.toml`:

```toml
[api]
port = 8872
```

## Create an agent

Send `POST /create_agent` with a JSON object containing:

- `agent_type`: `"claude"` or `"codex"`.
- `project`: an absolute directory path, or an unambiguous project directory
  name below `~/dev` (for example `ilium`, `money`, `api`, or `julia`).
- `prompt`: the task to submit.

The request does not succeed merely when a PTY is created. ilium waits until
the selected CLI has displayed its input composer, pastes the prompt safely,
presses Enter, then returns success.

```bash
curl --fail-with-body \
  --request POST http://127.0.0.1:8872/create_agent \
  --header 'Content-Type: application/json' \
  --data '{
    "agent_type": "codex",
    "project": "ilium",
    "prompt": "Add regression tests for the new HTTP API."
  }'
```

Use an absolute path when a project name could be ambiguous:

```bash
curl --fail-with-body \
  --request POST http://127.0.0.1:8872/create_agent \
  --header 'Content-Type: application/json' \
  --data '{
    "agent_type": "claude",
    "project": "/home/developer/dev/ai/money",
    "prompt": "Investigate the current failing test and report the cause."
  }'
```

A successful response identifies the created pane and confirms delivery:

```json
{
  "pane_id": 42,
  "project_path": "/home/developer/dev/ai/ilium",
  "prompt_delivered": true
}
```

An empty prompt, invalid project path, or an ambiguous bare project name
returns a JSON error with HTTP 400. Malformed JSON or an unsupported agent
type returns HTTP 422. A missing CLI, a PTY failure,
or a prompt that cannot reach a ready composer returns HTTP 500. Composer
readiness is bounded at two minutes so a request cannot wait forever.
