# Media and games

This page documents the animations that play media or run a small game: **Video**, **Audio spectrum**, **Images**, **Wikipedia**, **Galactic empires**, **Hex expedition** and **Vector TD**. Each section lists what the scene needs, how to set it up step by step, every scene control with its range and default, and what to do when something does not work. The shared controls that apply to every animation (colour, palette, dithering, speed, density, panel placement, frame-rate cap) are not repeated here; they are described in [Animations](animations.md).

## Contents

- [Before you start](#before-you-start)
- [Video](#video)
  - [What you need](#what-you-need)
  - [Play your own videos (Custom series)](#play-your-own-videos-custom-series)
  - [Play the bundled plant-growth playlist (Germination series)](#play-the-bundled-plant-growth-playlist-germination-series)
  - [Video controls](#video-controls)
  - [How video is processed](#how-video-is-processed)
  - [Licences catalogue](#licences-catalogue)
  - [Video troubleshooting](#video-troubleshooting)
- [Audio spectrum](#audio-spectrum)
  - [Platform requirements](#platform-requirements)
  - [Set it up](#set-it-up)
  - [Audio spectrum controls](#audio-spectrum-controls)
  - [Audio spectrum troubleshooting](#audio-spectrum-troubleshooting)
- [Images](#images)
  - [Show a single image](#show-a-single-image)
  - [Show a folder slideshow](#show-a-folder-slideshow)
  - [Show a URL slideshow](#show-a-url-slideshow)
  - [Images controls](#images-controls)
  - [Limits, cache and troubleshooting](#limits-cache-and-troubleshooting)
- [Wikipedia](#wikipedia)
  - [Wikipedia controls](#wikipedia-controls)
  - [Caching and offline use](#caching-and-offline-use)
- [Galactic empires](#galactic-empires)
- [Hex expedition](#hex-expedition)
- [Vector TD](#vector-td)
- [Related pages](#related-pages)

## Before you start

All seven scenes are chosen in **Settings → Animations**, the same place as every other background.

1. Open Settings and go to the **Animations** tab.
2. Use Up and Down to select the scene. The live preview fills the whole screen behind the controls.
3. Press Enter to jump into the scene's controls. The selected scene stays active while you adjust.
4. Press `f` for a full-screen preview without the controls in the way.

Things that hold for every scene on this page:

- Each scene keeps its own saved values when you select another scene, so switching away and back loses nothing.
- Changes save immediately to the project. If a save fails, the previous effective setting is kept and an error is shown.
- Animation is decoration only. It never changes terminal history or Ilium's copy sources, although selecting text with your outer terminal's own mouse selection can include the decorative Braille dots.
- The explicitly opened demo preview stays live even when Motion is Off; as a background, scenes freeze when Motion is Off.
- Shared speed, density and dithering apply to every scene. The shared colour rows are hidden for scenes that paint their own colours (for example Images).
- Scenes with a **Scene status** row use it to report problems (a missing helper program, no audio input, a failed download).

## Video

Plays a video file, folder, glob or URL as dithered Braille. The scene has two **Series**: **Custom** (your own sources) and **Germination** (a bundled playlist of plant-growth time-lapses with licence credits).

### What you need

| Requirement | Needed for |
| --- | --- |
| `ffmpeg` on `PATH` | Everything the Video scene does. It decodes frames to raw pictures at exactly the screen's dot resolution. |
| `ffprobe` on `PATH` | Germination, Random scenes playback, and any case where the clip's duration must be read. It ships with ffmpeg in most packages. |

ffmpeg and ffprobe are external programs that Ilium runs as child processes only; they are not bundled. When ffmpeg is missing, the **Scene status** row reads `ffmpeg not found: install ffmpeg` (or `ffprobe not found: install ffmpeg`). Install the package with your system's package manager and reopen the preview; a missing decoder is not retried in a loop.

### Play your own videos (Custom series)

1. In **Settings → Animations**, select **Video**.
2. Set **Series** to **Custom** (the default).
3. Select the **Source** row and press Enter to edit it. Enter one of:
   - a single video file, for example `~/Videos/loop.mp4`;
   - a folder, for example `~/Videos/screensavers`;
   - a glob, for example `~/Videos/**/*.mkv`;
   - an `http://` or `https://` URL;
   - several of these separated by semicolons: `~/Videos/a.mp4; https://example.com/b.webm`.
4. Confirm. A URL with a scheme other than `http://` or `https://` (for example `gopher://`) or a malformed URL is rejected with an error and the old value stays. Paths are not checked when you edit the row, so a folder that does not exist yet is accepted (it may be on a disk that is not mounted); the scene then shows `No videos found: <source>` until files appear.
5. Choose **Playback**, **Style**, **Fit** and the tone controls from [Video controls](#video-controls).

How sources are interpreted:

- A file you name explicitly is always accepted, whatever its extension. Folders and globs only pick up files with these extensions: `mp4`, `mkv`, `avi`, `mov`, `webm`, `m4v`, `mpg`, `mpeg`, `wmv`, `flv`, `ts`.
- A folder is scanned recursively when **Include sub-folders** is on. Discovery is bounded to 50,000 files and a directory depth of 32, so a glob such as `/**` cannot exhaust memory or time.
- URLs are opened directly by ffmpeg over `http`/`https` with a 20 second network timeout. Local files are opened with the `file` protocol only.
- An unencrypted `http://` URL plays but adds `warning: http:// is not encrypted` to the status. Prefer `https://`.
- Folder scans, opening, decoding and cleanup run on a worker away from input handling, so a slow disk or network never freezes the interface. Folders are rescanned periodically while playing.

### Play the bundled plant-growth playlist (Germination series)

1. Select **Video**, then set **Series** to **Germination**.
2. The **Source** and **Include sub-folders** rows disappear because the playlist is fixed. Everything else (playback mode, style, fit, tone, frame rate) works exactly as for Custom.
3. The first clip is fetched over HTTPS from its original Wikimedia Commons URL. While it loads, the status reads `Loading <title> - <author> (<licence>)`; while it plays it names the clip, author and licence.
4. Switch **Series** back to **Custom** at any time. Your Custom source text is retained exactly as you left it, even while Germination is active.

Properties of the series:

- The playlist is the catalogue in `ilium-ambient/assets/germination.json`: 51 time-lapse clips of germinating seeds, growing shoots and opening flowers. Clip sizes are 0.1 to 38.4 MB; the 64 MiB hard limit per clip is enforced.
- Every clip is pinned: its expected size and SHA-256 are stored in the catalogue and verified after download, so a changed or truncated file is rejected rather than played.
- **RAM only.** The current clip is held in memory, served to ffmpeg and ffprobe over a seekable loopback HTTP endpoint, and dropped when the next clip loads. No downloaded video cache is written to disk, and at most two clips exist in memory at once (a new clip waits briefly if the previous one is still stopping).
- Germination never uses local files; the Custom `Source` is ignored while it is active.
- Requests to the video host are spaced out and time-limited. After a failure the scene backs off (1 second, doubling to at most 20 seconds) instead of spinning.

### Video controls

Rows marked "conditional" only appear in the situation described. Defaults are in the third column.

| Control | Range or values | Default | Notes |
| --- | --- | --- | --- |
| Series | Custom, Germination | Custom | Germination hides Source and Include sub-folders. |
| Source (Custom only) | file, folder, glob, URL, or several separated by `;` | empty | Empty means nothing plays. Validated when edited. |
| Include sub-folders (Custom, not URL-only) | on / off | on | Applies to folder entries. Hidden when every entry is a URL. |
| Playback | Live, Slowed, Random scenes | Live | Live: real speed, files one after another, forever. Slowed: real content at a fraction of its speed. Random scenes: short excerpts from random files at random positions. |
| Speed (Slowed only) | 5 to 100 %, step 5 | 50 % | Lower is slower and dreamier. The status shows `(N% speed)`. |
| Scene length (Random scenes only) | 3 to 120 s, step 1 | 20 s | How long each excerpt plays before another is picked. |
| Shuffle (not Random scenes) | on / off | off | Play the files of a folder or glob in random order. |
| Repeat one file (not Random scenes) | on / off | off | Keep replaying the first file instead of advancing. |
| Style | Dithered, Colored, Mono ink | Dithered | Dithered: grey levels dithered into monochrome dots using your palette. Colored: dithered dots tinted with the colour of each cell. Mono ink: a line-art look of edges plus faint tones, like pen and ink. |
| Fit | Fit (letterbox), Fill (crop), Stretch | Fit (letterbox) | Fit shows the whole picture with black bars; Fill covers the screen and crops edges; Stretch distorts the picture to the screen shape. |
| Brightness | -100 to 100, step 5 | 0 | Lightens or darkens before dithering. |
| Contrast | -100 to 100, step 5 | 10 | Spreads dark and light tones; higher values give crisper dither patterns. |
| Gamma | 50 to 300 %, step 10 | 100 % | Above 100 % reveals detail in dark scenes; below 100 % deepens shadows. |
| Invert | on / off | off | Swaps light and dark; useful for bright videos on a dark terminal. |
| Detail | 0 to 100, step 5 | 25 | Sharpens before dithering so small features survive the low resolution. |
| Frame rate | 6 to 24 fps, step 1 | 15 fps | Redraw rate requested from the decoder and the host. Lower values use less CPU in both Ilium and ffmpeg. |
| Random seed (Random scenes or Shuffle on) | 0 to 9999, step 1 | 0 | 0 picks something new each time the scene starts; any other number replays the same sequence. |

Tips:

- For dark terminals, bright footage often looks best with **Invert** on, **Style** Mono ink, and **Contrast** raised.
- Reduce **Frame rate** first when ffmpeg uses too much CPU; the picture is only a few thousand dots, so 8 to 12 fps is usually enough.
- After you resize the terminal, the previous picture is resampled for a moment (`Resizing: <name>` in the status) while ffmpeg restarts at the new size. The restart waits until the size has been stable for half a second.

### How video is processed

- ffmpeg decodes to raw frames of exactly `2 x columns` by `4 x rows` dots, one pixel per Braille dot. A bounded queue of 12 frames sits between the decoder and the renderer, and the renderer always shows the newest frame that is due, so drawing never blocks on decoding.
- Commands never go through a shell. Paths are passed after `-i` with a `file:` prefix, and network protocols are restricted with a protocol whitelist.
- At most four video child processes may exist at once. Each child is registered so that closing the preview or switching scenes kills and reaps it; nothing lingers after a clip ends.
- A failing clip is skipped (`cannot play <name>`) and playback continues with the next one, with a growing back-off between failures.
- For troubleshooting only, setting `ILIUM_VIDEO_DIAGNOSTIC=1` in a task-owned process enables bounded scalar diagnostics through the existing debug log sink. It writes no media copy and no separate log file.

### Licences catalogue

Every Germination clip is a freely licensed work hosted on Wikimedia Commons. The catalogue file [`ilium-ambient/assets/germination.json`](../../ilium-ambient/assets/germination.json) records, for each clip:

| Field | Meaning |
| --- | --- |
| `id`, `title` | Stable identifier and the file title on Commons. |
| `url` | The original upload URL used for playback. |
| `source_page` | The Commons file page, which holds the full credit and history. |
| `author`, `credit`, `attribution` and their URL lists | Who made the clip and any attribution text. |
| `license`, `license_url` | The licence name and its text. |
| `source_sha1`, `download_sha256`, `expected_download_bytes` | Integrity data checked before a clip is played. |
| `source_duration_seconds` | Length of the original clip. |
| `attribution_required` | Whether the licence requires credit. |

The licences present in the catalogue are CC BY-SA 3.0 (16 clips), CC BY-SA 4.0 (12), CC BY 3.0 (13, including one recorded as "CC BY 3.0 at"), CC BY 2.0 (7), CC BY 4.0 (1), GFDL 1.2 (1) and public domain (1). The scene status always shows the title, author and licence of the clip on screen; use the `source_page` and `license_url` entries for the complete terms.

The playlist includes, among others, sunflower, maize, basil, bamboo, lentil, bean, pea, wheat and corn germination, and flower openings such as amaryllis, water lily, rose, Echinopsis, Lithops and Epiphyllum.

### Video troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `ffmpeg not found: install ffmpeg` | Install ffmpeg (and ffprobe) and make sure both are on the `PATH` of the Ilium server process. |
| `No videos found: <source>` | The path or glob matched nothing, or contains no files with a supported extension. Check the path, quote-free semicolons between entries, and **Include sub-folders**. |
| `cannot play <name>` | ffmpeg could not decode that file. The scene moves on to the next one. |
| Nothing but a faint play-button outline | No frame has arrived yet: the clip is starting, still downloading (Germination), or the source is empty. |
| `Previous RAM video is still stopping; retry shortly` | Germination switched clips faster than the previous one could be released; it retries by itself. |
| High CPU | Lower **Frame rate**, use **Fit** instead of Fill, or play a lower-resolution source. |

## Audio spectrum

A spectrum analyser of whatever your system is playing, drawn in Braille, with eight visual styles. It reads the audio input only while the scene is shown.

### Platform requirements

| Platform | What is used | Needs |
| --- | --- | --- |
| Linux | A helper process: `pw-record` (PipeWire) first for system output, `parec` (PulseAudio and PipeWire's Pulse layer) first for microphones and named devices. | At least one of `pw-record` or `parec` on `PATH`. If neither is installed the status reads `neither pw-record nor parec found (install pipewire-bin or pulseaudio-utils)`. The microphone and named devices fall back to the system audio library when no helper exists. |
| Windows | WASAPI loopback of the default output device. | Nothing extra. |
| macOS 14.6 and later | A CoreAudio loopback of the default output. | Nothing extra. |
| Older macOS | No native loopback. | A virtual loopback device such as BlackHole; choose **Named device** and enter part of its name. |

On Linux, `pw-record` follows the default output when you switch speakers or headphones, which a fixed monitor source does not, which is why it is tried first for **System output**.

### Set it up

1. Select **Audio spectrum** in **Settings → Animations**.
2. Choose **Audio source**:
   - **System output (loopback)**: what is playing right now (the default).
   - **Default microphone / input**: the default capture device.
   - **Named device**: a specific device (see below).
3. For **Named device**, enter the **Device name**:
   - Linux: a PulseAudio/PipeWire source name from `pactl list short sources`. To visualise an output, use its monitor, which ends in `.monitor`.
   - Windows and macOS: part of the device name, for example `BlackHole`.
4. Play some audio. The status row reads `Starting audio capture...` for a moment, `No audio playing` when the source delivers silence, or `Audio input unavailable: <reason>` when capture fails.
5. Pick a **Style** and tune the controls.

### Audio spectrum controls

The list of rows depends on the style: a row that does not apply to the chosen style is hidden.

| Control | Range or values | Default | Shown when | Notes |
| --- | --- | --- | --- | --- |
| Audio source | System output (loopback), Default microphone / input, Named device | System output | always | |
| Device name | text, up to 200 characters | empty | Named device | Control characters are stripped. |
| Style | Bars, Mirrored bars, Spectrum line, Filled area, Waveform (oscilloscope), Radial, Spectrogram, Pulse rings | Bars | always | Waveform is the raw signal; Spectrogram scrolls over time; Pulse rings react to bass. |
| Orientation | Bottom up, Top down, Left to right, Right to left | Bottom up | every style except Radial and Pulse rings | The edge the spectrum grows from. |
| Colors | Mono ink, Rainbow, Heat, Blue-white | Mono ink | always | Mono uses the global ink colour. Rainbow colours by frequency; Heat and Blue-white by height or intensity. |
| Frequency scale | Logarithmic, Mel, Linear | Logarithmic | every style except Waveform | Logarithmic matches music, Mel matches hearing, Linear shows treble in detail. |
| Bands | 8 to 128, step 4 | 48 | every style except Waveform | Number of frequency bands analysed. |
| Lowest frequency | 20 to 1000 Hz, step 10 | 40 Hz | every style except Waveform | Left edge. |
| Highest frequency | 2000 to 20000 Hz, step 500 | 16000 Hz | every style except Waveform | Right edge. |
| FFT size | 2048 (quicker response), 4096 (finer bass) | 2048 | every style except Waveform | Longer windows resolve low notes better but react a little slower. |
| Treble boost | 0 to 6 dB/oct, step 1 | 3 | every style except Waveform | Compensates for the natural roll-off of music, which falls about 3 dB per octave. |
| Auto gain | on / off | on | always | Follows loudness so the display fills the height. When on, only the span between floor and ceiling is used. |
| Floor | -100 to -30 dB, step 5 | -60 dB | every style except Waveform | Level shown as zero height. |
| Ceiling | -30 to 0 dB, step 2 | -10 dB | every style except Waveform | Level shown as full height (fixed gain only). Always kept at least 10 dB above the floor. |
| Sensitivity | 25 to 400 %, step 5 | 100 % | always | Multiplies the displayed height. |
| Smoothing | 0 to 100 %, step 5 | 60 % | always | How slowly bars fall back and how long peaks are held. |
| Peak markers | on / off | on | Bars, Mirrored bars, Spectrum line, Filled area, Radial | A marker rests at recent peaks, then falls. |
| Mirror | on / off | off | every style except Waveform and Pulse rings | Mirrors around the middle with the bass at the centre. |
| Bar width | 0 to 8 dots, step 1 | 0 | Bars, Mirrored bars | 0 fits one bar per band. |
| Bar gap | 0 to 4 dots, step 1 | 1 | Bars, Mirrored bars | Space between bars. |
| Scroll speed | 10 to 60 dots/s, step 5 | 30 | Spectrogram | How fast the spectrogram scrolls. |
| Refresh rate | 10 to 30 fps, step 2 | 24 fps | always | Higher is smoother and costs more CPU. |

Out-of-range values saved by hand are clamped when the settings load; the FFT size snaps to 2048 or 4096.

### Audio spectrum troubleshooting

| Symptom | Fix |
| --- | --- |
| `Audio input unavailable: neither pw-record nor parec found ...` | Install `pipewire-bin` (for `pw-record`) or `pulseaudio-utils` (for `parec`) on Linux. |
| `Audio input unavailable: no audio device matching "<name>"` | The device name did not match. Check `pactl list short sources`, or the device list of your OS. |
| `No audio playing` | Capture works but the source is silent. Check the volume and that audio is going to the default output. |
| Bars never reach the top or stay too low | Leave **Auto gain** on, or raise **Sensitivity**, or lower **Floor**. |
| Bass looks smeared | Use **FFT size** 4096 and a lower **Lowest frequency**. |
| No loopback on older macOS | Install BlackHole, route your output through it, then use **Named device** with `BlackHole`. |

## Images

Coloured Braille pictures from files, folders or URLs, with slow pan and zoom (Ken Burns motion) and, in the folder and URL modes, a slideshow with cross-fades. The scene paints its own cell colours, so the shared palette rows are hidden; the controls below set the colour treatment instead.

Supported image types: `png`, `jpg`, `jpeg`, `gif`, `bmp`, `webp`.

### Show a single image

1. Select **Images** in **Settings → Animations**.
2. Leave **Source** on **Single image** (the default).
3. Set **Image from** to one of:
   - **Built-in list**: pick a **Built-in image** (NightCafe dreamscape, Ubuntu-like mountain lake, Ubuntu-like golden desktop, Unsplash desk). These are links that are downloaded once and cached.
   - **Local file**: enter an absolute path in **Image file**; a leading `~/` is expanded. The file must exist and have a supported extension, otherwise an error is shown and the old value stays.
   - **URL**: enter an `https://` link in **Image URL**. It is downloaded in the background and cached.
4. Choose **Motion** and **Fit** as you like.

### Show a folder slideshow

1. Set **Source** to **Folders / globs**.
2. In **Folders**, enter directories or glob patterns separated by `;`, for example `~/Pictures;/data/*/wallpapers/**/*.jpg`. Globs support `*`, `?`, `[a-z]` and `**` for any depth. Hidden entries are skipped. Every folder (or the fixed part of a glob) must exist.
3. Leave **Include subfolders** on to also scan below each plain directory entry; glob patterns choose their own depth with `**`.
4. Pick the **Order** (Sequential in name order, or Shuffle), the **Seconds per image** and the **Cross-fade**.

### Show a URL slideshow

1. Set **Source** to **URL list**.
2. In **Image URLs**, enter `https://` image links separated by `;`, or one link to a `.txt` file that holds one image URL per line. Only `https://` is accepted; URLs may be up to 2000 characters.
3. Set the order, timing and cross-fade as for folders.

### Images controls

Rows depend on the source mode and on the motion setting. The "Row id" is the control name used in the project configuration.

| Control | Row id | Range or values | Default | Shown when |
| --- | --- | --- | --- | --- |
| Source | `mode` | Single image, Folders / globs, URL list | Single image | always |
| Image from | `source_kind` | Built-in list, Local file, URL | Built-in list | Single image |
| Built-in image | `builtin_image` | 4 bundled links | NightCafe dreamscape | Single image, Built-in list |
| Image file | `file` | absolute path (`~/` allowed) | empty | Single image, Local file |
| Image URL | `url` | `https://` link | empty | Single image, URL |
| Folders | `folders` | `;`-separated directories or globs | empty | Folders / globs |
| Include subfolders | `recursive` | on / off | on | Folders / globs |
| Image URLs | `urls` | `;`-separated `https://` links, or one `.txt` list | empty | URL list |
| Order | `order` | Sequential, Shuffle | Sequential | slideshow modes |
| Shuffle seed | `shuffle_seed` | 0 to 9999 | 0 | slideshow modes with Shuffle |
| Seconds per image | `display_seconds` | 3 to 1800 s, step 5 | 30 s | slideshow modes, or whenever Motion is not None |
| Cross-fade | `transition_seconds` | 0 to 30 s | 3 s | slideshow modes |
| Motion | `motion` | Slow zoom in, Slow zoom out, Pan left-right, Pan up-down, Random drift, Zoom + pan, None | Slow zoom in | always |
| Motion strength | `motion_strength` | 0 to 50 % | 12 % | Motion not None |
| Easing | `easing` | Smooth, Linear, Ease out | Smooth | Motion not None |
| Fit | `fit` | Fill (crop), Fit (borders), Stretch | Fill (crop) | always |
| Preset | `preset` | Dimmed, Vivid, Monochrome, Cool, Warm | Dimmed | always |
| Brightness | `brightness` | 0 to 200 % | 72 % | always |
| Contrast | `contrast` | 0 to 200 % | 82 % | always |
| Saturation | `saturation` | 0 to 200 % (0 is grey) | 82 % | always |
| Hue tint | `hue` | 0 to 360 deg; 180 is neutral, below is cooler, above is warmer | 180 | always |
| Intensity | `intensity` | 0 to 100 % | 48 % | always |
| Opacity | `opacity` | 0 to 100 % | 58 % | always |

Notes:

- The cross-fade is capped at half the display time. **Seconds per image** is also the length of one pan/zoom move, so with Motion on, a single image slowly completes one move per interval.
- A shuffle seed of 0 gives a different order at every start; any other value always gives the same order.
- The Preset sets the base colour treatment and the six sliders scale it. **Intensity** controls how many dots light up (higher also lights darker areas); **Opacity** controls how strongly the picture shows over the terminal background.
- With one image and Motion None the scene stops redrawing (1 frame per second); while a move, fade, slideshow step or load is in progress it redraws at about 12 frames per second. Choose **None** to save redraws.

### Limits, cache and troubleshooting

- Folder scans stop at 20,000 images, 50,000 directories and a depth of 24. A URL list holds at most 500 URLs; a `.txt` list file may be at most 1 MiB.
- Each download has a 15 second timeout and a 32 MiB size limit. Downloads are cached on disk under Ilium's cache directory (an `images` folder inside the `ambient` cache) and refreshed after 30 days; if a refresh fails, the stale copy is used.
- Only a few decoded pictures are kept in memory at once (the six most recent) and the next two are prefetched, so large folders are cheap to browse.
- Decoding runs on a worker thread; large images are decoded at a bounded size (between 1024x576 and 3840x2160).
- If a file or URL cannot be loaded, the **Scene status** row explains why and the slideshow moves on.

## Wikipedia

Scrolls random articles linked from today's English Wikipedia Main Page, slowly, with their headings, references, infoboxes and images. When an article ends, the scene pauses at the bottom (the dwell time), then picks another of the day's Main Page articles. It needs network access to Wikipedia on first use.

### Wikipedia controls

| Control | Range or values | Default | Notes |
| --- | --- | --- | --- |
| Page rendering | Braille, Text | Braille | Braille draws real font outlines as dots. Text uses readable terminal characters and keeps bold and italic. |
| Greyscale | on / off | on | Neutral ink and images. Turn it off to use the selected colour palette. |
| Page palette | Wikipedia, Pastel, Sepia, Night | Wikipedia | A palette for article ink, links, rules and images. The colour sliders adjust every preset. |
| Page zoom (Braille only) | 50 to 300 %, step 5 | 100 % | Scales the page's real font, headings and images; reflow keeps the page within the terminal width. |
| Page scroll | 0 to 100, in units of 0.1 row per second | 2 (0.2 row/s) | Scrolls the whole article slowly. Zero pauses the page without picking a new article. |
| Page dwell | 2 to 60 s | 10 s | Pause at the top and the bottom before moving on. |
| Palette hue shift | 0 to 359 degrees | 0 | Rotates the hue of text and images; visible with Greyscale off. |
| Palette saturation | 0 to 200 % | 100 % | Zero gives neutral greys; 100 % keeps the preset. |
| Page lightness | 10 to 100 % | 60 % | Adjusts ink and image brightness against the dark workspace background. |

Steps to get a coloured, readable page:

1. Select **Wikipedia** in **Settings → Animations**.
2. Set **Page rendering** to **Text** for crisp characters, or leave **Braille** and raise **Page zoom** for a larger dotted page.
3. Turn **Greyscale** off and choose a **Page palette** (for example Sepia or Night).
4. Adjust **Page lightness** (a dim page behind your terminals is easier on the eyes) and **Page scroll**.

### Caching and offline use

- Downloaded articles and images are cached in a `wikipedia` folder inside Ilium's `ambient` cache directory (on Linux `~/.cache/ilium/ambient/wikipedia`). Previously downloaded pages and images can be reused offline.
- Each request has a 15 second timeout. Articles are bounded (HTML up to 16 MiB, up to 20,000 blocks, 256 images, images up to 12 MiB each), and article parsing has a memory ceiling so a huge page cannot exhaust RAM.
- The help specimen in Settings is illustrative and does not fetch a page; a real page appears when the scene runs.

## Galactic empires

A procedural galaxy overview: a connected map of stars, colourful dithered territories and moving fleets. Empires expand, build fleets, alternate between peace and war, and eventually unify under one winner. It is an atmospheric, deliberately basic strategy simulation that uses no external game assets and no network access.

How it behaves:

- The galaxy has a connected backbone of hyperlanes plus optional extra links, so every system can be reached.
- Camera: a slow clockwise orbit that shows roughly one quarter of the galaxy's area at a time at 100 % zoom. Simulation speed and camera speed are independent.
- A late frontier campaign guarantees a finite ending. The winner stays visible for the **Victory pause**, then a new galaxy begins.
- Seed 0 chooses a fresh map every time; a positive seed reproduces the initial galaxy and simulation.

Settings that change how the map is generated start a new galaxy when you edit them (marked "restarts" below). All others apply to the running galaxy immediately.

There are 27 controls, in this order:

| Control | Range | Default | Effect |
| --- | --- | --- | --- |
| Simulation speed | 25 to 400 %, step 25 | 100 % | Rate of economy, diplomacy and fleet ticks. The global Speed also applies. Keeps the current galaxy. |
| Camera speed | 0 to 300 %, step 10 | 100 % | Clockwise orbit multiplier. 0 % holds the camera while empires keep moving. |
| Orbit period | 60 to 3600 s, step 60 | 900 s | Seconds for one orbit at Camera speed 100 % and global Speed 1. |
| Camera zoom | 50 to 200 %, step 10 | 100 % | 100 % shows a quarter of the disk's area; higher shows a smaller field without changing the galaxy. |
| Star systems | 120 to 720, step 20 | 480 | Size of the connected galaxy. Restarts. |
| Starting empires | 3 to 12 | 8 | Separately founded civilisations. Restarts. |
| Galaxy seed | 0 to 999,999 | 0 | 0 chooses a fresh galaxy; a positive seed reproduces it. Restarts. |
| Spiral arms | 2 to 6 | 4 | Number of arms in the generated map. Restarts. |
| Arm spread | 25 to 200 %, step 5 | 100 % | Angular scatter around each arm. Restarts. |
| Arm twist | 0 to 200 %, step 5 | 100 % | How much arms turn from centre to rim. Restarts. |
| Extra hyperlane links | 0 to 4 | 2 | Up to this many short extra links per star. Zero still keeps a connected backbone. Restarts. |
| Territory shading | 0 to 100 %, step 5 | 35 % | Overall territory ink. Zero hides fills, borders and contact but leaves stars and lanes. |
| Territory size | 50 to 150 %, step 5 | 100 % | Influence radius around each star; rebuilds only the territory field. |
| Territory softness | 50 to 200 %, step 5 | 100 % | Width of each soft edge; 100 % keeps the original rounded border. |
| Border emphasis | 0 to 200 %, step 10 | 100 % | Strength of the territory contour. Zero removes the contour, not the fill. |
| Contact emphasis | 0 to 200 %, step 10 | 100 % | Brightness where opposing territories meet; wars pulse slightly. |
| Star size | 50 to 200 %, step 10 | 100 % | Radius of system markers. |
| Star brightness | 0 to 100 %, step 5 | 100 % | Marker intensity. Zero hides markers without erasing territory or lanes. |
| Show hyperlanes | on / off | on | Draws the connection graph. Hiding it does not disconnect the simulation. |
| Hyperlane width | 50 to 200 %, step 10 | 100 % | Stroke radius of visible lanes. |
| Hyperlane brightness | 0 to 150 %, step 5 | 100 % | Visible lane intensity. Zero hides lane ink without changing fleet routes. |
| Show fleets | on / off | on | Moving markers on hyperlanes; off keeps the simulation running. |
| Fleet size | 50 to 200 %, step 10 | 100 % | Size of fleet heads and trails. |
| Fleet brightness | 0 to 100 %, step 5 | 100 % | Fleet marker intensity. Zero hides fleet ink only. |
| Fleet trails | 0 to 200 %, step 10 | 100 % | Trail length behind moving fleets. Zero disables trails. |
| Capture flashes | on / off | on | Freshly captured system markers briefly grow. |
| Victory pause | 10 to 120 s, step 5 | 30 s | Simulation seconds to show the winner before the next galaxy. Simulation speed and global Speed scale wall time. |

Examples:

- A calm wallpaper: Camera speed 30 %, Simulation speed 50 %, Territory shading 20 %, Show fleets off.
- A dense, busy galaxy: Star systems 720, Starting empires 12, Simulation speed 200 %.
- A reproducible galaxy to compare settings: set Galaxy seed to any positive number (for example 42) before changing visual controls.

## Hex expedition

An endless sea of explorer's islands, generated as you watch and panned over slowly, in the manner of the maps of adventure board games. Each island has its own shore and biomes, one moored ship, one distant goal (a temple or pyramid) and scattered villages, camps, ruins, caves, mines and shrines placed where they belong. Map types are jungle, savanna, desert, arctic and volcanic. When **Map type** is **All in turn**, each map is revealed in the next hex by hex behind a fog front that takes about 14 seconds. Tiles are animated with waves and foam, swaying palms and pines, rising smoke, flickering fires, geysers, glowing lava and weather.

The tile art is drawn by Ilium from shaded vector shapes and then dithered to Braille by the host; no game artwork is used. Every frame is a pure function of time and settings, so the same seed always generates the same maps.

| Control | Range or values | Default | Shown when | Effect |
| --- | --- | --- | --- | --- |
| Map type | All in turn, Jungle, Savanna, Desert, Arctic, Volcanic | All in turn | always | Which kind of expedition map to draw. |
| Seconds per map | 20 to 300 s, step 10 | 60 s | Map type is All in turn | How long each map type stays before the next is revealed. |
| Hex size | 10 to 40 dots, step 2 | 20 | always | Radius of one hex in Braille dots. Small hexes show more map; large ones show more sprite detail. |
| Pan speed | 0 to 300 %, step 10 | 100 % | always | Camera travel speed, on top of the global Speed. 0 % holds the camera still. |
| Pan route | Wandering route, East, North-east, South | Wandering route | always | A slowly curving expedition route, or a straight pan. |
| Tile animation | 0 to 300 %, step 10 | 100 % | always | Speed of waves, swaying trees, smoke, lava and fires. 0 % freezes the tiles. |
| Stepped tile frames | on / off | on | always | Plays tile animation as a few hand-drawn frames per second (6) like pixel art. The camera stays smooth. |
| Landmarks | 0 to 200 %, step 10 | 100 % | always | How many villages, camps, ruins, caves, mines and shrines each island has. Every island keeps its one ship and one goal while this is above 0 %. |
| Weather | on / off | on | always | Falling snow, rising embers, blown sand or fireflies, depending on the map type. |
| Cloud shadows | on / off | on | always | Shadows of drifting clouds pass over the map. |
| Brightness | 50 to 150 %, step 5 | 100 % | always | Scales tile brightness before the global dither is applied. |
| Biome colors | on / off | on | always | Colours each tile by terrain. Off uses your global palette. |
| Seed number | 0 to 999 | 7 | always | Selects the world. The same seed always generates the same maps. |

Tips: for a still postcard, set Pan speed 0 %, Tile animation 0 % and Weather off. For an exploration feel, keep Pan route on the wandering route and Landmarks above 100 %. Single map types are good when you want a consistent colour mood.

## Vector TD

A full-screen tower defence that plays itself, inspired by the [Vector TD](https://www.crazygames.com/game/vector-td) browser game. An AI places, upgrades and unlocks glowing vector towers against waves of monsters, sends waves early when its defence is strong, and moves from wave to wave, map to map and level to level while towers and monsters grow stronger. The towers, monsters and maps are Ilium's own. The standalone demo credits the design inspiration at the bottom right; everyday use never shows it.

How the game works:

- **Levels and waves.** A level is a set of waves (every tenth wave brings a boss). Clearing a level moves on to the next map and level. Later levels start with more money, stronger monsters and more tower types unlocked.
- **Towers.** The AI builds from eight tower types: Pulse, Needle and Chill from the start; Nova from the second level; Arc from the third; Lancer and Beacon from the fourth; Hive from the fifth. Tower damage also grows with the tech earned so far.
- **Monsters.** Drones, fast Darts, armoured Shells, Splitters (which break into Shards), Swarms, flying Wisps and Bosses.
- **Difficulty** scales monster health: Easy 0.8x, Normal 1.0x, Hard 1.3x. On Hard the AI sometimes loses a level and retries it with stronger towers.
- **Maps.** Six hand-designed axis-aligned paths: Switchback, Spiral, Comb, Twin gates, Staircase and Serpent. With **Map** set to All in turn, maps rotate as levels are cleared; the seed picks which map the cycle starts on.
- Changing any setting that alters what is played restarts the game (Map, Start level, Difficulty, Seed). The other settings only change how the game looks or how fast it runs, so a running game survives those edits.

| Control | Range or values | Default | Shown when | Effect |
| --- | --- | --- | --- | --- |
| Map | All in turn, Switchback, Spiral, Comb, Twin gates, Staircase, Serpent | All in turn | always | Play every map in turn, or stay on one. Restarts. |
| Start level | 1 to 12 | 1 | always | Later levels start richer, with stronger monsters and more towers unlocked. Restarts. |
| Waves per level | 5 to 40 | 20 | always | Waves to survive before a level is cleared; every tenth wave is a boss. |
| Difficulty | Easy, Normal, Hard | Normal | always | Monster health multiplier. Restarts. |
| Game speed | 25 to 400 %, step 25 | 100 % | always | How fast the game plays, on top of the global Speed. |
| Colours | Black and white, Colour | Colour | always | Black and white uses only the global palette; Colour gives every tower, monster and shot its own colour. |
| Colour scheme | Neon, Cool, Warm, Phosphor | Neon | Colours is Colour | Neon: cyan, magenta, yellow and green on black (the original vector look). Cool and Warm shift the base colours. Phosphor: everything in shades of one green. |
| Brightness | 20 to 200 %, step 5 | 100 % | always | How bright lines and fills are; lower values thin the dots out. |
| Contrast | 50 to 200 %, step 5 | 100 % | always | Separates dim and bright parts, such as grid against towers. |
| Hue | 0 to 360 deg, step 10 | 0 | Colours is Colour | Rotates every colour around the colour wheel. |
| Saturation | 0 to 200 %, step 5 | 100 % | Colours is Colour | 0 is grey, 100 the scheme as designed, 200 as vivid as possible. |
| Glow | 0 to 100 %, step 5 | 45 % | always | How strongly towers, monsters and the path are filled rather than only outlined. |
| Grid | on / off | on | always | The faint cell grid of the board. |
| Range rings | on / off | off | always | Always show every tower's firing range, not only when built or upgraded. |
| Status line | on / off | on | always | Level, wave, money and lives along the top. |
| Seed | 0 to 999 | 0 | always | Seeds the random details (spawn jitter, missile spread). Restarts. |

Examples: a relaxed, endless watch is Normal difficulty, Game speed 100 %, All in turn. To see late-game towers straight away set Start level 5 or higher. For a retro green-screen look set Colour scheme to Phosphor, or choose Black and white with Glow around 20 %.

## Related pages

- [Animations](animations.md): enabling backgrounds, the shared look, and the full list of scenes.
- [Nature and abstract scenes](animations-nature-and-abstract.md)
- [Maps, space and live data](animations-maps-space-and-live-data.md)
- [Settings](settings.md): all Settings tabs and where project settings are stored.
- [Demo gallery](demos.md): recordings, including the animations tour.
