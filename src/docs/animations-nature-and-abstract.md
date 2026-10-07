# Animations: nature and abstract

This guide documents the quiet, mostly procedural animations: water and shore scenes, clouds and weather, light and growth scenes, geometric and mathematical art, dithered shaders, machine-like generative scenes, 3D pipes, Wind and Carpet. For every animation it lists what you see and every scene control with its id, range or options, default and meaning. The shared look (colours, palettes, dithering, brightness, panel, frame-rate cap) applies to all of them and is described once in [Background animations](animations.md); scene controls only cover what is specific to the scene.

## Contents

- [Using scene controls](#using-scene-controls)
- [Built-in scenes with cached loops](#built-in-scenes-with-cached-loops): Shoreline, Moonlit water, Sleeping ridge, Windy hillside, Tea steam, Kelp, Stone caustics, Cloudlets, Two ripples, Lily pads (Quiet pond)
- [Quiet procedural scenes](#quiet-procedural-scenes): Aurora, Pollen, Fireflies, Window sunlight, Frost, Lighthouse
- [Geometry and mathematics](#geometry-and-mathematics): Paper-fold trace, Embroidery orbit, Prime constellations, Turning wallpaper, Unfinished circle, Threads through a needle, Ink that hesitates, Crop circles, Delayed reflection, Almost touching, Hidden wheel
- [3D pipes](#3d-pipes)
- [Dithered scenes](#dithered-scenes): Dithered water, Atlantic dusk, Dithered fBm clouds, Dithered waves, Dithr patterns
- [Machines and clocks](#machines-and-clocks): Cube clock, Box machine, Machine screen
- [Wind](#wind)
- [Growth](#growth)
- [Carpet](#carpet)
- [Notes on the Lily pads scene](#notes-on-the-lily-pads-scene)
- [Troubleshooting](#troubleshooting)

## Using scene controls

1. Open Settings (`Ctrl+B :`) and select the **Animations** tab (see [Background animations](animations.md)).
2. Select the scene in the list and press `Enter` to jump to its controls on the right. Controls appear under the shared rows.
3. Sliders take `Left`/`Right` (by the step shown) or a click or drag on the track. Choices and toggles cycle with `Left`/`Right`.
4. Every change applies to the running scene and saves immediately to the project (`.ilium/config.yaml`). Each scene keeps its own values.

Conventions in the tables below:

- **Id** is the stable control id used in saved settings, Semantic recommendations and automation. Built-in scenes with four sliders use `scene_control_0` to `scene_control_3`, in the order shown.
- **Seed** controls choose a repeatable layout: the same seed always draws the same scene. A seed change starts a new variation.
- Values outside a range are clamped to the range. Percent values are plain percentages of the scene designer's reference size.
- Defaults are deliberately quiet so the animation stays a background.
- Scenes marked "loop-cached" support **Playback: Loop** and **Loop seconds**; the others are always live. See [Playback](animations.md#playback-loop-and-live).

## Built-in scenes with cached loops

These ten scenes are drawn by Ilium itself, are deterministic, and can be played from a precomputed loop (Playback **Loop**, default) or live. Their four main sliders use the ids `scene_control_0` to `scene_control_3`.

### Shoreline

Shown in Settings as **Wave washing up sand** (`shoreline`). A diagonal wash with fine foam, wet sand and scattered sand grains. Two styles exist: **Classic** (the original single wash) and **Rich** (the default for new projects), which adds layered swell trains, uneven foam, trailing lace and clinging foam. A project whose saved shoreline settings predate the style key keeps looking Classic until you change it.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Tide reach | `scene_control_0` | 50 to 150%, step 5 | 100% | Maximum extent of each wash up the sand. |
| Foam width | `scene_control_1` | 25 to 200%, step 5 | 75% | Width of the moving foam ribbon. |
| Sand grains | `scene_control_2` | 0 to 100%, step 5 | 20% | Intensity of the deterministic sand grains. |
| Wash cycle | `scene_control_3` | 6 to 30 s, step 1 | 12 s | Duration of a complete advance, hold and retreat. |
| Style | `shoreline_style` | Classic, Rich | Rich | Classic is the original single wash. Rich adds wave sets, uneven foam, trailing lace and clinging foam. |

The following fifteen sliders appear only while **Style** is **Rich**:

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Swell trains | `shoreline_wave_sets` | 1 to 4, step 1 | 3 | How many overlapping swell trains cross the water, each with its own wavelength, speed and direction. |
| Swell angle | `shoreline_swell_angle` | 0 to 60 degrees, step 5 | 20 | How obliquely the swell meets the beach. Larger angles let each wash roll along the shore instead of arriving all at once. |
| Set irregularity | `shoreline_set_irregularity` | 0 to 100%, step 5 | 50% | How unequal successive waves are. Zero repeats one identical wash; higher values mix small and large waves. |
| Big wave every | `shoreline_big_wave_every` | 2 to 8 waves, step 1 | 4 | Every this many waves a larger one runs further up the beach. No effect at zero irregularity. |
| Shore meander | `shoreline_meander` | 0 to 200%, step 10 | 100% | How much the wash line wanders, including beach cusps where the water runs further up in some places. |
| Water chop | `shoreline_chop` | 0 to 100%, step 5 | 45% | Short crossing ripples on top of the swell. |
| Foam unevenness | `shoreline_foam_unevenness` | 0 to 100%, step 5 | 60% | How much foam thickness varies along the line, from an even ribbon to thick clots and thin threads. |
| Foam breakup | `shoreline_foam_breakup` | 0 to 100%, step 5 | 40% | How much the foam splits into drifting fragments and lets water show through. |
| Trailing lace | `shoreline_lace` | 0 to 100%, step 5 | 55% | Thin foam threads the retreating water leaves behind on the sand before they fade. |
| Clinging foam | `shoreline_stick_amount` | 0 to 100%, step 5 | 50% | How many small foam bits cling to the wet sand as the wave recedes. |
| Foam linger | `shoreline_stick_linger` | 5 to 60%, step 5 | 20% | How long clinging foam lasts, as a share of one wash cycle, before it disappears. |
| Wet sand darkness | `shoreline_wet_darkness` | 0 to 100%, step 5 | 85% | How much darker sand looks while it is wet. |
| Wet sand memory | `shoreline_wet_memory` | 0 to 100%, step 5 | 50% | How long wet sand stays dark after the water leaves, even into the next wave. |
| Backwash streaks | `shoreline_backwash` | 0 to 100%, step 5 | 45% | Fine streaks running down the beach behind the retreating water. |
| Sparkle | `shoreline_sparkle` | 0 to 100%, step 5 | 30% | Tiny glints on wet sand and near-shore crests. |

The bundled `beach` animation package ([Background animations](animations.md#animation-packages-iliumanim)) offers the same controls as a plugin.

### Moonlit water

Shown as **Moon over moving water** (`moonlit_water`). Crossing fine wavelets break the moon reflection into irregular horizontal fragments that widen toward the viewer.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Wave strength | `scene_control_0` | 0 to 200%, step 5 | 100% | Strength of the wavelets that fracture the reflection. |
| Ripple scale | `scene_control_1` | 50 to 200%, step 5 | 100% | Size of the ripples. |
| Reflection width | `scene_control_2` | 25 to 200%, step 5 | 100% | Width of the moon's reflection on the water. |
| Moon size | `scene_control_3` | 50 to 150%, step 5 | 100% | Size of the moon. |

### Sleeping ridge

Shown as **Clouds over a sleeping ridge** (`sleeping_ridge`). Cloud banks and layered valley mist drift over a stationary ridge.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Cloud cover | `scene_control_0` | 0 to 100%, step 5 | 60% | How much of the sky is covered by cloud. |
| Valley mist | `scene_control_1` | 0 to 100%, step 5 | 40% | Amount of layered mist in the valleys. |
| Ridge height | `scene_control_2` | 50 to 150%, step 5 | 100% | Height of the ridge. |
| Cloud drift | `scene_control_3` | 25 to 200%, step 5 | 100% | Speed at which the clouds drift. |

### Windy hillside

Shown as **Hillside brushed by wind** (`windy_hillside`). Many fine, irregularly placed stems and seed heads follow shared gusts across a rounded hillside.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Plant density | `scene_control_0` | 25 to 200%, step 5 | 100% | How densely the meadow is planted. |
| Wind strength | `scene_control_1` | 0 to 200%, step 5 | 100% | Strength of the traveling gusts. 0% leaves the plants still. |
| Plant height | `scene_control_2` | 50 to 150%, step 5 | 100% | Height of the stems. |
| Gust breadth | `scene_control_3` | 50 to 200%, step 5 | 100% | Breadth of each gust as it crosses the hill. |

### Tea steam

Shown as **Steam above a tea cup** (`tea_steam`). A smooth porcelain cup with a hollow handle sits beneath curling steam filaments.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Cup size | `scene_control_0` | 60 to 140%, step 5 | 100% | Size of the cup. |
| Steam wisps | `scene_control_1` | 1 to 6, step 1 | 4 | Number of steam filaments. |
| Steam curl | `scene_control_2` | 25 to 200%, step 5 | 100% | How much the steam curls. |
| Steam height | `scene_control_3` | 50 to 150%, step 5 | 100% | How high the steam rises. |

### Kelp

Shown as **Kelp in a gentle current** (`kelp`). Uneven clusters of tapered kelp ribbons twist and sway, with varied lengths and stronger motion near their tips. No ground line is drawn.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Plant density | `scene_control_0` | 25 to 200%, step 5 | 100% | How many ribbons grow. |
| Current strength | `scene_control_1` | 0 to 200%, step 5 | 100% | Strength of the current that sways the kelp. |
| Ribbon length | `scene_control_2` | 50 to 175%, step 5 | 100% | Length of the ribbons. |
| Clustering | `scene_control_3` | 0 to 100%, step 5 | 65% | How strongly plants gather into clusters. |

### Stone caustics

Shown as **Water caustics on stone** (`stone_caustics`). A moving light web bends over domed stones. Large and small stones gather in irregular clusters with sparse sand between them.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Stone relief | `scene_control_0` | 25 to 200%, step 5 | 100% | How strongly the stones dome up. |
| Light web scale | `scene_control_1` | 50 to 200%, step 5 | 100% | Size of the moving caustic web. |
| Small stones | `scene_control_2` | 0 to 100%, step 5 | 65% | Amount of small stones among the large ones. |
| Sand grains | `scene_control_3` | 0 to 100%, step 5 | 20% | Amount of sand between the stones. |

### Cloudlets

Shown as **Drifting cloud islands** (`cloudlets`). Soft rounded islands drift, join and separate.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Cloud islands | `scene_control_0` | 2 to 10, step 1 | 6 | Number of cloud islands. |
| Island size | `scene_control_1` | 50 to 175%, step 5 | 100% | Size of each island. |
| Joining softness | `scene_control_2` | 25 to 200%, step 5 | 100% | How softly neighbouring islands merge. |
| Drift breadth | `scene_control_3` | 25 to 150%, step 5 | 100% | How far the islands wander. |

### Two ripples

Shown as **Two gentle wave sources** (`two_ripples`). Faster radial wave crests cross and interfere while remaining visible through destructive interference.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Source separation | `scene_control_0` | 10 to 90%, step 2 | 54% | Distance between the two wave sources. |
| Wavelength | `scene_control_1` | 50 to 200%, step 5 | 100% | Distance between successive crests. |
| Interference | `scene_control_2` | 0 to 100%, step 5 | 65% | Strength of the interference pattern where the waves cross. |
| Damping | `scene_control_3` | 0 to 100%, step 5 | 25% | How quickly the waves fade with distance. |

### Lily pads (Quiet pond)

Shown as **Lily pads on a quiet pond** (`quiet_pond`; the older name `breathing_mountain` is accepted as an alias). Notched lily pads with fine veins float above a gently moving water surface, with fine reflections passing beneath. This is the lily-pad scene: there is no separate Lily pads animation.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Lily pads | `scene_control_0` | 3 to 64, step 1 | 7 | Number of opaque leaves. Up to 64 are supported. |
| Pad size | `scene_control_1` | 50 to 175%, step 5 | 100% | Size of each pad. |
| Water ripples | `scene_control_2` | 0 to 100%, step 5 | 70% | Strength of the ripples on the water. |
| Surface drift | `scene_control_3` | 25 to 200%, step 5 | 100% | Drift speed of the water surface. |
| Rooted placement | `natural_placement` | on / off | off | Cluster leaves around underwater root groups, with bounded petiole reach and spacing, instead of spreading them freely. |

## Quiet procedural scenes

These scenes are hosted by the ambient engine and are always live (no Loop cache). All are bounded, procedural and seeded.

### Aurora

Shown as **Northern lights** (`aurora`). Luminous curtains sway above a dark horizon. Horizon height and roughness change the dark cutout; optional seeded trees add a skyline without imported assets, and the ground and trees carry no sky ink.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Ground height | `horizon_height` | 10 to 65%, step 1 | 28% | Ink-free ground measured upward from the lower edge. |
| Horizon roughness | `horizon_roughness` | 0 to 100, step 1 | 36 | Seeded low hills; zero gives a level horizon. |
| Curtains | `curtain_count` | 2 to 10, step 1 | 6 | Overlapping bands with separate folds and crests. |
| Curtain motion | `curtain_motion` | 0 to 100, step 1 | 38 | Slow folding; zero holds the same geometry at every time. |
| Tree silhouettes | `trees` | on / off | on | Dark seeded trees rise just above the horizon without sky ink on them. |
| Tree density | `tree_density` | 0 to 100, step 1 | 55 | Zero leaves the horizon clear, even when trees are enabled. |
| Seed | `seed` | 0 to 9999, step 1 | 1 | Repeatable skyline and tree placement. |

### Pollen

Shown as **Pollen in a sunbeam** (`pollen`). Small drifting specks brighten only inside a slowly swaying shaft of sunlight. Beam angle, width, location and motion define the light mask. A count of zero removes the pollen while keeping the beam.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Pollen count | `count` | 0 to 128, step 1 | 40 | Small seeded population; zero leaves only the sunbeam. |
| Beam angle | `beam_angle` | -80 to 80, step 1 | 25 | Tilt from vertical in degrees. |
| Beam width | `beam_width` | 5 to 70, step 1 | 24 | Width of the shaft as a percentage of the field. |
| Beam location | `beam_location` | 0 to 100, step 1 | 50 | Horizontal center of the beam. |
| Beam motion | `beam_motion` | 0 to 100, step 1 | 15 | Slow sideways sway; zero holds the beam still. |
| Drift | `drift` | 0 to 100, step 1 | 35 | Speed of drifting pollen; zero freezes particle positions. |
| Seed | `seed` | 0 to 9999, step 1 | 1 | Selects repeatable pollen positions. |

### Fireflies

Shown as **Fireflies finding a rhythm** (`fireflies`). Seeded wandering lights gradually gather into a common pulse and drift out of agreement. Population, drift, pulse period and synchronization adjust the wandering lights and their shared rhythm.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Firefly count | `count` | 4 to 128, step 1 | 36 | Bounded population of independent luminous dots. |
| Drift | `drift` | 0 to 100, step 1 | 30 | Size of slow wandering paths; zero fixes positions. |
| Pulse period | `pulse_period` | 1 to 12, step 1 | 4 | Seconds between shared flashes. |
| Synchronization | `synchronization` | 0 to 100, step 1 | 80 | Strength of the gradual gathering into a common rhythm and separation. |
| Seed | `seed` | 0 to 9999, step 1 | 1 | Selects positions and individual pulse rhythms. |

### Window sunlight

Shown as **Window sunlight** (`window_sunlight`). One to four sheared window projections move across the field, each with separate 2 x 2 or 2 x 3 panes and optional pollen. Pane gaps stay dark, including when pollen is enabled; pollen shows only inside lit panes.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Windows | `window_count` | 1 to 4, step 1 | 1 | Independent projections arranged across the field. |
| Window size | `size` | 15 to 90, step 1 | 65 | Projection width within each window slot. |
| Window height | `height` | 15 to 85, step 1 | 48 | Height of each projected window. |
| Pane gap | `gap` | 3 to 35, step 1 | 12 | Dark separation between panes; pollen never lights in these gaps. |
| Horizontal location | `location_x` | 0 to 100, step 1 | 50 | Moves the entire group of projections sideways. |
| Vertical location | `location_y` | 10 to 90, step 1 | 50 | Moves projections up or down. |
| Projection slant | `shear` | -60 to 60, step 1 | 20 | Horizontal skew from the top to bottom of each projection. |
| Sun motion | `motion` | 0 to 100, step 1 | 20 | Slow movement and stretching of the projections. |
| Pollen count | `pollen_count` | 0 to 128, step 1 | 40 | Small seeded population revealed only inside illuminated panes. |
| Seed | `seed` | 0 to 9999, step 1 | 1 | Selects pollen paths and subtle pane texture. |
| Pane grid | `grid` | 2 x 2, 2 x 3 | 2 x 2 | Two columns with two or three rows of separate light panes. |
| Pollen | `pollen` | on / off | off | Reveal drifting pollen only within lit panes, preserving dark mullions. |

Note: **Pollen** is off by default. **Pollen count** only matters while Pollen is on.

### Frost

Shown as **Frost** (`frost`). Fine branching ice grows and retreats around the screen edges or around the foreground characters. In **Frame edges** mode the ice follows the border; in **Foreground characters** mode it grows beside visible text using only which cells are occupied. Frost never reads or edits terminal text.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Growth source | `mode` | Frame edges, Foreground characters | Frame edges | Start at the outer frame or beside visible text, without reading its content. |
| Reach | `reach` | 2 to 20 cells, step 1 | 9 cells | Maximum distance a crystalline branch travels from its source. |
| Branching | `branching` | 0 to 3, step 1 | 2 | Small side twigs along each cached main branch. |
| Growth time | `growth_seconds` | 3 to 30 s, step 1 | 10 s | Grow, hold briefly, retreat, then begin again. |
| Seed | `seed` | 0 to 9999, step 1 | 1 | Repeatable branch placement and crystalline bends. |

### Lighthouse

Shown as **Distant lighthouse** (`lighthouse`). A small dark lighthouse sweeps its light over short shimmering marks on the sea. Horizon, beam spread, sweep, speed and water glints control the simple coast and sea.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Horizon | `horizon` | 25 to 80, step 1 | 55 | Sea horizon as a percentage from the top. |
| Lighthouse location | `location` | 5 to 95, step 1 | 18 | Horizontal position of the simple coastal silhouette. |
| Beam width | `beam_width` | 3 to 40, step 1 | 12 | Angular spread of the sweeping light in degrees. |
| Sweep range | `sweep` | 10 to 85, step 1 | 65 | Maximum sweep above and below the horizon in degrees. |
| Sweep speed | `speed` | 1 to 100, step 1 | 20 | Speed of the returning sweep. |
| Water glints | `water_glints` | 0 to 100, step 1 | 55 | Strength of short reflected marks; zero hides them. |
| Seed | `seed` | 0 to 9999, step 1 | 1 | Selects the repeatable water glint pattern. |

## Geometry and mathematics

Bounded procedural drawings. Each scene's controls adjust its own geometry and motion independently of every other animation. All are always live.

### Paper-fold trace

Shown as **Paper-fold trace** (`paper_fold`). An angular dragon-curve trace gradually folds and opens, pausing between movements.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Fold depth | `depth` | 3 to 10, step 1 | 8 | Each additional fold doubles the path, up to 1024 segments. |
| Fold cycle | `cycle_seconds` | 12 to 120 s, step 1 | 40 s | Seconds for unfolding, resting, refolding and resting. |
| Path size | `scale` | 30 to 100, step 1 | 85 | Percentage of available space used by the trace. |

### Embroidery orbit

Shown as **Embroidery orbit** (`embroidery`). A moving stitch progressively reveals a delicate geometric flower (a Maurer rose drawn as straight chords).

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Rose parameter | `petals` | 2 to 12, step 1 | 6 | Integer frequency of the Maurer rose. |
| Stitch angle | `step_degrees` | 1 to 179, step 1 | 71 | Angular step between consecutive chords. |
| Stitch count | `stitches` | 60 to 720, step 1 | 360 | Maximum cached chords in the embroidery. |
| Drawing cycle | `cycle_seconds` | 12 to 120 s, step 1 | 45 s | Seconds for drawing, resting and fading. |

### Prime constellations

Shown as **Prime constellations** (`prime_constellations`). A slow illumination sweep reveals prime-number alignments on an Ulam spiral.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Spiral width | `grid_size` | 17 to 65, step 2 | 35 | Odd grid width; at most 4225 cached number positions. |
| Sweep period | `sweep_seconds` | 5 to 90 s, step 1 | 18 s | Time for diagonal illumination to cross the number spiral. |
| Faint dots | `background` | 0 to 40, step 1 | 12 | Brightness of nonprime points, in percent. |

### Turning wallpaper

Shown as **Turning wallpaper** (`wallpaper`). Repeated geometric motifs rotate into temporary larger shapes.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Repeat columns | `columns` | 3 to 18, step 1 | 8 | Number of repeating motifs across the viewport. |
| Rotation period | `rotation_seconds` | 8 to 120 s, step 1 | 36 s | Time for each tile motif to rotate once. |
| Motif | `motif` | Petals, Diamonds, Pinwheels | Petals | Choose petals, diamonds or pinwheels. |
| Symmetry | `symmetry` | Repeat, Mirror, Quarter turns | Mirror | Repeat identically, alternate mirror or alternate quarter turns. |

### Unfinished circle

Shown as **Unfinished circle** (`unfinished_circle`). Imperfect concentric arcs slowly turn and occasionally align their wandering gaps.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Ring count | `rings` | 1 to 12, step 1 | 5 | Concentric rings with independent drifting gaps. |
| Gap width | `gap_degrees` | 15 to 150, step 1 | 65 | Angular width of each missing arc. |
| Imperfection | `wobble` | 0 to 40, step 1 | 12 | Small radial ripples as a percentage of ring spacing. |
| Gap orbit | `turn_seconds` | 8 to 120 s, step 1 | 35 s | Seconds for the missing sections to orbit. |

### Threads through a needle

Shown as **Threads through a needle** (`needle_threads`). Drifting curved threads gather through one narrow opening before fanning apart.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Thread count | `threads` | 2 to 24, step 1 | 9 | Number of curves constrained through the shared eye. |
| Eye position | `opening_x` | 20 to 80, step 1 | 50 | Horizontal position of the shared opening, in percent. |
| Fan spread | `spread` | 10 to 90, step 1 | 65 | Vertical extent of the loose threads. |
| Sway period | `sway_seconds` | 6 to 90 s, step 1 | 24 s | Seconds for one gentle thread sway. |

### Ink that hesitates

Shown as **Ink that hesitates** (`hesitating_ink`). A gently curling stroke pauses, resumes, fades and begins again.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Curl count | `loops` | 2 to 12, step 1 | 5 | Number of curls in the bounded wandering stroke. |
| Drawing cycle | `cycle_seconds` | 12 to 120 s, step 1 | 40 s | Time for drawing, hesitating, resting and fading. |
| Pause strength | `hesitation` | 0 to 85, step 1 | 55 | Time held still within each curl. Zero draws continuously. |
| Path seed | `seed` | 0 to 9999, step 1 | 7 | Stable seed changes the curl phases and amplitudes. |

### Crop circles

Shown as **Crop circles** (`crop_circles`). Several visible drawers progressively trace bounded geometric formations across a textured, synthetic field. Choose a procedural arrangement or geometric adaptations of two documented formations (Milk Hill and Barbury); multiple drawers trace them simultaneously.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Pattern | `pattern` | Procedural, Milk Hill six arms, Barbury triangle | Procedural | A new arrangement or geometric adaptations of two documented formations. |
| Seed | `seed` | 0 to 9999, step 1 | 1 | Changes the invented arrangement and small reference variations. |
| Visible drawers | `drawers` | 2 to 8, step 1 | 3 | Independent tips advance concurrently on different assigned strokes. |
| Drawing speed | `drawing_speed` | 10 to 100%, step 1 | 45% | Shortens or lengthens the shared drawing interval. |
| Formation scale | `scale` | 35 to 100%, step 1 | 78% | Fits circles to terminal aspect without stretching them. |

### Delayed reflection

Shown as **Delayed reflection** (`delayed_reflection`). A swaying curve has a reflected partner that follows slightly behind, with gentle distortion.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Reflection delay | `delay_seconds` | 0 to 15 s, step 1 | 3 s | Seconds by which the lower ribbon follows the upper ribbon. |
| Waterline | `boundary` | 35 to 65, step 1 | 52 | Vertical position of the reflecting boundary, in percent. |
| Ripple distortion | `distortion` | 0 to 50, step 1 | 15 | Horizontal ripple amplitude of the reflected curve. |
| Sway period | `sway_seconds` | 6 to 90 s, step 1 | 22 s | Time for the original ribbon to sway once. |

### Almost touching

Shown as **Almost touching** (`almost_touching`). Two arcs approach, linger near one another and retreat without meeting.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Closest gap | `minimum_gap` | 1 to 20, step 1 | 4 | Minimum distance between tips, as a percentage of width. |
| Arc reach | `reach` | 20 to 90, step 1 | 60 | How far the arc bodies curl away from their tips. |
| Approach cycle | `cycle_seconds` | 8 to 100 s, step 1 | 26 s | Seconds for approach, lingering and retreat. |
| Near-contact linger | `dwell` | 0 to 85, step 1 | 55 | Higher values slow the tips near closest approach. |

### Hidden wheel

Shown as **Hidden wheel** (`hidden_wheel`). Orbiting dashes briefly light up to imply a wheel whose rim is never drawn.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Rim marks | `marks` | 12 to 96, step 1 | 40 | Short orbiting dashes suggesting an invisible wheel. |
| Visible sector | `visible_arc` | 20 to 95, step 1 | 65 | Percentage of the rim where marks can appear. |
| Turn period | `turn_seconds` | 8 to 120 s, step 1 | 32 s | Seconds for one wheel rotation. |
| Dash length | `dash_length` | 15 to 90, step 1 | 55 | Percentage of each mark spacing occupied by ink. |

## 3D pipes

Shown as **3D pipes** (`pipes`). Dithered black-and-white pipes grow through 3D space like the classic screensaver while the camera orbits. When the volume is full (or on a timer) the scene fades and a new layout starts. Always live.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Pipes | `pipe_count` | 1 to 16, step 1 | 5 | How many pipes grow at the same time. A pipe that gets stuck restarts elsewhere. |
| Grid size | `volume_size` | 4 to 16 cells, step 1 | 8 | Cells per side of the cubic volume. Larger grids mean thinner-looking pipes and a longer build. |
| Thickness | `pipe_thickness` | 10 to 60%, step 5 | 45% | Pipe diameter as a share of the grid spacing. |
| Turn chance | `turn_chance` | 0 to 100%, step 5 | 40% | Chance that a pipe turns at each grid point. 0% makes straight runs until a wall or another pipe. |
| Growth speed | `growth_speed` | 1 to 30 cells/s, step 1 | 5 | How many grid cells each pipe grows per second. |
| Orbit speed | `orbit_speed` | 0 to 100%, step 5 | 25% | How fast the camera circles the structure (100% is 20 degrees per second). 0% holds the camera still. |
| Field of view | `field_of_view` | 20 to 90 degrees, step 5 | 45 | Vertical viewing angle. Wide angles exaggerate perspective. |
| Joints | `joint_style` | Ball, Rounded, None | Ball | Ball spheres at every turn and end, rounded elbows, or bare mitered corners. |
| Shading | `shading` | Soft lit, Flat, High contrast | Soft lit | Soft lit gives smooth gradients, flat one tone per pipe, high contrast a bold halftone look. |
| Pattern | `pattern` | Plain, Rings, Checker | Plain | Optional dark rings or checker tiles on the pipe surface. |
| Reset | `reset_mode` | When full, Every N seconds | When full | Start a new layout when the volume is full (after a short hold), or on a fixed period. |
| Reset period | `reset_seconds` | 10 to 600 s, step 10 | 90 | Seconds between restarts. Shown only when Reset is Every N seconds. |
| Random seed | `seed_mode` | New each run, Fixed seed | New each run | New each run picks different layouts every time; a fixed seed repeats the same sequence. |
| Seed number | `seed` | 0 to 9999, step 1 | 1 | Selects the sequence of layouts. Shown only when Random seed is Fixed seed. |

## Dithered scenes

These scenes do their own one-bit dithering in the scene (or choose a dither pattern of their own) in addition to the shared Dither row; they are always live. Software rendering runs on the CPU in gentle slow motion by default. Where a **Renderer** row exists, **GPU** uses the graphics card when one is usable and is otherwise greyed out, with a reason shown when you rest the pointer on it.

### Dithered water

Shown as **Dithered water** (`dither_water`). Bayer-dithered 1-bit water: drifting caustic bands, a fading horizon and slow ripple rings. By default lit dots are coloured from deep blue to pale cyan.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Wave size | `wave_scale` | 50 to 300%, step 10 | 100% | Spatial frequency of the waves. Higher values give smaller, busier ripples. |
| Flow speed | `flow_speed` | 20 to 300%, step 10 | 100% | How fast the light bands drift and shimmer, on top of the global Speed setting. |
| Sharpness | `sharpness` | 1 to 6, step 1 | 3 | Higher values narrow the bright caustic lines and leave more dark water between them. |
| Density | `density` | -30 to 30%, step 2 | 0% | Shifts the black and white balance. Positive values switch more dots on. |
| Brightness | `brightness` | 15 to 80%, step 5 | 45% | Below the default, lit dots thin out. Above it, the blue tint gets brighter. |
| Perspective | `perspective` | 0 to 100%, step 5 | 50% | Squeezes the waves toward a horizon and fades the far water. 0% is a flat top-down surface. |
| Dither | `dither` | 2x2 chunky, 4x4 classic, 8x8 fine | 4x4 classic | Size of the fixed threshold matrix. Coarser matrices look chunkier. |
| Ripples | `ripples` | on / off | on | Draws slowly expanding elliptical rings on top of the waves. |
| Stepped motion | `stepped` | on / off | off | Advances the water in eight steps per second for a hand-animated pixel-art stutter. |
| Blue tint | `tint` | on / off | on | Colours lit dots from deep blue to pale cyan. Off uses your global palette. |
| Seed number | `seed` | 0 to 999, step 1 | 7 | Selects the wave phases and where ripples appear. |

### Atlantic dusk

Shown as **Atlantic dusk** (`atlantic_dusk`). A dithered sea and sky through a full day: drifting clouds, a sinking sun, then moon, stars and sparse wave glints.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Time of day | `time_of_day` | Cycle, Dawn, Noon, Dusk, Night | Cycle | Cycle animates the whole day. The other choices freeze the light at dawn, noon, dusk or night while clouds and waves keep moving. |
| Day length | `day_length_seconds` | 30 to 600 s, step 10 | 120 s | Seconds for one full day: day, dusk, night and dawn. Used only while Time of day is Cycle. |
| Cloud coverage | `cloud_coverage` | 0 to 100%, step 5 | 45% | How much of the sky is clouded. Clouds are brighter than the sky by day and darker at dusk. |
| Cloud speed | `cloud_speed` | 0 to 300%, step 10 | 100% | How fast the clouds drift sideways. 0 freezes them. |
| Wave speed | `wave_speed` | 0 to 300%, step 10 | 100% | How fast the wave lines roll. 0 freezes the sea. |
| Wave spacing | `wave_scale` | 50 to 200%, step 10 | 100% | Distance between wave lines. Small values give a choppier, busier sea. |
| Stars | `star_density` | 0 to 80 per thousand sky cells, step 5 | 30 | How many stars appear at night. 0 gives a starless night. |
| Contrast | `contrast` | 20 to 100%, step 5 | 55% | Overall brightness of the picture. The default keeps it quiet enough for a background. |
| Star twinkle | `twinkle` | on / off | on | Lets some single-dot stars blink out briefly, rarely and subtly. |
| Dither | `dither` | Host dither, Bayer 8x8, Bayer 4x4 | Host dither | Host dither hands soft gradients to the terminal dither. The Bayer options dither in the scene for a stepped, Playdate-like look. |
| Seed | `seed` | 0 to 999, step 1 | 41 | Chooses the cloud pattern and star positions. |

### Dithered fBm clouds

Shown as **Dithered fBm clouds** (`fbm_clouds`). Domain-warped noise clouds thresholded into one-bit dots. Runs in software with slow-motion defaults; a GPU renderer choice is offered.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Cloud scale | `scale` | 50 to 300%, step 10 | 100% | Larger values pack in smaller, busier clouds; smaller values give a few huge banks. |
| Fold speed | `drift` | 0 to 400%, step 10 | 100% | How fast the cloud folds slowly morph into each other. 0% freezes the folding. |
| Pan | `pan` | -40 to 40, step 1 | 5 | Sideways drift of the whole sky. Negative values move it the other way, 0 holds it still. |
| Warp | `warp` | 0 to 150%, step 10 | 100% | How much clouds displace themselves. 0% gives plain fractal noise, 100% the swirling look. |
| Detail | `octaves` | 2, 3, 4 | 4 | Noise octaves. Fewer octaves are smoother and cheaper, more add fine wisps. |
| Contrast | `contrast` | 50 to 300%, step 10 | 120% | Steepness of the dither response: lower is a softer, grainier sky. |
| Pixel size | `block` | 1 dot, 2 dots, 3 dots, 4 dots | 2 dots | Size of the chunky blocks the clouds are sampled at. Larger blocks are coarser and cheaper. |
| Dither | `dither` | Bayer 8x8, Gradient noise, White noise | Bayer 8x8 | Pattern of the one-bit threshold: regular crosshatch, even grain or rough stipple. |
| Brightness | `brightness` | 5 to 100%, step 5 | 35% | Intensity of lit dots. Keep it low for a quiet background. |
| Invert | `invert` | on / off | off | Swap lit and dark: dark clouds on a dotted field. |
| Seed | `seed` | 0 to 999, step 1 | 0 | Selects the cloud layout. |
| Renderer | `render_backend` | Software (slow-mo), GPU | Software (slow-mo) | Software runs on the CPU in gentle slow motion. GPU uses the graphics card when one is usable; otherwise the option is greyed out. |

### Dithered waves

Shown as **Dithered waves** (`dithered_waves`). A layered wave shader rendered on the CPU with ordered dithering, slow-motion by default.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Wave frequency | `wave_frequency` | 100 to 800%, step 25 | 300% | How many wave crests fit on screen. Higher values give narrower, busier bands. |
| Wave amplitude | `wave_amplitude` | 0 to 100%, step 5 | 30% | How strongly noise bends the bands. 0% gives smooth parallel swells, 100% churning silk. |
| Wave speed | `wave_speed` | 0 to 200%, step 5 | 50% | How fast the waves drift, on top of the global Speed setting. 0% freezes them. |
| Dither pattern | `dither_matrix` | Bayer 2x2, Bayer 4x4, Bayer 8x8, Noise | Bayer 4x4 | The fixed dot lattice the waves are cut into. Bayer 8x8 is finest, Noise looks like grain. |
| Gray levels | `levels` | 2 to 6, step 1 | 2 | Number of intensity steps. 2 gives pure on/off dots; more steps soften the bands. |
| Dither pixel | `pixel_size` | 1 to 4 dots, step 1 | 1 | Edge length of one dither pixel in Braille dots. Larger values look chunkier. |
| Brightness | `brightness` | 10 to 100%, step 5 | 35% | Peak dot intensity. The default keeps the background quiet behind text. |
| Contrast | `contrast` | 50 to 300%, step 10 | 150% | Sharpness of the bands: low values fill the screen with soft dots, high values isolate crests. |
| Dot bias | `bias` | -30 to 30%, step 2 | 0% | Shifts the dither threshold. Positive values add dots everywhere, negative values thin them out. |
| Ripple | `ripple` | on / off | off | A faint ring pattern that spreads from a slowly wandering point. |
| Layout seed | `seed` | 0 to 9999, step 1 | 1 | Picks a different wave layout. The same seed always draws the same waves. |
| Renderer | `render_backend` | Software (slow-mo), GPU | Software (slow-mo) | Software runs on the CPU in gentle slow motion; GPU uses the graphics card when usable. |

### Dithr patterns

Shown as **Dithr patterns** (`dithr_patterns`). Many dithr-style animated patterns to choose from, each looping seamlessly, with selectable dither algorithms.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Pattern | `pattern` | Caustics, Cellular, Halftone, Starfield, Moire, Smoke, Spiral, Tunnel, Plasma, Ripples | Caustics | The animated field that is dithered into dots. Every pattern loops seamlessly. |
| Dither | `dither` | Ordered (Bayer), Floyd-Steinberg, Random noise, Threshold | Ordered (Bayer) | Ordered is a regular screen, Floyd-Steinberg organic grain, random noise a slow twinkle, threshold flat shapes. |
| Density | `density` | 0 to 100, step 1 | 54 | Brightness bias. 50 is neutral; higher fills more dots, lower leaves only the brightest parts. |
| Complexity | `complexity` | 1 to 8, step 1 | 5 | Noise octaves, spiral arms, tunnel segments, star count or drop count, depending on the pattern. |
| Scale | `scale` | 20 to 200%, step 2 | 80% | Spatial frequency of the pattern. Higher means finer detail. |
| Dither amount | `dither_amount` | 0 to 100%, step 5 | 70% | 0% cuts the field at one level (banded); 100% applies the full dither texture. |
| Loop length | `loop_seconds` | 4 to 60 s, step 1 | 12 s | Animation time after which the pattern repeats exactly. Shorter loops move faster. |
| Contrast | `contrast` | 10 to 100%, step 5 | 40% | Brightest dot intensity. Keep it low for quiet background art. |
| Bayer matrix | `matrix` | 4 x 4, 8 x 8 | 8 x 8 | Matrix size of the ordered dither; 4 x 4 is coarser, 8 x 8 smoother. |
| Seed | `seed` | 0 to 999, step 1 | 7 | Selects the noise layout, star positions and drop centres. |
| Invert | `invert` | on / off | off | Draw dots on the dark parts of the pattern instead of the bright parts. |
| Renderer | `render_backend` | Software (slow-mo), GPU | Software (slow-mo) | Software is the full-quality slow-motion renderer. GPU uses the graphics card when one is usable. |

## Machines and clocks

### Cube clock

Shown as **Cube clock** (`cube_clock`). A quiet clock beside a slowly turning dotted cube, with an optional ring of 60 faint dots for the minute. The time zone is estimated from your shared location (15 degrees of longitude per hour, no daylight saving) unless you set an offset by hand.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Hour format | `hour_format` | 24 hours, 12 hours | 24 hours | 24 hours shows 07:05. 12 hours drops the leading zero and marks PM with two small dots above the last digit (AM has one). |
| Show seconds | `show_seconds` | on / off | off | Append the seconds to the digital readout. The minute ring shows them either way. |
| Readout | `clock_position` | Below cube, In front, Hidden | Below cube | Where the small digital time is drawn. Hidden leaves only the cube and the minute ring. |
| Cube size | `cube_size` | 20 to 60%, step 2 | 42% | Size of the cube relative to the smaller side of the pane. Large cubes can touch the readout. |
| Rotation speed | `rotation_rate` | 0 to 300%, step 10 | 100% | How fast the cube turns. 100% is one full turn about every 18 seconds; 0% freezes it. |
| Second tick | `second_tick` | on / off | on | Give the cube a small eased nudge at every real second, like a clock movement. |
| Minute ring | `minute_ring` | on / off | on | A ring of 60 faint dots behind the cube. The current second glows and leaves a short trail. |
| Face shading | `face_shading` | 0 to 100%, step 5 | 50% | Sparse dither on the faces turned towards you. 0% is a pure wireframe. |
| Brightness | `brightness` | 30 to 150%, step 5 | 100% | Scales every dot of this scene. The defaults are deliberately dim for use behind text. |
| Dust | `dust` | on / off | on | A few faint twinkling dots in the background. |
| Zone from location | `time_zone_from_location` | on / off | on | Estimate the time zone from the longitude of your location. Turn off to set the offset by hand. |
| UTC offset | `utc_offset_hours` | -12 to 14 h, step 1 | 0 h | Hours added to UTC. Used only when Zone from location is off. |

### Box machine

Shown as **Box machine** (`box_machine`). A generative machine of boxes, rails and tiny screens that slowly builds and rearranges itself.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Machine | `seed` | 0 to 9999, step 1 | 7 | The number that selects the machine layout. Every number builds a different machine. |
| Tile size | `tile_size` | 12 to 28 dots, step 4 | 20 | Edge of one machine tile. Smaller tiles make a denser machine with more loops. |
| Rail density | `density` | 5 to 100%, step 5 | 70% | How much of the floor carries rails. Low values leave large empty areas. |
| Screens | `screens` | 0 to 6, step 1 | 2 | Most little screens (dithered gradient and scrolling marquee) the machine may hold. 0 removes them. |
| Box speed | `speed` | 1 to 20 dots/s, step 1 | 6 | How fast boxes slide along the rails. Multiplied by the global Speed setting. |
| Box spacing | `spacing` | 16 to 48 dots, step 4 | 28 | Distance between boxes on one loop. Smaller values put more boxes on the rails. |
| Box size | `cube_size` | 3 to 6 dots, step 1 | 4 | Edge length of each box. |
| Rail brightness | `rail_level` | 0 to 60%, step 2 | 35% | Brightness of the rails. 0% hides them so the boxes seem to float. |
| Box brightness | `cube_level` | 10 to 100%, step 5 | 80% | Brightness of the box outlines. Their centres are drawn at half of it. |
| Dotted rails | `rails_dashed` | on / off | off | Draw rails as a dotted track instead of a solid line. |
| Alternate directions | `reverse_alt` | on / off | on | Loops take turns running clockwise and counterclockwise. Off sends every box the same way round. |

### Machine screen

Shown as **Machine screen** (`machine_screen`). A generative machine display of scanning dithered gradients and glyph-like blocks, with a scrolling bar band along the bottom.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Seed | `seed` | 0 to 9999, step 1 | 7 | Selects the wave phases, the marquee bar pattern and where the dots blink. |
| Gradient speed | `gradient_speed` | 10 to 200%, step 10 | 50% | How fast the two dithered waves drift across the panel. |
| Panel size | `panel_size` | 30 to 90%, step 5 | 60% | Width and height of the screen as a share of the whole background. |
| Dither scale | `dither_scale` | 1 to 4 dots, step 1 | 1 | Dots per dither cell. Larger cells give a coarser, more retro texture. |
| Marquee speed | `marquee_speed` | 0 to 12 dots/s, step 1 | 4 | Scroll speed of the bar band along the bottom. 0 holds it still. |
| Brightness | `brightness` | 5 to 40%, step 5 | 20% | Dot intensity of the panel. The host dithers it further, so lower values give a sparser, quieter picture. |
| Frame | `show_frame` | on / off | on | Draw a thin bezel around the panel. |
| Blinking dots | `blinking_dots` | on / off | on | A few sparse dots inside the gradient switch on and off twice a second. |

## Wind

Shown as **Wind** (`wind`). Dots are blown across the empty parts of your screen by a small physics simulation. The wind is fixed or slowly rotating, with gusts, air drag and optional gravity. Each dot has a weight: light dots follow the wind and pushes closely; heavy dots resist both and fall faster. Dots live only in cells that show no text and bounce off everything else.

Wind reacts to your terminals. When a cell gains a character, Wind works out where it came from: text that scrolls up, down or sideways pushes nearby dots the way it moves, at a configurable speed, so scrolling output appears to sweep the dots along. Text that appears from nowhere, such as typing, pushes dots away more gently. Optionally dots that pile into one cell merge into a larger dot character (twice as many make a larger one still).

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Dots | `dot_count` | 10 to 20000, step 50 | 2000 | How many dots the wind carries. They live only in empty screen cells. |
| Dot weight | `dot_weight` | 1 to 100, step 5 | 30 | Average weight. Light dots follow the wind and pushes closely; heavy dots resist both and fall faster under gravity. |
| Weight variation | `weight_variation` | 0 to 100%, step 5 | 40% | How much the weights of single dots differ. 0% makes all dots identical. |
| Air drag | `drag` | 1 to 100%, step 5 | 40% | Resistance of the air. High drag stops dots quickly and lowers their top speed. |
| Wind strength | `wind_strength` | 0 to 100%, step 5 | 35% | Force of the wind. 0% leaves only gravity and pushes. |
| Wind direction | `wind_angle` | 0 to 359 degrees, step 15 | 0 | Where the wind blows to: 0 right, 90 down, 180 left, 270 up. With a rotating wind this is the start direction. |
| Wind mode | `wind_mode` | Fixed, Rotating | Fixed | Fixed keeps one direction; Rotating turns the wind steadily around. |
| Rotation speed | `rotation_speed` | -90 to 90 degrees/s, step 5 | 12 | Degrees per second the wind turns. Positive turns clockwise on screen, negative counter-clockwise. Shown only in Rotating mode. |
| Gusts | `gusts` | 0 to 100%, step 5 | 30% | How much the wind strength and direction vary from place to place and over time. |
| Gravity | `gravity_enabled` | on / off | off | Pull every dot toward the bottom of the screen. Heavy dots fall faster than light ones. |
| Gravity strength | `gravity_strength` | 1 to 100%, step 5 | 30% | How strongly gravity pulls. Shown only while Gravity is on. |
| Bounce | `bounce` | 0 to 100%, step 5 | 30% | Speed a dot keeps when it hits text or a wall. 0% makes dots stick and slide. |
| Screen edges | `edge_mode` | Wrap around, Bounce | Wrap around | Dots wrap to the opposite side, or bounce off the screen edge. |
| Scroll push | `scroll_push` | 0 to 100%, step 5 | 60% | Speed given to dots by text that scrolls into their cell, in the direction it moves. 0% ignores scrolling. |
| Appear push | `appear_push` | 0 to 100%, step 5 | 20% | Speed given to dots by text that appears from nowhere, such as typing. Usually gentler than scrolling. |
| Push reach | `push_reach` | 0 to 4 cells, step 1 | 1 | How many cells away from changing text a dot is still pushed. 0 pushes only dots the text lands on. |
| Scroll detection | `scroll_range` | 1 to 8 cells, step 1 | 3 | Largest jump in cells that still counts as scrolling. Larger values follow fast scrolling but may misread new text. |
| Diffusion | `diffusion` | 0 to 100%, step 5 | 0% | Nearby dots push each other apart, which keeps them from bunching up. 0% lets dots pile together freely. |
| Dispersion | `dispersion` | 0 to 100%, step 5 | 0% | Every so often a random dot jumps to a random empty place on the screen. Higher values do it more often (up to 100 dots per second). 0% never does. |
| Merge dots | `merge_dots` | on / off | off | Dots piled into one cell become a larger dot character; twice as many become a larger one still. |
| Merge at | `merge_threshold` | 2 to 12 dots, step 1 | 3 | How many dots in one cell make the larger dot. Shown only while Merge dots is on. |
| Random seed | `seed` | 0 to 9999, step 1 | 1 | Selects the starting positions and weights of the dots. |
| Frame rate | `frame_rate` | 5 to 30 fps, step 1 | 20 | Redraws per second. Higher is smoother and uses more processor time. |

## Growth

Shown as **Growth** (`growth`). A seeded fungal colony expands through the empty cells of the terminal. Branching hyphae, rings, veined mats, and fast carpets are selectable morphologies. The outside edge can remain as a reserved source; pointer movement erases nearby growth, new characters erase growth beneath them, and scrolling transfers part of the colony in the scroll direction. The colony reseeds periodically so long sessions do not settle permanently.

| Control | Id | Range or options | Default | Meaning |
|---|---|---|---:|---|
| Growth pattern | `pattern` | Branching hyphae, Concentric rings, Veined mat, Fast carpet | Branching hyphae | Selects the colony morphology. |
| Color mode | `color_mode` | Monochrome, Palette hues | Monochrome | Uses one tone or the active palette. |
| Density | `density` | 1 to 100% | 55% | Amount of empty space the colony tends to fill. |
| Growth rate | `growth_rate` | 1 to 100% | 45% | Speed of tip extension. |
| Reseed interval | `reseed_seconds` | 1 to 3600 seconds | 60 s | Adds fresh seeds after the interval. |
| Reserve edge | `reserve_edge` | On or off | On | Keeps an outside rim available as a source. |
| Mouse erases | `mouse_erase` | On or off | On | Clears growth around the pointer. |
| Scroll erase / push | `scroll_erase`, `scroll_push` | 0 to 100% | 50%, 55% | Controls how scrolling clears and displaces the colony. |

## Carpet

Shown as **Carpet** (`carpet`). Parallel isometric Braille hatch lines bend over hidden moving spheres and tubes: you never see the objects, only how they lift the lines. The hidden objects come from one of nine simulations, chosen with **Simulation**. Carpet defaults to a 12 x 12 **Autonomous Snake** board; saved project settings take precedence. With **Infinite lines** on (the default) hatches continue to the screen edges while the diamond remains the interactive work area; turn it off to stop the lines at the diamond. The bundled `carpet` animation package ([Background animations](animations.md#animation-packages-iliumanim)) offers the same settings as a plugin.

### Simulations

| Simulation | What it does |
| --- | --- |
| Mouse hunters | A flock of hunters follows the mouse pointer over the diamond, keeping separation. |
| Autonomous Snake (default) | A food-seeking Snake plans safe routes on an even-sized board (a safe Hamiltonian cycle), with a tapered body, feeding pulses and gently breathing food. |
| Slow Life | Conway's Game of Life on a square board, with slow generations; easing interpolates births and deaths. |
| Automated chess | Legal chess played by a bounded-lookahead engine, with piece-specific lift heights. |
| Lichess TV chess | Follows the featured Lichess TV game (needs network access to `https://lichess.org`). |
| DVD ball | A bouncing ball. |
| Planetary orbits | Stylised planetary motion (not an astronomical ephemeris). |
| Digital clock | The civil time as digital numbers. |
| Analog clock | The civil time on an analog dial, with optional hands drawn as hidden tubes. |

### Controls shown for every simulation

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Simulation | `carpet_mode` | Mouse hunters, Autonomous Snake, Slow Life, Automated chess, Lichess TV chess, DVD ball, Planetary orbits, Digital clock, Analog clock | Autonomous Snake | Choose one of nine hidden-object simulations. |
| Camera yaw | `carpet_yaw` | 0 to 360 degrees, step 5 | 45 | Rotate the ground plane without rotating hidden model coordinates. |
| Camera pitch | `carpet_pitch` | 15 to 75 degrees, step 5 | 30 | Ground elevation angle; 30 degrees gives an isometric view. |
| Ground zoom | `carpet_zoom` | 30 to 200%, step 5 | 145% | Scale the projected carpet. |
| Hatch direction | `carpet_hatch_direction` | 0 to 180 degrees, step 5 | 90 | All lines share this direction in the ground plane. |
| Infinite lines | `carpet_infinite_lines` | on / off | on | Continue flat hatch lines to the viewport edges beyond the ground. |
| Hatch spacing | `carpet_spacing` | 2 to 24 dots, step 1 | 2 | Distance between parallel hatch lines measured in Braille dots. |
| Line width | `carpet_line_width` | 20 to 200%, step 5 | 90% | Thickness of one hatch line relative to one Braille dot. |
| Lift height | `carpet_height` | 5 to 250%, step 5 | 25% | Scale every hidden object height. |
| Object radius | `carpet_radius` | 5 to 120 per thousand of the ground width, step 5 | 55 | Radius in thousandths of the normalized ground width. |
| Bend softness | `carpet_softness` | 0 to 100%, step 5 | 50% | Shape the rounded lift around spheres and tubes without changing their support radius. |
| Motion easing | `carpet_easing_ms` | 0 to 3000 ms, step 50 (upper limit lowered per simulation) | 200 ms | Smooth transition duration. Limited to one game step (Snake), one generation (Life, up to 3000 ms) or 900 ms (Digital clock) so each change can settle. |
| Frame rate | `carpet_fps` | 1 to 30 fps, step 1 | 20 | Maximum Carpet redraw cadence; lower values reduce rendering work. |
| Simulation speed | `carpet_simulation_speed` | 5 to 500%, step 5 | 100% | Scale games and motion; civil clocks always display live time. |
| Seed | `carpet_seed` | 0 to 999999, step 1 | 17 | Reproducible simulation seed. A change starts a new simulation. |

### Mouse hunters

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Hunters | `carpet_hunters_count` | 1 to 64, step 1 | 6 | Number of mouse-following hunters. |
| Hunter speed | `carpet_hunters_speed` | 1 to 100 %/s, step 1 | 20 | Ground width travelled each second. |
| Separation | `carpet_hunters_separation` | 10 to 200 per thousand, step 5 | 50 | Minimum flock separation influence. |

### Autonomous Snake

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Snake grid | `carpet_snake_grid` | 4 to 32 cells, step 2 | 12 | Even-sized safe Hamiltonian board. |
| Snake move | `carpet_snake_step_ms` | 40 to 3000 ms, step 10 | 200 ms | Interval between moves. |
| Initial length | `carpet_snake_initial_length` | 2 to min(grid x grid - 1, 32), step 1 | 4 | Snake length at the beginning of each game. |
| Food count | `carpet_snake_food_count` | 1 to 16, step 1 | 3 | Maximum simultaneous food targets. |

### Slow Life

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Life grid | `carpet_life_grid` | 4 to 32 cells, step 1 | 24 | Square Conway Life board. |
| Life generation | `carpet_life_generation_ms` | 100 to 10000 ms, step 100 | 1500 ms | Slow default generation interval; easing interpolates births and deaths. |
| Life population | `carpet_life_density` | 1 to 90%, step 1 | 25% | Initial seeded live-cell density. |
| Life wrap | `carpet_life_wrap` | on / off | on | Join opposite Life board edges. |

### Automated chess and Lichess TV chess

Automated chess shows all rows below; Lichess TV chess shows only Piece easing and the six piece heights.

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Chess move | `carpet_chess_move_ms` | 200 to 10000 ms, step 100 | 1500 ms | Autonomous move interval. Automated chess only. |
| AI depth | `carpet_chess_ai_depth` | 1 to 3 plies, step 1 | 2 | Bounded lookahead for automated legal chess moves. Automated chess only. |
| AI budget | `carpet_chess_node_budget` | 100 to 5000 nodes, step 100 | 1500 | Hard search-work ceiling for every autonomous move. Automated chess only. |
| New game delay | `carpet_chess_restart_ms` | 500 to 15000 ms, step 500 | 3000 ms | Hold finished games before starting another. Automated chess only. |
| Game move limit | `carpet_chess_max_plies` | 40 to 600 plies, step 10 | 300 | Maximum game length before a new seeded game begins. Automated chess only. |
| Piece easing | `carpet_chess_easing_ms` | 0 to 3000 ms, step 50 | 650 ms | Duration of eased chess movements and capture fades. |
| Pawn height | `carpet_pawn_height` | 5 to 200%, step 5 | 45% | Lift for pawn pieces relative to base height. |
| Knight height | `carpet_knight_height` | 5 to 200%, step 5 | 75% | Lift for knight pieces relative to base height. |
| Bishop height | `carpet_bishop_height` | 5 to 200%, step 5 | 85% | Lift for bishop pieces relative to base height. |
| Rook height | `carpet_rook_height` | 5 to 200%, step 5 | 65% | Lift for rook pieces relative to base height. |
| Queen height | `carpet_queen_height` | 5 to 250%, step 5 | 115% | Lift for queen pieces relative to base height. |
| King height | `carpet_king_height` | 5 to 250%, step 5 | 130% | Lift for king pieces relative to base height. |

### DVD ball and Planetary orbits

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| Ball speed | `carpet_dvd_speed` | 1 to 100 %/s, step 1 | 15 | Bouncing ball ground speed. DVD ball only. |
| Orbit speed | `carpet_orbit_speed` | 1 to 500%, step 5 | 100% | Stylized planetary motion rate; not an astronomical ephemeris. Planetary orbits only. |
| Orbit scale | `carpet_orbit_scale` | 20 to 100%, step 5 | 100% | Scale the planetary system within the ground. Planetary orbits only. |

### Digital and analog clocks

| Control | Id | Range or options | Default | What it does |
| --- | --- | --- | --- | --- |
| UTC offset | `carpet_utc_offset_minutes` | -720 to 840 min, step 15 | 0 min | Civil clock offset from UTC; the default explicitly displays UTC. Both clocks. |
| Clock seconds | `carpet_clock_seconds` | on / off | on | Include seconds in both civil clocks. |
| 24-hour clock | `carpet_clock_24h` | on / off | on | Use 24-hour digital numbers. Digital clock only. |
| Clock hands | `carpet_clock_tubes` | on / off | off | Draw analog hands as hidden tubes between the centre and live points. Analog clock only. |

Clocks use the explicit UTC offset independently of animation speed: the civil clocks always show live time, whatever Simulation speed says.

## Notes on the Lily pads scene

- There is one lily-pad animation: **Lily pads on a quiet pond**, key `quiet_pond`. Its control table is under [Lily pads (Quiet pond)](#lily-pads-quiet-pond).
- **Lily pads** accepts 3 to 64 leaves; **Rooted placement** clusters them around underwater root groups instead of scattering them.
- Like the other nine built-in scenes it can use Loop playback.

## Troubleshooting

| Symptom | Likely cause and fix |
| --- | --- |
| A scene is too bright or busy | Lower the shared Brightness or Dot density, or use the Whisper preset. Scene-level quiet defaults already apply; Dithered scenes also have their own Brightness or Contrast rows. |
| A scene has no Loop controls | Only the ten built-in scenes cache loops. Hosted scenes in this guide are live-only. |
| GPU renderer is greyed out | No usable GPU was found. Rest the pointer on the row for the reason; Software works everywhere. |
| Dithered scenes look frozen | They default to slow motion. Raise the scene speed rows or the shared Speed (up to 300%), or the Frame rate cap if you set one. |
| Frost or Wind seem to ignore text | Both depend on which cells are occupied; they only see the Ilium workspace, not scrollback. Frost in Frame edges mode never reacts to text. |
| Cube clock shows the wrong time zone | The zone is estimated from the longitude of the shared location. Set the Location (see [Maps, space and live data](animations-maps-space-and-live-data.md)) or turn off Zone from location and set UTC offset. |
| Carpet chess is missing a setting | Rows are shown per simulation. Switch Simulation to the one that owns the row. |
| Lichess TV chess shows nothing | It needs network access to `https://lichess.org`. In the bundled plugin build this is a permission you can allow or deny. |

See also: [Background animations](animations.md), [Settings](settings.md), [Maps, space and live data](animations-maps-space-and-live-data.md), [Media and games](animations-media-and-games.md).
