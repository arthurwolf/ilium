### Added
- Scene plugins: a scene component draws into the Braille pixel field of a frame surface (new `SceneProgram` and `SceneField` in the plugin SDK, scene component glue in `#[ilium_plugin(side = "scene")]`, the scene component is built into the plugin bundle). The Scenes core plugin now ships a real `dithered-waves` scene.
- Animations: a new "Plugin scene" animation lists the scenes supplied by enabled plugins in a Scene row; each scene's own controls come from the `settings_schema` of its manifest entry (the new `order` keyword sorts them) and are saved per scene in the project settings.

### Changed
- "Dithered waves" is now the `dithered-waves` scene of the Scenes plugin instead of built-in code; a saved `animation: dithered_waves` opens it through the Plugin scene animation. Its controls are the same except that the GPU renderer option is gone (it always draws on the CPU) and a few rows are renamed (Pattern, Ink strength, Band sharpness).
- Disabling the Scenes plugin removes its scenes from the list at once.
- "Cube clock", "Dithered water" and "Dithered fBm clouds" are now the `cube-clock`, `dither-water` and `fbm-clouds` scenes of the Scenes plugin instead of built-in code; saved `cube_clock`, `dither_water` and `fbm_clouds` selections open them through the Plugin scene animation. Dithered fBm clouds no longer has a GPU renderer option (it always draws on the CPU), and its Contrast, Dither and Brightness rows are now Tone steepness, Pattern and Intensity (the shared look already provides colour contrast and brightness); its Detail and Pixel size choices are now sliders.
- Plugin scenes receive the shared observer location, so the Cube clock can still derive its time zone from it.
