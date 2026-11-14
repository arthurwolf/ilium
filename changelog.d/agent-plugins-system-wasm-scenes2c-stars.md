### Changed
- The "Stars overhead" scene now runs as a scene of the core scenes plugin (`plugins/core/scenes`), with its star catalogue embedded in the plugin; the built-in copy in `ilium-ambient` is gone, so disabling the plugin removes it. Saved "stars" selections load onto the plugin scene.
- Plugin manifests may mark a scene `uses_location: true`; the Animations settings then show the shared Location row for it.
