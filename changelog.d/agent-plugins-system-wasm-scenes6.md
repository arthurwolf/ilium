### Changed
- The ten classic scenes (Shoreline, Moonlit water, Sleeping ridge, Windy hillside, Tea steam, Kelp, Stone caustics, Cloudlets, Two ripples and Quiet pond) are now scenes of the Scenes plugin instead of built-in code. A saved animation that named one of them opens the Plugin scene animation, and with the Scenes plugin disabled they disappear from Settings. Each keeps its own sliders (and Shoreline its Classic/Rich style with the Rich-only rows) in the plugin's scene settings; colour, dithering and display stay in the shared rows.
- The voxel landscape's terrain, tree, village and wetland modules now live in the Scenes plugin source tree.

### Removed
- The built-in classic-scene renderer, its loop-cache scene hooks and its per-scene slider rows are gone from the client.
- The animation sandbox in `ilium-platform` (helper binary digest checks and its native sandbox tests) is gone together with the animation helper.
