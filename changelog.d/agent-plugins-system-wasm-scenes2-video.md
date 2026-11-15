### Added
- The Scenes plugin has a Video scene: it plays a picked video file, the videos of up to three picked folders (random scenes, slowed playback, shuffle, repeat one, fit, detail, dithered, colored and mono-ink styles) or the Germination series, through the host `media` import. Its colours follow the shared look. It is available beside the built-in Video animation, which is still present until that animation is removed.
- Scene plugins now get a non-waiting `next-frame` on the `media` import: while the decoder has no frame ready the call answers `timed-out` and the decoder keeps running, so a scene never stalls inside a frame.

### Changed
- In the plugin Video scene the Germination series has 45 of the 51 clips (the ones that fit the 16 MiB in-memory clip limit), each fetched in ranged chunks and checked against its pinned SHA-256; URL playback, glob entry and the brightness, contrast, gamma and invert rows are not part of it (the shared look owns colour).
