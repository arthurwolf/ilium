### Added
- Plugin background scenes can now use host services granted in their manifest's scene capabilities: their own data, cache and storage, the Minecraft saves folder (read only), web requests, background jobs, sound devices, video decoding through ffmpeg, the observer location and geocoding. Each service needs its grant, and a scene never reaches the project folder, the clipboard or another plugin's files.

### Changed
- A plugin scene receives the observer location only when it holds the `location.observer` grant.
