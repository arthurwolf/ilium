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
    Avx2Fma,
    Avx512,
    Avx512Fma,
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
    /// True only when the caller validated each force while generating its cache.
    pub(super) cell_wind_field_finite: bool,
    #[cfg(test)]
    swept_scalar_lanes: usize,
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
        let valid_force = self.cell_wind_field_finite
            || force.iter().all(|[x, y]| x.is_finite() && y.is_finite());
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
            if mask.is_empty() {
                self.blocked
                    .resize(width.saturating_mul(height).div_ceil(u32::BITS as usize), 0);
                self.blocked.fill(0);
            } else {
                fill_packed_collision_mask(&mut self.blocked, mask);
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
        let prepared = self.prepare(dots, inverse_masses, force, collision_mask, width, height);
        self.cell_wind_field_finite = false;
        if !prepared {
            return None;
        }
        match path {
            KernelPath::Avx512 | KernelPath::Avx512Fma => {
                #[cfg(target_arch = "x86_64")]
                // SAFETY: selected_path checks AVX-512F and prepare validates
                // gathered coordinates; pointer-adjacent blocks use scalars.
                unsafe {
                    x86::step_avx512(
                        self,
                        path == KernelPath::Avx512Fma,
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
            KernelPath::Avx2 | KernelPath::Avx2Fma => {
                #[cfg(target_arch = "x86_64")]
                // SAFETY: selected_path checks AVX2 and prepare validates
                // gathered coordinates; pointer-adjacent blocks use scalars.
                unsafe {
                    x86::step_avx2(
                        self,
                        path == KernelPath::Avx2Fma,
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

fn selected_path_from_features(avx2: bool, avx512f: bool, fma: bool) -> KernelPath {
    if avx512f && fma {
        KernelPath::Avx512Fma
    } else if avx512f {
        KernelPath::Avx512
    } else if avx2 && fma {
        KernelPath::Avx2Fma
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
            std::arch::is_x86_feature_detected!("fma"),
        )
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        selected_path_from_features(false, false, false)
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
fn pointer_force_batch<const LANES: usize>(
    x: &[f32],
    y: &[f32],
    weight_rolls: &[f32],
    active_lanes: u32,
    pointer: [f32; 2],
    width: usize,
    height: usize,
) -> ([f32; LANES], [f32; LANES]) {
    // The SIMD comparison marks rare pointer hits; enumerate only those lanes
    // before applying the exact scalar force. Lang et al. refill idle query lanes,
    // which Wind does not do and whose performance results do not transfer:
    // https://doi.org/10.1007/s00778-019-00547-y
    let mut force_x = [0.0; LANES];
    let mut force_y = [0.0; LANES];
    let mut remaining_lanes = active_lanes;
    while remaining_lanes != 0 {
        let lane = remaining_lanes.trailing_zeros() as usize;
        if lane >= LANES {
            break;
        }
        (force_x[lane], force_y[lane]) = super::sim::Sim::mouse_force_at_components(
            x[lane],
            y[lane],
            weight_rolls[lane],
            pointer,
            width as f32,
            height as f32,
        );
        remaining_lanes &= remaining_lanes - 1;
    }
    (force_x, force_y)
}

/// Packs cell flags to shrink the random SIMD-gather working set. Roaring
/// bitmap results motivate compact layouts, but do not measure this lookup:
/// https://arxiv.org/abs/1709.07821
fn fill_packed_collision_mask(packed: &mut Vec<i32>, mask: &[u8]) {
    packed.resize(mask.len().div_ceil(u32::BITS as usize), 0);
    packed.fill(0);
    for (cell, &source) in mask.iter().enumerate() {
        if source != 0 {
            packed[cell / u32::BITS as usize] |= (1_u32 << (cell % u32::BITS as usize)) as i32;
        }
    }
}

fn blocked(mask: &[i32], width: usize, height: usize, x: i32, y: i32) -> bool {
    let x = x.rem_euclid(width as i32) as usize;
    let y = y.rem_euclid(height as i32) as usize;
    let cell = y * width + x;
    mask[cell / u32::BITS as usize] & (1_u32 << (cell % u32::BITS as usize)) as i32 != 0
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
    use super::{pointer_force_batch, scalar_lane, State, MOUSE_RADIUS};
    use std::arch::x86_64::*;

    // Fused arithmetic changes rounding slightly; the dense-frame tests bound
    // accumulated state error and compare the resulting raster occupancy.
    #[target_feature(enable = "avx2,fma")]
    unsafe fn integrate_avx2_fma(
        x: __m256,
        y: __m256,
        vx: __m256,
        vy: __m256,
        fx: __m256,
        fy: __m256,
        mouse_x: __m256,
        mouse_y: __m256,
        mass: __m256,
        drag: __m256,
        gravity: __m256,
        dt: __m256,
        aspect_dt: __m256,
        low: __m256,
        high: __m256,
    ) -> (__m256, __m256, __m256, __m256) {
        let ax = _mm256_mul_ps(_mm256_fnmadd_ps(drag, vx, _mm256_add_ps(fx, mouse_x)), mass);
        let ay = _mm256_fmadd_ps(
            _mm256_fnmadd_ps(drag, vy, _mm256_add_ps(fy, mouse_y)),
            mass,
            gravity,
        );
        let nvx = _mm256_min_ps(high, _mm256_max_ps(low, _mm256_fmadd_ps(ax, dt, vx)));
        let nvy = _mm256_min_ps(high, _mm256_max_ps(low, _mm256_fmadd_ps(ay, dt, vy)));
        (
            nvx,
            nvy,
            _mm256_fmadd_ps(nvx, dt, x),
            _mm256_fmadd_ps(nvy, aspect_dt, y),
        )
    }

    #[target_feature(enable = "avx512f,fma")]
    unsafe fn integrate_avx512_fma(
        x: __m512,
        y: __m512,
        vx: __m512,
        vy: __m512,
        fx: __m512,
        fy: __m512,
        mouse_x: __m512,
        mouse_y: __m512,
        mass: __m512,
        drag: __m512,
        gravity: __m512,
        dt: __m512,
        aspect_dt: __m512,
        low: __m512,
        high: __m512,
    ) -> (__m512, __m512, __m512, __m512) {
        let ax = _mm512_mul_ps(_mm512_fnmadd_ps(drag, vx, _mm512_add_ps(fx, mouse_x)), mass);
        let ay = _mm512_fmadd_ps(
            _mm512_fnmadd_ps(drag, vy, _mm512_add_ps(fy, mouse_y)),
            mass,
            gravity,
        );
        let nvx = _mm512_min_ps(high, _mm512_max_ps(low, _mm512_fmadd_ps(ax, dt, vx)));
        let nvy = _mm512_min_ps(high, _mm512_max_ps(low, _mm512_fmadd_ps(ay, dt, vy)));
        (
            nvx,
            nvy,
            _mm512_fmadd_ps(nvx, dt, x),
            _mm512_fmadd_ps(nvy, aspect_dt, y),
        )
    }

    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn step_avx2(
        state: &mut State,
        use_fma: bool,
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
        let pointer_x_v =
            _mm256_set1_ps(pointer.map_or(0.0, |position| position[0] * width as f32));
        let pointer_y_v =
            _mm256_set1_ps(pointer.map_or(0.0, |position| position[1] * height as f32));
        let pointer_aspect_v = _mm256_set1_ps(aspect);
        let pointer_radius_squared_v = _mm256_set1_ps(MOUSE_RADIUS * MOUSE_RADIUS);
        let vector_end = count / 8 * 8;
        for start in (0..vector_end).step_by(8) {
            let x = _mm256_loadu_ps(state.x.as_ptr().add(start));
            let y = _mm256_loadu_ps(state.y.as_ptr().add(start));
            let (mouse_x, mouse_y) = if let Some(pointer) = pointer {
                let dx = _mm256_sub_ps(x, pointer_x_v);
                let dy = _mm256_mul_ps(_mm256_sub_ps(y, pointer_y_v), pointer_aspect_v);
                let distance_squared = _mm256_add_ps(_mm256_mul_ps(dx, dx), _mm256_mul_ps(dy, dy));
                let active_lanes = _mm256_movemask_ps(_mm256_cmp_ps(
                    distance_squared,
                    pointer_radius_squared_v,
                    _CMP_LT_OQ,
                )) as u32;
                let (force_x, force_y) = pointer_force_batch::<8>(
                    &state.x[start..start + 8],
                    &state.y[start..start + 8],
                    &state.weight_rolls[start..start + 8],
                    active_lanes,
                    pointer,
                    width,
                    height,
                );
                (
                    _mm256_loadu_ps(force_x.as_ptr()),
                    _mm256_loadu_ps(force_y.as_ptr()),
                )
            } else {
                (_mm256_setzero_ps(), _mm256_setzero_ps())
            };
            let vx = _mm256_loadu_ps(state.vx.as_ptr().add(start));
            let vy = _mm256_loadu_ps(state.vy.as_ptr().add(start));
            let mass = _mm256_loadu_ps(masses.as_ptr().add(start));
            let cell = _mm256_add_epi32(
                _mm256_mullo_epi32(_mm256_cvttps_epi32(y), width_v),
                _mm256_cvttps_epi32(x),
            );
            // Watanabe and Nakagawa found padded AoS competitive for an
            // indirect particle-force gather, but their Lennard-Jones kernel
            // is not a Wind benchmark: https://arxiv.org/abs/1806.05713.
            // Here each cell's x/y force is consumed together; our isolated
            // matched gather microbenchmark favored this interleaved layout.
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
            let (nvx, nvy, nx, ny) = if use_fma {
                // SAFETY: this route is selected only when runtime detection
                // confirms both AVX2 and FMA support.
                unsafe {
                    integrate_avx2_fma(
                        x, y, vx, vy, fx, fy, mouse_x, mouse_y, mass, drag_v, gravity_v, dt_v,
                        aspect_v, low, high,
                    )
                }
            } else {
                let ax = _mm256_mul_ps(
                    _mm256_sub_ps(_mm256_add_ps(fx, mouse_x), _mm256_mul_ps(drag_v, vx)),
                    mass,
                );
                let ay = _mm256_add_ps(
                    _mm256_mul_ps(
                        _mm256_sub_ps(_mm256_add_ps(fy, mouse_y), _mm256_mul_ps(drag_v, vy)),
                        mass,
                    ),
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
                (
                    nvx,
                    nvy,
                    _mm256_add_ps(x, _mm256_mul_ps(nvx, dt_v)),
                    _mm256_add_ps(y, _mm256_mul_ps(nvy, aspect_v)),
                )
            };
            // Empty-screen wrapping needs no collision-cell boundaries. Avoid
            // four conversions and two vector floors for every particle block.
            let (column, row, next_column, next_row) = if collisions {
                (
                    _mm256_cvttps_epi32(x),
                    _mm256_cvttps_epi32(y),
                    _mm256_cvttps_epi32(_mm256_floor_ps(nx)),
                    _mm256_cvttps_epi32(_mm256_floor_ps(ny)),
                )
            } else {
                (zero_i, zero_i, zero_i, zero_i)
            };
            let mut swept_lanes = 0;
            if collisions {
                let delta_x = _mm256_sub_epi32(next_column, column);
                let delta_y = _mm256_sub_epi32(next_row, row);
                let multi_cell_x = _mm256_cmpgt_epi32(_mm256_abs_epi32(delta_x), one_i);
                let multi_cell_y = _mm256_cmpgt_epi32(_mm256_abs_epi32(delta_y), one_i);
                let multi_cell = _mm256_or_si256(multi_cell_x, multi_cell_y);
                swept_lanes = _mm256_movemask_ps(_mm256_castsi256_ps(multi_cell)) as u32;
                if swept_lanes != 0 {
                    if swept_lanes == 0xff {
                        #[cfg(test)]
                        {
                            state.swept_scalar_lanes += 8;
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
                    // Keep only unsafe high-displacement lanes scalar. Lang et al. use
                    // AVX-512 masks to refill idle query-pipeline lanes; Wind preserves
                    // particle indices and only enumerates exceptional lanes here, so
                    // their query speedups do not predict this kernel's performance:
                    // https://doi.org/10.1007/s00778-019-00547-y
                    let mut remaining_lanes = swept_lanes;
                    while remaining_lanes != 0 {
                        let lane = remaining_lanes.trailing_zeros() as usize;
                        #[cfg(test)]
                        {
                            state.swept_scalar_lanes += 1;
                        }
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
                            start + lane,
                        );
                        remaining_lanes &= remaining_lanes - 1;
                    }
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
                let x_word_index = _mm256_srli_epi32(x_index, 5);
                let x_bit_offset = _mm256_and_si256(x_index, _mm256_set1_epi32(31));
                let x_word = _mm256_mask_i32gather_epi32(
                    zero_i,
                    state.blocked.as_ptr(),
                    x_word_index,
                    crosses_x,
                    4,
                );
                let blocked_x = _mm256_cmpgt_epi32(
                    _mm256_and_si256(_mm256_srlv_epi32(x_word, x_bit_offset), one_i),
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
                let y_word_index = _mm256_srli_epi32(y_index, 5);
                let y_bit_offset = _mm256_and_si256(y_index, _mm256_set1_epi32(31));
                let y_word = _mm256_mask_i32gather_epi32(
                    zero_i,
                    state.blocked.as_ptr(),
                    y_word_index,
                    crosses_y,
                    4,
                );
                let blocked_y = _mm256_cmpgt_epi32(
                    _mm256_and_si256(_mm256_srlv_epi32(y_word, y_bit_offset), one_i),
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
            if swept_lanes == 0 {
                _mm256_storeu_ps(state.x.as_mut_ptr().add(start), out_x);
                _mm256_storeu_ps(state.y.as_mut_ptr().add(start), out_y);
                _mm256_storeu_ps(state.vx.as_mut_ptr().add(start), out_vx);
                _mm256_storeu_ps(state.vy.as_mut_ptr().add(start), out_vy);
            } else {
                let vector_lanes = _mm256_set_epi32(
                    if swept_lanes & 0x80 == 0 { -1 } else { 0 },
                    if swept_lanes & 0x40 == 0 { -1 } else { 0 },
                    if swept_lanes & 0x20 == 0 { -1 } else { 0 },
                    if swept_lanes & 0x10 == 0 { -1 } else { 0 },
                    if swept_lanes & 0x08 == 0 { -1 } else { 0 },
                    if swept_lanes & 0x04 == 0 { -1 } else { 0 },
                    if swept_lanes & 0x02 == 0 { -1 } else { 0 },
                    if swept_lanes & 0x01 == 0 { -1 } else { 0 },
                );
                _mm256_maskstore_ps(state.x.as_mut_ptr().add(start), vector_lanes, out_x);
                _mm256_maskstore_ps(state.y.as_mut_ptr().add(start), vector_lanes, out_y);
                _mm256_maskstore_ps(state.vx.as_mut_ptr().add(start), vector_lanes, out_vx);
                _mm256_maskstore_ps(state.vy.as_mut_ptr().add(start), vector_lanes, out_vy);
            }
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
        use_fma: bool,
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
        let pointer_x_v =
            _mm512_set1_ps(pointer.map_or(0.0, |position| position[0] * width as f32));
        let pointer_y_v =
            _mm512_set1_ps(pointer.map_or(0.0, |position| position[1] * height as f32));
        let pointer_aspect_v = _mm512_set1_ps(aspect);
        let pointer_radius_squared_v = _mm512_set1_ps(MOUSE_RADIUS * MOUSE_RADIUS);
        let vector_end = count / 16 * 16;
        for start in (0..vector_end).step_by(16) {
            let x = _mm512_loadu_ps(state.x.as_ptr().add(start));
            let y = _mm512_loadu_ps(state.y.as_ptr().add(start));
            let (mouse_x, mouse_y) = if let Some(pointer) = pointer {
                let dx = _mm512_sub_ps(x, pointer_x_v);
                let dy = _mm512_mul_ps(_mm512_sub_ps(y, pointer_y_v), pointer_aspect_v);
                let distance_squared = _mm512_add_ps(_mm512_mul_ps(dx, dx), _mm512_mul_ps(dy, dy));
                let active_lanes =
                    _mm512_cmp_ps_mask(distance_squared, pointer_radius_squared_v, _CMP_LT_OQ)
                        as u32;
                let (force_x, force_y) = pointer_force_batch::<16>(
                    &state.x[start..start + 16],
                    &state.y[start..start + 16],
                    &state.weight_rolls[start..start + 16],
                    active_lanes,
                    pointer,
                    width,
                    height,
                );
                (
                    _mm512_loadu_ps(force_x.as_ptr()),
                    _mm512_loadu_ps(force_y.as_ptr()),
                )
            } else {
                (_mm512_setzero_ps(), _mm512_setzero_ps())
            };
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
            let (nvx, nvy, nx, ny) = if use_fma {
                // SAFETY: this route is selected only when runtime detection
                // confirms AVX-512F and FMA support.
                unsafe {
                    integrate_avx512_fma(
                        x, y, vx, vy, fx, fy, mouse_x, mouse_y, mass, drag_v, gravity_v, dt_v,
                        aspect_v, low, high,
                    )
                }
            } else {
                let ax = _mm512_mul_ps(
                    _mm512_sub_ps(_mm512_add_ps(fx, mouse_x), _mm512_mul_ps(drag_v, vx)),
                    mass,
                );
                let ay = _mm512_add_ps(
                    _mm512_mul_ps(
                        _mm512_sub_ps(_mm512_add_ps(fy, mouse_y), _mm512_mul_ps(drag_v, vy)),
                        mass,
                    ),
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
                (
                    nvx,
                    nvy,
                    _mm512_add_ps(x, _mm512_mul_ps(nvx, dt_v)),
                    _mm512_add_ps(y, _mm512_mul_ps(nvy, aspect_v)),
                )
            };
            // Empty-screen wrapping does not inspect cell boundaries; skip
            // collision-only float-to-int conversions in that common path.
            let (next_col, next_row) = if collisions {
                (
                    _mm512_cvt_roundps_epi32::<{ _MM_FROUND_TO_NEG_INF | _MM_FROUND_NO_EXC }>(nx),
                    _mm512_cvt_roundps_epi32::<{ _MM_FROUND_TO_NEG_INF | _MM_FROUND_NO_EXC }>(ny),
                )
            } else {
                (zero_i, zero_i)
            };
            let mut swept_lanes = 0_u16;
            if collisions {
                let delta_x = _mm512_sub_epi32(next_col, col);
                let delta_y = _mm512_sub_epi32(next_row, row);
                let multi_x = _mm512_cmp_epi32_mask(delta_x, one_i, _MM_CMPINT_NLE)
                    | _mm512_cmp_epi32_mask(delta_x, _mm512_set1_epi32(-1), _MM_CMPINT_LT);
                let multi_y = _mm512_cmp_epi32_mask(delta_y, one_i, _MM_CMPINT_NLE)
                    | _mm512_cmp_epi32_mask(delta_y, _mm512_set1_epi32(-1), _MM_CMPINT_LT);
                swept_lanes = multi_x | multi_y;
                if swept_lanes == u16::MAX {
                    #[cfg(test)]
                    {
                        state.swept_scalar_lanes += 16;
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
                if swept_lanes != 0 {
                    let mut remaining_lanes = swept_lanes;
                    while remaining_lanes != 0 {
                        let lane = remaining_lanes.trailing_zeros() as usize;
                        #[cfg(test)]
                        {
                            state.swept_scalar_lanes += 1;
                        }
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
                            start + lane,
                        );
                        remaining_lanes &= remaining_lanes - 1;
                    }
                }
            }
            let (mut out_x, mut out_y, mut out_vx, mut out_vy) = (nx, ny, nvx, nvy);
            if collisions {
                let crosses_x = _mm512_cmp_epi32_mask(next_col, col, _MM_CMPINT_NE);
                let wrapped_col = wrap_index512(next_col, width_v, zero_i, one_i);
                let ix = _mm512_add_epi32(_mm512_mullo_epi32(row, width_v), wrapped_col);
                let x_word_index = _mm512_srli_epi32(ix, 5);
                let x_bit_offset = _mm512_and_si512(ix, _mm512_set1_epi32(31));
                let x_word = _mm512_mask_i32gather_epi32(
                    zero_i,
                    crosses_x,
                    x_word_index,
                    state.blocked.as_ptr(),
                    4,
                );
                let blocked_x = _mm512_cmp_epi32_mask(
                    _mm512_and_si512(_mm512_srlv_epi32(x_word, x_bit_offset), one_i),
                    zero_i,
                    _MM_CMPINT_NLE,
                );
                let after_col = _mm512_mask_blend_epi32(blocked_x, next_col, col);
                let crosses_y = _mm512_cmp_epi32_mask(next_row, row, _MM_CMPINT_NE);
                let wrapped_row = wrap_index512(next_row, height_v, zero_i, one_i);
                let wrapped_col = wrap_index512(after_col, width_v, zero_i, one_i);
                let iy = _mm512_add_epi32(_mm512_mullo_epi32(wrapped_row, width_v), wrapped_col);
                let y_word_index = _mm512_srli_epi32(iy, 5);
                let y_bit_offset = _mm512_and_si512(iy, _mm512_set1_epi32(31));
                let y_word = _mm512_mask_i32gather_epi32(
                    zero_i,
                    crosses_y,
                    y_word_index,
                    state.blocked.as_ptr(),
                    4,
                );
                let blocked_y = _mm512_cmp_epi32_mask(
                    _mm512_and_si512(_mm512_srlv_epi32(y_word, y_bit_offset), one_i),
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
            if swept_lanes == 0 {
                _mm512_storeu_ps(state.x.as_mut_ptr().add(start), out_x);
                _mm512_storeu_ps(state.y.as_mut_ptr().add(start), out_y);
                _mm512_storeu_ps(state.vx.as_mut_ptr().add(start), out_vx);
                _mm512_storeu_ps(state.vy.as_mut_ptr().add(start), out_vy);
            } else {
                let vector_lanes = !swept_lanes;
                _mm512_mask_storeu_ps(state.x.as_mut_ptr().add(start), vector_lanes, out_x);
                _mm512_mask_storeu_ps(state.y.as_mut_ptr().add(start), vector_lanes, out_y);
                _mm512_mask_storeu_ps(state.vx.as_mut_ptr().add(start), vector_lanes, out_vx);
                _mm512_mask_storeu_ps(state.vy.as_mut_ptr().add(start), vector_lanes, out_vy);
            }
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
        mask[1 * width + width - 1] = 1;
        mask[3 * width + 2] = 1;
        let mut packed_mask = Vec::new();
        fill_packed_collision_mask(&mut packed_mask, &mask);

        assert!(swept_x_blocked(&packed_mask, width, height, 1, 3, 1));
        assert!(swept_x_blocked(&packed_mask, width, height, 3, 1, 1));
        assert!(swept_x_blocked(&packed_mask, width, height, 7, 9, 1));
        assert!(swept_x_blocked(&packed_mask, width, height, 0, -2, 1));
        assert!(!swept_x_blocked(&packed_mask, width, height, 3, 4, 1));
        assert!(swept_y_blocked(&packed_mask, width, height, 0, 2, 2));
        assert!(swept_y_blocked(&packed_mask, width, height, 2, 0, 2));
        assert!(swept_y_blocked(&packed_mask, width, height, 3, 5, 2));
        assert!(swept_y_blocked(&packed_mask, width, height, 0, -2, 2));
    }

    #[test]
    fn runtime_dispatch_requires_each_cpu_feature() {
        assert_eq!(
            selected_path_from_features(false, true, false),
            KernelPath::Avx512
        );
        assert_eq!(
            selected_path_from_features(false, true, true),
            KernelPath::Avx512Fma
        );
        assert_eq!(
            selected_path_from_features(true, false, false),
            KernelPath::Avx2
        );
        assert_eq!(
            selected_path_from_features(true, false, true),
            KernelPath::Avx2Fma
        );
        assert_eq!(
            selected_path_from_features(true, true, false),
            KernelPath::Avx512
        );
        assert_eq!(
            selected_path_from_features(true, true, true),
            KernelPath::Avx512Fma
        );
        assert_eq!(
            selected_path_from_features(false, false, true),
            KernelPath::Scalar
        );
    }

    #[test]
    fn fma_kernel_tracks_scalar_state_and_raster_when_supported() {
        #[cfg(not(target_arch = "x86_64"))]
        return;
        #[cfg(target_arch = "x86_64")]
        {
            if !std::arch::is_x86_feature_detected!("fma") {
                return;
            }
            let mut paths = Vec::with_capacity(2);
            if std::arch::is_x86_feature_detected!("avx2") {
                paths.push(KernelPath::Avx2Fma);
            }
            if std::arch::is_x86_feature_detected!("avx512f") {
                paths.push(KernelPath::Avx512Fma);
            }
            if paths.is_empty() {
                return;
            }
            const FRAMES: usize = 300;
            for path in paths {
                for count in [20_000, 50_000] {
                    let mut expected = fixture(count);
                    let mut actual = expected.clone();
                    let masses = vec![0.75; count];
                    let force = vec![[2.25, -1.5]; 160 * 50];
                    let mask = vec![0; 160 * 50];
                    let mut state = State::default();
                    assert!(state.prepare(&actual, &masses, &force, &mask, 160, 50));
                    for _ in 0..FRAMES {
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
                        // SAFETY: the runtime checks above verify this path's ISA,
                        // and the fixture stays inside the validated force grid.
                        unsafe {
                            match path {
                                KernelPath::Avx2Fma => x86::step_avx2(
                                    &mut state,
                                    true,
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
                                    false,
                                    None,
                                ),
                                KernelPath::Avx512Fma => x86::step_avx512(
                                    &mut state,
                                    true,
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
                                    false,
                                    None,
                                ),
                                _ => unreachable!("only runtime-verified FMA paths are added"),
                            }
                        }
                    }
                    // The test calls the low-level kernels directly, bypassing
                    // `State::step_wrapped`, which normally marks this cache dirty.
                    state.dirty = true;
                    state.flush_to_dots(&mut actual);
                    let mut max_delta = 0.0_f32;
                    let mut reference_raster = vec![false; 160 * 50];
                    let mut fma_raster = reference_raster.clone();
                    for (reference, candidate) in expected.iter().zip(&actual) {
                        let delta_x = (reference.x - candidate.x).abs();
                        let delta_y = (reference.y - candidate.y).abs();
                        max_delta = max_delta
                            .max(delta_x.min(160.0 - delta_x))
                            .max(delta_y.min(50.0 - delta_y))
                            .max((reference.vx - candidate.vx).abs())
                            .max((reference.vy - candidate.vy).abs());
                        reference_raster[reference.y as usize * 160 + reference.x as usize] = true;
                        fma_raster[candidate.y as usize * 160 + candidate.x as usize] = true;
                    }
                    let intersection = reference_raster
                        .iter()
                        .zip(&fma_raster)
                        .filter(|(left, right)| **left && **right)
                        .count();
                    let union = reference_raster
                        .iter()
                        .zip(&fma_raster)
                        .filter(|(left, right)| **left || **right)
                        .count();
                    assert!(
                        max_delta < 0.01,
                        "{path:?}, {count} dots: max wrapped-state delta {max_delta}"
                    );
                    assert!(
                        intersection as f32 / union.max(1) as f32 >= 0.995,
                        "{path:?}, {count} dots: raster Jaccard below 0.995"
                    );
                }
            }
        }
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
            assert!(matches!(
                path,
                Some(
                    KernelPath::Avx2
                        | KernelPath::Avx2Fma
                        | KernelPath::Avx512
                        | KernelPath::Avx512Fma
                )
            ));
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
    fn collision_free_runtime_path_matches_scalar_motion() {
        if selected_path() == KernelPath::Scalar {
            return;
        }
        let count = 20_003;
        let mut expected = fixture(count);
        let mut actual = expected.clone();
        let masses = vec![0.75; count];
        let force = vec![[2.25, -1.5]; 160 * 50];
        let mask = vec![0; 160 * 50];
        let mut state = State::default();

        for _ in 0..8 {
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
                    &mut actual,
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
        }
        state.flush_to_dots(&mut actual);

        for (expected, actual) in expected.iter().zip(&actual) {
            assert!((expected.x - actual.x).abs() < 0.01);
            assert!((expected.y - actual.y).abs() < 0.01);
            assert!((expected.vx - actual.vx).abs() < 0.01);
            assert!((expected.vy - actual.vy).abs() < 0.01);
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
            assert!(matches!(
                path,
                KernelPath::Avx2 | KernelPath::Avx2Fma | KernelPath::Avx512 | KernelPath::Avx512Fma
            ));
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
    fn collision_mask_cache_packs_cells_into_u32_words() {
        let dots = fixture(1);
        let masses = [1.0];
        let force = vec![[0.0, 0.0]; 70];
        let mut mask = vec![0; 70];
        for cell in [0, 31, 32, 69] {
            mask[cell] = 1;
        }

        let mut state = State::default();
        assert!(state.prepare(&dots, &masses, &force, &mask, 70, 1));

        assert_eq!(state.blocked, vec![i32::MIN | 1, 1, 1 << 5]);
    }

    #[test]
    fn pointer_force_batch_evaluates_only_masked_lanes() {
        let x = [80.0, 80.5, 86.0, 85.99];
        let y = [25.0; 4];
        let weight_rolls = [0.25; 4];
        let pointer = [0.5, 0.5];
        let (force_x, force_y) =
            pointer_force_batch::<4>(&x, &y, &weight_rolls, 0b1011, pointer, 160, 50);
        for lane in [0, 1, 3] {
            let expected = super::super::sim::Sim::mouse_force_at_components(
                x[lane],
                y[lane],
                weight_rolls[lane],
                pointer,
                160.0,
                50.0,
            );
            assert_eq!((force_x[lane], force_y[lane]), expected);
        }
        assert_eq!((force_x[2], force_y[2]), (0.0, 0.0));

        let (force_x, force_y) =
            pointer_force_batch::<4>(&x, &y, &weight_rolls, 0, pointer, 160, 50);
        assert_eq!(force_x, [0.0; 4]);
        assert_eq!(force_y, [0.0; 4]);
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
    fn pointer_force_matches_scalar_while_using_simd_dispatch() {
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
        assert!(matches!(
            path,
            Some(
                KernelPath::Avx2 | KernelPath::Avx2Fma | KernelPath::Avx512 | KernelPath::Avx512Fma
            )
        ));
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
    fn avx2_pointer_force_matches_scalar_when_runtime_supported() {
        if !std::arch::is_x86_feature_detected!("avx2") {
            return;
        }
        assert_forced_pointer_kernel(KernelPath::Avx2);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx512_pointer_force_matches_scalar_when_runtime_supported() {
        if !std::arch::is_x86_feature_detected!("avx512f") {
            return;
        }
        assert_forced_pointer_kernel(KernelPath::Avx512);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_pointer_force_matches_scalar_for_20003_dots_when_runtime_supported() {
        if !std::arch::is_x86_feature_detected!("avx2") {
            return;
        }
        assert_forced_pointer_kernel_with_count(KernelPath::Avx2, 20_003);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx512_pointer_force_matches_scalar_for_20003_dots_when_runtime_supported() {
        if !std::arch::is_x86_feature_detected!("avx512f") {
            return;
        }
        assert_forced_pointer_kernel_with_count(KernelPath::Avx512, 20_003);
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
    #[test]
    fn swept_collision_fallback_only_scalarizes_lanes_that_cross_multiple_cells() {
        if std::arch::is_x86_feature_detected!("avx2") {
            assert_selective_collision_fallback(KernelPath::Avx2);
        }

        if std::arch::is_x86_feature_detected!("avx512f") {
            assert_selective_collision_fallback(KernelPath::Avx512);
        }
    }

    #[cfg(target_arch = "x86_64")]
    fn assert_forced_pointer_kernel(path: KernelPath) {
        let lanes = match path {
            KernelPath::Avx2 | KernelPath::Avx2Fma => 8,
            KernelPath::Avx512 | KernelPath::Avx512Fma => 16,
            KernelPath::Scalar => unreachable!(),
        };
        assert_forced_pointer_kernel_with_count(path, lanes * 2 + 3);
    }

    #[cfg(target_arch = "x86_64")]
    fn assert_forced_pointer_kernel_with_count(path: KernelPath, count: usize) {
        let pointer = [0.5, 0.5];
        let (width, height) = (160, 50);
        let mut expected = fixture(count);
        expected[0].x = 80.0;
        expected[0].y = 25.0;
        expected[0].weight_roll = 0.25;
        expected[1].x = 80.5;
        expected[1].y = 25.0;
        expected[2].x = 78.5;
        expected[2].y = 25.0;
        expected[3].x = 86.0;
        expected[3].y = 25.0;
        expected[4].x = 85.99;
        expected[4].y = 25.0;
        for dot in expected.iter_mut().skip(5) {
            dot.x = 20.0 + (dot.x as usize % 20) as f32;
            dot.y = 5.0 + (dot.y as usize % 10) as f32;
        }
        if count == 20_003 {
            for (dot, x) in expected.iter_mut().skip(count - 3).zip([80.5, 86.0, 85.99]) {
                dot.x = x;
                dot.y = 25.0;
            }
        }
        let mut actual = expected.clone();
        let masses = vec![0.75; count];
        let force = vec![[0.0, 0.0]; width * height];
        let mask = vec![0; width * height];
        let dt = 1.0 / 60.0;
        for dot in &mut expected {
            let (mouse_x, mouse_y) = super::super::sim::Sim::mouse_force_at_components(
                dot.x,
                dot.y,
                dot.weight_roll,
                pointer,
                width as f32,
                height as f32,
            );
            dot.vx = (dot.vx + mouse_x * 0.75 * dt).clamp(-45.0, 45.0);
            dot.vy = (dot.vy + mouse_y * 0.75 * dt).clamp(-45.0, 45.0);
            dot.x = wrap_any(dot.x + dot.vx * dt, width as f32);
            dot.y = wrap_any(dot.y + dot.vy * dt / 2.0, height as f32);
        }

        let mut state = State::default();
        assert!(state.prepare(&actual, &masses, &force, &mask, width, height));
        // SAFETY: each test caller checks the exact ISA feature before selecting the kernel.
        unsafe {
            match path {
                KernelPath::Avx2 | KernelPath::Avx2Fma => x86::step_avx2(
                    &mut state,
                    path == KernelPath::Avx2Fma,
                    &masses,
                    &force,
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
                ),
                KernelPath::Avx512 | KernelPath::Avx512Fma => x86::step_avx512(
                    &mut state,
                    path == KernelPath::Avx512Fma,
                    &masses,
                    &force,
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
                ),
                KernelPath::Scalar => unreachable!(),
            }
        }
        state.dirty = true;
        state.flush_to_dots(&mut actual);

        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual.x - expected.x).abs() < 0.001);
            assert!((actual.y - expected.y).abs() < 0.001);
            assert!((actual.vx - expected.vx).abs() < 0.001);
            assert!((actual.vy - expected.vy).abs() < 0.001);
        }
    }

    #[cfg(target_arch = "x86_64")]
    fn assert_forced_collision_path(path: KernelPath) {
        let count = match path {
            KernelPath::Avx2 | KernelPath::Avx2Fma => 8,
            KernelPath::Avx512 | KernelPath::Avx512Fma => 16,
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
                KernelPath::Avx2 | KernelPath::Avx2Fma => x86::step_avx2(
                    &mut state,
                    path == KernelPath::Avx2Fma,
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
                KernelPath::Avx512 | KernelPath::Avx512Fma => x86::step_avx512(
                    &mut state,
                    path == KernelPath::Avx512Fma,
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
        assert_eq!(state.swept_scalar_lanes, count);
    }

    #[cfg(target_arch = "x86_64")]
    fn assert_selective_collision_fallback(path: KernelPath) {
        let count = match path {
            KernelPath::Avx2 | KernelPath::Avx2Fma => 8,
            KernelPath::Avx512 | KernelPath::Avx512Fma => 16,
            KernelPath::Scalar => unreachable!(),
        };
        let mut dots = vec![
            Dot {
                x: 4.25,
                y: 2.5,
                vx: 0.0,
                vy: 0.0,
                weight_roll: 0.0,
            };
            count
        ];
        let crossing_lane = count - 1;
        dots[crossing_lane].x = 1.9;
        dots[crossing_lane].y = 1.5;
        dots[crossing_lane].vx = 45.0;
        let masses = vec![1.0; count];
        let force = vec![[0.0, 0.0]; 8 * 4];
        let mut mask = vec![0; 8 * 4];
        mask[1 * 8 + 2] = 1;
        let mut actual_state = State::default();
        let mut expected_state = State::default();
        assert!(actual_state.prepare(&dots, &masses, &force, &mask, 8, 4));
        assert!(expected_state.prepare(&dots, &masses, &force, &mask, 8, 4));
        for index in 0..count {
            scalar_lane(
                &mut expected_state,
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
                index,
            );
        }

        // SAFETY: the runtime feature check above verifies the selected kernel.
        unsafe {
            match path {
                KernelPath::Avx2 | KernelPath::Avx2Fma => x86::step_avx2(
                    &mut actual_state,
                    path == KernelPath::Avx2Fma,
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
                KernelPath::Avx512 | KernelPath::Avx512Fma => x86::step_avx512(
                    &mut actual_state,
                    path == KernelPath::Avx512Fma,
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
        assert_eq!(actual_state.swept_scalar_lanes, 1);
        for index in 0..count {
            assert_eq!(actual_state.x[index], expected_state.x[index]);
            assert_eq!(actual_state.y[index], expected_state.y[index]);
            assert_eq!(actual_state.vx[index], expected_state.vx[index]);
            assert_eq!(actual_state.vy[index], expected_state.vy[index]);
        }
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
                        KernelPath::Avx2 | KernelPath::Avx2Fma => x86::step_avx2(
                            &mut state,
                            path == KernelPath::Avx2Fma,
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
                        KernelPath::Avx512 | KernelPath::Avx512Fma => x86::step_avx512(
                            &mut state,
                            path == KernelPath::Avx512Fma,
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

#[cfg(all(test, target_arch = "x86_64"))]
mod kernel_width_benchmark {
    use super::{x86, Dot, KernelPath, State};
    use std::{hint::black_box, time::Instant};

    const WIDTH: usize = 160;
    const HEIGHT: usize = 50;
    const STEPS: usize = 128;

    fn fixture(count: usize) -> (Vec<Dot>, Vec<f32>, Vec<[f32; 2]>) {
        let mut random = 0x9e37_79b9_7f4a_7c15_u64;
        let dots = (0..count)
            .map(|index| {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                Dot {
                    x: (random as usize % WIDTH) as f32 + 0.25,
                    y: ((random >> 32) as usize % HEIGHT) as f32 + 0.25,
                    vx: ((index % 17) as f32 - 8.0) * 0.2,
                    vy: ((index % 13) as f32 - 6.0) * 0.15,
                    weight_roll: 0.0,
                }
            })
            .collect();
        let masses = vec![0.75; count];
        let force = (0..WIDTH * HEIGHT)
            .map(|cell| {
                let x = (cell % WIDTH) as f32;
                let y = (cell / WIDTH) as f32;
                [(x * 0.012).sin(), (y * 0.017).cos()]
            })
            .collect();
        (dots, masses, force)
    }

    fn prepared_state(dots: &[Dot], masses: &[f32], force: &[[f32; 2]]) -> State {
        let mut state = State {
            cell_wind_field_finite: true,
            ..State::default()
        };
        assert!(state.prepare(dots, masses, force, &[], WIDTH, HEIGHT));
        state.loaded = true;
        state
    }

    fn run_kernel(path: KernelPath, state: &mut State, masses: &[f32], force: &[[f32; 2]]) {
        // SAFETY: the ignored benchmark checks runtime feature support, and its
        // fixture keeps every gathered coordinate within the validated grid.
        unsafe {
            match path {
                KernelPath::Avx2 | KernelPath::Avx2Fma => x86::step_avx2(
                    state,
                    path == KernelPath::Avx2Fma,
                    masses,
                    force,
                    WIDTH,
                    HEIGHT,
                    1.0 / 60.0,
                    0.2,
                    0.0,
                    0.3,
                    45.0,
                    2.0,
                    false,
                    None,
                ),
                KernelPath::Avx512 | KernelPath::Avx512Fma => x86::step_avx512(
                    state,
                    path == KernelPath::Avx512Fma,
                    masses,
                    force,
                    WIDTH,
                    HEIGHT,
                    1.0 / 60.0,
                    0.2,
                    0.0,
                    0.3,
                    45.0,
                    2.0,
                    false,
                    None,
                ),
                KernelPath::Scalar => unreachable!(),
            }
        }
        state.dirty = true;
    }

    fn median(mut values: Vec<u128>) -> u128 {
        values.sort_unstable();
        values[values.len() / 2]
    }

    #[test]
    #[ignore = "paired AVX2/AVX-512 and FMA Wind kernel throughput experiment"]
    fn report_wind_simd_kernel_throughput() {
        if !std::arch::is_x86_feature_detected!("avx2")
            || !std::arch::is_x86_feature_detected!("avx512f")
        {
            println!("{{\"type\":\"warning\",\"message\":\"host lacks AVX2 or AVX-512F\"}}");
            return;
        }
        let mut paths = vec![KernelPath::Avx2, KernelPath::Avx512];
        if std::arch::is_x86_feature_detected!("fma") {
            paths.push(KernelPath::Avx2Fma);
            paths.push(KernelPath::Avx512Fma);
        }
        for count in [20_000, 50_000] {
            let (dots, masses, force) = fixture(count);
            let mut reference = prepared_state(&dots, &masses, &force);
            run_kernel(KernelPath::Avx2, &mut reference, &masses, &force);
            for path in paths.iter().copied().skip(1) {
                let mut candidate = prepared_state(&dots, &masses, &force);
                run_kernel(path, &mut candidate, &masses, &force);
                for ((expected_x, actual_x), (expected_y, actual_y)) in reference
                    .x
                    .iter()
                    .zip(&candidate.x)
                    .zip(reference.y.iter().zip(&candidate.y))
                {
                    assert!(
                        (expected_x - actual_x).abs() < 0.01
                            && (expected_y - actual_y).abs() < 0.01,
                        "{path:?} position mismatch"
                    );
                }
            }

            let mut avx2_times = Vec::with_capacity(7);
            let mut avx2_fma_times = Vec::with_capacity(7);
            let mut avx512_times = Vec::with_capacity(7);
            let mut avx512_fma_times = Vec::with_capacity(7);
            for sample in 0..7 {
                let mut sample_paths = paths.clone();
                if sample % 2 != 0 {
                    sample_paths.reverse();
                }
                for path in sample_paths {
                    let mut state = prepared_state(&dots, &masses, &force);
                    let start = Instant::now();
                    for _ in 0..STEPS {
                        run_kernel(path, &mut state, &masses, &force);
                    }
                    black_box(state.x[0]);
                    let elapsed = start.elapsed().as_nanos();
                    match path {
                        KernelPath::Avx2 => avx2_times.push(elapsed),
                        KernelPath::Avx2Fma => avx2_fma_times.push(elapsed),
                        KernelPath::Avx512 => avx512_times.push(elapsed),
                        KernelPath::Avx512Fma => avx512_fma_times.push(elapsed),
                        KernelPath::Scalar => unreachable!(),
                    }
                }
            }
            let avx2_median = median(avx2_times);
            let avx2_fma_median = (!avx2_fma_times.is_empty()).then(|| median(avx2_fma_times));
            let avx512_median = median(avx512_times);
            let avx512_fma_median =
                (!avx512_fma_times.is_empty()).then(|| median(avx512_fma_times));
            let avx2_fma_median_json =
                avx2_fma_median.map_or_else(|| "null".to_owned(), |value| value.to_string());
            let avx512_fma_median_json =
                avx512_fma_median.map_or_else(|| "null".to_owned(), |value| value.to_string());
            println!(
                "{{\"type\":\"result\",\"dots\":{count},\"steps_per_sample\":{STEPS},\"samples\":7,\"avx2_median_ns\":{avx2_median},\"avx2_fma_median_ns\":{avx2_fma_median_json},\"avx512_median_ns\":{avx512_median},\"avx512_fma_median_ns\":{avx512_fma_median_json},\"avx512_over_avx2_speedup\":{:.6},\"avx2_fma_speedup\":{:.6},\"avx512_fma_speedup\":{:.6},\"scope\":\"Wind SIMD integration kernel only; excludes force-cache build, rendering and terminal IO\"}}",
                avx2_median as f64 / avx512_median as f64,
                avx2_fma_median.map_or(0.0, |median| avx2_median as f64 / median as f64),
                avx512_fma_median.map_or(0.0, |median| avx512_median as f64 / median as f64),
            );
        }
    }
}
