//! Host implementation of `ilium:host/gpu`.
//!
//! Only the host's own kernels run (api-gpu). This import runs them on the CPU fallback, which
//! gives the same pixels a GPU path must give; `available` reports whether a GPU path is wired
//! in, and none is. Inputs and outputs are gray8 tone fields (one byte per pixel) except
//! `palette-map`, which returns RGB (three bytes per pixel).

use ilium_plugin_api::generated::host::gpu::{GpuKernel, HostGpu};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use serde_json::{Map, Value};

const BLUR_RADIUS_RANGE: (i64, i64) = (1, 64);
const BLUR_RADIUS_DEFAULT: i64 = 2;
const THRESHOLD_LEVEL_DEFAULT: i64 = 128;
const PALETTE_LEN_RANGE: (usize, usize) = (2, 256);
const DITHER_LEVELS_RANGE: (i64, i64) = (2, 256);
const SCALE_DIMENSION_RANGE: (i64, i64) = (1, 8192);

/// Host side of the `gpu` interface.
#[derive(Debug)]
pub struct GpuImport;

fn invalid_argument(message: impl Into<String>) -> PluginError {
    let message: String = message.into();
    PluginError {
        code: ErrorCode::InvalidArgument,
        message: message.chars().take(256).collect(),
        retry_after_ms: None,
    }
}

/// Reads an optional integer parameter in `range`; floats and strings are refused.
fn integer_param(
    params: &Map<String, Value>,
    key: &str,
    range: (i64, i64),
) -> Result<Option<i64>, PluginError> {
    let Some(value) = params.get(key) else {
        return Ok(None);
    };
    let number = value
        .as_i64()
        .ok_or_else(|| invalid_argument(format!("{key} must be an integer")))?;
    if number < range.0 || number > range.1 {
        return Err(invalid_argument(format!(
            "{key} must be {} to {}",
            range.0, range.1
        )));
    }
    Ok(Some(number))
}

fn parse_params(params_json: &str) -> Result<Map<String, Value>, PluginError> {
    match serde_json::from_str::<Value>(params_json) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(invalid_argument("params-json must be a JSON object")),
        Err(error) => Err(invalid_argument(format!("params-json is not JSON: {error}"))),
    }
}

/// Separable box blur with clamped edges; window is `2 * radius + 1` pixels.
fn blur(input: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    let window = (2 * radius + 1) as u32;
    let mut horizontal = vec![0_u8; input.len()];
    for row in 0..height {
        for column in 0..width {
            let mut sum = 0_u32;
            for offset in 0..=(2 * radius) {
                let source = (column + offset).saturating_sub(radius).min(width - 1);
                sum += u32::from(input[row * width + source]);
            }
            horizontal[row * width + column] = ((sum + window / 2) / window) as u8;
        }
    }
    let mut output = vec![0_u8; input.len()];
    for row in 0..height {
        for column in 0..width {
            let mut sum = 0_u32;
            for offset in 0..=(2 * radius) {
                let source = (row + offset).saturating_sub(radius).min(height - 1);
                sum += u32::from(horizontal[source * width + column]);
            }
            output[row * width + column] = ((sum + window / 2) / window) as u8;
        }
    }
    output
}

fn palette_map(input: &[u8], palette: &[[u8; 3]]) -> Vec<u8> {
    let last = palette.len() - 1;
    let mut output = Vec::with_capacity(input.len() * 3);
    for tone in input {
        let index = (usize::from(*tone) * last + 127) / 255;
        output.extend_from_slice(&palette[index]);
    }
    output
}

fn parse_palette(params: &Map<String, Value>) -> Result<Vec<[u8; 3]>, PluginError> {
    let entries = params
        .get("palette")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_argument("palette is required and must be an array"))?;
    if entries.len() < PALETTE_LEN_RANGE.0 || entries.len() > PALETTE_LEN_RANGE.1 {
        return Err(invalid_argument("palette must hold 2 to 256 entries"));
    }
    entries
        .iter()
        .map(|entry| {
            let triple = entry
                .as_array()
                .filter(|channels| channels.len() == 3)
                .ok_or_else(|| invalid_argument("palette entries are [r,g,b] triples"))?;
            let mut color = [0_u8; 3];
            for (slot, channel) in color.iter_mut().zip(triple) {
                *slot = channel
                    .as_u64()
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or_else(|| invalid_argument("palette channels are integers 0 to 255"))?;
            }
            Ok(color)
        })
        .collect()
}

/// Bayer index matrix of side `side` (a power of two), values `0..side * side`.
fn bayer_matrix(side: usize) -> Vec<u32> {
    let mut matrix = vec![0_u32];
    let mut current = 1;
    while current < side {
        let next = current * 2;
        let mut grown = vec![0_u32; next * next];
        for row in 0..current {
            for column in 0..current {
                let base = 4 * matrix[row * current + column];
                grown[row * next + column] = base;
                grown[row * next + column + current] = base + 2;
                grown[(row + current) * next + column] = base + 3;
                grown[(row + current) * next + column + current] = base + 1;
            }
        }
        matrix = grown;
        current = next;
    }
    matrix
}

fn dither_ordered(input: &[u8], width: usize, side: usize, levels: u32) -> Vec<u8> {
    let matrix = bayer_matrix(side);
    let cells = (side * side) as f64;
    let steps = f64::from(levels - 1);
    input
        .iter()
        .enumerate()
        .map(|(index, tone)| {
            let (row, column) = (index / width, index % width);
            let threshold = (f64::from(matrix[(row % side) * side + column % side]) + 0.5) / cells;
            let scaled = f64::from(*tone) / 255.0 * steps;
            let base = scaled.floor();
            let level = if scaled - base > threshold { base + 1.0 } else { base };
            (level / steps * 255.0).round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Nearest-neighbour resample sampling pixel centres.
fn scale(input: &[u8], width: usize, height: usize, out_width: usize, out_height: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(out_width * out_height);
    for row in 0..out_height {
        let source_row = (row * height / out_height).min(height - 1);
        for column in 0..out_width {
            let source_column = (column * width / out_width).min(width - 1);
            output.push(input[source_row * width + source_column]);
        }
    }
    output
}

impl HostGpu for GpuImport {
    fn run(
        &mut self,
        kernel: GpuKernel,
        input: Vec<u8>,
        width: u32,
        height: u32,
        params_json: String,
    ) -> Result<Vec<u8>, PluginError> {
        let params = parse_params(&params_json)?;
        if width == 0 || height == 0 {
            return Err(invalid_argument("width and height must be at least 1"));
        }
        let (width, height) = (width as usize, height as usize);
        let expected_len = width
            .checked_mul(height)
            .ok_or_else(|| invalid_argument("width times height overflows"))?;
        if input.len() != expected_len {
            return Err(invalid_argument(format!(
                "input holds {} bytes, expected {expected_len} (width * height)",
                input.len()
            )));
        }
        match kernel {
            GpuKernel::Blur => {
                let radius = integer_param(&params, "radius", BLUR_RADIUS_RANGE)?
                    .unwrap_or(BLUR_RADIUS_DEFAULT);
                Ok(blur(&input, width, height, radius as usize))
            }
            GpuKernel::Threshold => {
                let level = integer_param(&params, "level", (0, 255))?
                    .unwrap_or(THRESHOLD_LEVEL_DEFAULT);
                Ok(input
                    .iter()
                    .map(|tone| if i64::from(*tone) >= level { 255 } else { 0 })
                    .collect())
            }
            GpuKernel::PaletteMap => Ok(palette_map(&input, &parse_palette(&params)?)),
            GpuKernel::DitherOrdered => {
                let side = match params.get("matrix") {
                    None => 4,
                    Some(Value::String(name)) => match name.as_str() {
                        "bayer2" => 2,
                        "bayer4" => 4,
                        "bayer8" => 8,
                        _ => return Err(invalid_argument("matrix must be bayer2, bayer4 or bayer8")),
                    },
                    Some(_) => return Err(invalid_argument("matrix must be a string")),
                };
                let levels = integer_param(&params, "levels", DITHER_LEVELS_RANGE)?.unwrap_or(2);
                Ok(dither_ordered(&input, width, side, levels as u32))
            }
            GpuKernel::Scale => {
                let required = |key: &str| {
                    integer_param(&params, key, SCALE_DIMENSION_RANGE)?
                        .ok_or_else(|| invalid_argument(format!("{key} is required")))
                };
                let out_width = required("out-width")?;
                let out_height = required("out-height")?;
                Ok(scale(&input, width, height, out_width as usize, out_height as usize))
            }
        }
    }

    fn available(&mut self) -> bool {
        // No GPU path is linked; `run` always uses the CPU fallback.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::GpuImport;
    use ilium_plugin_api::generated::host::gpu::{GpuKernel, HostGpu};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

    /// Largest number of palette entries `palette-map` accepts (api-gpu, kernel parameter table).
    const PALETTE_MAX_ENTRIES: usize = 256;

    /// Largest `scale` output dimension and largest `levels` value the manual allows.
    const SCALE_MAX_DIMENSION: u32 = 8192;
    const LEVELS_MAX: u32 = 256;

    /// Builds a tone field of `width * height` bytes, one byte per pixel (the scene's own tone raster).
    fn tone_input(width: u32, height: u32) -> Vec<u8> {
        (0..(width as usize) * (height as usize))
            .map(|index| (index % 256) as u8)
            .collect()
    }

    /// Calls `run` on a fresh import with a valid tone field of the given size.
    fn run_kernel(
        kernel: GpuKernel,
        width: u32,
        height: u32,
        params_json: &str,
    ) -> Result<Vec<u8>, PluginError> {
        let mut gpu = GpuImport;
        gpu.run(
            kernel,
            tone_input(width, height),
            width,
            height,
            params_json.to_string(),
        )
    }

    /// Asserts that the call returned exactly the expected error code.
    fn assert_error_code(result: Result<Vec<u8>, PluginError>, expected: ErrorCode) {
        match result {
            Err(error) => assert_eq!(error.code, expected, "message: {}", error.message),
            Ok(output) => panic!("expected {expected:?}, got Ok with {} bytes", output.len()),
        }
    }

    /// Builds a `palette-map` parameter object with `count` `[r,g,b]` triples.
    fn palette_params(count: usize) -> String {
        let entries: Vec<String> = (0..count)
            .map(|index| format!("[{},{},{}]", index % 256, 0, 255 - (index % 256)))
            .collect();
        format!("{{\"palette\":[{}]}}", entries.join(","))
    }

    /// REQ: F-104
    #[test]
    fn available_returns_the_same_answer_on_repeated_calls() {
        let mut gpu = GpuImport;
        let first = gpu.available();
        let second = gpu.available();
        assert_eq!(first, second);
    }

    /// REQ: F-104
    #[test]
    fn run_blur_with_valid_input_returns_ok() {
        assert!(run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":2}"#).is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_threshold_with_valid_input_returns_ok() {
        assert!(run_kernel(GpuKernel::Threshold, 4, 4, r#"{"level":128}"#).is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_palette_map_with_valid_palette_returns_ok() {
        assert!(run_kernel(GpuKernel::PaletteMap, 4, 4, &palette_params(4)).is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_dither_ordered_with_valid_parameters_returns_ok() {
        assert!(run_kernel(
            GpuKernel::DitherOrdered,
            4,
            4,
            r#"{"matrix":"bayer8","levels":4}"#
        )
        .is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_scale_with_valid_output_size_returns_ok() {
        assert!(run_kernel(GpuKernel::Scale, 4, 4, r#"{"out-width":8,"out-height":2}"#).is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_single_pixel_input_returns_ok() {
        let mut gpu = GpuImport;
        let result = gpu.run(
            GpuKernel::Blur,
            vec![7],
            1,
            1,
            r#"{"radius":1}"#.to_string(),
        );
        assert!(result.is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_empty_params_object_uses_documented_defaults() {
        assert!(run_kernel(GpuKernel::Blur, 4, 4, "{}").is_ok());
        assert!(run_kernel(GpuKernel::Threshold, 4, 4, "{}").is_ok());
        assert!(run_kernel(GpuKernel::DitherOrdered, 4, 4, "{}").is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_ignores_unknown_parameter_keys() {
        assert!(run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":2,"unknown-key":true}"#).is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_blur_radius_at_lower_bound_returns_ok() {
        assert!(run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":1}"#).is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_blur_radius_at_upper_bound_returns_ok() {
        assert!(run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":64}"#).is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_blur_radius_below_lower_bound_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":0}"#),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_blur_radius_above_upper_bound_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":65}"#),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_blur_radius_not_an_integer_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":2.5}"#),
            ErrorCode::InvalidArgument,
        );
        assert_error_code(
            run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":"two"}"#),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_threshold_level_at_both_bounds_returns_ok() {
        assert!(run_kernel(GpuKernel::Threshold, 4, 4, r#"{"level":0}"#).is_ok());
        assert!(run_kernel(GpuKernel::Threshold, 4, 4, r#"{"level":255}"#).is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_threshold_level_outside_range_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Threshold, 4, 4, r#"{"level":256}"#),
            ErrorCode::InvalidArgument,
        );
        assert_error_code(
            run_kernel(GpuKernel::Threshold, 4, 4, r#"{"level":-1}"#),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_palette_map_with_palette_at_both_bounds_returns_ok() {
        assert!(run_kernel(GpuKernel::PaletteMap, 4, 4, &palette_params(2)).is_ok());
        assert!(run_kernel(
            GpuKernel::PaletteMap,
            4,
            4,
            &palette_params(PALETTE_MAX_ENTRIES)
        )
        .is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_palette_map_with_one_entry_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::PaletteMap, 4, 4, &palette_params(1)),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_palette_map_with_too_many_entries_is_invalid_argument() {
        assert_error_code(
            run_kernel(
                GpuKernel::PaletteMap,
                4,
                4,
                &palette_params(PALETTE_MAX_ENTRIES + 1),
            ),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_palette_map_without_palette_key_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::PaletteMap, 4, 4, "{}"),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_palette_map_with_bad_triple_is_invalid_argument() {
        assert_error_code(
            run_kernel(
                GpuKernel::PaletteMap,
                4,
                4,
                r#"{"palette":[[0,0],[255,255,255]]}"#,
            ),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_dither_ordered_accepts_each_matrix_name() {
        for matrix_name in ["bayer2", "bayer4", "bayer8"] {
            let params = format!("{{\"matrix\":\"{matrix_name}\"}}");
            assert!(
                run_kernel(GpuKernel::DitherOrdered, 4, 4, &params).is_ok(),
                "matrix {matrix_name}"
            );
        }
    }

    /// REQ: F-104
    #[test]
    fn run_dither_ordered_unknown_matrix_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::DitherOrdered, 4, 4, r#"{"matrix":"bayer3"}"#),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_dither_ordered_levels_at_both_bounds_returns_ok() {
        assert!(run_kernel(GpuKernel::DitherOrdered, 4, 4, r#"{"levels":2}"#).is_ok());
        assert!(run_kernel(
            GpuKernel::DitherOrdered,
            4,
            4,
            &format!("{{\"levels\":{LEVELS_MAX}}}")
        )
        .is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_dither_ordered_levels_outside_range_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::DitherOrdered, 4, 4, r#"{"levels":1}"#),
            ErrorCode::InvalidArgument,
        );
        assert_error_code(
            run_kernel(
                GpuKernel::DitherOrdered,
                4,
                4,
                &format!("{{\"levels\":{}}}", LEVELS_MAX + 1),
            ),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_scale_output_size_at_both_bounds_returns_ok() {
        assert!(run_kernel(GpuKernel::Scale, 4, 4, r#"{"out-width":1,"out-height":1}"#).is_ok());
        assert!(run_kernel(
            GpuKernel::Scale,
            4,
            4,
            &format!("{{\"out-width\":{SCALE_MAX_DIMENSION},\"out-height\":1}}")
        )
        .is_ok());
    }

    /// REQ: F-104
    #[test]
    fn run_scale_output_size_outside_range_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Scale, 4, 4, r#"{"out-width":0,"out-height":1}"#),
            ErrorCode::InvalidArgument,
        );
        assert_error_code(
            run_kernel(
                GpuKernel::Scale,
                4,
                4,
                &format!(
                    "{{\"out-width\":1,\"out-height\":{}}}",
                    SCALE_MAX_DIMENSION + 1
                ),
            ),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_scale_without_output_size_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Scale, 4, 4, "{}"),
            ErrorCode::InvalidArgument,
        );
        assert_error_code(
            run_kernel(GpuKernel::Scale, 4, 4, r#"{"out-width":8}"#),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_malformed_params_json_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Blur, 4, 4, r#"{"radius":"#),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_empty_params_string_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Blur, 4, 4, ""),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_params_that_are_not_a_json_object_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Blur, 4, 4, "[2]"),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_zero_width_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Blur, 0, 4, "{}"),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_zero_height_is_invalid_argument() {
        assert_error_code(
            run_kernel(GpuKernel::Blur, 4, 0, "{}"),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: F-104
    #[test]
    fn run_empty_input_with_zero_dimensions_is_invalid_argument() {
        let mut gpu = GpuImport;
        let result = gpu.run(GpuKernel::Blur, Vec::new(), 0, 0, "{}".to_string());
        assert_error_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: F-104
    #[test]
    fn run_invalid_argument_message_is_at_most_256_characters_without_retry_hint() {
        let result = run_kernel(GpuKernel::Blur, 0, 4, "{}");
        match result {
            Err(error) => {
                assert!(
                    error.message.chars().count() <= 256,
                    "message too long: {}",
                    error.message.chars().count()
                );
                assert_eq!(error.retry_after_ms, None);
            }
            Ok(_) => panic!("expected invalid-argument, got Ok"),
        }
    }
}
