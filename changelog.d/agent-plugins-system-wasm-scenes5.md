### Changed

- Carpet and Live chess are scenes of the Scenes plugin (`ilium.core.scenes`) instead of built-in animations; disabling the plugin removes both from the Animations list and from the Semantic recommendations. A saved selection of either still loads, as the plugin scene. Live chess now polls the public Lichess TV every few seconds (grant `http:lichess.org`) instead of holding a stream open, and Carpet's automated chess searches inside the frame under its node budget instead of on a worker thread.
- Plugin scene settings can hide a row until a choice has a given value (`visible_when` in the manifest schema), and a scene may declare up to 64 rows. Scene guests now get an `http` import limited to their granted hosts that never blocks a frame.
