### Changed
- The live graph, live earthquakes, live aircraft and live boats background scenes now run in the
  Scenes plugin (`ilium.core.scenes`) and are removed from the built-in animation engine. They
  need the plugin's network grants; disabling the plugin removes them. Their brightness and hue
  rows are now called ink and tint rows. The Wikipedia graph sources report that the event
  stream is unavailable to a scene.
