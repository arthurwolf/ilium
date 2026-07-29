#!/usr/bin/env bun

/**
 * Displays a live, dependency-free gallery of one-cell Braille animations.
 *
 * Run:
 *     bun docs/scripts/ilium-braille-pattern-gallery.ts
 *
 * Optional:
 *     bun docs/scripts/ilium-braille-pattern-gallery.ts --filter orbit
 *     bun docs/scripts/ilium-braille-pattern-gallery.ts --list
 */

interface Point {
    row: number;
    column: number;
}

interface Pattern {
    name: string;
    family: PatternFamily;
    frames: readonly string[];
    frame_duration_ms: number;
}

interface Options {
    filter: string;
    is_color_enabled: boolean;
    is_help_requested: boolean;
    is_list_requested: boolean;
    refresh_rate_fps: number;
}

type PatternFamily =
    | "classic"
    | "orbit"
    | "scan"
    | "wave"
    | "pulse"
    | "fill"
    | "twinkle"
    | "tour";

// Braille's Unicode bit order is irregular on the fourth row, so this map
// keeps every geometric generator below expressed as readable 2x4 points.
const BRAILLE_BIT_BY_POSITION: readonly (readonly number[])[] = [
    [0, 3],
    [1, 4],
    [2, 5],
    [6, 7],
];

const CLOCKWISE_RIM: readonly Point[] = [
    { row: 0, column: 0 },
    { row: 0, column: 1 },
    { row: 1, column: 1 },
    { row: 2, column: 1 },
    { row: 3, column: 1 },
    { row: 3, column: 0 },
    { row: 2, column: 0 },
    { row: 1, column: 0 },
];

const SNAKE_PATH: readonly Point[] = [
    { row: 0, column: 0 },
    { row: 0, column: 1 },
    { row: 1, column: 1 },
    { row: 1, column: 0 },
    { row: 2, column: 0 },
    { row: 2, column: 1 },
    { row: 3, column: 1 },
    { row: 3, column: 0 },
];

const BOUNCING_ROWS: readonly number[] = [0, 1, 2, 3, 2, 1];
const ALL_ROWS: readonly number[] = [0, 1, 2, 3];
const ALL_COLUMNS: readonly number[] = [0, 1];

const FAMILY_COLORS: Readonly<Record<PatternFamily, string>> = {
    classic: "\u001b[38;5;81m",
    orbit: "\u001b[38;5;117m",
    scan: "\u001b[38;5;221m",
    wave: "\u001b[38;5;213m",
    pulse: "\u001b[38;5;203m",
    fill: "\u001b[38;5;149m",
    twinkle: "\u001b[38;5;183m",
    tour: "\u001b[38;5;250m",
};

const ANSI_RESET = "\u001b[0m";
const ANSI_BOLD = "\u001b[1m";
const ANSI_DIM = "\u001b[2m";
const ANSI_ALT_SCREEN_ENTER = "\u001b[?1049h";
const ANSI_ALT_SCREEN_LEAVE = "\u001b[?1049l";
const ANSI_CURSOR_HIDE = "\u001b[?25l";
const ANSI_CURSOR_SHOW = "\u001b[?25h";
const ANSI_CLEAR_AND_HOME = "\u001b[2J\u001b[H";

/**
 * Converts a set of geometric dots into one Unicode Braille cell.
 */
function braille_from_points(points: readonly Point[]): string {
    let mask = 0;

    for (const point of points) {
        const bit = BRAILLE_BIT_BY_POSITION[point.row]?.[point.column];

        if (bit === undefined) {
            throw new Error(`Invalid Braille point (${point.row}, ${point.column})`);
        }

        mask |= 1 << bit;
    }

    return String.fromCodePoint(0x2800 + mask);
}

/**
 * Converts an 8-bit Braille mask directly into its Unicode cell.
 */
function braille_from_mask(mask: number): string {
    return String.fromCodePoint(0x2800 + (mask & 0xff));
}

/**
 * Makes a row-wide frame at the requested vertical position.
 */
function row_frame(row: number): string {
    return braille_from_points(ALL_COLUMNS.map((column) => ({ row, column })));
}

/**
 * Makes a column-high frame at the requested horizontal position.
 */
function column_frame(column: number): string {
    return braille_from_points(ALL_ROWS.map((row) => ({ row, column })));
}

/**
 * Selects a moving window on a cyclic path, producing orbit and comet forms.
 */
function cyclic_window_frames(path: readonly Point[], window_size: number): string[] {
    return path.map((_point, head_index) => {
        const points: Point[] = [];

        for (let trail_offset = 0; trail_offset < window_size; trail_offset += 1) {
            const point_index = (head_index - trail_offset + path.length) % path.length;

            points.push(path[point_index]);
        }

        return braille_from_points(points);
    });
}

/**
 * Accumulates path dots, optionally draining them again for a breathing fill.
 */
function cumulative_path_frames(path: readonly Point[], is_draining: boolean): string[] {
    const fill_frames = path.map((_point, index) => braille_from_points(path.slice(0, index + 1)));

    if (!is_draining) {
        return fill_frames;
    }

    // Excluding both endpoints prevents a visible pause at empty and full.
    const drain_frames = fill_frames.slice(0, -1).reverse().slice(1);

    return [...fill_frames, ...drain_frames];
}

/**
 * Builds a two-rail wave with independently moving left and right dots.
 */
function rail_wave_frames(
    left_rows: readonly number[],
    right_rows: readonly number[],
): string[] {
    const frame_count = Math.max(left_rows.length, right_rows.length);
    const frames: string[] = [];

    for (let frame_index = 0; frame_index < frame_count; frame_index += 1) {
        frames.push(
            braille_from_points([
                { row: left_rows[frame_index % left_rows.length], column: 0 },
                { row: right_rows[frame_index % right_rows.length], column: 1 },
            ]),
        );
    }

    return frames;
}

/**
 * Produces deterministic pseudo-random cells so the gallery never changes
 * character merely because it was restarted.
 */
function deterministic_frames(seed: number, frame_count: number, maximum_dots?: number): string[] {
    let state = seed >>> 0;
    const frames: string[] = [];

    while (frames.length < frame_count) {
        // Xorshift32 is tiny, repeatable, and sufficient for visual texture.
        state ^= state << 13;
        state ^= state >>> 17;
        state ^= state << 5;

        let mask = state & 0xff;

        if (maximum_dots !== undefined) {
            const selected_bits: number[] = [];

            for (let bit = 0; bit < 8; bit += 1) {
                if ((mask & (1 << bit)) !== 0) {
                    selected_bits.push(bit);
                }
            }

            mask = selected_bits
                .slice(0, maximum_dots)
                .reduce((selected_mask, bit) => selected_mask | (1 << bit), 0);
        }

        // A blank frame reads as a dropped render, so retain at least one dot.
        frames.push(braille_from_mask(mask === 0 ? 1 : mask));
    }

    return frames;
}

/**
 * Creates density ramps with a stable shuffled dot order.
 */
function density_ramp_frames(dot_order: readonly number[], is_symmetric: boolean): string[] {
    const fill_frames = dot_order.map((_dot, index) => {
        const mask = dot_order
            .slice(0, index + 1)
            .reduce((selected_mask, bit) => selected_mask | (1 << bit), 0);

        return braille_from_mask(mask);
    });

    return is_symmetric
        ? [...fill_frames, ...fill_frames.slice(0, -1).reverse().slice(1)]
        : fill_frames;
}

/**
 * Registers a pattern while keeping the catalogue declarations compact.
 */
function pattern(
    name: string,
    family: PatternFamily,
    frames: readonly string[],
    frame_duration_ms = 90,
): Pattern {
    return { name, family, frames, frame_duration_ms };
}

const top_pair = row_frame(0);
const upper_middle_pair = row_frame(1);
const lower_middle_pair = row_frame(2);
const bottom_pair = row_frame(3);
const left_column = column_frame(0);
const right_column = column_frame(1);
const all_dots = braille_from_mask(0xff);
const checker_a = braille_from_points([
    { row: 0, column: 0 },
    { row: 1, column: 1 },
    { row: 2, column: 0 },
    { row: 3, column: 1 },
]);
const checker_b = braille_from_points([
    { row: 0, column: 1 },
    { row: 1, column: 0 },
    { row: 2, column: 1 },
    { row: 3, column: 0 },
]);
const corners = braille_from_points([
    { row: 0, column: 0 },
    { row: 0, column: 1 },
    { row: 3, column: 0 },
    { row: 3, column: 1 },
]);
const middle_block = braille_from_points([
    { row: 1, column: 0 },
    { row: 1, column: 1 },
    { row: 2, column: 0 },
    { row: 2, column: 1 },
]);
const clockwise_rim_reverse = [...CLOCKWISE_RIM].reverse();
const snake_reverse = [...SNAKE_PATH].reverse();

// The catalogue combines established terminal-spinner forms with geometric
// generators that cover the useful motion vocabulary available in one cell.
const PATTERNS: readonly Pattern[] = [
    pattern("Ilium classic", "classic", Array.from("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"), 90),
    pattern("Classic reverse", "classic", Array.from("⠏⠇⠧⠦⠴⠼⠸⠹⠙⠋"), 90),
    pattern("Dense wheel", "classic", Array.from("⣾⣽⣻⢿⡿⣟⣯⣷"), 85),
    pattern("Dense reverse", "classic", Array.from("⣷⣯⣟⡿⢿⣻⣽⣾"), 85),
    pattern("Soft loop", "classic", Array.from("⠄⠆⠇⠋⠙⠸⠰⠠⠰⠸⠙⠋⠇⠆"), 85),
    pattern("Angular loop", "classic", Array.from("⠋⠙⠚⠞⠖⠦⠴⠲⠳⠓"), 90),
    pattern("Horizontal loop", "classic", Array.from("⠁⠉⠙⠚⠒⠂⠒⠲⠴⠤⠄⠤⠴⠲⠒⠚⠙⠉"), 75),
    pattern("Vertical loop", "classic", Array.from("⡀⡄⡆⡇⣇⣧⣤⣠⣤⣧⣇⡇⡆⡄"), 85),

    pattern("Orbit clockwise", "orbit", cyclic_window_frames(CLOCKWISE_RIM, 1), 95),
    pattern("Orbit counter-CW", "orbit", cyclic_window_frames(clockwise_rim_reverse, 1), 95),
    pattern("Orbit pair", "orbit", cyclic_window_frames(CLOCKWISE_RIM, 2), 95),
    pattern("Pair counter-CW", "orbit", cyclic_window_frames(clockwise_rim_reverse, 2), 95),
    pattern("Orbit comet", "orbit", cyclic_window_frames(CLOCKWISE_RIM, 3), 95),
    pattern("Comet counter-CW", "orbit", cyclic_window_frames(clockwise_rim_reverse, 3), 95),
    pattern(
        "Opposed orbit",
        "orbit",
        CLOCKWISE_RIM.map((_point, index) =>
            braille_from_points([
                CLOCKWISE_RIM[index],
                CLOCKWISE_RIM[(index + 4) % CLOCKWISE_RIM.length],
            ]),
        ),
        105,
    ),
    pattern(
        "Corner orbit",
        "orbit",
        cyclic_window_frames(
            [
                { row: 0, column: 0 },
                { row: 0, column: 1 },
                { row: 3, column: 1 },
                { row: 3, column: 0 },
            ],
            1,
        ),
        130,
    ),
    pattern(
        "Quadrant orbit",
        "orbit",
        [
            braille_from_points([{ row: 0, column: 0 }, { row: 1, column: 0 }]),
            braille_from_points([{ row: 0, column: 1 }, { row: 1, column: 1 }]),
            braille_from_points([{ row: 2, column: 1 }, { row: 3, column: 1 }]),
            braille_from_points([{ row: 2, column: 0 }, { row: 3, column: 0 }]),
        ],
        135,
    ),
    pattern("Snake chase", "orbit", cyclic_window_frames(SNAKE_PATH, 2), 100),
    pattern("Snake reverse", "orbit", cyclic_window_frames(snake_reverse, 2), 100),

    pattern("Row scan down", "scan", ALL_ROWS.map(row_frame), 120),
    pattern("Row scan up", "scan", [...ALL_ROWS].reverse().map(row_frame), 120),
    pattern("Row bounce", "scan", BOUNCING_ROWS.map(row_frame), 110),
    pattern("Dot down left", "scan", ALL_ROWS.map((row) => braille_from_points([{ row, column: 0 }])), 120),
    pattern("Dot down right", "scan", ALL_ROWS.map((row) => braille_from_points([{ row, column: 1 }])), 120),
    pattern("Column scan", "scan", [left_column, right_column], 220),
    pattern("Column bounce", "scan", [left_column, right_column, right_column, left_column], 140),
    pattern("Snake scan", "scan", SNAKE_PATH.map((point) => braille_from_points([point])), 90),
    pattern("Snake scan reverse", "scan", snake_reverse.map((point) => braille_from_points([point])), 90),
    pattern(
        "Split scan",
        "scan",
        rail_wave_frames([0, 1, 2, 3], [3, 2, 1, 0]),
        115,
    ),

    pattern(
        "Band wave",
        "wave",
        BOUNCING_ROWS.map((row, index) =>
            braille_from_points([
                { row, column: 0 },
                { row, column: 1 },
                {
                    row: BOUNCING_ROWS[(index - 1 + BOUNCING_ROWS.length) % BOUNCING_ROWS.length],
                    column: 0,
                },
                {
                    row: BOUNCING_ROWS[(index - 1 + BOUNCING_ROWS.length) % BOUNCING_ROWS.length],
                    column: 1,
                },
            ]),
        ),
        110,
    ),
    pattern("Offset wave", "wave", rail_wave_frames(BOUNCING_ROWS, [1, 2, 3, 2, 1, 0]), 110),
    pattern(
        "Crossing wave",
        "wave",
        rail_wave_frames(
            [0, 1, 2, 3, 3, 2, 1, 0],
            [3, 2, 1, 0, 0, 1, 2, 3],
        ),
        110,
    ),
    pattern("Falling wave", "wave", rail_wave_frames([0, 1, 2, 3], [2, 3, 0, 1]), 120),
    pattern("Rising wave", "wave", rail_wave_frames([3, 2, 1, 0], [1, 0, 3, 2]), 120),
    pattern("Tight helix", "wave", rail_wave_frames([0, 1, 2, 3], [1, 2, 3, 0]), 105),
    pattern("Wide helix", "wave", rail_wave_frames([0, 1, 2, 3], [3, 0, 1, 2]), 105),
    pattern(
        "Row ripple",
        "wave",
        [
            top_pair,
            braille_from_mask(0x1b),
            braille_from_mask(0x3f),
            all_dots,
            braille_from_mask(0xf6),
            bottom_pair,
        ],
        100,
    ),
    pattern("Diagonal flip", "wave", [checker_a, checker_b], 170),
    pattern("Diagonal flicker", "wave", [checker_a, all_dots, checker_b, all_dots], 130),

    pattern("Center bloom", "pulse", [middle_block, all_dots, middle_block, braille_from_mask(0)], 145),
    pattern("Edge breathe", "pulse", [corners, middle_block], 180),
    pattern("Horizontal breathe", "pulse", [middle_block, all_dots, corners, all_dots], 150),
    pattern("Vertical breathe", "pulse", [left_column, all_dots, right_column, all_dots], 150),
    pattern("Checker pulse", "pulse", [checker_a, middle_block, checker_b, corners], 140),
    pattern("Full heartbeat", "pulse", [middle_block, all_dots, middle_block, all_dots, middle_block, middle_block], 105),
    pattern("Corner heartbeat", "pulse", [corners, all_dots, corners, corners, middle_block], 130),
    pattern("Hourglass pulse", "pulse", Array.from("⣶⢿⣶⣿⣶⡿"), 145),
    pattern("Pinch pulse", "pulse", Array.from("⣀⣤⣶⣿⣶⣤"), 130),
    pattern("Breathing dots", "pulse", density_ramp_frames([1, 4, 2, 5, 0, 3, 6, 7], true), 100),

    pattern("Clockwise fill", "fill", cumulative_path_frames(CLOCKWISE_RIM, false), 95),
    pattern("Clockwise breathe", "fill", cumulative_path_frames(CLOCKWISE_RIM, true), 90),
    pattern("Counter-CW fill", "fill", cumulative_path_frames(clockwise_rim_reverse, false), 95),
    pattern("Counter-CW breathe", "fill", cumulative_path_frames(clockwise_rim_reverse, true), 90),
    pattern("Snake fill", "fill", cumulative_path_frames(SNAKE_PATH, false), 95),
    pattern("Snake breathe", "fill", cumulative_path_frames(SNAKE_PATH, true), 90),
    pattern("Top-down fill", "fill", [top_pair, braille_from_mask(0x1b), braille_from_mask(0x3f), all_dots], 125),
    pattern("Bottom-up fill", "fill", [bottom_pair, braille_from_mask(0xe4), braille_from_mask(0xf6), all_dots], 125),
    pattern("Column wipe", "fill", [left_column, all_dots, right_column, braille_from_mask(0)], 155),
    pattern("Shuffled fill", "fill", density_ramp_frames([4, 0, 6, 3, 2, 7, 1, 5], true), 95),

    pattern("Sparse sparkle", "twinkle", deterministic_frames(0x1a2b3c4d, 24, 2), 100),
    pattern("Triple sparkle", "twinkle", deterministic_frames(0x5e6f7788, 24, 3), 95),
    pattern("Dense shimmer", "twinkle", deterministic_frames(0x9abcdeff, 32), 80),
    pattern("Slow shimmer", "twinkle", deterministic_frames(0x31415926, 20), 155),
    pattern("Falling rain", "twinkle", Array.from("⠁⠈⠂⠐⠄⠠⡀⢀"), 105),
    pattern("Rising embers", "twinkle", Array.from("⡀⢀⠄⠠⠂⠐⠁⠈"), 120),
    pattern("Alternating rain", "twinkle", Array.from("⠁⠐⠄⢀⠈⠂⠠⡀"), 115),
    pattern("Static crackle", "twinkle", deterministic_frames(0xdeadbeef, 48, 5), 55),

    // These two tours are exhaustive over the 256 possible Braille states.
    // Binary order changes many dots at carries; Gray order changes one dot.
    pattern(
        "All 256 states",
        "tour",
        Array.from({ length: 256 }, (_unused, mask) => braille_from_mask(mask)),
        35,
    ),
    pattern(
        "Gray-code tour",
        "tour",
        Array.from({ length: 256 }, (_unused, index) => braille_from_mask(index ^ (index >> 1))),
        35,
    ),
];

/**
 * Fails early if a catalogue edit introduces malformed or unusable frames.
 */
function validate_patterns(patterns: readonly Pattern[]): void {
    const names = new Set<string>();
    const frame_sequences = new Map<string, string>();

    for (const current_pattern of patterns) {
        if (names.has(current_pattern.name)) {
            throw new Error(`Duplicate pattern name: ${current_pattern.name}`);
        }

        names.add(current_pattern.name);

        const frame_sequence = current_pattern.frames.join("");
        const existing_pattern_name = frame_sequences.get(frame_sequence);

        if (existing_pattern_name !== undefined) {
            throw new Error(
                `Duplicate frame sequence: ${existing_pattern_name} / ${current_pattern.name}`,
            );
        }

        frame_sequences.set(frame_sequence, current_pattern.name);

        if (current_pattern.frames.length < 2) {
            throw new Error(`Pattern needs at least two frames: ${current_pattern.name}`);
        }

        for (const frame of current_pattern.frames) {
            if (Array.from(frame).length !== 1) {
                throw new Error(`Pattern frame is not one cell: ${current_pattern.name}`);
            }

            const code_point = frame.codePointAt(0);

            if (code_point === undefined || code_point < 0x2800 || code_point > 0x28ff) {
                throw new Error(`Pattern frame is not Braille: ${current_pattern.name}`);
            }
        }
    }
}

/**
 * Parses the small dependency-free command-line surface.
 */
function parse_options(arguments_to_parse: readonly string[]): Options {
    const remaining_arguments = [...arguments_to_parse];
    const options: Options = {
        filter: "",
        is_color_enabled: true,
        is_help_requested: false,
        is_list_requested: false,
        refresh_rate_fps: 30,
    };

    while (remaining_arguments.length > 0) {
        const argument = remaining_arguments.shift();

        if (argument === "--filter") {
            options.filter = remaining_arguments.shift() ?? "";
        } else if (argument === "--fps") {
            const requested_fps = Number(remaining_arguments.shift());

            if (!Number.isFinite(requested_fps) || requested_fps < 1 || requested_fps > 120) {
                throw new Error("--fps must be between 1 and 120");
            }

            options.refresh_rate_fps = requested_fps;
        } else if (argument === "--no-color") {
            options.is_color_enabled = false;
        } else if (argument === "--list") {
            options.is_list_requested = true;
        } else if (argument === "--help" || argument === "-h") {
            options.is_help_requested = true;
        } else {
            throw new Error(`Unknown option: ${argument ?? ""}`);
        }
    }

    return options;
}

/**
 * Keeps a label inside its dynamically allocated terminal cell.
 */
function fit_text(text: string, width: number): string {
    if (width <= 0) {
        return "";
    }

    if (text.length <= width) {
        return text.padEnd(width);
    }

    if (width === 1) {
        return "…";
    }

    return `${text.slice(0, width - 1)}…`;
}

/**
 * Colors one already-width-bounded cell without changing its layout width.
 */
function colorize(text: string, family: PatternFamily, is_color_enabled: boolean): string {
    if (!is_color_enabled) {
        return text;
    }

    return `${FAMILY_COLORS[family]}${text}${ANSI_RESET}`;
}

/**
 * Renders every selected pattern in terminal-height columns.
 */
function render_gallery(
    patterns: readonly Pattern[],
    options: Options,
    started_at_ms: number,
): string {
    const terminal_width = Math.max(process.stdout.columns ?? 100, 20);
    const terminal_height = Math.max(process.stdout.rows ?? 30, 8);
    const header_row_count = 3;
    const footer_row_count = 2;
    const available_pattern_rows = Math.max(terminal_height - header_row_count - footer_row_count, 1);
    const pattern_column_count = Math.ceil(patterns.length / available_pattern_rows);
    const pattern_cell_width = Math.max(Math.floor(terminal_width / pattern_column_count), 1);
    const rows_to_render = Math.min(available_pattern_rows, patterns.length);
    const elapsed_ms = Date.now() - started_at_ms;
    const output_lines: string[] = [];
    const title = `ilium Braille pattern gallery · ${patterns.length} patterns`;
    const subtitle = "One fixed-width cell each · q / Esc / Ctrl-C to quit";

    output_lines.push(
        options.is_color_enabled
            ? `${ANSI_BOLD}${fit_text(title, terminal_width)}${ANSI_RESET}`
            : fit_text(title, terminal_width),
    );
    output_lines.push(
        options.is_color_enabled
            ? `${ANSI_DIM}${fit_text(subtitle, terminal_width)}${ANSI_RESET}`
            : fit_text(subtitle, terminal_width),
    );
    output_lines.push("".padEnd(terminal_width, "─"));

    for (let row = 0; row < rows_to_render; row += 1) {
        const cells: string[] = [];

        for (let column = 0; column < pattern_column_count; column += 1) {
            const pattern_index = column * available_pattern_rows + row;
            const current_pattern = patterns[pattern_index];

            if (current_pattern === undefined) {
                cells.push(" ".repeat(pattern_cell_width));
                continue;
            }

            const frame_index =
                Math.floor(elapsed_ms / current_pattern.frame_duration_ms)
                % current_pattern.frames.length;
            const frame = current_pattern.frames[frame_index];
            const cell_text = fit_text(`${frame} ${current_pattern.name}`, pattern_cell_width);

            cells.push(colorize(cell_text, current_pattern.family, options.is_color_enabled));
        }

        output_lines.push(cells.join(""));
    }

    while (output_lines.length < terminal_height - footer_row_count) {
        output_lines.push("");
    }

    output_lines.push("".padEnd(terminal_width, "─"));
    output_lines.push(
        fit_text(
            "Families: classic · orbit · scan · wave · pulse · fill · twinkle · exhaustive tours",
            terminal_width,
        ),
    );

    // Avoiding a trailing newline prevents the terminal from scrolling the
    // alternate screen when the final row reaches its bottom-right corner.
    return `${ANSI_CLEAR_AND_HOME}${output_lines.join("\n")}`;
}

/**
 * Prints static frame sequences for review, piping, and automated checking.
 */
function print_pattern_list(patterns: readonly Pattern[]): void {
    const longest_name = Math.max(...patterns.map((current_pattern) => current_pattern.name.length));

    for (const current_pattern of patterns) {
        process.stdout.write(
            `${current_pattern.name.padEnd(longest_name)}  `
            + `${current_pattern.frames.join(" ")}  `
            + `(${current_pattern.frame_duration_ms} ms)\n`,
        );
    }
}

/**
 * Prints the command-line help.
 */
function print_help(): void {
    process.stdout.write(
        [
            "Usage: bun docs/scripts/ilium-braille-pattern-gallery.ts [options]",
            "",
            "Options:",
            "  --filter TEXT   Show names/families containing TEXT",
            "  --fps NUMBER    Redraw rate from 1 to 120 (default: 30)",
            "  --no-color      Disable ANSI family colors",
            "  --list          Print every frame sequence and exit",
            "  -h, --help      Show this help",
            "",
        ].join("\n"),
    );
}

validate_patterns(PATTERNS);

const options = parse_options(Bun.argv.slice(2));

if (options.is_help_requested) {
    print_help();
    process.exit(0);
}

const normalized_filter = options.filter.trim().toLocaleLowerCase();
const selected_patterns = PATTERNS.filter((current_pattern) => {
    if (normalized_filter === "") {
        return true;
    }

    return current_pattern.name.toLocaleLowerCase().includes(normalized_filter)
        || current_pattern.family.includes(normalized_filter);
});

if (selected_patterns.length === 0) {
    process.stderr.write(`No patterns match "${options.filter}".\n`);
    process.exit(1);
}

// Non-interactive output cannot update in place, so it falls back to the
// useful static representation rather than emitting endless ANSI frames.
if (options.is_list_requested || !process.stdout.isTTY) {
    print_pattern_list(selected_patterns);
    process.exit(0);
}

let is_cleaned_up = false;
const started_at_ms = Date.now();
const refresh_interval_ms = Math.max(Math.round(1000 / options.refresh_rate_fps), 8);

/**
 * Restores terminal state exactly once on every exit path.
 */
function clean_up_terminal(): void {
    if (is_cleaned_up) {
        return;
    }

    is_cleaned_up = true;

    if (process.stdin.isTTY) {
        process.stdin.setRawMode(false);
        process.stdin.pause();
    }

    process.stdout.write(`${ANSI_RESET}${ANSI_CURSOR_SHOW}${ANSI_ALT_SCREEN_LEAVE}`);
}

process.stdout.write(`${ANSI_ALT_SCREEN_ENTER}${ANSI_CURSOR_HIDE}`);

if (process.stdin.isTTY) {
    process.stdin.setRawMode(true);
    process.stdin.resume();
    process.stdin.on("data", (chunk: Buffer) => {
        const pressed_key = chunk.toString("utf8");

        if (pressed_key === "q" || pressed_key === "Q" || pressed_key === "\u001b" || pressed_key === "\u0003") {
            clean_up_terminal();
            process.exit(0);
        }
    });
}

process.on("SIGINT", () => {
    clean_up_terminal();
    process.exit(130);
});

process.on("SIGTERM", () => {
    clean_up_terminal();
    process.exit(143);
});

process.on("exit", clean_up_terminal);

const render_interval = setInterval(() => {
    process.stdout.write(render_gallery(selected_patterns, options, started_at_ms));
}, refresh_interval_ms);

// The first frame appears immediately rather than after one refresh period.
process.stdout.write(render_gallery(selected_patterns, options, started_at_ms));

// Retaining the interval reference documents that it intentionally owns the
// process lifetime; cleanup is driven by keyboard input or process signals.
void render_interval;
