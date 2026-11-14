### Changed

- The Voxel Landscape background animation's assets, geometry, surface and colour code moved from `ilium-ambient` into the core scenes plugin (`plugins/core/scenes/src/voxel_landscape`). The kernel scene kind, its settings field, its client rows and its tests were removed; a saved `voxel_landscape` kind loads as a plugin scene. A guest-side scene entry (job-based engine, guest file access) is still to be wired, so the scene is unavailable until then. A new `minecraft-java` file scope lets scene plugins read a Java Edition install.
