//! Host implementation of `ilium:host/gpu` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostGpu` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::gpu::{GpuKernel, HostGpu};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `gpu` interface.
#[derive(Debug)]
pub struct GpuImport;

impl HostGpu for GpuImport {
    fn run(
        &mut self,
        _kernel: GpuKernel,
        _input: Vec<u8>,
        _width: u32,
        _height: u32,
        _params_json: String,
    ) -> Result<Vec<u8>, PluginError> {
        unimplemented!("host::imports::gpu::run")
    }

    fn available(&mut self) -> bool {
        unimplemented!("host::imports::gpu::available")
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
