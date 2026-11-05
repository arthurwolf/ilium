---
layout: page
title: Accessibility of scene, glyph-plane and canvas panes
permalink: /pages/a11y-non-text-panes/
---

## Accessibility of scene, glyph-plane and canvas panes

This page specifies how a pane that draws cells without text reaches screen-reader and keyboard users. A scene or glyph-plane pane paints shapes, pixels or animated cells, and a screen reader cannot read those cells on their own. The rules below require every such pane to carry a short description, a status line that states its current condition, keyboard-only controls and a limit on motion. Plugin authors need these rules when they write a scene plugin, and reviewers and accessibility testers need them to check a release.

## Which panes are non-text

A non-text pane is a pane whose content is drawn as cells that carry no readable words. Scene panes and glyph-plane panes are non-text panes, because a plugin fills them with frames of cells that describe an image, an animation or a game board. The host treats these panes differently from text panes, which show terminal output and are read through the normal terminal path. The rules on this page apply to every scene or glyph-plane pane, whether the plugin ships with Ilium or is installed later. See [Scenes](/pages/api-scenes/) and [Glyph plane surfaces](/pages/api-surface-glyph-plane/) for how these panes are built.

## Required description and status line

A scene or glyph-plane plugin must declare a `description` field in its manifest. The description must be between 1 and 200 characters long. It states what the pane shows in plain words, such as "Animated weather map of the current region". When the manifest has no `description`, install refuses the plugin. The refusal exits with a non-zero code and prints the error code `manifest.description-missing`. See [Manifest reference](/pages/reference-manifest/) for the other manifest fields.

The plugin also reports a state line through the `scene.state` call. The state line is one short sentence that gives the current condition of the scene. Every required piece of information must appear in that line. In screen-reader mode, the host prints the manifest description followed by the current state line. It does not print the cell grid. Users who rely on speech therefore hear what the pane is and what it is doing, and nothing else about the drawing.

## Keyboard control of scenes

Every scene control must be reachable with the keyboard alone. A control that only responds to a pointer leaves keyboard users unable to change the scene. Each control therefore needs a key binding, and the pointer binding is an addition to it, never a replacement. Authors check this with `ilium plugin lint`. The lint command rejects a control that has a pointer binding and no key binding, and it prints the error code `ui.mouse-only-control` followed by the name of the control. Fix the lint error by adding a key binding to the control, then run the lint command again until it passes. For the key rules in general, see [Keyboard and focus](/pages/a11y-keyboard-and-focus/).

## Linear mode output for a non-text pane

Screen-reader mode replaces the cell grid of a scene with linear text. The linear output contains the manifest description, then the state line that `scene.state` returns. It contains no cell grid at all. A tester can confirm this by enabling screen-reader mode, opening a scene pane in the test harness and reading the linear output. The output must show the description first and the state line second, with no grid rows between them. For the general rules on screen-reader output, see [Screen-reader mode and localized text](/pages/a11y-screen-reader-and-localized-text/).

When a scene changes its condition, the state line changes with it. A scene that moves into an alert state must return a new state line in the same frame in which its cells change. A screen-reader user never hears a state that lags behind what the cells show.

## Reduced motion for scenes

When the reduced motion setting is on, a scene is limited to at most one frame per second, or to one static frame. A scene must stay within that limit whatever its own animation timing. A tester can check the limit by enabling reduced motion, running a scene for 10 seconds in the test harness and reading the frame counter. The counter must show at most 10 frames for that run. See [Plugin test harness](/pages/reference-plugin-test-harness/) for how the harness runs a scene and reports its frames.

## Checklist

1. Confirm that the manifest of each scene or glyph-plane plugin has a `description` between 1 and 200 characters.
2. Install a scene plugin without a `description` and confirm that the install exits non-zero and prints `manifest.description-missing`.
3. Turn on screen-reader mode, open a scene pane and confirm that the linear output shows the description and then the state line, with no cell grid.
4. Declare a test control with a pointer binding and no key binding, run `ilium plugin lint` and confirm that it prints `ui.mouse-only-control` with the control name.
5. Move a scene into an alert state and confirm that `scene.state` changes in the same frame as the cells.
6. Turn on reduced motion, run a scene for 10 seconds and confirm that the frame counter shows at most 10 frames.

## Requirements

- **[PDT-017]** A scene or glyph-plane pane MUST declare a `description` of 1 to 200 characters in its manifest, and install MUST refuse a manifest without one with `manifest.description-missing`.
- **[PDT-018]** In screen-reader mode the host MUST print the description and the current scene state line instead of cells.
- **[PDT-019]** With reduced motion on, a scene MUST be limited to at most 1 frame per second or one static frame.
- **[PDT-020]** Every scene control MUST be reachable with the keyboard only, and `ilium plugin lint` MUST reject a mouse-only control with `ui.mouse-only-control`.
- **[PDT-021]** A scene MUST NOT convey required information in cells alone; every required state MUST appear in the state line.
