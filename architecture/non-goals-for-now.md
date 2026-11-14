## Non-goals (for now)

- The user-written JavaScript animation-package system (`animation_plugins`, `plugin_backend`, `ilium-animation-js`) has been removed with no replacement; scenes are native or WebAssembly plugins. WebAssembly extension-system implementation is authorized project scope; use the existing plugin contracts and test plans.
- No built-in SSH/remote-session sharing (RMUX already owns that niche).
- No general-purpose external agent-driving SDK or remotely callable automation surface. The explicitly user-operated local voice controller can send terminal input through the same guarded client request path as keyboard input, and `ilium voice say` lets a local process add sentences to that voice conversation, but the model, not the CLI, decides what they mean and the policy layer still applies; neither exposes ilium as an orchestration server.

