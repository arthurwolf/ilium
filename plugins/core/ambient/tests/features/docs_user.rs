//! The shared look, shared dithering and display controls, playback and semantic animation as the
//! user documentation (src/docs/animations.md) states them. The plugin must reproduce them (F-093,
//! F-094, F-103); these tests pin the numbers the controls must offer.

use crate::common::*;

/// The text of one `##`/`###` section of the animations user document, heading line excluded.
fn section(heading: &str) -> String {
    let doc = user_doc("animations.md");
    let start = doc
        .find(&format!("\n{heading}\n"))
        .unwrap_or_else(|| panic!("no section {heading}"))
        + heading.len()
        + 2;
    let rest = &doc[start..];
    let level = heading.chars().take_while(|symbol| *symbol == '#').count();
    let mut end = rest.len();
    for (offset, _) in rest.match_indices("\n#") {
        let hashes = rest[offset + 1..]
            .chars()
            .take_while(|symbol| *symbol == '#')
            .count();
        if hashes <= level {
            end = offset;
            break;
        }
    }
    rest[..end].to_string()
}

/// The items of a numbered list in `text`, in order, without the number.
fn numbered_items(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (number, rest) = line.split_once(". ")?;
            number.parse::<u32>().ok()?;
            Some(rest.trim().to_string())
        })
        .collect()
}

/// The first-column cells of a markdown table in `text`, without the header and the rule.
fn table_first_cells(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .collect()
}

fn table_cells(text: &str, first: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| {
            line.starts_with('|')
                && line.trim_matches('|').split('|').next().map(str::trim) == Some(first)
        })
        .unwrap_or_else(|| panic!("no row {first}"));
    line.trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

// --- palette ----------------------------------------------------------------------------------

/// REQ: F-093
#[test]
fn doc_f_093_there_are_thirty_eight_palettes_in_menu_order() {
    let palettes = numbered_items(&section("### Palette"));
    assert_eq!(palettes.len(), 38);
    assert!(section("### Palette").contains("There are 38 palettes, in menu order:"));
}

/// REQ: F-093
#[test]
fn doc_f_093_scene_colors_is_the_first_and_default_palette() {
    let palettes = numbered_items(&section("### Palette"));
    assert_eq!(palettes[0], "Scene colors");
    assert!(section("### Palette").contains("**Scene colors** (default)"));
}

/// REQ: F-093
#[test]
fn doc_f_093_palette_menu_starts_and_ends_with_the_documented_names() {
    let palettes = numbered_items(&section("### Palette"));
    assert_eq!(
        &palettes[..4],
        ["Scene colors", "Rainbow", "Pastel rainbow", "Cotton candy"]
    );
    assert_eq!(
        &palettes[34..],
        ["Gruvbox", "Tokyo Night", "Rose gold", "Slate"]
    );
}

/// REQ: F-093
#[test]
fn doc_f_093_palette_names_are_unique() {
    let mut palettes = numbered_items(&section("### Palette"));
    palettes.sort();
    palettes.dedup();
    assert_eq!(palettes.len(), 38);
}

/// REQ: F-093
#[test]
fn doc_f_093_the_color_blind_safe_palette_is_cividis() {
    let palettes = numbered_items(&section("### Palette"));
    assert_eq!(palettes[24], "Cividis (color-blind safe)");
}

// --- colour mode ------------------------------------------------------------------------------

/// REQ: F-093
#[test]
fn doc_f_093_colour_modes_are_color_greyscale_and_monotone_with_color_the_default() {
    let modes = table_first_cells(&section("### Colour mode"));
    assert_eq!(modes, ["Color (default)", "Greyscale", "Monotone"]);
}

/// REQ: F-093
#[test]
fn doc_f_093_grey_tint_and_ink_controls_keep_their_ranges_and_defaults() {
    let text = section("### Colour mode");
    assert!(text.contains(
        "**Grey tint hue** 0 to 359 degrees, default 40; **Grey tint** 0 to 100%, default 0"
    ));
    assert!(text.contains("**Ink lightness** (0 to 100%, default 60)"));
    assert!(text.contains("**Ink hue** (0 to 359 degrees, default 210)"));
    assert!(text.contains("**Ink saturation** (0 to 100%, default 0"));
    assert!(text.contains("153,153,153"));
}

// --- tone controls and colour source ---------------------------------------------------------

/// REQ: F-093
#[test]
fn doc_f_093_tone_controls_are_the_nine_documented_rows() {
    let rows = table_first_cells(&section("### Tone controls"));
    assert_eq!(
        rows,
        [
            "Brightness",
            "Contrast",
            "Gamma",
            "Color intensity",
            "Hue shift",
            "Invert colors",
            "Edge fade",
            "Pattern contrast",
            "Invert pattern"
        ]
    );
}

/// REQ: F-093
#[test]
fn doc_f_093_tone_control_ranges_and_defaults() {
    let text = section("### Tone controls");
    for (row, range) in [
        ("Brightness", "1 to 200%, 100"),
        ("Contrast", "0 to 200%, 100"),
        ("Gamma", "30 to 300%, 100"),
        ("Color intensity", "0 to 200%, 100"),
        ("Hue shift", "-180 to 180 degrees, 0"),
        ("Invert colors", "off"),
        ("Edge fade", "0 to 100%, 0"),
        ("Pattern contrast", "50 to 200%, 100"),
        ("Invert pattern", "off"),
    ] {
        assert_eq!(table_cells(&text, row)[1], range, "{row}");
    }
}

/// REQ: F-093
#[test]
fn doc_f_093_colour_source_rows_reverse_shift_and_spread_keep_their_ranges() {
    let text = section("### Colour from, reverse, shift and spread");
    assert_eq!(table_cells(&text, "Reverse")[1], "off");
    assert_eq!(table_cells(&text, "Palette shift")[1], "0 to 100%, 0");
    assert_eq!(table_cells(&text, "Palette spread")[1], "25 to 400%, 100");
    assert!(table_cells(&text, "Color from")[1].starts_with("choice, **Scene or coverage**"));
}

/// REQ: F-093
#[test]
fn doc_f_093_rows_that_do_not_apply_to_the_colour_mode_are_hidden() {
    let text = section("### Colour from, reverse, shift and spread");
    assert!(text.contains(
        "These rows are hidden in Monotone mode; Palette itself is hidden in Greyscale mode."
    ));
    assert!(section("## The shared look")
        .contains("Rows that do not apply to the current colour mode are hidden."));
}

/// REQ: F-093
#[test]
fn doc_f_093_colour_filter_strength_range_and_first_option() {
    let text = section("### Colour filters");
    assert!(text.contains("**Filter strength** (0 to 100%, default 100)"));
    assert_eq!(numbered_items(&text)[0], "None");
    assert_eq!(numbered_items(&text)[1], "Invert");
}

/// REQ: F-093
#[test]
fn doc_f_093_style_presets_start_with_custom_and_original() {
    let items = numbered_items(&section("### Style presets"));
    assert_eq!(
        &items[..5],
        [
            "Custom",
            "Original",
            "Whisper (barely visible)",
            "Subtle",
            "Quiet grey"
        ]
    );
}

/// A scene gets the shared look for free and adds no controls of its own for it.
///
/// REQ: F-093
#[test]
fn doc_f_093_the_look_is_global_and_scenes_must_not_add_their_own_colour_controls() {
    assert!(section("## The shared look")
        .contains("a scene must not add its own brightness, palette or saturation controls"));
}

// --- dithering ---------------------------------------------------------------------------------

/// REQ: F-094
#[test]
fn doc_f_094_there_are_fifteen_dither_methods_with_bayer_eight_by_eight_the_default() {
    let text = section("## Shared dithering");
    assert!(text.contains("There are 15 methods:"));
    assert!(text.contains("Default: **Ordered (Bayer 8x8)**"));
    assert_eq!(table_first_cells(&text).len(), 15);
}

/// REQ: F-094
#[test]
fn doc_f_094_dither_methods_in_menu_order() {
    let text = section("## Shared dithering");
    let methods: Vec<String> = text
        .lines()
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .nth(1)
                .expect("method")
                .trim()
                .to_string()
        })
        .collect();
    assert_eq!(
        methods,
        [
            "Ordered (Bayer 8x8)",
            "Stippled",
            "Coarse (Bayer 2x2)",
            "Bayer 4x4",
            "Fine (Bayer 16x16)",
            "Blue noise",
            "Gradient noise",
            "Halftone dots",
            "Scan lines",
            "Diagonal lines",
            "Crosshatch",
            "White noise",
            "Floyd-Steinberg",
            "Atkinson",
            "Sierra Lite"
        ]
    );
}

/// REQ: F-094
#[test]
fn doc_f_094_the_last_three_methods_are_error_diffusion() {
    let text = section("## Shared dithering");
    for method in ["Floyd-Steinberg", "Atkinson", "Sierra Lite"] {
        assert!(text.lines().any(|line| line.contains(method) && line.trim_end().ends_with("Error diffusion |")), "{method}");
    }
}

// --- display -----------------------------------------------------------------------------------

/// REQ: F-094
#[test]
fn doc_f_094_shared_display_rows_are_background_speed_density_cap_and_show_in() {
    let rows = table_first_cells(&section("## Shared display controls"));
    assert_eq!(
        rows,
        [
            "Background",
            "Speed",
            "Dot density",
            "Frame rate cap",
            "Show in"
        ]
    );
}

/// REQ: F-094
#[test]
fn doc_f_094_display_ranges_and_defaults() {
    let text = section("## Shared display controls");
    assert_eq!(table_cells(&text, "Background")[1], "off");
    assert_eq!(table_cells(&text, "Speed")[1], "25 to 300%, step 5, 100");
    assert_eq!(
        table_cells(&text, "Dot density")[1],
        "25 to 100%, step 5, 60"
    );
    assert_eq!(table_cells(&text, "Frame rate cap")[1], "0 to 30 fps, 0");
    assert_eq!(
        table_cells(&text, "Show in")[1],
        "Both panels, Left panel only, Right panel only; default Both"
    );
}

/// REQ: F-094
#[test]
fn doc_f_094_a_cap_of_zero_lets_each_scene_choose_its_own_rate() {
    assert!(
        section("## Shared display controls").contains("0 lets each scene choose its own rate.")
    );
}

/// REQ: F-094
#[test]
fn doc_f_094_the_shared_cap_is_an_upper_limit_on_top_of_scene_rates() {
    assert!(section("## Shared display controls")
        .contains("the shared cap is an upper limit on top of them."));
}

/// REQ: F-093, F-094
#[test]
fn doc_f_094_defaults_when_nothing_is_saved() {
    let text = section("## Saving: one global setting");
    assert!(text.contains(
        "Defaults when nothing is saved: Background off, scene **Wave washing up sand**, Speed 100%, Dot density 60%, Dither **Ordered (Bayer 8x8)**, Show in **Both panels**, frame-rate cap 0 (each scene's own rate), Playback **Loop** with a 60 second loop, Color mode **Color** with **Scene colors** and no preset."
    ));
}

/// REQ: F-093, COR-033
#[test]
fn doc_cor_033_animation_choices_are_one_global_setting_under_the_animation_key() {
    let text = section("## Saving: one global setting");
    assert!(text.contains("saved as soon as you change them in `~/.config/ilium/animation/.ilium/config.yaml`, under the `animation:` key."));
}

// --- playback and semantic ---------------------------------------------------------------------

/// REQ: F-093
#[test]
fn doc_f_093_playback_modes_are_loop_by_default_and_live() {
    let modes = table_first_cells(&section("## Playback: Loop and Live"));
    assert_eq!(modes, ["Loop (default)", "Live"]);
}

/// REQ: F-093
#[test]
fn doc_f_093_loop_seconds_range_default_and_cache_ceiling() {
    let text = section("## Playback: Loop and Live");
    assert!(text.contains("**Loop seconds** is 1 to 120 seconds, default 60."));
    assert!(text.contains("A 128 MiB packed-frame ceiling limits each cache."));
    assert!(
        text.contains("generation runs in a cancellable background worker at 30 frames per second")
    );
}

/// REQ: F-093
#[test]
fn doc_f_093_changing_colour_never_rebuilds_the_loop_cache() {
    assert!(section("## Playback: Loop and Live").contains(
        "Changing colour, palette, preset, filter, brightness or panel never rebuilds it."
    ));
}

/// REQ: F-103
#[test]
fn doc_f_103_semantic_scopes_are_project_by_default_and_entry() {
    let scopes = table_first_cells(&section("## Semantic animation"));
    assert_eq!(scopes, ["Project (default)", "Entry"]);
}

/// REQ: F-103
#[test]
fn doc_f_103_semantic_is_opt_in_and_changes_no_authored_settings() {
    let text = section("## Semantic animation");
    assert!(text.contains("Semantic is opt-in. It does not replace your authored animation settings or edit terminal content"));
    assert!(text.contains(
        "Changing the selection makes no extra AI request; the stored result is applied."
    ));
}

/// REQ: F-103
#[test]
fn doc_f_103_semantic_recommendations_cannot_set_paths_devices_or_credentials() {
    assert!(section("## Semantic animation")
        .contains("recommendations cannot set file paths, devices or credentials."));
}

/// REQ: F-103
#[test]
fn doc_f_103_without_a_valid_recommendation_the_status_line_asks_to_reorganize() {
    assert!(section("## Semantic animation")
        .contains("If the selected target has no valid saved recommendation, the status line says so and asks you to reorganize the project."));
}

/// REQ: F-103
#[test]
fn doc_f_103_semantic_varies_per_project_while_other_choices_are_global() {
    assert!(section("## Saving: one global setting")
        .contains("Only the Semantic animation varies per project"));
}
