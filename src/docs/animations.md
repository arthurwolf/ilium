# Background animations

Ilium can draw a slow Braille-dot animation behind your terminals: a quiet shoreline, a night sky, a map, a video, a game that plays itself. Animations are decoration only. They fill cells that would otherwise be blank, never change terminal history or the text Ilium copies, and are off by default. This page covers how to turn them on, how settings are saved, the playback and Semantic modes, the look and display controls shared by every animation, the full list of 59 animations and the bundled animation packages. Per-animation controls live in the three scene guides linked below.

## Contents

- [Quick start](#quick-start)
- [How the Animations settings tab is laid out](#how-the-animations-settings-tab-is-laid-out)
- [Saving: one global setting](#saving-one-global-setting)
- [Playback: Loop and Live](#playback-loop-and-live)
- [Semantic animation](#semantic-animation)
- [The shared look](#the-shared-look)
- [Shared dithering](#shared-dithering)
- [Shared display controls](#shared-display-controls)
- [All 59 animations](#all-59-animations)
- [Scene guides](#scene-guides)
- [Animation packages (.iliumanim)](#animation-packages-iliumanim)
- [Troubleshooting](#troubleshooting)

## Quick start

1. Open Settings with `Ctrl+B :` (the default prefix is `Ctrl+B`; see [Settings](settings.md) for remapping).
2. Select the **Animations** tab.
3. Choose a scene in the scene list on the left. Backgrounds are off until you also switch **Background** on.
4. Switch **Background** on. The animation now appears behind the tree panel and the terminal panes, in the blank cells only.
5. Press `f` (or click the **Full screen preview** row) to see the animation with every control hidden, exactly as it appears behind the workspace. Any key or click returns to the controls; the returning key does nothing else. The preview works even while **Background** is off.
6. Adjust the shared look (colour, brightness, dithering, panel) and the scene's own controls. Each change applies and saves immediately.

If the animation is too prominent, lower **Brightness** (down to 1%) or pick the **Whisper (barely visible)** style preset, and lower **Dot density**.

## How the Animations settings tab is laid out

The tab has two columns.

- Left: the scene list with previous/next navigation, then the shared rows (Background, Speed, Dot density, Dither, Frame rate cap, Show in, the shared look rows, pattern rows, Playback and loop rows).
- Right: the selected animation's own rows ("scene controls"), plus a scene status line for hosted scenes and a location row for the scenes that use one.

Row behaviour, shared by every scene control:

| Row type | How to change it |
| --- | --- |
| Slider | `Left`/`Right` adjusts by the control's step; click or drag on the track to choose a value directly. |
| Choice | `Left`/`Right` cycles the options. |
| Toggle | `Left`/`Right` or click switches it. |
| Text | `Enter` opens a prompt (path, URL, address); `Enter` confirms, `Esc` cancels. |

A greyed-out choice (for example the GPU renderer when no usable GPU exists) is marked unavailable. `Left`/`Right`, `Enter` and clicks skip it, and resting the pointer on the row explains why. Select a scene row and press `Enter` to jump to its controls. Every scene keeps its own saved values when you select another scene. A help line under the list explains the selected row.

The two source tabs at the top of the scene list are **Native** (the animations built into Ilium) and **Plugin** (installed `.iliumanim` packages). Switch with `Alt+Left` / `Alt+Right`, or click the tab. Browsing the Plugin tab never changes the active animation. See [Animation packages](#animation-packages-iliumanim).

**Scene status** (hosted scenes, Wikipedia and Semantic) is one line reporting what the running scene says about itself: a download in progress, a missing `ffmpeg`, no audio input, an unreadable folder. `OK` means nothing is wrong. The row is always shown for these scenes so the rows below it never shift when a message appears.

**Location** (Stars overhead, Earth at night, Satellite clouds, and OpenStreetMap's own map location) opens a picker. Type an address and press `Enter` to search, type `lat, lon` such as `48.857, 2.352` for coordinates, or click the world map. Nothing is saved until you confirm; `Esc` keeps the previous location. One location is shared by Stars, Earth at night and Satellite clouds. Address search sends the typed text to the geocoding service. See [Maps, space and live data](animations-maps-space-and-live-data.md).

## Saving: one global setting

Your animation choices are one global setting, saved as soon as you change them in `~/.config/ilium/animation/.ilium/config.yaml`, under the `animation:` key. Every project and session shares them; selecting another project never changes the animation. Only the Semantic animation varies per project, because it picks each project's scene from that project's latest restructuring recommendation. The first time a client starts without a global file it adopts the `animation:` block of the project it was launched in; other projects' old `animation:` blocks are no longer read. The `kind` value is the snake_case name from the [animation table](#all-59-animations) (for example `quiet_pond`). Each scene's settings are stored separately, so choosing another scene and coming back restores your values.

Global settings (providers, keys, sounds and so on) live in `~/.config/ilium/config.toml` on Linux and are unrelated to animation choices; see [Settings](settings.md).

Defaults when nothing is saved: Background off, scene **Wave washing up sand**, Speed 100%, Dot density 60%, Dither **Ordered (Bayer 8x8)**, Show in **Both panels**, frame-rate cap 0 (each scene's own rate), Playback **Loop** with a 60 second loop, Color mode **Color** with **Scene colors** and no preset.

## Playback: Loop and Live

The **Playback** row is shown only for the ten built-in procedural scenes that can be cached: Wave washing up sand, Moon over moving water, Clouds over a sleeping ridge, Hillside brushed by wind, Steam above a tea cup, Kelp in a gentle current, Water caustics on stone, Drifting cloud islands, Two gentle wave sources and Lily pads on a quiet pond. Every other animation (all hosted scenes, Wikipedia and Semantic) is always live and the row is hidden for them, because their output depends on data, processes, the wall clock or audio.

| Mode | What it does |
| --- | --- |
| Loop (default) | Plays a precomputed loop from RAM. Smooth and cheap while playing. |
| Live | Evaluates the scene at every frame. No loop cache is built. |

Loop details:

- **Loop seconds** is 1 to 120 seconds, default 60. The slider is exponential, so short loops are easy to fine-tune; arrow keys adjust one second at a time. The row appears only while Playback is Loop.
- Loop generation runs in a cancellable background worker at 30 frames per second. Until a replacement cache is complete after you change something, the live renderer may be used.
- The **Cache status** line shows resident packed-frame RAM against the projected RAM, then recomputation progress and an ETA. The projection uses the current terminal geometry.
- A 128 MiB packed-frame ceiling limits each cache. A request that would exceed it stays editable and plays in Live mode instead. Scene scratch space and whole-process RAM are counted separately.
- The cache stores packed Braille geometry only. Changing colour, palette, preset, filter, brightness or panel never rebuilds it.

## Semantic animation

**Semantic** is an animation choice that does not pick a scene itself. It shows the animation recommended during AI tree reorganization (see [Titles and instructions](titles-and-instructions.md) and [Inference and privacy](inference-and-privacy.md) for reorganization and the inference providers it needs).

1. In **Settings -> Animations**, select **Semantic**.
2. Switch **Background** on.
3. Choose the **Recommendation scope**.
4. Reorganize the project tree so recommendations exist.

| Scope | Behaviour |
| --- | --- |
| Project (default) | Uses the recommendation for the selected entry's project. |
| Entry | Uses the recommendation for the selected tree entry. Entries include panes, groups and split views. |

Every reorganization records recommendations including scene parameters: Paris work can use the offline Paris map, and pathfinding work can use Carpet's Snake. Changing the selection makes no extra AI request; the stored result is applied. If the selected target has no valid saved recommendation, the status line says so and asks you to reorganize the project.

Semantic is opt-in. It does not replace your authored animation settings or edit terminal content, and recommendations cannot set file paths, devices or credentials.

## The shared look

Colour, brightness, dithering, panel and frame rate are shared by all animations. A scene draws tone into a dot raster; the shared look then colours it. Changing any shared control changes every animation, and a scene must not add its own brightness, palette or saturation controls. Rows that do not apply to the current colour mode are hidden.

### Style presets

**Style preset** applies a ready-made look in one step: colour mode, palette, colour source, brightness, contrast, gamma, saturation, edge fade, grey tint, a colour filter with its strength, and for some presets a dither and a density. Editing any look value afterwards switches the row back to **Custom**; choosing **Custom** itself changes nothing. A preset that names no dither or density leaves yours alone. There are 58 entries: Custom plus 57 named presets, in menu order:

1. Custom
2. Original
3. Whisper (barely visible)
4. Subtle
5. Quiet grey
6. Soft pastel
7. Vivid
8. Neon night
9. Midnight blue
10. Ember glow
11. Matrix
12. Amber terminal
13. Paper and ink
14. Blueprint
15. Sunset haze
16. Deep forest
17. Aurora night
18. Vaporwave
19. High contrast
20. Ghost (dim monotone)
21. Retro print
22. Pastel dream
23. Pastel candy
24. Faded film
25. Matte mood
26. Sepia memory
27. Cyanotype print
28. Night shift
29. Candlelight
30. Moonlit
31. Dusk
32. Dawn
33. Ice cave
34. Inferno
35. Thermal camera
36. Night vision
37. Infrared
38. Noir
39. Silver screen
40. Game Boy
41. Hologram
42. Teal and orange
43. Sunset duotone
44. Synth duotone
45. Royal duotone
46. Mint fresh
47. Rose quartz
48. Lavender haze
49. Vintage photo
50. Polaroid
51. Bleach bypass
52. Cross process
53. Red glow (dim)
54. Amber glow (dim)
55. Blue hour (dim)
56. Focus dim (cool grey)
57. Negative
58. Poster pop

### Colour mode

| Mode | Behaviour |
| --- | --- |
| Color (default) | Uses a palette, or the scene's own colours with **Scene colors**. |
| Greyscale | Shades of grey, with an optional tint (**Grey tint hue** 0 to 359 degrees, default 40; **Grey tint** 0 to 100%, default 0). |
| Monotone | Every dot in one ink colour, set with **Ink lightness** (0 to 100%, default 60), **Ink hue** (0 to 359 degrees, default 210) and **Ink saturation** (0 to 100%, default 0, which keeps the dots neutral grey: 153,153,153 at the default lightness). |

### Palette

In Color mode, **Palette** picks the gradient colours come from. **Scene colors** (default) keeps what the animation paints itself; the others recolour it by brightness, and colour scenes that have no colours of their own. There are 38 palettes, in menu order:

1. Scene colors
2. Rainbow
3. Pastel rainbow
4. Cotton candy
5. Sea glass
6. Peach cream
7. Lavender haze
8. Neon
9. Vaporwave
10. Synthwave
11. Cyberpunk
12. Sunset
13. Sunrise
14. Ocean
15. Deep sea
16. Forest
17. Moss and stone
18. Autumn
19. Ember
20. Ice
21. Aurora
22. Viridis
23. Plasma
24. Inferno
25. Cividis (color-blind safe)
26. Turbo
27. Sepia
28. Warm paper
29. Amber terminal
30. Green phosphor
31. Blueprint
32. Solarized
33. Nord
34. Dracula
35. Gruvbox
36. Tokyo Night
37. Rose gold
38. Slate

### Colour from, reverse, shift and spread

| Row | Range, default | Purpose |
| --- | --- | --- |
| Color from | choice, **Scene or coverage** | What picks the position on the palette or grey ramp. Options: Scene or coverage, Flat, Dot coverage, Top to bottom, Left to right, Diagonal, Centre to edge, Drifting in time. |
| Reverse | off | Run the palette from its other end. |
| Palette shift | 0 to 100%, 0 | Rotate the palette by a share of its length; colours wrap round. |
| Palette spread | 25 to 400%, 100 | How much of the palette the whole source range covers. Below 100% shows a narrow slice; above 100% repeats the extremes sooner. |

These rows are hidden in Monotone mode; Palette itself is hidden in Greyscale mode.

### Tone controls

| Row | Range, default | Purpose |
| --- | --- | --- |
| Brightness | 1 to 200%, 100 | Overall brightness. Lower it to keep the background discreet; above 100% brightens. |
| Contrast | 0 to 200%, 100 | Spreads colours away from (above 100%) or toward (below 100%) mid-grey. Zero is flat mid-grey. |
| Gamma | 30 to 300%, 100 | Mid-tone curve: above 100% lifts the mid-tones, below deepens them, without moving black or white. |
| Color intensity | 0 to 200%, 100 | Saturation multiplier. Zero is grey. |
| Hue shift | -180 to 180 degrees, 0 | Rotates every colour round the colour wheel without changing brightness. |
| Invert colors | off | Replaces every colour by its opposite tone. Combine with Brightness to keep an inverted look discreet. |
| Edge fade | 0 to 100%, 0 | Soft vignette that darkens the animation toward the screen edges. |
| Pattern contrast | 50 to 200%, 100 | Sharpens or softens the scene's dot tones before dithering. Above 100% keeps only brighter dots; below fills in shadows. |
| Invert pattern | off | Swaps lit and unlit dots, like a photographic negative of the dot pattern. |

### Colour filters

**Color filter** is a true colour transform applied after the palette and tone settings, independent of the presets (a preset may select one). **Filter strength** (0 to 100%, default 100) blends the filter with the unfiltered colour; 0% leaves colours untouched. There are 46 entries (None plus 45 transforms):

1. None
2. Invert
3. Red filter
4. Green filter
5. Blue filter
6. Cyan filter
7. Magenta filter
8. Yellow filter
9. Amber filter
10. Sepia
11. Cyanotype
12. Night vision
13. Infrared
14. Thermal camera
15. Cool
16. Warm
17. Moonlight
18. Candlelight
19. Dusk
20. Dawn
21. Pastel
22. Faded film
23. Matte
24. Vivid pop
25. Noir
26. Silver
27. Solarize
28. Posterize
29. Game Boy
30. Cross process
31. Bleach bypass
32. Teal and orange
33. Duotone blue-orange
34. Duotone pink-teal
35. Duotone purple-gold
36. Hologram
37. Night shift (no blue)
38. Swap red and blue
39. Rotate channels
40. Ice
41. Fire
42. Mint
43. Rose
44. Lavender
45. Vintage
46. Polaroid

## Shared dithering

**Dither** decides how dot tones become on/off Braille dots. It applies to every scene that has no dither choice of its own (a few scenes also offer their own matrix choices, described in their sections). Default: **Ordered (Bayer 8x8)**. There are 15 methods:

| # | Method | Family |
| --- | --- | --- |
| 1 | Ordered (Bayer 8x8) | Ordered matrix |
| 2 | Stippled | Stable stippling |
| 3 | Coarse (Bayer 2x2) | Ordered matrix |
| 4 | Bayer 4x4 | Ordered matrix |
| 5 | Fine (Bayer 16x16) | Ordered matrix |
| 6 | Blue noise | Void-and-cluster blue noise tile |
| 7 | Gradient noise | Interleaved gradient noise |
| 8 | Halftone dots | Halftone screen |
| 9 | Scan lines | Line screen |
| 10 | Diagonal lines | Line screen |
| 11 | Crosshatch | Line screen |
| 12 | White noise | Random threshold |
| 13 | Floyd-Steinberg | Error diffusion |
| 14 | Atkinson | Error diffusion |
| 15 | Sierra Lite | Error diffusion |

Error diffusion (13 to 15) can shimmer on moving scenes; the matrix modes light about the requested fraction of dots. A style preset may change the dither and the dot density when you apply it.

## Shared display controls

| Row | Range, default | Purpose |
| --- | --- | --- |
| Background | off | Show the animation behind the workspace. The Full screen preview ignores this switch. |
| Speed | 25 to 300%, step 5, 100 | Multiplier on the scene rate. Stars and Solar system also have their own simulation-speed choices. |
| Dot density | 25 to 100%, step 5, 60 | How many of the scene's dots are drawn. Geometry stays at full Braille dot resolution. |
| Frame rate cap | 0 to 30 fps, 0 | Highest redraw rate. 0 lets each scene choose its own rate. A lower cap saves CPU on slow machines or batteries. |
| Show in | Both panels, Left panel only, Right panel only; default Both | Where the animation appears: behind both workspace panels, only behind the tree on the left, or only behind the terminal panes on the right. |

Some scenes also have their own frame-rate or speed rows (Wind and Carpet have a **Frame rate**, for instance); the shared cap is an upper limit on top of them.

## All 59 animations

Order below is the order of the animation list in Settings. The key is the value stored in `.ilium/config.yaml`. The last column says which page documents the scene controls. Scene names are exactly as shown in Settings.

| # | Animation | Key | What it shows | Documented in |
| --- | --- | --- | --- | --- |
| 1 | Wave washing up sand | `shoreline` | A diagonal wash with fine foam, wet sand and scattered grains. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 2 | Moon over moving water | `moonlit_water` | Crossing wavelets fracture a widening moonlit reflection. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 3 | Clouds over a sleeping ridge | `sleeping_ridge` | Layered clouds and valley mist drift over quiet ridges. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 4 | Hillside brushed by wind | `windy_hillside` | A dense meadow of fine blades and seed heads follows traveling gusts. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 5 | Steam above a tea cup | `tea_steam` | Fine translucent wisps curl above a rounded porcelain cup. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 6 | Kelp in a gentle current | `kelp` | Uneven clusters of tapering ribbons twist through layered currents. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 7 | Water caustics on stone | `stone_caustics` | A moving light web bends over domed stones, pebbles and sparse sand. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 8 | Drifting cloud islands | `cloudlets` | Soft cloud islands gather, join and separate as they drift. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 9 | Two gentle wave sources | `two_ripples` | Continuous outward crests brighten and dim where two wave fields meet. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 10 | Lily pads on a quiet pond | `quiet_pond` | Notched lily pads rest on a pond while fine reflections pass beneath. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 11 | 3D pipes | `pipes` | Dithered black-and-white pipes grow through 3D space, like the classic screensaver. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 12 | Stars overhead | `stars` | The real night sky above your location, right now, as a perfect star map. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 13 | Earth at night | `night_lights` | City lights seen from orbit, on a borderless map of the dark Earth. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 14 | Satellite clouds | `clouds` | Live weather-satellite clouds, global or over your location. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 15 | Video | `video` | Play video files, folders or URLs as dithered Braille. | [animations-media-and-games.md](animations-media-and-games.md) |
| 16 | Audio spectrum | `spectrum` | A spectrum analyzer of whatever your system is playing. | [animations-media-and-games.md](animations-media-and-games.md) |
| 17 | Images | `images` | Colored Braille images from files, folders or URLs, with slow pan and zoom. | [animations-media-and-games.md](animations-media-and-games.md) |
| 18 | Dithered water | `dither_water` | Bayer-dithered 1-bit water: drifting caustic bands, horizon fade and slow ripple rings. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 19 | Atlantic dusk | `atlantic_dusk` | Dithered sea and sky through a full day: drifting clouds, sinking sun, moon and stars. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 20 | Cube clock | `cube_clock` | A quiet clock beside a slowly turning dotted cube. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 21 | Box machine | `box_machine` | A generative machine of boxes and rails that slowly builds and rearranges itself. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 22 | Machine screen | `machine_screen` | A generative machine display of scanning patterns and glyph-like blocks. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 23 | Dithered fBm clouds | `fbm_clouds` | Domain-warped noise clouds thresholded into one-bit dots (software, slow-mo by default). | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 24 | Dithered waves | `dithered_waves` | Layered wave shader rendered on the CPU with ordered dithering (software, slow-mo by default). | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 25 | Dithr patterns | `dithr_patterns` | Many dithr-style animated patterns to choose from, with selectable dither algorithms (software, slow-mo by default). | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 26 | Hex expedition | `hex_expedition` | An endless explorer's hex map, generated as you watch: jungle, savanna, desert, arctic and volcanic lands with animated water, trees, smoke, fires and lava, panned by a slow camera. | [animations-media-and-games.md](animations-media-and-games.md) |
| 27 | Vector TD | `vector_td` | A tower defense that plays itself: an AI builds, upgrades and unlocks glowing vector towers against waves of monsters, level after level, across several maps. | [animations-media-and-games.md](animations-media-and-games.md) |
| 28 | Wikipedia | `wikipedia` | Today's Wikipedia articles, slowly scrolling as readable text or font-rendered Braille, with images and infoboxes. | [animations-media-and-games.md](animations-media-and-games.md) |
| 29 | Galactic empires | `galactic_empires` | Procedural star empires expand along hyperlanes, negotiate, fight and unify while a slow camera circles the galaxy. | [animations-media-and-games.md](animations-media-and-games.md) |
| 30 | Voxel landscape | `voxel_landscape` | A seeded isometric block world with forests, deserts, villages, caves and ravines, drifting past in monochrome or pastel dithering. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 31 | Solar system | `solar_system` | Eight planets orbit the Sun, with independent distance and size scales and simulated time. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 32 | Topographic maps | `topographic_maps` | Contour maps of Earth, the Moon, Mars, Venus, Mercury, Ceres and fictional worlds from real elevation surveys, drawn as Braille dots on a slowly panning map or turning globe. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 33 | Live graphs | `graph` | Public observations as Braille lines, bars or genuine OHLC candles, with selectable sources and time scales. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 34 | Digits of Pi | `pi` | Exact Pi digits as terminal text or real-font Braille, with scrolling and separate hues for each digit. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 35 | Live earthquakes | `earthquakes` | USGS events of every reported magnitude on a coastline map, with pulsing markers and magnitude labels. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 36 | Live aircraft | `aircraft` | OpenSky's reported airborne positions worldwide, with independent map and aircraft styling; anonymous updates every fifteen minutes. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 37 | Live boats | `boats` | Received AIS positions on a world coastline: broader OpenSeaFeed by default, with Finnish Digitraffic as an explicit alternative. Coverage is incomplete. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 38 | Live chess | `chess` | The featured Lichess TV game's actual positions with dithered piece silhouettes. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 39 | OpenStreetMap | `open_street_map` | Real OpenStreetMap streets, buildings, waterways, parks and railways around ten world places, drawn as Braille dots with fixed or panning cameras. | [animations-maps-space-and-live-data.md](animations-maps-space-and-live-data.md) |
| 40 | Carpet | `carpet` | Isometric hatch lines lift over hidden moving spheres and tubes: mouse hunters, Snake, Life, legal chess, Lichess TV, a DVD ball, planets and civil clocks. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 41 | Wind | `wind` | Dots blown by a fixed or rotating wind through the empty parts of your screen; scrolling and new text push them around. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 42 | Semantic | `semantic` | Use the animation recommended during tree reorganization for the selected project or entry. | this page |
| 43 | Northern lights | `aurora` | Luminous curtains sway above a dark horizon with adjustable hills and optional seeded trees. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 44 | Pollen in a sunbeam | `pollen` | Small drifting specks brighten only inside a slowly swaying shaft of sunlight. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 45 | Fireflies finding a rhythm | `fireflies` | Seeded wandering lights gradually gather into a common pulse and drift out of agreement. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 46 | Window sunlight | `window_sunlight` | One or several sheared window projections move across the field, with separate 2 x 2 or 2 x 3 panes and optional pollen. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 47 | Frost | `frost` | Fine branching ice grows and retreats around the screen edges or around the foreground character mask. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 48 | Distant lighthouse | `lighthouse` | A small dark lighthouse sweeps its light over short shimmering marks on the sea. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 49 | Paper-fold trace | `paper_fold` | An angular dragon-curve trace gradually folds and opens, pausing between movements. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 50 | Embroidery orbit | `embroidery` | A moving stitch progressively reveals a delicate geometric flower. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 51 | Prime constellations | `prime_constellations` | A slow illumination sweep reveals prime-number alignments on an Ulam spiral. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 52 | Turning wallpaper | `wallpaper` | Repeated geometric motifs rotate into temporary larger shapes. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 53 | Unfinished circle | `unfinished_circle` | Imperfect concentric arcs slowly turn and occasionally align their wandering gaps. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 54 | Threads through a needle | `needle_threads` | Drifting curved threads gather through one narrow opening before fanning apart. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 55 | Ink that hesitates | `hesitating_ink` | A gently curling stroke pauses, resumes, fades and begins again. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 56 | Crop circles | `crop_circles` | Several visible drawers progressively trace bounded geometric formations across a textured field. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 57 | Delayed reflection | `delayed_reflection` | A swaying curve has a reflected partner that follows slightly behind, with gentle distortion. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 58 | Almost touching | `almost_touching` | Two arcs approach, linger near one another and retreat without meeting. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |
| 59 | Hidden wheel | `hidden_wheel` | Orbiting dashes briefly light up to imply a wheel whose rim is never drawn. | [animations-nature-and-abstract.md](animations-nature-and-abstract.md) |

Notes:

- The key `breathing_mountain` is accepted as an alias of `quiet_pond` for older saved files.
- The scene "Lily pads on a quiet pond" is the single lily-pad animation (`quiet_pond`).
- Network scenes (Earth at night, Satellite clouds, OpenStreetMap, live data, Wikipedia, URL video and images) need network access; their status line explains what is missing. See [Inference and privacy](inference-and-privacy.md) for what leaves your machine.

## Scene guides

- [Nature and abstract](animations-nature-and-abstract.md): shoreline, water, clouds, quiet procedural scenes (aurora, fireflies, frost, crop circles and more), generative art, dithered shaders, 3D pipes, Wind and Carpet. Every control, range and default.
- [Maps, space and live data](animations-maps-space-and-live-data.md): sky, Earth, maps, solar system, galaxy and live public data feeds.
- [Media and games](animations-media-and-games.md): video, images, audio spectrum, Wikipedia and self-playing games.

Related: [Settings](settings.md), [Demos](demos.md) (the recorded animation gallery).

## Animation packages (.iliumanim)

Besides the built-in ("Native") scenes, Ilium can run animation packages: `.iliumanim` archives containing a `manifest.json`, an entry script (`entry.mjs`) and assets. Packages are browsed in **Settings -> Animations -> Plugin**.

Two official packages ship with every release, version 1.0.0:

| Package | Contents |
| --- | --- |
| `beach-1.0.0.iliumanim` | The native shoreline algorithms ported to standalone TypeScript. Settings mirror the shoreline controls: Style (classic or rich), Tide reach, Foam width, Sand grains, Wash cycle and the fifteen rich-style sliders. |
| `carpet-1.0.0.iliumanim` | The native Carpet algorithms ported to TypeScript, with all of Carpet's settings. Declares two capabilities: pointer input over the animation viewport and HTTP access to `https://lichess.org` (for the Lichess TV chess mode). |

Beach supports live and pre-rendered plans for both styles. Carpet supports both modes for Snake, Life, automated chess, DVD ball and planetary orbits; mouse hunters, Lichess TV and both clocks are live-only. Packages declare their own resource limits (heap, per-frame bytes, render time).

How packages are found and run:

1. Ilium searches three directories, in this order: the directory beside the animation helper (the bundled packages; keep the two `.iliumanim` files together with the `ilium`, `ilium-server` and helper executables), the `animation-plugins` folder under the Ilium data directory, and the `animation-plugins` folder under the Ilium config directory (on Linux `~/.local/share/ilium/animation-plugins` and `~/.config/ilium/animation-plugins`). The installer owns these directories; Ilium does not create them.
2. Only regular files are read: symlinks and nested directories are not followed. The catalogue is limited to 256 packages; bundled packages keep their slots. Invalid archives are reported as catalogue issues and never evaluated.
3. Browsing lists metadata only. Nothing is evaluated until you select a package, and activation re-opens and fully verifies the archive.
4. The archive's verified files are loaded into RAM on demand. Bundled JavaScript runs under V8 in a separate confined helper process (`ilium-animation-helper`); package files are not extracted into user directories.
5. If a package asks for a capability (such as network access to an origin), Ilium shows a protected permission review. The choices are **Allow this session**, **Always allow this scope**, **Deny this session** and **Always deny this scope**; the default is deny.
6. Each package keeps its own settings and mode when you switch between packages.

Plugin packages and the helper need Linux Bubblewrap (`/usr/bin/bwrap`) and a cgroup-v2 delegation that lets Ilium create its own child group; the Linux packages and the requirement are described in [Installation](installation.md) and [Building from source](building-from-source.md). The release process that builds and checks the packages is in [../../release/RELEASING.md](../../release/RELEASING.md).

### Authoring and packaging

Each animation is a separate TypeScript project with a `README.md`, `manifest.json`, `package.json`, `src/index.mts`, any relative `.mts` imports, and optional `assets/`. The Beach and Carpet projects share a sibling `sdk/` directory containing types, development dependencies and the package builder. Install those dependencies with `bun install --cwd ../sdk --frozen-lockfile` from either project, then run:

```sh
bun run check
bun run test
bun run package
```

The builder bundles relative imports into `entry.mjs` and writes `dist/<id>-<version>.iliumanim`, a ZIP-compatible archive with the manifest and inventoried assets. Every runtime file has a byte count and SHA256 digest; source tests are excluded. Copy the archive into an animation discovery directory listed above.

The entry exports `plan(settings, mode, environment)` and asynchronous `create(host, settings, accepted_plan)`. The plan declares the active mode's inputs, permissions and drawing format. The resulting scene renders into a host-leased frame and calls `frame.present()`; it must release frame references after each callback. `reconfigure` applies settings changes, and `dispose` releases scene-owned work. Scripts use Ilium's host APIs for approved inputs and HTTP requests; the runtime provides neither Node filesystem access nor browser DOM APIs. A package's declared playback modes describe its plans; usable playback also requires matching helper and host support.

## Troubleshooting

| Symptom | Likely cause and fix |
| --- | --- |
| Nothing appears | **Background** is off, or Show in is set to the other panel. Switch it on; try the Full screen preview (`f`). |
| Animation is distracting | Lower Brightness, raise Pattern contrast carefully, lower Dot density, or apply the Whisper preset. |
| Playback row is missing | The selected scene is live-only (hosted scenes, Wikipedia, Semantic). Only the ten built-in procedural scenes can loop. |
| Loop starts in Live mode | The projected cache would exceed 128 MiB. Shorten Loop seconds or shrink the terminal. |
| High CPU use | Set a Frame rate cap (for example 10 fps) or use Loop playback where available. |
| Semantic shows a "reorganize" status | No recommendation is stored for the selected project or entry. Reorganize the tree. |
| Video, Earth or map scenes show a status message | Read Scene status: missing `ffmpeg`/`ffprobe`, offline, or a download in progress. See the matching scene guide. |
| Plugin tab says no packages installed | The bundled `.iliumanim` files are not beside the animation helper. Reinstall or keep the release files together. |
