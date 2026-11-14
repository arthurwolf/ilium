### Changed
- Earth at night is now a scene of the Scenes plugin. The scene no longer runs a worker thread: imagery loads one bounded step per frame through the plugin's network access (`gibs.earthdata.nasa.gov`) and the downloaded tiles are cached in the plugin's own storage. A saved `night_lights` selection loads as the plugin scene kind.
