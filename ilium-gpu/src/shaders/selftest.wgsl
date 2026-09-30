// Self-test kernel: a diagonal gradient, used only by the crate's own tests.
//
// Binding contract shared by every kernel (see `wgpu_backend.rs`):
//   @group(0) @binding(0) storage, read_write: the output field, width * height f32.
//   @group(0) @binding(1) uniform: a 16-byte header {width, height, 0, 0} followed by the
//                         job's f32 uniforms (padded to a multiple of 16 bytes).
//
// Output: gain * (x + y) / (width + height - 2), where gain is uniforms[0].

struct Params {
    width: u32,
    height: u32,
    reserved_a: u32,
    reserved_b: u32,
    gain: vec4<f32>,
}

@group(0) @binding(0) var<storage, read_write> field: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.width || id.y >= params.height) {
        return;
    }
    let span = max(f32(params.width + params.height) - 2.0, 1.0);
    field[id.y * params.width + id.x] = params.gain.x * f32(id.x + id.y) / span;
}
