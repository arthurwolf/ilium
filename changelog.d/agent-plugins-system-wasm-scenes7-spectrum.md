### Changed
- Audio spectrum is now a scene of the Scenes plugin. The host records the sound (what the system is playing, the default input or a named device) through the plugin's `audio.loopback` and `audio.capture` grants and hands the chunks to the scene, which analyses them each frame without a worker thread or helper process of its own. A saved `spectrum` selection loads as the plugin scene kind.

### Added
- A scene plugin can now export `handler` (`#[ilium_plugin(side = "scene", exports = ["handler"])]`) to receive the events of its imports between frames, such as the audio chunks of an open recording.
