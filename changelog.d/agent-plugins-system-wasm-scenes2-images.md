### Changed
- Images is now a scene of the Scenes plugin (Plugin scene, Scene row) instead of a built-in animation, so it disappears when the plugin is disabled. It shows one of four bundled pictures, a picked image file, or a slideshow over up to three picked folders, with the same pan, zoom, fit and cross-fade controls. Its colours follow the shared look. A saved `images` selection loads as the plugin scene kind.
- Semantic animation recommendations can no longer choose Images, and authored resources apply to Video only.

### Removed
- The Images scene's own colour rows, the URL and URL-list sources, the `;`-separated folder and glob entry and its on-disk download cache. Folders are now chosen with folder pickers and the colour comes from the shared look.
