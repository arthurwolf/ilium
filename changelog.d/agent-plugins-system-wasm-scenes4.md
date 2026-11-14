### Added
- Scene plugins can draw terminal text: a scene writes the glyph plane (`SceneField::put_glyph`, sent with `write-glyphs`) and the client shows each character in place of the Braille dots of its cell.

### Changed
- "Digits of Pi" is now the `pi-digits` scene of the Scenes plugin instead of built-in code; a saved `animation: pi` opens it through the Plugin scene animation. The digits are prepared a slice per frame (no worker thread), the font is rasterized with swash, and the colour rows are renamed (Pi colour angle, Pi ink level, Digit N colour angle) with the choice "Pi rendering" saved as `native-text` or `font-braille`.

### Removed
- The JavaScript animation-package system is gone with no replacement: user `.iliumanim` packages, the Native/Plugin source tabs in Animation settings, the package permission review, the `ilium-animation-js` crate and `ilium-animation-helper` binary, and the hidden `release-animation-probe` command. Scenes are native or WebAssembly plugins.
