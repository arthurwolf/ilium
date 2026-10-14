//! Runtime-dispatched threshold packing for dense animation rasters.

use std::sync::OnceLock;

const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];

type PackFn = fn(&[f32], &[f32], usize, usize, f32, &mut [u8]);

static PACK_FN: OnceLock<PackFn> = OnceLock::new();

pub(super) fn pack_thresholded(
    dots: &[f32],
    thresholds: &[f32],
    width: usize,
    height: usize,
    density: f32,
    cells: &mut [u8],
) {
    let pack = *PACK_FN.get_or_init(select_pack);
    pack(dots, thresholds, width, height, density, cells);
}

fn select_pack() -> PackFn {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::arch::is_x86_feature_detected!("avx2") {
        return pack_avx2;
    }
    pack_scalar
}

fn pack_scalar(
    dots: &[f32],
    thresholds: &[f32],
    width: usize,
    height: usize,
    density: f32,
    cells: &mut [u8],
) {
    let row_width = width * 2;
    for (cell_row, row_cells) in cells.chunks_exact_mut(width).take(height).enumerate() {
        row_cells.fill(0);
        for (dy, bits) in BITS.iter().enumerate() {
            let row_start = (cell_row * 4 + dy) * row_width;
            for (column, cell) in row_cells.iter_mut().enumerate() {
                let index = row_start + column * 2;
                *cell |= u8::from(dots[index] * density > thresholds[index]) * bits[0]
                    | u8::from(dots[index + 1] * density > thresholds[index + 1]) * bits[1];
            }
        }
    }
}

#[cfg(target_arch = "x86")]
use std::arch::x86::{
    _mm256_cmp_ps, _mm256_loadu_ps, _mm256_movemask_ps, _mm256_mul_ps, _mm256_set1_ps, _CMP_GT_OQ,
};
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{
    _mm256_cmp_ps, _mm256_loadu_ps, _mm256_movemask_ps, _mm256_mul_ps, _mm256_set1_ps, _CMP_GT_OQ,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn pack_avx2(
    dots: &[f32],
    thresholds: &[f32],
    width: usize,
    height: usize,
    density: f32,
    cells: &mut [u8],
) {
    // Dispatch is selected once after runtime feature detection.
    unsafe { pack_avx2_inner(dots, thresholds, width, height, density, cells) };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn pack_avx2_inner(
    dots: &[f32],
    thresholds: &[f32],
    width: usize,
    height: usize,
    density: f32,
    cells: &mut [u8],
) {
    let row_width = width * 2;
    let vector_cells = width / 4 * 4;
    let density_vector = _mm256_set1_ps(density);
    for (cell_row, row_cells) in cells.chunks_exact_mut(width).take(height).enumerate() {
        row_cells.fill(0);
        for (dy, bits) in BITS.iter().enumerate() {
            let row_start = (cell_row * 4 + dy) * row_width;
            for cell_group in (0..vector_cells).step_by(4) {
                let sample_start = row_start + cell_group * 2;
                let dot_lanes = &dots[sample_start..sample_start + 8];
                let threshold_lanes = &thresholds[sample_start..sample_start + 8];
                // Predicate-to-bitmask materialization is the AVX2 filtering pattern described
                // by Beier et al. (https://link.springer.com/article/10.1007/s13222-022-00431-0);
                // their database results do not predict this renderer's speedup.
                let mask = unsafe {
                    let scaled = _mm256_mul_ps(_mm256_loadu_ps(dot_lanes.as_ptr()), density_vector);
                    _mm256_movemask_ps(_mm256_cmp_ps(
                        scaled,
                        _mm256_loadu_ps(threshold_lanes.as_ptr()),
                        _CMP_GT_OQ,
                    ))
                } as u8;
                for lane_cell in 0..4 {
                    let pair = (mask >> (lane_cell * 2)) & 0b11;
                    row_cells[cell_group + lane_cell] |=
                        (pair & 1) * bits[0] | ((pair >> 1) & 1) * bits[1];
                }
            }
            for column in vector_cells..width {
                let index = row_start + column * 2;
                row_cells[column] |= u8::from(dots[index] * density > thresholds[index]) * bits[0]
                    | u8::from(dots[index + 1] * density > thresholds[index + 1]) * bits[1];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pack_scalar, pack_thresholded};

    fn scalar_reference(
        dots: &[f32],
        thresholds: &[f32],
        width: usize,
        height: usize,
        density: f32,
    ) -> Vec<u8> {
        const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        let row_width = width * 2;
        let mut cells = vec![0; width * height];
        for cell_row in 0..height {
            for dy in 0..4 {
                let row = cell_row * 4 + dy;
                for column in 0..width {
                    for dx in 0..2 {
                        let index = row * row_width + column * 2 + dx;
                        if dots[index] * density > thresholds[index] {
                            cells[cell_row * width + column] |= BITS[dy][dx];
                        }
                    }
                }
            }
        }
        cells
    }

    #[test]
    fn runtime_threshold_packing_matches_scalar_for_cell_tails_and_non_finite_samples() {
        for width in [1, 3, 4, 5, 7, 8, 9, 160] {
            for height in [1, 3] {
                let sample_count = width * height * 8;
                let dots = (0..sample_count)
                    .map(|index| match index % 17 {
                        0 => f32::NAN,
                        1 => f32::INFINITY,
                        2 => f32::NEG_INFINITY,
                        _ => ((index * 37 % 103) as f32 - 1.0) / 101.0,
                    })
                    .collect::<Vec<_>>();
                let thresholds = (0..sample_count)
                    .map(|index| (index * 19 % 101) as f32 / 100.0)
                    .collect::<Vec<_>>();

                for density in [0.0, 0.45, 1.0] {
                    let expected = scalar_reference(&dots, &thresholds, width, height, density);
                    let mut actual = vec![0; width * height];
                    pack_thresholded(&dots, &thresholds, width, height, density, &mut actual);
                    assert_eq!(
                        actual, expected,
                        "width={width}, height={height}, density={density}"
                    );

                    let mut scalar = vec![0; width * height];
                    pack_scalar(&dots, &thresholds, width, height, density, &mut scalar);
                    assert_eq!(scalar, expected);

                    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                    if std::arch::is_x86_feature_detected!("avx2") {
                        let mut vector = vec![0; width * height];
                        super::pack_avx2(&dots, &thresholds, width, height, density, &mut vector);
                        assert_eq!(vector, expected, "AVX2 width={width}, height={height}");
                    }
                }
            }
        }
    }
}
