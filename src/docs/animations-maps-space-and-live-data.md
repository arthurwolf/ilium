# Animations: maps, space and live data

This page documents the animated backgrounds that show the sky, the Earth, other worlds, maps and live public data: Stars, Satellite clouds, Earth at night, Solar system, Topographic maps, Voxel landscape (generated worlds and your saved Java worlds), OpenStreetMap, Live graphs, Live earthquakes, Live aircraft, Live boats, Live chess and Digits of Pi. Every scene control is listed with its range, its default and the condition under which its row appears. For how to switch backgrounds on, the colour, palette and dithering controls that every scene shares, and the other scene families, see [Animations](animations.md).

Contents:

- [Before you start](#before-you-start)
- [Shared location (Stars, Earth at night, Satellite clouds)](#shared-location-stars-earth-at-night-satellite-clouds)
- [Stars overhead](#stars-overhead)
- [Satellite clouds](#satellite-clouds)
- [Earth at night](#earth-at-night)
- [Solar system](#solar-system)
- [Topographic maps](#topographic-maps)
- [Voxel landscape](#voxel-landscape)
- [OpenStreetMap](#openstreetmap)
- [Live data scenes](#live-data-scenes)
  - [How live data behaves](#how-live-data-behaves)
  - [Live graphs](#live-graphs)
  - [Live earthquakes](#live-earthquakes)
  - [Live aircraft](#live-aircraft)
  - [Live boats](#live-boats)
  - [Live chess](#live-chess)
  - [Digits of Pi](#digits-of-pi)
- [Data sources and attribution](#data-sources-and-attribution)
- [Troubleshooting](#troubleshooting)

## Before you start

1. Open **Settings** and choose the **Animations** tab.
2. Turn **Background** on. Backgrounds are off by default.
3. Pick the scene in the scene list. The scene's own rows appear beneath the shared rows (Background, Speed, Density, Dither, palette and the other shared look controls).
4. Press `f` (or click the **Full screen preview** row) to see the scene without the settings; any key returns.

Things that hold for every scene on this page:

- Settings save immediately, per project, in `.ilium/config.yaml`. They sit under the top-level `animation:` mapping as flat keys named after the scene: `stars`, `solar_system`, `topographic_maps`, `openstreetmap`, `night_lights`, `clouds`, `voxel_landscape`, `graph`, `pi`, `earthquakes`, `aircraft`, `boats`, `chess`, plus a single shared `location`. Out-of-range values in the file are clamped when loaded; the ranges below are the clamped ranges.
- All of these scenes are **live-only**: they are evaluated as you watch, so the Playback (Loop or Live) row is hidden for them.
- Colour, palette, brightness, contrast, dithering, frame-rate cap and panel placement are global, not per scene. Scenes here only expose what is specific to them. Earth at night paints its own cell colours, so the palette rows are hidden for it.
- Animations are decoration only. They never change terminal history, text you copy, or agent detection. Outer terminal selection can include the decorative Braille.
- Scenes that use the network show a **Scene status** row (progress, an offline message, or the provider notice). Network access follows the rules in [Inference and privacy](inference-and-privacy.md) only for AI features; animations contact the public services listed under [Data sources and attribution](#data-sources-and-attribution) and nothing else.
- Shared look and display rows are described in [Animations](animations.md); general settings navigation is in [Settings](settings.md).

## Shared location (Stars, Earth at night, Satellite clouds)

These three scenes show the world as seen from one place, the shared **Location**. You set it once and every one of them uses it. It is stored as `location` (label, latitude, longitude) in the project config. The default is **Greenwich, London** (51.4779, -0.0015).

The Location row appears only when the selected scene uses it (Stars overhead, Earth at night or Satellite clouds). OpenStreetMap has its own, independent **Map location** (see [OpenStreetMap](#openstreetmap)) and never changes the shared location, and the shared location never moves the OpenStreetMap view.

To set the location:

1. Select a location-aware scene in **Settings -> Animations**.
2. Press Enter on the **Location** row (or click it). The picker opens.
3. Choose a place in any of three ways:
   - **Address or place name.** Type it and press Enter. Up to five candidates appear in the results list; move down into it with Down, choose with Up/Down and Enter.
   - **Coordinates.** Type `latitude, longitude`, for example `48.857, 2.352`. Accepted forms include `48.857, 2.352`, `48.857 2.352`, `48.857N 2.352E` and `-33.9 151.2`. Coordinates are parsed locally; nothing is sent anywhere.
   - **World map.** A Braille world map with a crosshair. Click a point, or move the crosshair with the arrow keys; hold Shift for steps of five cells.
4. Press Enter on the map (or the **Use location** button) to confirm. Esc cancels and leaves the previous location untouched. Nothing is saved until you confirm.

Keys in the picker: Tab and Shift+Tab move between the input field, the results list and the map; Esc cancels.

Notes:

- Address search sends the text you typed to the geocoding service, Open-Meteo's geocoding API (place data from GeoNames, CC BY 4.0). It matches a single place name rather than a full postal address: "Paris, France" is looked up by its first segment, and the remaining segments are used to rank the candidates. Answers are cached on disk for 30 days, so a repeated query never touches the network again.
- A search that is still running blocks a second one; wait for it or press Esc.
- At most four address workers run at once. If they are busy the picker says so; try again shortly.
- Latitude is clamped to -90..90 and longitude wrapped into -180..180.
- The picker credits the Natural Earth land mask it draws.

## Stars overhead

The real night sky above your shared location, right now, drawn as a star map in Braille dots. Positions come from the Yale Bright Star Catalogue, rotated into the local horizon using real sidereal time, as if there were no Sun, atmosphere or light pollution. The scene does no network access and no file access: it computes the sky from the clock and your location, and redraws about once a second.

Controls (rows marked "only when" appear conditionally):

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| Projection | Dome (looking up), Panorama (horizon strip), Patch (aimed window) | Dome | Dome looks straight up with the horizon at the edge. Panorama unrolls the sky along the horizon. Patch is a window aimed at one spot. |
| Lens | Stereographic, Equidistant fisheye | Stereographic | Only when Projection is Dome or Patch. Stereographic keeps shapes true and stretches the edges; equidistant fisheye keeps angles proportional to distance. |
| Top of the dome / Look direction | 0-360 degrees, step 5 | 0 | Compass bearing: 0 north, 90 east, 180 south, 270 west. Labelled "Top of the dome" for Dome and "Look direction" otherwise. Values wrap (360 is 0). |
| Look elevation | 0-90 degrees, step 5 | 50 | Only when Projection is Patch. 90 looks at the zenith. |
| Field of view | 10-180 degrees, step 5 | 180 | Across the panel height for the dome, across its width for panorama and patch. Small values zoom in. |
| Magnitude limit | 10-65 (tenths of a magnitude), step 1 | 50 | Faintest star drawn. 50 is magnitude 5.0; 65 is the naked-eye limit of 6.5. Lower shows fewer stars. |
| Star style | Realistic, Monochrome dots | Realistic | Realistic draws bright stars as bigger dot clusters and fades faint ones; monochrome draws every star as one full dot. |
| Star size | Normal, Small, Large | Normal | Only when Star style is Realistic. Size of the clusters of the brightest stars. |
| Star colors | On/off | On | Only when Star style is Realistic. Tints each star with its real colour (blue-white to orange-red) instead of the global palette. |
| Brightness gamma | 50-200 %, step 5 | 100 | Above 100 brightens faint stars; below 100 keeps only the brighter ones prominent. |
| Twinkle | On/off | Off | A subtle shimmer, stronger near the horizon. |
| Milky Way | On/off | On | A faint stippled band along the galactic plane, brightest towards Sagittarius. |
| Horizon line | On/off | On | On clips the sky at the ground and draws a horizon ring or line. Off includes objects below the horizon and expands the view to fill the panel (the whole sky). |
| Compass marks | On/off | On | Ticks at north, east, south and west; north has a longer tick and a marker. |
| Constellation lines | On/off | Off | Faint traditional stick figures of the main constellations. |
| Moon | On/off | On | The Moon with its current phase (low-precision ephemeris, about 0.3 degrees). |
| Planets | On/off | On | Mercury, Venus, Mars, Jupiter and Saturn where they really are. |
| Simulated satellites | On/off | Off | Illustrative low-Earth-orbit satellites in inclined orbits. They are simulated: not a live catalogue, not real tracked positions or pass predictions. |
| Precession | On/off | On | Corrects the J2000 catalogue positions for the slow wobble of the Earth's axis up to the date shown. |
| Time speed | x1 (real time), x10, x60 (1 min/s), x600 (10 min/s), x3600 (1 h/s), x86400 (1 day/s) | x1 | Simulated seconds per real second. Global animation speed also applies. |
| Time offset | -168 to 168 hours, step 1 | 0 | Shifts the simulated time to see the sky of earlier or later hours or days. |
| Start from | Now, Fixed date and time | Now | Begin at the real clock or at a fixed moment. |
| Start date and time | Text, `YYYY-MM-DD HH:MM` (UTC) | empty | Only when Start from is Fixed date and time. Universal time, for example `2026-12-21 22:00`; seconds are optional. Empty means now. Text that cannot be parsed is flagged and the real clock is used. |

Steps:

1. Set your [location](#shared-location-stars-earth-at-night-satellite-clouds).
2. Leave Projection on Dome for a classic all-sky chart, or choose Patch and set Look direction, Look elevation and Field of view to examine one region.
3. For a sky at a specific moment, set Start from to Fixed date and time and enter the UTC time, then leave Time speed at x1 to watch it from that moment in real time, or raise it (for example x3600) to see the sky rotate.

## Satellite clouds

Live weather-satellite cloud cover, either as a region around your shared location or for the whole Earth, shown as the newest image or as a looping time-lapse of the last hours. Downloads happen on a background worker; the Scene status row shows progress or an offline message. While the network is unreachable the last good image stays visible. Downloaded images are cached on disk in the `ambient` folder of Ilium's platform cache directory.

Satellite sources (the **Satellite** row):

| Option | What it is | Image cadence |
| --- | --- | --- |
| Automatic | The geostationary satellite that sees your location best (within 65 degrees of its sub-satellite point), or the world infrared mosaic for the whole Earth or when no satellite sees you well | n/a |
| GOES-East (Americas) | NASA GIBS GeoColor, sub-satellite longitude -75.2 | 10 minutes, about an hour behind real time |
| GOES-West (Pacific) | NASA GIBS GeoColor, sub-satellite longitude -137.2 | 10 minutes, about an hour behind real time |
| Meteosat (Europe, Africa) | EUMETSAT EUMETView infrared, longitude 0 | 15 minutes |
| Meteosat IODC (Indian Ocean) | EUMETSAT EUMETView infrared, longitude 45.5 | 15 minutes |
| World infrared mosaic | EUMETSAT global mosaic of all geostationary satellites | 3 hours |
| Daily true colour (MODIS) | NASA GIBS MODIS Terra corrected-reflectance polar mosaic | daily |

If a source yields nothing, Ilium falls back automatically: any geostationary source falls back to the world infrared mosaic, and the world mosaic falls back to the daily MODIS image. An explicit choice is honoured as long as it works.

Controls:

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| Coverage | Around my location, Whole Earth | Around my location | |
| Satellite | see table above | Automatic | |
| Projection | Flat map, Globe, Mollweide | Globe | Only when Coverage is Whole Earth. Mollweide is an equal-area ellipse. The globe is centred on your location. |
| Globe rotation | 0-30 deg/min, step 1 | 2 | Only for Whole Earth with Globe. 0 keeps your location in the centre. |
| Zoom level | 1-6 | 2 | Only when Coverage is Around my location. 1 shows about 120 degrees of latitude; each level halves the view, down to about 4 degrees. |
| History | Latest image only, Last 6 hours, Last 12 hours, Last 24 hours | Latest image only | Longer loops download more frames, once. Frames are spaced on an epoch-aligned grid so refreshes reuse cached images. |
| Playback speed | 1-12 fps | 4 | Only when History is not "Latest image only". |
| Smoothing | 0-100 %, step 10 | 60 | Only when History is not "Latest image only". Share of each frame interval spent cross-fading. |
| Check for new images | 5-180 min, step 5 | 20 | How often Ilium looks for newer images. |
| Contrast | 50-300 %, step 10 | 130 | How strongly clouds separate from the ground. |
| Brightness | -50 to 50 %, step 5 | 0 | Shifts the whole picture lighter or darker. |
| Ground dimming | 0-100 %, step 5 | 50 | Darkens land and sea so clouds stand out. |
| Invert | On/off | Off | Dark clouds on a light ground. |
| Land underlay | On/off | Off | Faint fill from the embedded land mask, useful over the ocean. |
| Underlay brightness | 5-60 %, step 5 | 20 | Only when Land underlay is on. |
| Location marker | On/off | Off | A blinking dot at the shared location. |
| Colour tint | On/off | Off | Tints each cell with the satellite's own colours (true colour where the source has them) instead of one ink colour. |

Edge cases and notes:

- GOES-East and GOES-West have true-colour imagery; the Meteosat and world mosaic sources are infrared, so Colour tint has less to show there.
- The Himawari (Pacific and Asia) satellites are not used directly; that part of the world is served by the world infrared mosaic.
- The cloud scene needs network access. If you stay offline, it keeps showing the last image it cached; with no cache it shows an offline message in Scene status.
- Imagery credit: NASA GIBS and EUMETSAT (Copyright EUMETSAT).

## Earth at night

City lights seen from orbit on a borderless map of the dark Earth, centred on your shared location. Imagery is NASA GIBS (keyless, 500 m tile matrix): by default the daily gap-filled VIIRS night radiance product (the newest day is usually yesterday; Ilium probes the newest day and up to four earlier ones), with the static VIIRS Black Marble 2016 composite as the complete-coverage fallback. A worker downloads a bounded set of tiles (15 at level 2, at most 50 at level 3), mosaics them into one luminance map, caches it on disk as PNG and refreshes every few hours. If a download fails it retries after five minutes and keeps showing the last good mosaic. The tile cache has a 120 MiB budget.

This scene paints its own cell colours, so the shared palette rows are hidden.

Controls:

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| Imagery | Latest daily (VIIRS), Black Marble 2016 | Latest daily (VIIRS) | Black Marble is a static, cleaner composite. |
| Projection | Flat map, Globe, Mollweide | Flat map | The globe is centred on your location. |
| Zoom | 100-600 %, step 25 | 100 | 100 shows the whole world; larger values magnify around your location. |
| Globe rotation | 0-30 deg/min, step 1 | 3 | Only for Globe. 0 keeps your location in the centre. |
| Brightness | 25-400 %, step 5 | 120 | Overall gain after gamma. |
| Gamma | 30-300 %, step 5 | 70 | Below 100 reveals faint towns; above 100 keeps only the brightest cities. |
| Threshold | 0-60 %, step 1 | 8 | Light dimmer than this share of full scale is treated as darkness, which keeps dithering clean. |
| Glow | 0-100 %, step 5 | 20 | A soft halo around bright cities so they survive coarse dithering. |
| Detail | Automatic, Coarse (fewer tiles), Fine (more tiles) | Automatic | Automatic follows the terminal size. |
| Day/night shading | On/off | Off | Hides lights on the sunlit side using the real position of the Sun. |
| Daylight dimming | 0-100 %, step 5 | 70 | Only when Day/night shading is on. How completely daylight hides the lights. |
| Coastlines | On/off | Off | A faint coastline for orientation. Off by default: the map has no borders or labels. |
| Coastline brightness | 5-100 %, step 5 | 25 | Only when Coastlines is on. |
| Location marker | On/off | Off | A blinking dot at the shared location. |
| Check for new imagery | 1-48 h, step 1 | 6 | How often to look for a newer daily image. |

Tip: for a dramatic view, choose Globe, turn Day/night shading on, and leave Globe rotation at its default.

## Solar system

A heliocentric, top-down view of the Sun and all eight planets. Orbits follow approximate JPL elements starting at J2000, including eccentricity and inclination. The scene does no network access. The shared Speed control also scales the simulation.

Because real distances and sizes are wildly different, two independent scales let you trade legibility for realism. A body smaller than one dot keeps a one-dot marker so it remains visible.

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| Distance realism | 0-100 %, step 1 | 0 | 0 compresses orbital distances logarithmically; 100 preserves physical distance ratios, so inner planets cluster near the Sun. |
| Size realism | 0-100 %, step 1 | 0 | 0 exaggerates body sizes; 100 uses physical radii at the orbital scale. |
| Simulation speed | 1 hour/s, 1 day/s, 10 days/s, 30 days/s, 1 year/s, 10 years/s | 30 days/s | Simulated time per animation second. |
| Orbit paths | On/off | On | Draws each visible planet's orbit. |
| Mercury, Venus, Earth, Mars, Jupiter, Saturn, Uranus, Neptune | On/off each | all On | Hidden planets do not change the scale of the rest. |

## Topographic maps

Contour maps drawn as Braille dots, on a slowly panning flat map or a turning globe. Each line is one elevation step. All elevation data is embedded in Ilium (about 0.2 degree resolution), so the scene needs no network and no downloads.

Worlds:

| World | Data |
| --- | --- |
| Earth | NOAA ETOPO 2022 |
| Moon | NASA LOLA |
| Mars | NASA MOLA |
| Venus | NASA Magellan |
| Mercury | NASA MESSENGER |
| Ceres | NASA Dawn |
| Aeria (fictional archipelago) | Generated from the seed |
| Pangaea Prime (fictional supercontinent) | Generated from the seed |
| Ridgeworld (fictional mountain belts) | Generated from the seed |
| Craterlands (fictional airless world) | Generated from the seed |

Controls:

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| World | All worlds in turn, Real worlds in turn, Fictional worlds in turn, then each of the ten worlds | All worlds in turn | Cycles run real then fictional. The next world dissolves in over a few seconds. |
| Seconds per world | 20-600 s, step 5 | 75 | How long each world stays when several are in turn. |
| Fictional seed | 0-9999 | 7 | Each seed gives different continents, mountain belts and craters. |
| Projection | Flat map, Globe | Flat map | The globe turns instead of sliding. |
| Zoom | 50-1600 %, step 10 | 100 | 100 fits the whole world; higher shows a smaller region in more detail. |
| Contour lines | 6-80 | 16 | Number of lines across the whole relief, at round elevations. Ignored when a fixed spacing is set. |
| Contour spacing | 0-10000 m, step 50 | 0 | Fixed elevation step. 0 derives the step from Contour lines. |
| Index line every | 0-10 | 5 | Every Nth contour is drawn heavier, like a survey map. 0 draws none. |
| Line thickness | 1-3 dots | 1 | |
| Lines below zero | Solid, Dotted, Hidden | Dotted | How contours below the zero level (sea floor, basins) are drawn. |
| Emphasise zero level | On/off | On | Draws the zero-elevation contour (the shore on Earth) heavier. |
| Map outline | On/off | On | The edge of the map, or the limb of the globe. |
| Relief shading | 0-100 %, step 5 | 0 | Sparse dots that light north-west-facing slopes in addition to the contours. |
| Colours | Global colour, Hypsometric tints, Natural for the world, Heat, Ice | Natural for the world | Hypsometric is blue below zero, green to white above. Natural gives rust for Mars, grey for the Moon, and so on. Global colour uses the shared palette. |
| Pan | Wandering route, East, West, North, South, North-east, Still | Wandering route | The globe turns instead of sliding. |
| Pan speed | 0-400 %, step 10 | 100 | 0 holds the view; 100 moves about 5 dots per second at any zoom. |
| Start latitude | -80 to 80, step 5 | 15 | Also the fixed latitude for a still view. |
| Start longitude | -180 to 180, step 5 | 0 | |
| Zero level shift | -3000 to 3000 m, step 50 | 0 | Raises or lowers the zero level that the shore line and "Lines below zero" refer to, flooding or draining the world. |
| Tide range | 0-2000 m, step 50 | 0 | Lets the zero level rise and fall by this much (peak to peak), so shores creep across the map. 0 keeps it fixed. |
| Tide period | 10-600 s, step 10 | 120 | Only when Tide range is above 0. Seconds per full rise and fall. |

Recipes:

- **A survey sheet of one world:** World = Earth, Projection = Flat map, Contour lines = 30, Index line every = 5, Pan = Still, Zoom = 300, and set Start latitude and longitude to the area you want.
- **Flooded Earth:** Zero level shift = 200 to 1000 m, Lines below zero = Hidden.
- **Breathing coastlines:** Tide range = 500, Tide period = 60.
- **Round numbers:** Contour spacing = 500 m for a constant 500 m step.

## Voxel landscape

An isometric block world that pans slowly past, drawn as monochrome or pastel dithering. It has two sources chosen by **World source**: **Generated** (Ilium's own seeded terrain) and **Saved maps** (an experimental, read-only viewer of your own Java worlds).

The scene is surface scenery only. Decoration never changes terminal output, history, copied text or agent detection.

### Generated worlds

Generated terrain is seeded and deterministic: the same seed always recreates the same world, including its structures. Camera, colour and detail changes never regenerate a different landscape. The generator offers a large catalogue of surface biomes (plains, forests, deserts, badlands, taiga, jungle, swamp, snowy and mountain biomes and more), forests, cacti, connected villages, ruins and landmarks, cave mouths and ravines, built from 208 feature recipes and 64 original Ilium materials. The textures are original procedural pixel artwork; no Minecraft or resource-pack assets are bundled with Ilium.

Controls with the Generated source:

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| World source | Generated, Saved maps | Generated | |
| World seed | Integer 0-4294967295 | 71839 | Entered as text. Invalid text is rejected without changing anything. |
| Scene atmosphere | Day, Night, Thunderstorm | Day | A fixed atmosphere with matching light and surface creature scenes. Terrain and structures keep their positions. |
| Tile zoom | 25-400 %, step 5 | 150 | Enlarges isometric tiles to inspect blocks, or zooms out to see more landscape. |
| Detail | Terrain, Landmarks, Landscape, All features | Landscape | Visible decoration density. Terrain and structure positions stay stable across levels. |
| Camera speed | 0-200 %, step 5 | 25 | 0 freezes the camera; global animation speed also applies. |
| Camera direction | East, South, North-east, South-east | East | A world-space direction; the isometric projection keeps both ground axes visible. |
| Dither color | Black and white, Texture colors | Texture colors | Texture colours use the artwork's colours (pastel); black and white is monochrome shading. Global density controls dot coverage. |
| Landscape palette | Original colors, Rose garden, Cool mist, Amber evening | Original colors | A tint over the texture colours. |
| Hue tint | 0-360 degrees, step 5 | 180 | 180 is neutral; lower favours warm tones, higher favours cool tones. |
| Color saturation | 0-100 %, step 5 | 100 | Zero makes every material grey while keeping its shading. |
| Color lightness | 5-100 %, step 5 | 100 | Brightness of lit dots, not the number of dots. Does not replace the global background lightness. |
| Vegetation density | 0-200 %, step 5 | 100 | Trees, flowers, crops and other plants. Anchor positions remain deterministic. |
| Structure density | 0-200 %, step 5 | 100 | Villages, ruins and landmarks. 0 leaves natural terrain only. |
| Rivers | On/off | On | Carves coherent river channels and fills them to their water level. |
| Ravines | On/off | On | Narrow deep fissures that expose stratified rock faces. |
| Cave mouths | On/off | Off | Surface cave openings with dark entrances; not a subterranean camera. |

The pack rows described under [Texture pack profiles](#texture-pack-profiles-and-custom-packs) also appear.

### Saved maps

**World source -> Saved maps** is an experimental reader that renders your own saved Java Edition worlds. It is strictly read-only: Ilium never modifies a save, and missing terrain is never generated.

Steps:

1. In **Settings -> Animations**, select **Voxel landscape** and set **World source** to **Saved maps**.
2. Leave **Saved maps folder** blank to use the official launcher's saves folder, or enter an absolute path (see the table below).
3. Make sure the official Java launcher has installed the **1.19.3** client (Ilium reads `versions/1.19.3/1.19.3.jar` in the official Java game folder). Saved rendering needs those assets even when your saves live elsewhere, and Ilium does not download them.
4. Watch the **Scene status** row. It reports preparation progress and any reason a world cannot be shown.

Default saves folder (blank **Saved maps folder**):

| Operating system | Folder |
| --- | --- |
| Linux | `~/.minecraft/saves` |
| macOS | `~/Library/Application Support/minecraft/saves` |
| Windows | `%APPDATA%\.minecraft\saves` |

For another launcher, enter the absolute folder that contains your world folders. A relative path, a path with a NUL character, or one longer than 4096 bytes is rejected. A world folder counts as a candidate only if it contains `level.dat` and a `region` folder. Discovery looks at direct children only and is bounded (it refuses a folder with more than 4096 entries or more than 512 maps; choose a smaller folder in that case).

Version limits and behaviour:

- The reader admits Anvil chunk **DataVersion 2834 through 3218** (the last matches the Java 1.19.3 format) and only fully saved chunks.
- Older or newer formats, unfinished chunks, missing source coverage or unsupported pack content produce a status message instead of a picture.
- Tour selection seeks block-derived biome appearances on odd runs and structures on even runs. Appearance history counts only terrain that is present in the emitted background.
- Native rendering and automatic tours remain experimental.

Rows shown with the Saved maps source: World source, Saved maps folder, the pack rows, Tile zoom, Camera speed, Dither color, Landscape palette, Hue tint, Color saturation and Color lightness. The generation-only rows (World seed, Scene atmosphere, Detail, Camera direction, Vegetation density, Structure density, Rivers, Ravines, Cave mouths) are hidden. Your generated settings, including the seed and pack paths, are preserved and come back when you switch to Generated again.

### Texture pack profiles and custom packs

The pack rows let a saved world (and the rendering pipeline) use a texture pack you already have. Ilium bundles none of them. The **Custom pack profile** row (labelled **Full texture pack** with the Generated source) selects one of eleven reviewed profiles; each profile keeps its own custom path and settings, so switching profile never relabels another profile's path.

Profiles: Jicklus, F8thful, Whimscape, GoodVibes / Acaitart (the default selection), deathcap ProgrammerArt, Textureless, Plasticator, PixelPerfectionCE, Faithful32, Faithful64, Antumbra. Each is a private-testing profile with its own recorded author credit, licence record, restrictions and known missing coverage; missing textures or models are reported explicitly rather than invented. GoodVibes / Acaitart is an extracted art tree (CC BY 4.0, credit Acaitart) rather than a ready Java pack, so it needs explicit compatibility geometry; Textureless has no water; Plasticator has Java and Bedrock variants.

To use a local pack with Saved maps:

1. Choose a **Custom pack profile**.
2. Enter its absolute path in **Custom pack file or folder** (a ZIP archive or an extracted directory). Blank uses the installed Java 1.19.3 assets. A selected Java pack overrides their models and textures.
3. Under **Pack source type** choose **ZIP archive** or **Directory** to match.
4. If the pack's files are nested inside another folder, enter that relative folder in **Root inside pack**. It must be a safe relative path (maximum 512 bytes, no control characters).

All pack rows:

| Row | Options | Default | Notes |
| --- | --- | --- | --- |
| Custom pack profile / Full texture pack | the eleven profiles above | GoodVibes / Acaitart | |
| Custom pack file or folder | absolute path, up to 4096 bytes | empty | Control characters are rejected. Each profile retains its own path. |
| Root inside pack | relative folder, up to 512 bytes | empty | Use only when the archive nests its pack files. |
| Pack source type | ZIP archive, Directory | ZIP archive | How to read the selected local source. |
| Pack edition | Reviewed primary edition, Plasticator Bedrock 2.4 | Reviewed primary edition | Bedrock is available only for the reviewed Plasticator variant. |
| Official models add-on | absolute path | empty | Only Textureless has a reviewed internal model add-on. |
| Add-on source type | ZIP archive, Directory | ZIP archive | Applies only to the Textureless add-on. |
| Use last duplicate ZIP member | On/off | Off | Textureless only: record duplicate members and choose the final central-directory entry. |
| Target pack format | `major.minor` | 999.0 | Target for authored overlays; a format outside the declared range needs explicit compatibility evidence. |

## OpenStreetMap

Real streets, buildings, water, green spaces and railways drawn as Braille dots from OpenStreetMap data, with a fixed camera, a slow orbit or an east-west pan. It has three **Map source** choices:

| Map source | Needs | Notes |
| --- | --- | --- |
| World catalogue | nothing (works offline) | Bundled, gzip-compressed extracts of ten places. No network traffic at all. |
| Local OSM extract | an Overpass-format `out geom` JSON file on disk | Parsed on an owned worker with strict size (16 MiB) and geometry bounds. |
| Custom Overpass | an HTTPS Overpass interpreter endpoint you are authorized to use | One bounded, cached query per place. Pan and style changes reuse the geometry. No public service is contacted by default. |

Map data: [OpenStreetMap contributors](https://www.openstreetmap.org/copyright), ODbL.

### Offline catalogue and tour lists

The ten bundled places are Paris, London, Venice, New York, Tokyo, Cape Town, Sydney, Rio de Janeiro, Singapore and Reykjavik. With **Catalogue mode = Classic place** you pick one (**Place**) or let **Place selection** (Selected place, Ordered tour, Shuffled tour) visit them in turn, spending **Place duration** (30-1800 s, default 120, step 30) at each; global Speed applies.

With **Catalogue mode = Offline tour list** (or any Overpass source in "Named place list" mode), tours use named lists with stable member IDs and unscaled wall time:

| List | Members | Offline |
| --- | --- | --- |
| Offline world (10 extracts) | all ten bundled places | yes |
| Offline Europe | Paris, London, Venice, Reykjavik | yes |
| Offline Asia-Pacific | Tokyo, Singapore, Sydney | yes |
| Offline coastal cities | Venice, Cape Town, Sydney, Rio de Janeiro, Singapore, Reykjavik | yes |
| Offline Americas | New York, Rio de Janeiro | yes |
| Historic towns and centres | Bruges, Dubrovnik, Tallinn, Quebec City | needs Overpass |
| Landmarks and monuments | Taj Mahal, Acropolis, Cologne Cathedral, Mont-Saint-Michel | needs Overpass |
| Harbours and coastal cities | Porto, Willemstad, Valparaiso | needs Overpass |
| Urban parks | Central Park, Golden Gate Park | needs Overpass |
| Street grids and diagonals | Philadelphia, Chicago, La Plata | needs Overpass |
| World sampler (all destinations) | all 26 destinations | partly (ten are bundled) |

Rules for lists: every member occurs once per cycle; shuffled tours keep the starting place first; unseen elapsed stops are not fetched on catch-up; list tours use a 60-second minimum **Place duration** (default 120, range 60-1800, step 30). Lists never geocode or prefetch. A stop that fails stays failed until you explicitly edit the source or selection. Choosing a non-bundled list with the World catalogue source is refused with a message asking you to pick an offline list or configure Overpass.

### Choosing a map location

The **Map location** row (shown for this scene instead of Location) opens the same picker as the [shared location](#shared-location-stars-earth-at-night-satellite-clouds), titled "OpenStreetMap location". You can click a world-map point, type `latitude, longitude`, or search a city or street address by pressing Enter.

Important behaviour:

- Searching an address only returns coordinates. It does not download map geometry.
- Choosing an arbitrary point switches the source to **Custom Overpass** in "Selected coordinates" mode, because only a configured Overpass service can supply geometry for an arbitrary place. Without a configured service the scene tells you so.
- Latitude must be within -85 to 85 and longitude within -180 to 180 for map locations.
- The map location is independent of the shared location used by Stars, Earth at night and Satellite clouds, in both directions.

### Place search providers

The **Place search** row selects who answers address searches in the picker. Only pressing Enter in the picker submits a search; typing coordinates and moving the map stay local.

| Option | Behaviour |
| --- | --- |
| Photon (city/address) | Default. Endpoint defaults to `https://photon.komoot.io/api/` (editable in the **Photon service** row). The default operator welcomes reasonable project use with no availability guarantee; use your own service for larger workloads. |
| Configured Nominatim | Needs an HTTPS endpoint in the **Nominatim service** row (for example `https://your-service/search`). The public OSMF Nominatim host is deliberately refused; there is no autocomplete or bulk search. Follow the [Nominatim usage policy](https://operations.osmfoundation.org/policies/nominatim/). |
| Open-Meteo (city only) | The older city-name service, not house-address lookup. Open-Meteo terms apply; places are GeoNames data (CC BY 4.0). |
| Disabled | The picker still accepts coordinates and map clicks. |

Search endpoints must be ASCII HTTPS URLs with an explicit host and valid port, without credentials, query string or fragment, at most 2048 characters. Invalid values are rejected and leave the setting unchanged.

### Overpass services and local files

For **Custom Overpass**, set **Overpass service** to the interpreter URL, for example `https://your-service/api/interpreter` (HTTPS only, no credentials, query or fragment). Choose a service you are authorized to use; a public instance is unsuitable as a permanent application backend.

Ilium's request behaviour is deliberately conservative:

- One query per place, covering a box of about 0.016 degrees of latitude by 0.024 degrees of longitude around the centre (roads, buildings and building parts, waterways, natural, land use, leisure, railways, amenities, tourism, historic features and shops), with a 15-second server timeout and a 16 MiB size cap.
- HTTPS only, no redirects, a 5-second connect timeout and 20-second overall timeout, JSON responses only.
- At most one OSM request at a time and at least 60 seconds between request starts. HTTP 429 or 503 extends the cooldown (using `Retry-After` when present). There is no automatic retry after an HTTP error.
- Custom provider failures retain the last good map. Pan, camera and style edits never trigger a new request; only an explicit edit of the source, endpoint, coordinates, list or destination does.

For **Local OSM extract**, set **OSM JSON file** to an absolute path (up to 4096 bytes) of an Overpass `out geom` JSON file, and **Map center** to the extract's centre as `latitude, longitude`. Editing coordinates clears any old place label.

### OpenStreetMap controls

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| Map source | World catalogue, Local OSM extract, Custom Overpass | World catalogue | |
| Catalogue mode | Classic place, Offline tour list | Classic place | Only for World catalogue. |
| Place | the ten bundled places | Paris | Classic mode only. |
| Tour list | the lists above | Offline world (10 extracts) | |
| Starting place | members of the chosen list | first member | Stable destination IDs are saved, not row positions. |
| Place selection | Selected place, Ordered tour, Shuffled tour | Selected place | |
| Place duration | 30-1800 s (60-1800 for lists), step 30 | 120 | |
| OSM JSON file | absolute path | empty | Local OSM extract source. |
| Overpass service | HTTPS URL | empty | Custom Overpass source. |
| Destination mode | Selected coordinates, Named place list | Selected coordinates | Custom Overpass only. |
| Map center | `latitude, longitude` | `48.8584, 2.2945` | Local and Custom sources in coordinate mode. |
| Place search / Photon service / Nominatim service | see above | Photon | |
| Camera | Fixed, Slow orbit, East-west pan | Slow orbit | Moves only within the loaded extract; makes no network requests. |
| Pan speed | 0-300 %, step 10 | 100 | 100 completes a slow sweep in two minutes. 0 holds the camera; global Speed also applies. |
| Map width | 300-2000 m, step 100 | 1400 | Width of the viewport. Smaller values zoom in. |
| Map brightness | 0-200 %, step 5 | 100 | Dot intensity before shared density and dithering; 0 hides every map dot. |
| Line weight | 25-200 %, step 5 | 100 | Roads, railway tracks and building outlines. |
| Roads and paths | On/off | On | Highway ways, including footpaths. |
| Buildings | On/off | On | Closed building footprints, including multipolygon holes. |
| Water | On/off | On | Rivers, waterways and water-area polygons. |
| Green areas | On/off | On | Parks, woods, grass, farmland and other green land use. |
| Railways | On/off | On | |
| Points of interest | On/off | Off | Small dots at amenity nodes. |

Clipped or incomplete polygon boundaries remain outlines.

### Attribution

Map data is from OpenStreetMap contributors under the Open Database Licence (ODbL); see the [copyright page](https://www.openstreetmap.org/copyright). The picker credits its [Natural Earth](https://www.naturalearthdata.com/about/terms-of-use/) land mask and the selected search provider. City-only results use [Open-Meteo](https://open-meteo.com/en/terms) and GeoNames (CC BY 4.0). Help screens never fetch data.

## Live data scenes

The scenes from [Live graphs](#live-graphs) to [Live chess](#live-chess) read public, keyless services. Digits of Pi is on this page too because it is a data-style scene, but it computes locally.

### How live data behaves

- **Last good data is kept.** If a request fails, is malformed, oversized or cancelled, the scene keeps showing the last successful data together with the error and the age of that data. A failed refresh never blanks the display.
- **No simulated events.** Ilium never invents samples, earthquakes, aircraft, ships or chess moves to fill a gap. Missing history stays empty.
- **Provider time, receipt time.** Observation times come from the provider and are shown separately from the time Ilium received them. A successful request does not guarantee a new observation.
- **Request floors.** Each source has a minimum interval. A requested refresh shorter than the floor is raised to it; the row's help shows the effective interval. Failed requests back off with bounded delays.
- **Bounded and cached.** Responses are size-limited. Aircraft and boat snapshots are cached on disk and shared by every Ilium client on the machine, so extra clients, reopened scenes or a restart reuse the original receipt rather than hitting the provider again.
- **Source changes isolate data.** Switching a source clears the previous source's data; errors retain only the selected source's last good data. Failures never silently switch providers.
- **Offline.** With no network you see the last good data, if any, plus an offline or error status.
- Every live scene needs network access. Credits and licences are listed under [Data sources and attribution](#data-sources-and-attribution).

### Live graphs

Provider-time line graphs, bars and genuine OHLC candles from public keyless sources. There are 32 series. Choose one in the **Source** row.

| Group | Series (id) | Unit | Provider | Minimum refresh |
| --- | --- | --- | --- | --- |
| Crypto (8, with real OHLC candles) | Bitcoin / USD (`btc_usd`), Ethereum / USD, Solana / USD, Litecoin / USD, Dogecoin / USD, Cardano / USD, XRP / USD, Avalanche / USD | USD | Coinbase Exchange candles | 60 s |
| Currency (8, daily) | EUR / USD, EUR / GBP, EUR / JPY, EUR / CHF, EUR / CAD, EUR / AUD, EUR / SEK, EUR / NOK (ECB daily) | quote currency | ECB reference rates via Frankfurter | 1 hour |
| Solar wind (3) | Solar wind speed, Solar wind density, Solar wind temperature | km/s, protons/cm3, K | NOAA SWPC, active RTSW spacecraft | 60 s |
| Magnetic field (4) | Interplanetary magnetic field (Bt), Magnetic field Bx (GSM), By (GSM), Bz (GSM) | nT | NOAA SWPC, active RTSW spacecraft | 60 s |
| ISS (4) | ISS altitude, ISS orbital speed, ISS latitude, ISS longitude | km, km/h, degrees | Where the ISS at? (orbital estimate) | 5 s |
| Earthquakes (2) | Earthquake activity (events per hour), Earthquake magnitudes | events/hour, magnitude | USGS all-day feed | 60 s |
| Randomness (1) | Public randomness (Quicknet) | 0-1 | drand Quicknet beacon | 3 s |
| Wikipedia (2) | Wikipedia edit activity (edits/s), Wikipedia bot share (% of known edits) | per second, percent | Wikimedia EventStreams | 5 s |

Controls:

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| Source | the 32 series | Bitcoin / USD | The help detail shows the attribution, unit and documentation link of the selected series. Selecting ISS, Wikipedia or drand from a non-fast source moves a default two-hour window down to one minute; windows you set yourself are retained. |
| Chart | Line, Bars, OHLC candles | Line | OHLC candles are available only for the eight crypto series; the option is disabled with an explanation elsewhere. A saved Candles choice on a non-OHLC series falls back to Line. Candles use the provider's actual open, high, low and close, never invented samples. |
| Time window | 1 minute, 5 minutes, 30 minutes, 1 hour, 2 hours, 6 hours, 1 day, 1 week, 30 days, 90 days, 1 year | 2 hours | A rolling window of provider timestamps. A saved custom span (up to 525600 minutes) is kept and shown as "Custom saved span". Daily currency rates need longer windows to show more than a point or two. |
| Requested refresh | 5-3600 s, step 5 | 60 | Raised to the source's floor. The help shows the effective interval. |
| Brightness | 0-100 %, step 5 | 65 | Intensity of chart ink, including the dim axes. |
| Chart hue | 0-360 degrees, step 10 | 190 | Colour of lines, bars and axes. |
| Rising candle hue | 0-360 degrees, step 10 | 120 | Only when Chart is OHLC candles. Close at or above open. |
| Falling candle hue | 0-360 degrees, step 10 | 0 | Only when Chart is OHLC candles. Close below open. |

Details worth knowing:

- Candle interval is chosen automatically from the window: the smallest of 1 min, 5 min, 15 min, 1 hour, 6 hours or 1 day that needs at most about 300 candles. Changing the window can change the interval, which clears the previous interval's history. A one-year window uses two bounded requests.
- History keeps at most 20,000 measurements.
- Currency series fetch about the last 366 days of ECB daily reference rates. These are daily EUR reference rates, not intraday or trading prices; the chart coordinate for a date is UTC midnight.
- Wikipedia values are an aggregate of public edits across languages; the bot share counts only edits with a known bot classification.
- Quicknet values are shown without BLS signature verification.
- ISS values are an orbital estimate, not a telemetry feed.
- Earthquake activity counts events per provider hour, including unknown magnitudes.

### Live earthquakes

Reported earthquakes on a Braille world coastline, with pulsing markers and magnitude labels. The data is the USGS all-day GeoJSON summary feed of every reported magnitude, including tiny, zero, negative and unknown magnitudes. It refreshes at most once per minute, and a feed refresh does not imply a new earthquake.

| Row | Range | Default | Notes |
| --- | --- | --- | --- |
| Requested refresh | 5-3600 s, step 5 | 60 | Floor of 60 s for earthquakes. |
| Map hue | 0-360 degrees, step 10 | 210 | Colour of the embedded Natural Earth coastlines. |
| Map brightness | 0-100 %, step 5 | 45 | Coastline brightness, independent of markers. |
| Marker hue | 0-360 degrees, step 10 | 35 | Markers and magnitude labels. |
| Marker brightness | 0-100 %, step 5 | 90 | Brightness of reported-object markers (twice the map brightness by default). |
| Magnitude labels | On/off | On | Shows reported magnitudes, including zero and negative values; `?` means unknown. Colliding labels are omitted; markers are never omitted for that reason. |

### Live aircraft

Airborne positions reported by the OpenSky Network, plotted over a Braille world coastline. Every reported aircraft is exactly one undithered dot, drawn at about twice the brightness of the coastline.

- Anonymous global access has a limited credit budget, so refresh is at least **15 minutes** (900 s). The floor is shared through a local reservation, so several Ilium clients on one machine do not multiply requests.
- Coverage is incomplete: OpenSky's anonymous data comes from terrestrial receivers, so open ocean has few or no reports. This is not a list of every aircraft worldwide.

| Row | Range | Default | Notes |
| --- | --- | --- | --- |
| Requested refresh | 5-3600 s, step 5 | 60 | Raised to 900 s. |
| Map hue / Map brightness | 0-360 / 0-100 % | 210 / 45 | Coastline. |
| Marker hue / Marker brightness | 0-360 / 0-100 % | 35 / 90 | Aircraft dots. Separate from the boat scene's settings. |
| Heading arrows | On/off | Off | Draws a heading arrow around each dot. Off draws exactly one dot per aircraft. |

### Live boats

Received ship (AIS) positions over a Braille world coastline. Choose the data source in **Boat source**:

| Source | Coverage | Minimum refresh | Notes |
| --- | --- | --- | --- |
| OpenSeaFeed (broader) | Reported AIS positions from many regions; incomplete reception | 60 s | Default. Position age is unavailable: its timestamp is the latest AIS update of any record, not a coordinate fix. Snapshot build time, latest update and network receipt are shown separately. Known non-vessel identities (search-and-rescue aircraft, navigation aids, distress devices, coast stations and group identities) are excluded; other unusual identities are retained. |
| Digitraffic (Finnish) | Finnish waters only | 30 s | Fintraffic / Digitraffic AIS. Reported position timestamps are the provider's; local receipt is separate. Reported identities are not authenticated. |

Neither source inventories all ships. Switching source clears the previous source's data. The **Boat source** row is followed by six read-only notices (Read credit, Read provider URL, Read license URL, Read coverage, Read changes, Read time semantics): open one to read its complete value; they cannot be edited.

Other rows are the same as for aircraft: Requested refresh (default 60, 5-3600 s), Map hue, Map brightness, Marker hue, Marker brightness and Heading arrows (default Off, one dot per vessel). Boats and aircraft keep separate saved settings.

### Live chess

The featured game on Lichess TV, shown with original dithered chess-piece silhouettes. Ilium follows the Lichess TV streaming feed (`lichess.org/api/tv/feed`) and shows the real positions, and the clocks as last reported at the latest event; the feed carries no observation timestamp, so the status line shows the receipt time instead. After a dropped connection, the last board stays on screen with an error and the time since receipt, and Ilium reconnects with a delay of 5 seconds doubling up to 60 seconds (at least 60 seconds after an HTTP 429). No synthetic game or invented move is ever substituted.

| Row | Range | Default | Notes |
| --- | --- | --- | --- |
| Black at bottom | On/off | Off | Rotates the board by 180 degrees; pieces stay upright. |
| Chess brightness | 0-200 %, step 5 | 100 | Intensity before Braille dithering; 0 hides the board and pieces. |
| White red / green / blue | 0-255, step 5 | 235 / 222 / 184 | Colour of White's pieces. |
| Black red / green / blue | 0-255, step 5 | 100 / 164 / 205 | Colour of Black's pieces. |
| Board red / green / blue | 0-255, step 5 | 80 / 95 / 105 | Colour of the board. |

Colour channels are independent; changing a colour preserves dot density. Carpet's Lichess TV mode is described in [Animations](animations.md).

### Digits of Pi

Exact digits of Pi scroll as native terminal text or as real-font Braille. The digits are calculated locally (no network); up to 20,000 exact digits are available and the selected prefix repeats. Native text follows your terminal font; Font Braille uses the bundled Cascadia Code.

| Row | Range or options | Default | Notes |
| --- | --- | --- | --- |
| Pi rendering | Native text, Font Braille | Native text | Font Braille rasterises the bundled font on an owned worker. |
| Font size | 8-48 dots, step 2 | 20 | Only for Font Braille. Native text uses your terminal's configured size. |
| Exact digits | 1-20000, step 100 | 4096 | Includes the initial 3; the decimal separator is added for display. |
| Scroll speed | 0-2000 (units of 0.1 characters per second), step 10 | 10 (1 character per second) | 0 holds the prefix; global Speed also applies. |
| Pi hue | 0-360 degrees, step 5 | 180 | Common colour of the decimal digits when digit colours are off. |
| Pi brightness | 0-100 %, step 5 | 65 | 0 hides both native and Braille text. |
| Colors by digit | On/off | Off | Gives each decimal digit its own hue; the decimal separator keeps the common hue. |
| Digit 0 hue ... Digit 9 hue | 0-360 degrees, step 5 | 0, 36, 72, 108, 144, 180, 216, 252, 288, 324 | Only when Colors by digit is on. |

## Data sources and attribution

| Scene | Source | Terms and credit |
| --- | --- | --- |
| Stars overhead | Yale Bright Star Catalogue (embedded), low-precision ephemerides | no network |
| Satellite clouds | NASA GIBS (GOES-East and GOES-West GeoColor, MODIS Terra), EUMETSAT EUMETView (Meteosat, Meteosat IODC, world mosaic) | Copyright EUMETSAT; EUMETSAT [data licensing](https://www.eumetsat.int/eumetsat-data-licensing) |
| Earth at night | NASA GIBS VIIRS night radiance and Black Marble | NASA GIBS |
| Shared location and city search | Open-Meteo geocoding (GeoNames data) | CC BY 4.0, [Open-Meteo terms](https://open-meteo.com/en/terms) |
| Solar system | approximate JPL orbital elements (embedded) | no network |
| Topographic maps | NOAA ETOPO 2022; NASA LOLA, MOLA, Magellan, MESSENGER, Dawn (embedded) | no network |
| Voxel landscape | Generated; saved maps read from your own folders | original textures; your packs keep their own licences |
| OpenStreetMap | Bundled extracts, your Overpass service or file; Photon, Nominatim or Open-Meteo for search | OpenStreetMap contributors, ODbL; Natural Earth land mask; search provider terms |
| Live graphs | Coinbase Exchange, ECB via Frankfurter, NOAA SWPC, Where the ISS at?, USGS, drand Quicknet, Wikimedia EventStreams | see per-series credit in the Source row's help |
| Live earthquakes | USGS | USGS |
| Live aircraft | OpenSky Network | OpenSky; anonymous budget applies |
| Live boats | OpenSeaFeed contributors (CC BY 4.0) or Fintraffic / Digitraffic (CC BY 4.0) | credits shown in the notices |
| Live chess | Lichess TV | Lichess |
| Coastlines on the live maps | Natural Earth (public domain, embedded) | no network |

Ilium sends its own user agent with these requests, throttles per host and caches responses in the `ambient` folder of the platform cache directory.

## Troubleshooting

- **Scene status says offline or shows an error.** The network scenes keep the last good data. Check connectivity; the next successful refresh recovers automatically. For Overpass, wait out the 60-second spacing (or the provider's `Retry-After` cooldown) and fix the endpoint if it is rejected.
- **A live scene never updates faster than expected.** Raising Requested refresh is honoured, lowering it below the source floor is not. Aircraft are always at least 15 minutes apart.
- **Aircraft or boats look sparse.** Coverage is incomplete by nature of the provider (terrestrial receivers for OpenSky, reception for AIS). Digitraffic shows Finnish waters only.
- **OpenStreetMap shows "needs a configured Overpass service".** A new map point or a non-bundled tour list needs an Overpass endpoint; choose an offline list or set **Overpass service** (HTTPS, no credentials).
- **Place search fails.** Check **Place search**: Nominatim has no default endpoint and the public OSMF host is refused. Coordinates always work offline.
- **Saved maps show a status message instead of a world.** Check the 1.19.3 client is installed in the official Java folder, the world has `level.dat` and `region`, and its DataVersion is within 2834-3218 with fully saved chunks.
- **Stars look wrong.** Confirm the shared Location, and that Start from is Now (or a correct UTC time) and Time offset is 0.
- **Earth at night or clouds are empty on first use.** The first image has to download; the Scene status row shows progress. Imagery is typically a day (night lights) or about an hour (GOES) behind real time.
- **The Location row is missing.** It only appears for Stars overhead, Earth at night and Satellite clouds; OpenStreetMap uses Map location.
