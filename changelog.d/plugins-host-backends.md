### Added
- Plugins can ask for text completions and the model list through the inference provider chosen in the Inference settings tab; a plugin that ends cancels its pending completions.
- Plugins holding `audio.playback` can play WAV sounds through the system player; any audio grant lists the real input and output devices.
- Plugins can read the Claude and Codex session and configuration folders through read-only file scopes once they hold the matching grant.
- Plugin `vector` indexes are saved on disk per plugin and side, so they survive a restart, and `vector.embed` sends the texts to the inference provider chosen in Settings (it reports `unsupported` while that provider offers no embeddings).
- Plugin audio capture: on the client, `audio.open-capture` and `audio.open-loopback` record the microphone or the desktop output through PulseAudio or PipeWire and deliver 20 ms `audio-frames` events.
- The plugin journal is kept beside the session snapshot, so plugin journal entries survive a server restart.
- Plugin inference on the client uses the paid proxies from the Kilo Gateway settings.

### Fixed
- Plugin HTTP fetches now decode chunked response bodies instead of returning the chunk framing.
- `workspace.register` and `workspace.list` now check worktrees against the live session: the project's real repositories, the panes that exist, and the panes running an agent. `workspace.release` refuses a worktree that is still in use.
- A plugin without the `journal.read` grant now reads only its own journal entries, the host sets each entry's source to the writing plugin, and plugins can no longer write host entry kinds.
- `audio.open-capture` and `audio.open-loopback` now require the `audio.capture` and `audio.loopback` grants.
