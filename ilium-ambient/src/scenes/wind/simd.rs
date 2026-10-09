//! Runtime-dispatched particle integration for Wind's wrapped, gusted path.
//!
//! The scalar implementation is the portable contract. x86-64 kernels are
//! selected only after runtime CPU feature detection; all other targets use
//! the scalar path without requiring a separate build.
//!
//! Dense Structure-of-Arrays layouts are a standard SIMD technique; SoAx
//! motivates this layout experiment, not its measured performance:
//! https://arxiv.org/abs/1710.03462
//! Rust's runtime feature detection is documented here:
//! https://doc.rust-lang.org/std/arch/macro.is_x86_feature_detected.html

use super::sim::{Dot, MOUSE_RADIUS};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KernelPath {
    Scalar,
    Avx2,
    Avx512,
}

/// Reused SoA buffers keep SIMD particle state canonical between frames. SoAx
/// reports gains for its particle-wise integration workload; that result does
/// not establish the best layout for Wind's gathered cell-force field:
/// https://arxiv.org/abs/1710.03462. The scene keeps an AoS mirror for its
/// established renderer and non-SIMD physics.
#[derive(Default)]
pub(super) struct State {
    x: Vec<f32>,
    y: Vec<f32>,
    vx: Vec<f32>,
    vy: Vec<f32>,
    weight_rolls: Vec<f32>,
    pub(super) blocked: Vec<i32>,
    /// SoA positions stay canonical across frames; `Dot` is a read-boundary mirror.
    pub(super) loaded: bool,
    pub(super) dirty: bool,
    #[cfg(test)]
    pointer_scalar_fallback_blocks: usize,
}

impl State {
    fn prepare(
        &mut self,
        dots: &[Dot],
        inverse_masses: &[f32],
        force: &[[f32; 2]],
        mask: &[u8],
        width: usize,
        height: usize,
    ) -> bool {
        let valid_force = force.iter().all(|[x, y]| x.is_finite() && y.is_finite());
        let mut valid = valid_force;
        if self.loaded {
            if self.x.len() != dots.len() || inverse_masses.len() != dots.len() {
                return false;
            }
        } else {
            self.x.resize(dots.len(), 0.0);
            self.y.resize(dots.len(), 0.0);
            self.vx.resize(dots.len(), 0.0);
            self.vy.resize(dots.len(), 0.0);
            self.weight_rolls.resize(dots.len(), 0.0);
            for (index, dot) in dots.iter().enumerate() {
                self.x[index] = dot.x;
                self.y[index] = dot.y;
                self.vx[index] = dot.vx;
                self.vy[index] = dot.vy;
                self.weight_rolls[index] = dot.weight_roll;
                let cell_x = dot.x as usize;
                let cell_y = dot.y as usize;
                valid &= dot.x.is_finite()
                    && dot.y.is_finite()
                    && dot.vx.is_finite()
                    && dot.vy.is_finite()
                    && dot.weight_roll.is_finite()
                    && inverse_masses[index].is_finite()
                    && (0.0..width as f32).contains(&dot.x)
                    && (0.0..height as f32).contains(&dot.y)
                    && cell_x < width
                    && cell_y < height;
            }
        }
        if !self.loaded {
            self.blocked.resize(mask.len(), 0);
            for (target, &source) in self.blocked.iter_mut().zip(mask) {
                *target = i32::from(source != 0);
            }
        }
        valid
    }

    pub(super) fn flush_to_dots(&mut self, dots: &mut [Dot]) {
        if !self.dirty {
            return;
        }
        for (index, dot) in dots.iter_mut().enumerate() {
            dot.x = self.x[index];
            dot.y = self.y[index];
            dot.vx = self.vx[index];
            dot.vy = self.vy[index];
        }
        self.dirty = false;
    }

    pub(super) fn positions(&self) -> Option<(&[f32], &[f32])> {
        self.loaded.then_some((&self.x, &self.y))
    }

    pub(super) fn invalidate(&mut self) {
        debug_assert!(!self.dirty, "flush canonical SoA state before invalidation");
        self.loaded = false;
    }

    /// Integrates one SIMD substep when edge wrapping is active. Unsupported
    /// CPUs or modes return `None` before touching dots, so the caller can run
    /// its existing scalar path without adapter or allocation overhead.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn step_wrapped(
        &mut self,
        dots: &mut [Dot],
        inverse_masses: &[f32],
        force: &[[f32; 2]],
        collision_mask: &[u8],
        width: usize,
        height: usize,
        dt: f32,
        drag: f32,
        gravity: f32,
        bounce: f32,
        speed_limit: f32,
        aspect: f32,
        collisions: bool,
        pointer: Option<[f32; 2]>,
    ) -> Option<KernelPath> {
        let cell_count = width.checked_mul(height)?;
        if dots.len() != inverse_masses.len()
            || force.len() != cell_count
            || cell_count > (i32::MAX as usize / 8)
            || (collisions && collision_mask.len() != cell_count)
            || width == 0
            || height == 0
            || width > i32::MAX as usize
            || height > i32::MAX as usize
            || dt <= 0.0
            || speed_limit < 0.0
            || aspect <= 0.0
            || pointer.is_some_and(|[x, y]| {
                !x.is_finite()
                    || !y.is_finite()
                    || !(0.0..=1.0).contains(&x)
                    || !(0.0..=1.0).contains(&y)
            })
            || ![dt, drag, gravity, bounce, speed_limit, aspect]
                .iter()
                .all(|value| value.is_finite())
        {
            return None;
        }

        let path = selected_path();
        if path == KernelPath::Scalar {
            return None;
        }
        // Wrapped SIMD coordinates are corrected with one add/subtract. Since
        // integration clamps speed first, these bounds prove every lane stays
        // within that one-wrap range; tiny screens fall back to the scalar path.
        let max_dx = speed_limit * dt;
        let max_dy = max_dx / aspect;
        if max_dx >= width as f32 || max_dy >= height as f32 {
            return None;
        }
        if !self.prepare(dots, inverse_masses, force, collision_mask, width, height) {
            return None;
        }
        match path {
            KernelPath::Avx512 => {
                #[cfg(target_arch = "x86_64")]
                // SAFETY: selected_path checks AVX-512F and prepare validates
                // gathered coordinates; pointer-adjacent blocks use scalars.
                unsafe {
                    x86::step_avx512(
                        self,
                        inverse_masses,
                        force,
                        width,
                        height,
                        dt,
                        drag,
                        gravity,
                        bounce,
                        speed_limit,
                        aspect,
                        collisions,
                        pointer,
                    )
                }
                #[cfg(not(target_arch = "x86_64"))]
                scalar_step(
                    self,
                    inverse_masses,
                    force,
                    width,
                    height,
                    dt,
                    drag,
                    gravity,
                    bounce,
                    speed_limit,
                    aspect,
                    collisions,
                    pointer,
                );
            }
            KernelPath::Avx2 => {
                #[cfg(target_arch = "x86_64")]
                // SAFETY: selected_path checks AVX2 and prepare validates
                // gathered coordinates; pointer-adjacent blocks use scalars.
                unsafe {
                    x86::step_avx2(
                        self,
                        inverse_masses,
                        force,
                        width,
                        height,
                        dt,
                        drag,
                        gravity,
                        bounce,
                        speed_limit,
                        aspect,
                        collisions,
                        pointer,
                    )
                }
                #[cfg(not(target_arch = "x86_64"))]
                scalar_step(
                    self,
                    inverse_masses,
                    force,
                    width,
                    height,
                    dt,
                    drag,
                    gravity,
                    bounce,
                    speed_limit,
                    aspect,
                    collisions,
                    pointer,
                );
            }
            KernelPath::Scalar => {
                scalar_step(
                    self,
                    inverse_masses,
                    force,
                    width,
                    height,
                    dt,
                    drag,
                    gravity,
                    bounce,
                    speed_limit,
                    aspect,
                    collisions,
                    pointer,
                );
            }
        }
        self.loaded = true;
        self.dirty = true;
        Some(path)
    }
}

fn selected_path_from_features(avx2: bool, avx512f: bool) -> KernelPath {
    if avx512f {
        KernelPath::Avx512
    } else if avx2 {
        KernelPath::Avx2
    } else {
        KernelPath::Scalar
    }
}

fn selected_path() -> KernelPath {
    #[cfg(target_arch = "x86_64")]
    {
        selected_path_from_features(
            std::arch::is_x86_feature_detected!("avx2"),
            std::arch::is_x86_feature_detected!("avx512f"),
        )
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        selected_path_from_features(false, false)
    }
}

fn scalar_lane(
    state: &mut State,
    inverse_masses: &[f32],
    force: &[[f32; 2]],
    width: usize,
    height: usize,
    dt: f32,
    drag: f32,
    gravity: f32,
    bounce: f32,
    speed_limit: f32,
    aspect: f32,
    collisions: bool,
    pointer: Option<[f32; 2]>,
    index: usize,
) {
    let x = state.x[index];
    let y = state.y[index];
    let mut vx = state.vx[index];
    let mut vy = state.vy[index];
    let column = x.floor() as i32;
    let row = y.floor() as i32;
    let cell = row as usize * width + column as usize;
    let [force_x, force_y] = force[cell];
    let (mouse_x, mouse_y) = pointer.map_or((0.0, 0.0), |pointer| {
        super::sim::Sim::mouse_force_at_components(
            x,
            y,
            state.weight_rolls[index],
            pointer,
            width as f32,
            height as f32,
        )
    });
    let ax = (force_x + mouse_x - drag * vx) * inverse_masses[index];
    let ay = (force_y + mouse_y - drag * vy) * inverse_masses[index] + gravity;
    vx = (vx + ax * dt).clamp(-speed_limit, speed_limit);
    vy = (vy + ay * dt).clamp(-speed_limit, speed_limit);
    let next_x = x + vx * dt;
    let next_y = y + vy * dt / aspect;
    let (mut out_x, mut out_y) = (next_x, next_y);
    if collisions {
        let next_column = next_x.floor() as i32;
        let column_after =
            if swept_x_blocked(&state.blocked, width, height, column, next_column, row) {
                vx = -vx * bounce;
                out_x = x;
                column
            } else {
                next_column
            };
        let next_row = next_y.floor() as i32;
        if swept_y_blocked(&state.blocked, width, height, row, next_row, column_after) {
            vy = -vy * bounce;
            out_y = y;
        }
    }
    state.x[index] = wrap_any(out_x, width as f32);
    state.y[index] = wrap_any(out_y, height as f32);
    state.vx[index] = vx;
    state.vy[index] = vy;
}

#[inline]
fn pointer_affects_block(
    x: &[f32],
    y: &[f32],
    pointer: [f32; 2],
    width: usize,
    height: usize,
    aspect: f32,
) -> bool {
    let pointer_x = pointer[0] * width as f32;
    let pointer_y = pointer[1] * height as f32;
    x.iter().zip(y).any(|(&x, &y)| {
        let dx = x - pointer_x;
        let dy = (y - pointer_y) * aspect;
        dx * dx + dy * dy < MOUSE_RADIUS * MOUSE_RADIUS
    })
}

fn blocked(mask: &[i32], width: usize, height: usize, x: i32, y: i32) -> bool {
    let x = x.rem_euclid(width as i32) as usize;
    let y = y.rem_euclid(height as i32) as usize;
    mask[y * width + x] != 0
}

fn swept_x_blocked(
    mask: &[i32],
    width: usize,
    height: usize,
    from: i32,
    to: i32,
    row: i32,
) -> bool {
    let direction = (to - from).signum();
    let mut column = from + direction;
    while direction != 0
        && (if direction > 0 {
            column <= to
        } else {
            column >= to
        })
    {
        if blocked(mask, width, height, column, row) {
            return true;
        }
        column += direction;
    }
    false
}

fn swept_y_blocked(
    mask: &[i32],
    width: usize,
    height: usize,
    from: i32,
    to: i32,
    column: i32,
) -> bool {
    let direction = (to - from).signum();
    let mut row = from + direction;
    while direction != 0 && (if direction > 0 { row <= to } else { row >= to }) {
        if blocked(mask, width, height, column, row) {
            return true;
        }
        row += direction;
    }
    false
}

fn wrap_any(value: f32, extent: f32) -> f32 {
    if value < 0.0 || value >= extent {
        let wrapped = value.rem_euclid(extent);
        if wrapped >= extent {
            0.0
        } else {
            wrapped
        }
    } else {
        value
    }
}

fn scalar_step(
    state: &mut State,
    inverse_masses: &[f32],
    force: &[[f32; 2]],
    width: usize,
    height: usize,
    dt: f32,
    drag: f32,
    gravity: f32,
    bounce: f32,
    speed_limit: f32,
    aspect: f32,
    collisions: bool,
    pointer: Option<[f32; 2]>,
) {
    for index in 0..state.x.len() {
        scalar_lane(
            state,
            inverse_masses,
            force,
            width,
            height,
            dt,
            drag,
            gravity,
            bounce,
            speed_limit,
            aspect,
            collisions,
            pointer,
            index,
        );
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::{pointer_affects_block, scalar_lane, State};
    use std::arch::x86_64::*;

    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn step_avx2(
        state: &mut State,
        masses: &[f32],
        force: &[[f32; 2]],
        width: usize,
        height: usize,
        dt: f32,
        drag: f32,
        gravity: f32,
        bounce: f32,
        speed_limit: f32,
        aspect: f32,
        collisions: bool,
        pointer: Option<[f32; 2]>,
    ) {
        let count = state.x.len();
        let (w, h) = (width as i32, height as i32);
        let width_v = _mm256_set1_epi32(w);
        let height_v = _mm256_set1_epi32(h);
        let dt_v = _mm256_set1_ps(dt);
        let aspect_v = _mm256_set1_ps(dt / aspect);
        let drag_v = _mm256_set1_ps(drag);
        let gravity_v = _mm256_set1_ps(gravity);
        let bounce_v = _mm256_set1_ps(bounce);
        let high = _mm256_set1_ps(speed_limit);
        let low = _mm256_set1_ps(-speed_limit);
        let zero_i = _mm256_setzero_si256();
        let one_i = _mm256_set1_epi32(1);
        let vector_end = count / 8 * 8;
        for start in (0..vector_end).step_by(8) {
            let x = _mm256_loadu_ps(state.x.as_ptr().add(start));
            let y = _mm256_loadu_ps(state.y.as_ptr().add(start));
            if pointer.is_some_and(|pointer| {
                pointer_affects_block(
                    &state.x[start..start + 8],
                    &state.y[start..start + 8],
                    pointer,
                    width,
                    height,
                    aspect,
                )
            }) {
                #[cfg(test)]
                {
                    state.pointer_scalar_fallback_blocks += 1;
                }
                scalar_block(
                    state,
                    masses,
                    force,
                    start,
                    start + 8,
                    width,
                    height,
                    dt,
                    drag,
                    gravity,
                    bounce,
                    speed_limit,
                    aspect,
                    collisions,
                    pointer,
                );
                continue;
            }
            let vx = _mm256_loadu_ps(state.vx.as_ptr().add(start));
            let vy = _mm256_loadu_ps(state.vy.as_ptr().add(start));
            let mass = _mm256_loadu_ps(masses.as_ptr().add(start));
            let cell = _mm256_add_epi32(
                _mm256_mullo_epi32(_mm256_cvttps_epi32(y), width_v),
                _mm256_cvttps_epi32(x),
            );
            // Per-dot field sampling needs indexed loads. Intel cautions that
            // gather-heavy loops can trail unit-stride loops, so compare this
            // against spatial ordering before assuming wider vectors will win:
            // https://www.intel.com/content/www/us/en/developer/articles/training/explicit-vector-programming-best-known-methods.html
            // Benchmark direct gathers against the prior per-cell SoA copy;
            // Intel documents gathers as non-unit-stride vector loads whose
            // cost depends on the access pattern (same source above).
            let force_bytes = force.as_ptr().cast::<u8>();
            let cell_bytes = _mm256_slli_epi32(cell, 3);
            let fx = _mm256_i32gather_ps(force_bytes.cast(), cell_bytes, 1);
            let fy = _mm256_i32gather_ps(force_bytes.add(4).cast(), cell_bytes, 1);
            let ax = _mm256_mul_ps(_mm256_sub_ps(fx, _mm256_mul_ps(drag_v, vx)), mass);
            let ay = _mm256_add_ps(
                _mm256_mul_ps(_mm256_sub_ps(fy, _mm256_mul_ps(drag_v, vy)), mass),
                gravity_v,
            );
            let nvx = _mm256_min_ps(
                high,
                _mm256_max_ps(low, _mm256_add_ps(vx, _mm256_mul_ps(ax, dt_v))),
            );
            let nvy = _mm256_min_ps(
                high,
                _mm256_max_ps(low, _mm256_add_ps(vy, _mm256_mul_ps(ay, dt_v))),
            );
            let nx = _mm256_add_ps(x, _mm256_mul_ps(nvx, dt_v));
            let ny = _mm256_add_ps(y, _mm256_mul_ps(nvy, aspect_v));
            let column = _mm256_cvttps_epi32(x);
            let row = _mm256_cvttps_epi32(y);
            let next_column = _mm256_cvttps_epi32(_mm256_floor_ps(nx));
            let next_row = _mm256_cvttps_epi32(_mm256_floor_ps(ny));
            if collisions {
                let delta_x = _mm256_sub_epi32(next_column, column);
                let delta_y = _mm256_sub_epi32(next_row, row);
                let multi_cell_x = _mm256_cmpgt_epi32(_mm256_abs_epi32(delta_x), one_i);
                let multi_cell_y = _mm256_cmpgt_epi32(_mm256_abs_epi32(delta_y), one_i);
                if _mm256_movemask_ps(_mm256_castsi256_ps(_mm256_or_si256(
                    multi_cell_x,
                    multi_cell_y,
                ))) != 0
                {
                    scalar_block(
                        state,
                        masses,
                        force,
                        start,
                        start + 8,
                        width,
                        height,
                        dt,
                        drag,
                        gravity,
                        bounce,
                        speed_limit,
                        aspect,
                        collisions,
                        pointer,
                    );
                    continue;
                }
            }
            let (mut out_x, mut out_y, mut out_vx, mut out_vy) = (nx, ny, nvx, nvy);
            if collisions {
                let crosses_x = _mm256_xor_si256(
                    _mm256_cmpeq_epi32(next_column, column),
                    _mm256_set1_epi32(-1),
                );
                let wrapped_column = wrap_index(next_column, width_v, zero_i, one_i);
                let x_index = _mm256_add_epi32(_mm256_mullo_epi32(row, width_v), wrapped_column);
                let blocked_x = _mm256_cmpgt_epi32(
                    _mm256_mask_i32gather_epi32(
                        zero_i,
                        state.blocked.as_ptr(),
                        x_index,
                        crosses_x,
                        4,
                    ),
                    zero_i,
                );
                let after_column = _mm256_blendv_epi8(next_column, column, blocked_x);
                let crosses_y =
                    _mm256_xor_si256(_mm256_cmpeq_epi32(next_row, row), _mm256_set1_epi32(-1));
                let wrapped_row = wrap_index(next_row, height_v, zero_i, one_i);
                let wrapped_after_column = wrap_index(after_column, width_v, zero_i, one_i);
                let y_index = _mm256_add_epi32(
                    _mm256_mullo_epi32(wrapped_row, width_v),
                    wrapped_after_column,
                );
                let blocked_y = _mm256_cmpgt_epi32(
                    _mm256_mask_i32gather_epi32(
                        zero_i,
                        state.blocked.as_ptr(),
                        y_index,
                        crosses_y,
                        4,
                    ),
                    zero_i,
                );
                out_x = _mm256_blendv_ps(nx, x, _mm256_castsi256_ps(blocked_x));
                out_y = _mm256_blendv_ps(ny, y, _mm256_castsi256_ps(blocked_y));
                out_vx = _mm256_blendv_ps(
                    nvx,
                    _mm256_mul_ps(_mm256_sub_ps(_mm256_setzero_ps(), nvx), bounce_v),
                    _mm256_castsi256_ps(blocked_x),
                );
                out_vy = _mm256_blendv_ps(
                    nvy,
                    _mm256_mul_ps(_mm256_sub_ps(_mm256_setzero_ps(), nvy), bounce_v),
                    _mm256_castsi256_ps(blocked_y),
                );
            }
            let width_f = _mm256_set1_ps(width as f32);
            let height_f = _mm256_set1_ps(height as f32);
            out_x = wrap_float(out_x, width_f);
            out_y = wrap_float(out_y, height_f);
            _mm256_storeu_ps(state.x.as_mut_ptr().add(start), out_x);
            _mm256_storeu_ps(state.y.as_mut_ptr().add(start), out_y);
            _mm256_storeu_ps(state.vx.as_mut_ptr().add(start), out_vx);
            _mm256_storeu_ps(state.vy.as_mut_ptr().add(start), out_vy);
        }
        for index in vector_end..count {
            scalar_lane(
                state,
                masses,
                force,
                width,
                height,
                dt,
                drag,
                gravity,
                bounce,
                speed_limit,
                aspect,
                collisions,
                pointer,
                index,
            );
        }
    }

    #[inline]
    fn scalar_block(
        state: &mut State,
        masses: &[f32],
        force: &[[f32; 2]],
        start: usize,
        end: usize,
        width: usize,
        height: usize,
        dt: f32,
        drag: f32,
        gravity: f32,
        bounce: f32,
        speed_limit: f32,
        aspect: f32,
        collisions: bool,
        pointer: Option<[f32; 2]>,
    ) {
        for index in start..end {
            scalar_lane(
                state,
                masses,
                force,
                width,
                height,
                dt,
                drag,
                gravity,
                bounce,
                speed_limit,
                aspect,
                collisions,
                pointer,
                index,
            );
        }
    }

    #[inline]
    unsafe fn wrap_index(value: __m256i, extent: __m256i, zero: __m256i, one: __m256i) -> __m256i {
        let adjusted = _mm256_add_epi32(
            value,
            _mm256_and_si256(_mm256_cmpgt_epi32(zero, value), extent),
        );
        _mm256_sub_epi32(
            adjusted,
            _mm256_and_si256(
                _mm256_cmpgt_epi32(adjusted, _mm256_sub_epi32(extent, one)),
                extent,
            ),
        )
    }

    #[inline]
    unsafe fn wrap_float(value: __m256, extent: __m256) -> __m256 {
        let below = _mm256_cmp_ps(value, _mm256_setzero_ps(), _CMP_LT_OQ);
        let value = _mm256_blendv_ps(value, _mm256_add_ps(value, extent), below);
        let above = _mm256_cmp_ps(value, extent, _CMP_GE_OQ);
        _mm256_blendv_ps(value, _mm256_sub_ps(value, extent), above)
    }

    #[inline]
    unsafe fn wrap_index512(
        value: __m512i,
        extent: __m512i,
        zero: __m512i,
        one: __m512i,
    ) -> __m512i {
        let below = _mm512_cmp_epi32_mask(value, zero, _MM_CMPINT_LT);
        let adjusted = _mm512_mask_add_epi32(value, below, value, extent);
        let above = _mm512_cmp_epi32_mask(adjusted, _mm512_sub_epi32(extent, one), _MM_CMPINT_NLE);
        _mm512_mask_sub_epi32(adjusted, above, adjusted, extent)
    }

    #[inline]
    unsafe fn wrap_float512(value: __m512, extent: __m512) -> __m512 {
        let below = _mm512_cmp_ps_mask(value, _mm512_setzero_ps(), _CMP_LT_OQ);
        let value = _mm512_mask_add_ps(value, below, value, extent);
        let above = _mm512_cmp_ps_mask(value, extent, _CMP_GE_OQ);
        _mm512_mask_sub_ps(value, above, value, extent)
    }

    // AVX-512 is runtime-selected after Rust's feature detector reports AVX-512F.
    #[target_feature(enable = "avx512f")]
    pub(super) unsafe fn step_avx512(
        state: &mut State,
        masses: &[f32],
        force: &[[f32; 2]],
        width: usize,
        height: usize,
        dt: f32,
        drag: f32,
        gravity: f32,
        bounce: f32,
        speed_limit: f32,
        aspect: f32,
        collisions: bool,
        pointer: Option<[f32; 2]>,
    ) {
        let count = state.x.len();
        let (w, h) = (width as i32, height as i32);
        let width_v = _mm512_set1_epi32(w);
        let height_v = _mm512_set1_epi32(h);
        let dt_v = _mm512_set1_ps(dt);
        let aspect_v = _mm512_set1_ps(dt / aspect);
        let drag_v = _mm512_set1_ps(drag);
        let gravity_v = _mm512_set1_ps(gravity);
        let bounce_v = _mm512_set1_ps(bounce);
        let high = _mm512_set1_ps(speed_limit);
        let low = _mm512_set1_ps(-speed_limit);
        let zero_i = _mm512_setzero_si512();
        let one_i = _mm512_set1_epi32(1);
        let vector_end = count / 16 * 16;
        for start in (0..vector_end).step_by(16) {
            let x = _mm512_loadu_ps(state.x.as_ptr().add(start));
            let y = _mm512_loadu_ps(state.y.as_ptr().add(start));
            if pointer.is_some_and(|pointer| {
                pointer_affects_block(
                    &state.x[start..start + 16],
                    &state.y[start..start + 16],
                    pointer,
                    width,
                    height,
                    aspect,
                )
            }) {
                #[cfg(test)]
                {
                    state.pointer_scalar_fallback_blocks += 1;
                }
                scalar_block(
                    state,
                    masses,
                    force,
                    start,
                    start + 16,
                    width,
                    height,
                    dt,
                    drag,
                    gravity,
                    bounce,
                    speed_limit,
                    aspect,
                    collisions,
                    pointer,
                );
                continue;
            }
            let vx = _mm512_loadu_ps(state.vx.as_ptr().add(start));
            let vy = _mm512_loadu_ps(state.vy.as_ptr().add(start));
            let mass = _mm512_loadu_ps(masses.as_ptr().add(start));
            let col = _mm512_cvttps_epi32(x);
            let row = _mm512_cvttps_epi32(y);
            let cell = _mm512_add_epi32(_mm512_mullo_epi32(row, width_v), col);
            let force_bytes = force.as_ptr().cast::<u8>();
            let cell_bytes = _mm512_slli_epi32(cell, 3);
            let fx = _mm512_i32gather_ps::<1>(cell_bytes, force_bytes.cast());
            let fy = _mm512_i32gather_ps::<1>(cell_bytes, force_bytes.add(4).cast());
            let ax = _mm512_mul_ps(_mm512_sub_ps(fx, _mm512_mul_ps(drag_v, vx)), mass);
            let ay = _mm512_add_ps(
                _mm512_mul_ps(_mm512_sub_ps(fy, _mm512_mul_ps(drag_v, vy)), mass),
                gravity_v,
            );
            let nvx = _mm512_min_ps(
                high,
                _mm512_max_ps(low, _mm512_add_ps(vx, _mm512_mul_ps(ax, dt_v))),
            );
            let nvy = _mm512_min_ps(
                high,
                _mm512_max_ps(low, _mm512_add_ps(vy, _mm512_mul_ps(ay, dt_v))),
            );
            let nx = _mm512_add_ps(x, _mm512_mul_ps(nvx, dt_v));
            let ny = _mm512_add_ps(y, _mm512_mul_ps(nvy, aspect_v));
            let next_col =
                _mm512_cvt_roundps_epi32::<{ _MM_FROUND_TO_NEG_INF | _MM_FROUND_NO_EXC }>(nx);
            let next_row =
                _mm512_cvt_roundps_epi32::<{ _MM_FROUND_TO_NEG_INF | _MM_FROUND_NO_EXC }>(ny);
            if collisions {
                let delta_x = _mm512_sub_epi32(next_col, col);
                let delta_y = _mm512_sub_epi32(next_row, row);
                let multi_x = _mm512_cmp_epi32_mask(delta_x, one_i, _MM_CMPINT_NLE)
                    | _mm512_cmp_epi32_mask(delta_x, _mm512_set1_epi32(-1), _MM_CMPINT_LT);
                let multi_y = _mm512_cmp_epi32_mask(delta_y, one_i, _MM_CMPINT_NLE)
                    | _mm512_cmp_epi32_mask(delta_y, _mm512_set1_epi32(-1), _MM_CMPINT_LT);
                if (multi_x | multi_y) != 0 {
                    scalar_block(
                        state,
                        masses,
                        force,
                        start,
                        start + 16,
                        width,
                        height,
                        dt,
                        drag,
                        gravity,
                        bounce,
                        speed_limit,
                        aspect,
                        collisions,
                        pointer,
                    );
                    continue;
                }
            }
            let (mut out_x, mut out_y, mut out_vx, mut out_vy) = (nx, ny, nvx, nvy);
            if collisions {
                let crosses_x = _mm512_cmp_epi32_mask(next_col, col, _MM_CMPINT_NE);
                let wrapped_col = wrap_index512(next_col, width_v, zero_i, one_i);
                let ix = _mm512_add_epi32(_mm512_mullo_epi32(row, width_v), wrapped_col);
                let blocked_x = _mm512_cmp_epi32_mask(
                    _mm512_mask_i32gather_epi32(zero_i, crosses_x, ix, state.blocked.as_ptr(), 4),
                    zero_i,
                    _MM_CMPINT_NLE,
                );
                let after_col = _mm512_mask_blend_epi32(blocked_x, next_col, col);
                let crosses_y = _mm512_cmp_epi32_mask(next_row, row, _MM_CMPINT_NE);
                let wrapped_row = wrap_index512(next_row, height_v, zero_i, one_i);
                let wrapped_col = wrap_index512(after_col, width_v, zero_i, one_i);
                let iy = _mm512_add_epi32(_mm512_mullo_epi32(wrapped_row, width_v), wrapped_col);
                let blocked_y = _mm512_cmp_epi32_mask(
                    _mm512_mask_i32gather_epi32(zero_i, crosses_y, iy, state.blocked.as_ptr(), 4),
                    zero_i,
                    _MM_CMPINT_NLE,
                );
                out_x = _mm512_mask_blend_ps(blocked_x, nx, x);
                out_y = _mm512_mask_blend_ps(blocked_y, ny, y);
                out_vx = _mm512_mask_blend_ps(
                    blocked_x,
                    nvx,
                    _mm512_mul_ps(_mm512_sub_ps(_mm512_setzero_ps(), nvx), bounce_v),
                );
                out_vy = _mm512_mask_blend_ps(
                    blocked_y,
                    nvy,
                    _mm512_mul_ps(_mm512_sub_ps(_mm512_setzero_ps(), nvy), bounce_v),
                );
            }
            out_x = wrap_float512(out_x, _mm512_set1_ps(width as f32));
            out_y = wrap_float512(out_y, _mm512_set1_ps(height as f32));
            _mm512_storeu_ps(state.x.as_mut_ptr().add(start), out_x);
            _mm512_storeu_ps(state.y.as_mut_ptr().add(start), out_y);
            _mm512_storeu_ps(state.vx.as_mut_ptr().add(start), out_vx);
            _mm512_storeu_ps(state.vy.as_mut_ptr().add(start), out_vy);
        }
        for index in vector_end..count {
            scalar_lane(
                state,
                masses,
                force,
                width,
                height,
                dt,
                drag,
                gravity,
                bounce,
                speed_limit,
                aspect,
                collisions,
                pointer,
                index,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swept_collision_probes_intermediate_cells_in_both_directions_and_across_wrap() {
        let (width, height) = (8, 4);
        let mut mask = vec![0; width * height];
        mask[1 * width + 2] = 1;
        mask[1 * width] = 1;

        assert!(swept_x_blocked(&mask, width, height, 1, 3, 1));
        assert!(swept_x_blocked(&mask, width, height, 3, 1, 1));
        assert!(swept_x_blocked(&mask, width, height, 7, 9, 1));
        assert!(!swept_x_blocked(&mask, width, height, 3, 4, 1));
        assert!(swept_y_blocked(&mask, width, height, 0, 2, 2));
        assert!(swept_y_blocked(&mask, width, height, 2, 0, 2));
    }

    #[test]
    fn avx512f_dispatch_does_not_require_avx2() {
        assert_eq!(selected_path_from_features(false, true), KernelPath::Avx512);
        assert_eq!(selected_path_from_features(true, false), KernelPath::Avx2);
        assert_eq!(selected_path_from_features(true, true), KernelPath::Avx512);
        assert_eq!(
            selected_path_from_features(false, false),
            KernelPath::Scalar
        );
    }

    #[test]
    fn runtime_path_preserves_scalar_motion_and_raster_for_dense_frames() {
        // Include a non-vector-multiple population so the scalar tail stays
        // covered alongside the target 20k and 50k workloads.
        for count in [20_000, 50_000, 20_003] {
            let mut dots = fixture(count);
            let mut expected = dots.clone();
            let mut actual_state = State::default();
            let masses = vec![0.75; count];
            let force = vec![[2.25, -1.5]; 160 * 50];
            let mut mask = vec![0; 160 * 50];
            for row in 0..50 {
                for column in 0..160 {
                    mask[row * 160 + column] = u8::from((column / 8 + row / 3) % 5 == 0);
                }
            }
            let mut path = None;
            for _ in 0..120 {
                reference_step(
                    &mut expected,
                    &masses,
                    &force,
                    &mask,
                    160,
                    50,
                    1.0 / 30.0,
                    2.0,
                    0.0,
                    0.3,
                    45.0,
                    2.0,
                );
                path = actual_state.step_wrapped(
                    &mut dots,
                    &masses,
                    &force,
                    &mask,
                    160,
                    50,
                    1.0 / 30.0,
                    2.0,
                    0.0,
                    0.3,
                    45.0,
                    2.0,
                    true,
                    None,
                );
                if path.is_none() {
                    return;
                }
            }
            actual_state.flush_to_dots(&mut dots);
            assert!(matches!(path, Some(KernelPath::Avx2 | KernelPath::Avx512)));
            let mut raster_a = vec![false; 160 * 50 * 8];
            let mut raster_b = raster_a.clone();
            let mut max_delta = 0.0_f32;
            for (left, right) in expected.iter().zip(&dots) {
                max_delta = max_delta
                    .max((left.x - right.x).abs())
                    .max((left.y - right.y).abs());
                max_delta = max_delta
                    .max((left.vx - right.vx).abs())
                    .max((left.vy - right.vy).abs());
                raster_a[(left.y as usize * 160 + left.x as usize) * 8] = true;
                raster_b[(right.y as usize * 160 + right.x as usize) * 8] = true;
            }
            assert!(max_delta < 0.01, "maximum state delta was {max_delta}");
            let intersection = raster_a
                .iter()
                .zip(&raster_b)
                .filter(|(a, b)| **a && **b)
                .count();
            let union = raster_a
                .iter()
                .zip(&raster_b)
                .filter(|(a, b)| **a || **b)
                .count();
            assert!(intersection as f32 / union.max(1) as f32 >= 0.995);
        }
    }

    #[test]
    fn dynamic_dimensions_and_varying_forces_match_with_scalar_tail() {
        let (width, height, count) = (137, 43, 20_003);
        let mut expected: Vec<Dot> = (0..count)
            .map(|index| Dot {
                x: ((index * 37) % (width - 1)) as f32 + 0.25,
                y: ((index * 19) % (height - 1)) as f32 + 0.25,
                vx: ((index % 13) as f32 - 6.0) * 1.7,
                vy: ((index % 11) as f32 - 5.0) * 1.5,
                weight_roll: 0.0,
            })
            .collect();
        let mut actual = expected.clone();
        let masses = vec![0.75; count];
        let force: Vec<[f32; 2]> = (0..width * height)
            .map(|index| {
                let phase = index as f32 * 0.021;
                [phase.sin() * 3.5, (phase * 0.73).cos() * 2.0]
            })
            .collect();
        let mask: Vec<u8> = (0..width * height)
            .map(|index| {
                let (column, row) = (index % width, index / width);
                u8::from((column % 11 == 0 && row % 3 != 0) || (row % 9 == 0 && column % 4 == 0))
            })
            .collect();
        let mut state = State::default();

        for _ in 0..120 {
            reference_step(
                &mut expected,
                &masses,
                &force,
                &mask,
                width,
                height,
                1.0 / 30.0,
                1.5,
                0.25,
                0.3,
                45.0,
                2.0,
            );
            let Some(path) = state.step_wrapped(
                &mut actual,
                &masses,
                &force,
                &mask,
                width,
                height,
                1.0 / 30.0,
                1.5,
                0.25,
                0.3,
                45.0,
                2.0,
                true,
                None,
            ) else {
                return;
            };
            assert!(matches!(path, KernelPath::Avx2 | KernelPath::Avx512));
        }
        state.flush_to_dots(&mut actual);

        let mut raster_expected = vec![false; width * height * 8];
        let mut raster_actual = raster_expected.clone();
        let mut max_delta = 0.0_f32;
        for (left, right) in expected.iter().zip(&actual) {
            max_delta = max_delta
                .max((left.x - right.x).abs())
                .max((left.y - right.y).abs())
                .max((left.vx - right.vx).abs())
                .max((left.vy - right.vy).abs());
            raster_expected[(left.y as usize * width + left.x as usize) * 8] = true;
            raster_actual[(right.y as usize * width + right.x as usize) * 8] = true;
        }
        assert!(max_delta < 0.01, "maximum state delta was {max_delta}");
        let intersection = raster_expected
            .iter()
            .zip(&raster_actual)
            .filter(|(left, right)| **left && **right)
            .count();
        let union = raster_expected
            .iter()
            .zip(&raster_actual)
            .filter(|(left, right)| **left || **right)
            .count();
        assert!(intersection as f32 / union.max(1) as f32 >= 0.995);
    }

    #[allow(clippy::too_many_arguments)]
    fn reference_step(
        dots: &mut [Dot],
        masses: &[f32],
        force: &[[f32; 2]],
        mask: &[u8],
        width: usize,
        height: usize,
        dt: f32,
        drag: f32,
        gravity: f32,
        bounce: f32,
        speed: f32,
        aspect: f32,
    ) {
        for (index, dot) in dots.iter_mut().enumerate() {
            let (column, row) = (dot.x.floor() as i32, dot.y.floor() as i32);
            let [fx, fy] = force[row as usize * width + column as usize];
            let ax = (fx - drag * dot.vx) * masses[index];
            let ay = (fy - drag * dot.vy) * masses[index] + gravity;
            dot.vx = (dot.vx + ax * dt).clamp(-speed, speed);
            dot.vy = (dot.vy + ay * dt).clamp(-speed, speed);
            let (next_x, next_y) = (dot.x + dot.vx * dt, dot.y + dot.vy * dt / aspect);
            let next_column = next_x.floor() as i32;
            let column_after = if next_column != column
                && mask[row as usize * width + next_column.rem_euclid(width as i32) as usize] != 0
            {
                dot.vx = -dot.vx * bounce;
                column
            } else {
                dot.x = next_x;
                next_column
            };
            let next_row = next_y.floor() as i32;
            if next_row != row
                && mask[next_row.rem_euclid(height as i32) as usize * width
                    + column_after.rem_euclid(width as i32) as usize]
                    != 0
            {
                dot.vy = -dot.vy * bounce;
            } else {
                dot.y = next_y;
            }
            dot.x = wrap_any(dot.x, width as f32);
            dot.y = wrap_any(dot.y, height as f32);
        }
    }

    fn fixture(count: usize) -> Vec<Dot> {
        (0..count)
            .map(|index| Dot {
                x: ((index * 37) % 159) as f32 + 0.25,
                y: ((index * 19) % 49) as f32 + 0.25,
                vx: ((index % 17) as f32 - 8.0) * 0.2,
                vy: ((index % 13) as f32 - 6.0) * 0.15,
                weight_roll: 0.0,
            })
            .collect()
    }

    #[test]
    fn invalid_input_is_refused_without_mutating_dots() {
        let mut dots = fixture(3);
        let original = dots.clone();
        let mut state = State::default();
        assert!(state
            .step_wrapped(
                &mut dots,
                &[1.0; 3],
                &[],
                &[],
                160,
                50,
                1.0 / 30.0,
                2.0,
                0.0,
                0.3,
                45.0,
                2.0,
                false,
                None,
            )
            .is_none());
        assert_eq!(dots, original);
    }

    #[test]
    fn repeated_steps_keep_soa_canonical_until_the_read_boundary() {
        if selected_path() == KernelPath::Scalar {
            return;
        }
        let mut dots = fixture(257);
        let mut expected = dots.clone();
        let masses = vec![0.75; dots.len()];
        let force = vec![[2.25, -1.5]; 160 * 50];
        let mask = vec![0; 160 * 50];
        let mut state = State::default();

        for _ in 0..2 {
            reference_step(
                &mut expected,
                &masses,
                &force,
                &mask,
                160,
                50,
                1.0 / 30.0,
                2.0,
                0.0,
                0.3,
                45.0,
                2.0,
            );
            assert!(state
                .step_wrapped(
                    &mut dots,
                    &masses,
                    &force,
                    &mask,
                    160,
                    50,
                    1.0 / 30.0,
                    2.0,
                    0.0,
                    0.3,
                    45.0,
                    2.0,
                    false,
                    None,
                )
                .is_some());
            assert!(state.loaded && state.dirty);
        }

        state.flush_to_dots(&mut dots);
        let max_delta = dots
            .iter()
            .zip(&expected)
            .map(|(actual, expected)| {
                (actual.x - expected.x)
                    .abs()
                    .max((actual.y - expected.y).abs())
                    .max((actual.vx - expected.vx).abs())
                    .max((actual.vy - expected.vy).abs())
            })
            .fold(0.0_f32, f32::max);
        assert!(max_delta < 0.01, "maximum state delta was {max_delta}");
        assert!(!state.dirty);
    }

    #[test]
    fn pointer_force_uses_exact_scalar_batches_and_keeps_simd_dispatch() {
        if selected_path() == KernelPath::Scalar {
            return;
        }
        let pointer = [0.5, 0.5];
        let (width, height) = (160, 50);
        let dt = 1.0 / 60.0;
        let mut dots = fixture(32);
        dots[0].x = 80.0;
        dots[0].y = 25.0;
        dots[1].x = 80.5;
        dots[1].y = 25.0;
        dots[2].x = 78.5;
        dots[2].y = 25.0;
        for dot in dots.iter_mut().skip(3) {
            dot.x = 20.0 + (dot.x as usize % 20) as f32;
            dot.y = 5.0 + (dot.y as usize % 10) as f32;
        }
        let mut expected = dots.clone();
        let masses = vec![0.75; dots.len()];
        let force = vec![[0.0, 0.0]; width * height];
        let mask = vec![0; width * height];
        for (index, dot) in expected.iter_mut().enumerate() {
            let (mouse_x, mouse_y) = super::super::sim::Sim::mouse_force_at_components(
                dot.x,
                dot.y,
                dot.weight_roll,
                pointer,
                width as f32,
                height as f32,
            );
            dot.vx = (dot.vx + mouse_x * masses[index] * dt).clamp(-45.0, 45.0);
            dot.vy = (dot.vy + mouse_y * masses[index] * dt).clamp(-45.0, 45.0);
            dot.x = wrap_any(dot.x + dot.vx * dt, width as f32);
            dot.y = wrap_any(dot.y + dot.vy * dt / 2.0, height as f32);
        }

        let mut state = State::default();
        let path = state.step_wrapped(
            &mut dots,
            &masses,
            &force,
            &mask,
            width,
            height,
            dt,
            0.0,
            0.0,
            0.3,
            45.0,
            2.0,
            false,
            Some(pointer),
        );
        assert!(matches!(path, Some(KernelPath::Avx2 | KernelPath::Avx512)));
        assert_eq!(
            state.pointer_scalar_fallback_blocks, 0,
            "pointer-adjacent batches should keep vector integration"
        );
        state.flush_to_dots(&mut dots);

        for (actual, expected) in dots.iter().zip(expected) {
            assert!((actual.x - expected.x).abs() < 0.001);
            assert!((actual.y - expected.y).abs() < 0.001);
            assert!((actual.vx - expected.vx).abs() < 0.001);
            assert!((actual.vy - expected.vy).abs() < 0.001);
        }
        assert!(dots[1].vx > 0.0 && dots[2].vx < 0.0);
    }

    #[test]
    fn scalar_lane_bounces_when_a_fast_dot_crosses_an_occupied_cell() {
        let mut dots = vec![Dot {
            x: 1.9,
            y: 1.5,
            vx: 45.0,
            vy: 0.0,
            weight_roll: 0.0,
        }];
        let masses = [1.0];
        let force = vec![[0.0, 0.0]; 8 * 4];
        let mut mask = vec![0; 8 * 4];
        mask[1 * 8 + 2] = 1;
        let mut state = State::default();
        assert!(state.prepare(&dots, &masses, &force, &mask, 8, 4));

        scalar_lane(
            &mut state,
            &masses,
            &force,
            8,
            4,
            1.0 / 30.0,
            0.0,
            0.0,
            0.5,
            45.0,
            2.0,
            true,
            None,
            0,
        );
        assert_eq!(state.x[0], 1.9, "the dot must not pass through cell 2");
        assert!(
            state.vx[0] < 0.0,
            "the dot must bounce from the occupied cell"
        );
    }

    #[test]
    fn runtime_vector_kernel_bounces_when_a_dot_crosses_an_occupied_cell() {
        if selected_path() == KernelPath::Scalar {
            return;
        }
        let dot = Dot {
            x: 1.9,
            y: 1.5,
            vx: 45.0,
            vy: 0.0,
            weight_roll: 0.0,
        };
        let mut dots = vec![dot; 16];
        let masses = [1.0; 16];
        let force = vec![[0.0, 0.0]; 8 * 4];
        let mut mask = vec![0; 8 * 4];
        mask[1 * 8 + 2] = 1;
        let mut state = State::default();

        assert!(state
            .step_wrapped(
                &mut dots,
                &masses,
                &force,
                &mask,
                8,
                4,
                1.0 / 30.0,
                0.0,
                0.0,
                0.5,
                45.0,
                2.0,
                true,
                None,
            )
            .is_some());
        state.flush_to_dots(&mut dots);

        assert!(dots.iter().all(|dot| dot.x == 1.9));
        assert!(dots.iter().all(|dot| dot.vx < 0.0));
    }

    #[test]
    fn rounded_negative_wrap_stays_strictly_inside_screen_extent() {
        for extent in [1.0_f32, 50.0, 160.0] {
            let wrapped = wrap_any(-f32::MIN_POSITIVE, extent);
            assert!((0.0..extent).contains(&wrapped));
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_kernel_matches_scalar_when_runtime_supported() {
        if !std::arch::is_x86_feature_detected!("avx2") {
            return;
        }
        assert_forced_kernel(KernelPath::Avx2);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx512_kernel_matches_scalar_when_runtime_supported() {
        if !std::arch::is_x86_feature_detected!("avx512f") {
            return;
        }
        assert_forced_kernel(KernelPath::Avx512);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_kernel_bounces_before_an_occupied_intermediate_cell() {
        if !std::arch::is_x86_feature_detected!("avx2") {
            return;
        }
        assert_forced_collision_path(KernelPath::Avx2);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx512_kernel_bounces_before_an_occupied_intermediate_cell() {
        if !std::arch::is_x86_feature_detected!("avx512f") {
            return;
        }
        assert_forced_collision_path(KernelPath::Avx512);
    }

    #[cfg(target_arch = "x86_64")]
    fn assert_forced_collision_path(path: KernelPath) {
        let count = match path {
            KernelPath::Avx2 => 8,
            KernelPath::Avx512 => 16,
            KernelPath::Scalar => unreachable!(),
        };
        let dot = Dot {
            x: 1.9,
            y: 1.5,
            vx: 45.0,
            vy: 0.0,
            weight_roll: 0.0,
        };
        let dots = vec![dot; count];
        let masses = vec![1.0; count];
        let force = vec![[0.0, 0.0]; 8 * 4];
        let mut mask = vec![0; 8 * 4];
        mask[1 * 8 + 2] = 1;
        let mut state = State::default();
        assert!(state.prepare(&dots, &masses, &force, &mask, 8, 4));

        // SAFETY: each caller checks the ISA feature before selecting this path.
        unsafe {
            match path {
                KernelPath::Avx2 => x86::step_avx2(
                    &mut state,
                    &masses,
                    &force,
                    8,
                    4,
                    1.0 / 30.0,
                    0.0,
                    0.0,
                    0.5,
                    45.0,
                    2.0,
                    true,
                    None,
                ),
                KernelPath::Avx512 => x86::step_avx512(
                    &mut state,
                    &masses,
                    &force,
                    8,
                    4,
                    1.0 / 30.0,
                    0.0,
                    0.0,
                    0.5,
                    45.0,
                    2.0,
                    true,
                    None,
                ),
                KernelPath::Scalar => unreachable!(),
            }
        }

        assert!(state.x.iter().all(|x| *x == 1.9));
        assert!(state.vx.iter().all(|vx| *vx < 0.0));
    }

    #[cfg(target_arch = "x86_64")]
    fn assert_forced_kernel(path: KernelPath) {
        for count in [20_000, 50_000, 20_003] {
            let mut expected = fixture(count);
            let mut actual = expected.clone();
            let masses = vec![0.75; count];
            let force = vec![[2.25, -1.5]; 160 * 50];
            let mut mask = vec![0; 160 * 50];
            for row in 0..50 {
                for column in 0..160 {
                    mask[row * 160 + column] = u8::from((column / 8 + row / 3) % 5 == 0);
                }
            }
            let mut state = State::default();
            for _ in 0..120 {
                reference_step(
                    &mut expected,
                    &masses,
                    &force,
                    &mask,
                    160,
                    50,
                    1.0 / 30.0,
                    2.0,
                    0.0,
                    0.3,
                    45.0,
                    2.0,
                );
                assert!(state.prepare(&actual, &masses, &force, &mask, 160, 50));
                // SAFETY: the test checks the selected CPU feature immediately above.
                unsafe {
                    match path {
                        KernelPath::Avx2 => x86::step_avx2(
                            &mut state,
                            &masses,
                            &force,
                            160,
                            50,
                            1.0 / 30.0,
                            2.0,
                            0.0,
                            0.3,
                            45.0,
                            2.0,
                            true,
                            None,
                        ),
                        KernelPath::Avx512 => x86::step_avx512(
                            &mut state,
                            &masses,
                            &force,
                            160,
                            50,
                            1.0 / 30.0,
                            2.0,
                            0.0,
                            0.3,
                            45.0,
                            2.0,
                            true,
                            None,
                        ),
                        KernelPath::Scalar => unreachable!(),
                    }
                }
                state.dirty = true;
                state.flush_to_dots(&mut actual);
            }
            let mut max_delta = 0.0_f32;
            let mut raster_a = vec![false; 160 * 50 * 8];
            let mut raster_b = raster_a.clone();
            for (left, right) in expected.iter().zip(&actual) {
                max_delta = max_delta
                    .max((left.x - right.x).abs())
                    .max((left.y - right.y).abs())
                    .max((left.vx - right.vx).abs())
                    .max((left.vy - right.vy).abs());
                raster_a[(left.y as usize * 160 + left.x as usize) * 8] = true;
                raster_b[(right.y as usize * 160 + right.x as usize) * 8] = true;
            }
            assert!(max_delta < 0.01, "{path:?} maximum state delta {max_delta}");
            let intersection = raster_a
                .iter()
                .zip(&raster_b)
                .filter(|(a, b)| **a && **b)
                .count();
            let union = raster_a
                .iter()
                .zip(&raster_b)
                .filter(|(a, b)| **a || **b)
                .count();
            assert!(intersection as f32 / union.max(1) as f32 >= 0.995);
        }
    }
}
