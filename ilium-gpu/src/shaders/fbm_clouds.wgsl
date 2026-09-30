// fBm clouds kernel: a line-by-line port of the software scene in
// ilium-ambient/src/scenes/fbm_clouds/{field,mod}.rs.
//
// Binding contract (see wgpu_backend.rs): binding 0 = output dots, binding 1 =
// uniforms with a 16-byte header {width, height, 0, 0} followed by the 16 job
// uniforms documented in ilium-ambient/src/scenes/fbm_clouds/gpu.rs. Keep the
// indices below in sync with that file.
//
// The noise lattice is the CPU's splitmix64 table. WGSL has no 64-bit
// integers, so texel `i` (= state0 + (i + 1) * GAMMA, then the splitmix64
// finaliser) is recomputed on demand with 64-bit arithmetic emulated on
// vec2<u32> (x = low word, y = high word). The texel values are bit-identical
// to the CPU's; only f32 rounding of the noise maths can differ.

struct Params {
    size: vec4<u32>,
    values: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<storage, read_write> output: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;

// Uniform indices (scalar i lives at params.values[i / 4][i % 4]).
const U_BLOCK: u32 = 0u;          // pixel block size in dots, 1..4
const U_OCTAVES: u32 = 1u;        // fBm octaves, 2..4
const U_WARP: u32 = 2u;           // warp strength (percent / 100)
const U_PHASE: u32 = 3u;          // layer slide (source's t * 0.25 term)
const U_PAN: u32 = 4u;            // pan offset in noise-space units
const U_SCALE: u32 = 5u;          // noise frequency (3 * scale percent / 100)
const U_CONTRAST: u32 = 6u;       // contrast (percent / 100)
const U_BRIGHTNESS: u32 = 7u;     // lit-dot intensity (percent / 100)
const U_INVERT: u32 = 8u;         // 0 or 1
const U_DITHER: u32 = 9u;         // 0 bayer, 1 gradient noise, 2 white noise
const U_SEED: u32 = 10u;          // cloud layout seed, 0..999
const U_FLIP: u32 = 11u;          // vertical flip factor -(1 + 0.0625 * aspect)
const U_INVERSE_HEIGHT: u32 = 12u; // 1 / height in dots

const LATTICE_MASK: i32 = 127;
const LUMINANCE_GAIN: f32 = 0.3;
const GLOW_SHARE: f32 = 0.15;
const DITHER_TILE: u32 = 64u;

const GAMMA: vec2<u32> = vec2<u32>(0x7f4a7c15u, 0x9e3779b9u);
const MIX_ONE: vec2<u32> = vec2<u32>(0x1ce4e5b9u, 0xbf58476du);
const MIX_TWO: vec2<u32> = vec2<u32>(0x133111ebu, 0x94d049bbu);

var<private> octave_lacunarity: array<f32, 4> = array<f32, 4>(2.02, 2.03, 2.01, 2.0);
var<private> octave_weight: array<f32, 4> = array<f32, 4>(0.5, 0.25, 0.125, 0.0625);
var<private> bayer_8: array<u32, 64> = array<u32, 64>(
    0u, 48u, 12u, 60u, 3u, 51u, 15u, 63u,
    32u, 16u, 44u, 28u, 35u, 19u, 47u, 31u,
    8u, 56u, 4u, 52u, 11u, 59u, 7u, 55u,
    40u, 24u, 36u, 20u, 43u, 27u, 39u, 23u,
    2u, 50u, 14u, 62u, 1u, 49u, 13u, 61u,
    34u, 18u, 46u, 30u, 33u, 17u, 45u, 29u,
    10u, 58u, 6u, 54u, 9u, 57u, 5u, 53u,
    42u, 26u, 38u, 22u, 41u, 25u, 37u, 21u,
);
// Initial splitmix64 state of the current invocation (depends on the seed).
var<private> lattice_state0: vec2<u32>;

fn param(index: u32) -> f32 {
    return params.values[index / 4u][index % 4u];
}

// Full 64-bit product of two u32 as (low, high), via 16-bit limbs.
fn mul32_wide(a: u32, b: u32) -> vec2<u32> {
    let a0 = a & 0xffffu;
    let a1 = a >> 16u;
    let b0 = b & 0xffffu;
    let b1 = b >> 16u;
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let middle = (p00 >> 16u) + (p01 & 0xffffu) + (p10 & 0xffffu);
    let low = (p00 & 0xffffu) | (middle << 16u);
    let high = p11 + (p01 >> 16u) + (p10 >> 16u) + (middle >> 16u);
    return vec2<u32>(low, high);
}

// Wrapping 64-bit multiply.
fn mul64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let wide = mul32_wide(a.x, b.x);
    return vec2<u32>(wide.x, wide.y + a.x * b.y + a.y * b.x);
}

// Wrapping 64-bit add.
fn add64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let low = a.x + b.x;
    let carry = select(0u, 1u, low < a.x);
    return vec2<u32>(low, a.y + b.y + carry);
}

// value ^ (value >> shift) for 0 < shift < 32.
fn xor_shift_right64(value: vec2<u32>, shift: u32) -> vec2<u32> {
    let low = (value.x >> shift) | (value.y << (32u - shift));
    let high = value.y >> shift;
    return vec2<u32>(value.x ^ low, value.y ^ high);
}

fn texel(x: i32, y: i32) -> f32 {
    let index = u32((y & LATTICE_MASK) * 128 + (x & LATTICE_MASK));
    var mixed = add64(lattice_state0, mul64(vec2<u32>(index + 1u, 0u), GAMMA));
    mixed = mul64(xor_shift_right64(mixed, 30u), MIX_ONE);
    mixed = mul64(xor_shift_right64(mixed, 27u), MIX_TWO);
    mixed = xor_shift_right64(mixed, 31u);
    // (mixed >> 40) as f32 / 2^24 on the CPU.
    return f32(mixed.y >> 8u) / 16777216.0;
}

fn noise(x: f32, y: f32) -> f32 {
    let floor_x = floor(x);
    let floor_y = floor(y);
    let fraction_x = x - floor_x;
    let fraction_y = y - floor_y;
    let smooth_x = fraction_x * fraction_x * (3.0 - 2.0 * fraction_x);
    let smooth_y = fraction_y * fraction_y * (3.0 - 2.0 * fraction_y);
    let cell_x = i32(floor_x);
    let cell_y = i32(floor_y);
    let top = texel(cell_x, cell_y);
    let top_right = texel(cell_x + 1, cell_y);
    let bottom = texel(cell_x, cell_y + 1);
    let bottom_right = texel(cell_x + 1, cell_y + 1);
    let upper = top + (top_right - top) * smooth_x;
    let lower = bottom + (bottom_right - bottom) * smooth_x;
    return upper + (lower - upper) * smooth_y;
}

fn fbm(start_x: f32, start_y: f32, octaves: u32) -> f32 {
    var x = start_x;
    var y = start_y;
    var sum = 0.0;
    var weight_sum = 0.0;
    for (var octave = 0u; octave < octaves; octave = octave + 1u) {
        sum = sum + octave_weight[octave] * noise(x, y);
        weight_sum = weight_sum + octave_weight[octave];
        let scale = octave_lacunarity[octave];
        let rotated_x = (0.8 * x + 0.6 * y) * scale;
        y = (-0.6 * x + 0.8 * y) * scale;
        x = rotated_x;
    }
    return sum / weight_sum;
}

fn cloud_luminance(x: f32, y: f32, octaves: u32, warp: f32, phase: f32) -> f32 {
    let q_x = fbm(x + phase, y + phase, octaves);
    let q_y = fbm(x + 1.0, y + 1.0, octaves);
    let base_x = x + warp * q_x;
    let base_y = y + warp * q_y;
    let r_x = fbm(base_x + 1.7 + 0.31 * phase, base_y + 9.2 + 0.31 * phase, octaves);
    let r_y = fbm(base_x + 8.3 + 0.21 * phase, base_y + 2.8 + 0.21 * phase, octaves);
    let density = fbm(x + warp * r_x, y + warp * r_y, octaves);

    let cyan_mix = clamp(density * density * 2.0, 0.0, 1.0);
    var red = 1.0 + (0.3 - 1.0) * cyan_mix;
    var green = 1.0 + (1.6 - 1.0) * cyan_mix;
    var blue = green;
    let brown_mix = clamp(sqrt(q_x * q_x + q_y * q_y), 0.0, 1.0);
    red = red + (0.4 - red) * brown_mix;
    green = green + (0.2 - green) * brown_mix;
    blue = blue + (0.16 - blue) * brown_mix;
    let blue_mix = clamp(r_x, 0.0, 1.0);
    red = red + (0.4 - red) * blue_mix;
    green = green + (0.7 - green) * blue_mix;
    blue = blue + (3.0 - blue) * blue_mix;
    return red * red * red * 0.299 + green * green * green * 0.587 + blue * blue * blue * 0.114;
}

// Mirrors ilium_ambient::raster::hash.
fn hash_threshold(x: i32, y: i32) -> f32 {
    var value = (u32(x) * 0x8da6b343u) ^ (u32(y) * 0xd8163841u) ^ 0xcb1ab31fu;
    value = value ^ (value >> 16u);
    value = value * 0x7feb352du;
    value = value ^ (value >> 15u);
    value = value * 0x846ca68bu;
    value = value ^ (value >> 16u);
    return f32(value & 0xffffu) / 65535.0;
}

fn dither_threshold(mode: u32, pixel_x: u32, pixel_y: u32, seed: u32) -> f32 {
    let tile_x = pixel_x % DITHER_TILE;
    let tile_y = pixel_y % DITHER_TILE;
    if (mode == 0u) {
        return (f32(bayer_8[(tile_y % 8u) * 8u + (tile_x % 8u)]) + 0.5) / 64.0;
    }
    if (mode == 1u) {
        let ramp = 0.06711056 * f32(tile_x) + 0.00583715 * f32(tile_y);
        return fract(52.982918 * fract(ramp));
    }
    return hash_threshold(i32(tile_x) + i32(seed) * 64, i32(tile_y) * 3 + 1);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = params.size.x;
    let height = params.size.y;
    if (id.x >= width || id.y >= height) {
        return;
    }
    let block = param(U_BLOCK);
    let block_size = u32(block);
    let octaves = u32(param(U_OCTAVES));
    let warp = param(U_WARP);
    let phase = param(U_PHASE);
    let pan = param(U_PAN);
    let scale = param(U_SCALE);
    let contrast = param(U_CONTRAST);
    let brightness = param(U_BRIGHTNESS);
    let is_inverted = param(U_INVERT) > 0.5;
    let dither_mode = u32(param(U_DITHER));
    let seed = u32(param(U_SEED));
    let flip = param(U_FLIP);
    let inverse_height = param(U_INVERSE_HEIGHT);

    let seed_bits = (seed << 20u) | seed;
    lattice_state0 = vec2<u32>(GAMMA.x ^ seed_bits, GAMMA.y);

    // Tone of this dot's block, evaluated like `sample_tones` on the CPU.
    let row = id.y / block_size;
    let column = id.x / block_size;
    let half_block = block * 0.5;
    let v = (f32(row) * block + half_block) * inverse_height;
    let noise_y = (-1.0 + 2.0 * v * flip) * scale;
    let u = (f32(column) * block + half_block) * inverse_height;
    let noise_x = (-1.0 + 2.0 * u - pan) * scale;
    let luminance = cloud_luminance(noise_x, noise_y, octaves, warp, phase);
    let tone = (luminance * LUMINANCE_GAIN - 0.5) * contrast + 0.5;

    var is_lit = tone >= dither_threshold(dither_mode, id.x, id.y, seed);
    var soft = clamp(tone, 0.0, 1.0);
    if (is_inverted) {
        is_lit = !is_lit;
        soft = 1.0 - soft;
    }
    var hard = 0.0;
    if (is_lit) {
        hard = 1.0 - GLOW_SHARE;
    }
    output[id.y * width + id.x] = brightness * (hard + GLOW_SHARE * soft);
}
