### Added
- Plugins can ask for text completions and the model list through the inference provider chosen in the Inference settings tab; a plugin that ends cancels its pending completions.
- Plugins holding `audio.playback` can play WAV sounds through the system player; any audio grant lists the real input and output devices.
- Plugins can read the Claude and Codex session and configuration folders through read-only file scopes once they hold the matching grant.

### Fixed
- Plugin HTTP fetches now decode chunked response bodies instead of returning the chunk framing.
