// MYCELIA — Phase A plumbing test. A single compute pass that writes an animated pattern into the
// shared `mold_display` texture, proving the compute→texture→material path on Bevy 0.19 before any
// Physarum / reaction-diffusion is layered in. Later phases replace `gradient` with the real
// agents/diffuse/field passes writing the same texture.

// MUST byte-match `MoldParams` in `src/mycelia/mod.rs` (std140 uniform layout).
struct MoldParams {
    world_origin: vec2<f32>,
    world_extent: vec2<f32>,
    field_res: vec2<f32>,
    time: f32,
    _pad: f32,
};

@group(0) @binding(0) var display: texture_storage_2d<rgba16float, write>;
@group(0) @binding(1) var<uniform> params: MoldParams;

@compute @workgroup_size(8, 8, 1)
fn gradient(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(display);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    // Normalized field coords [0,1].
    let uv = vec2<f32>(f32(id.x), f32(id.y)) / vec2<f32>(f32(dims.x), f32(dims.y));
    let t = params.time;

    // A drifting interference pattern: obviously animated + obviously spatial, so the screenshot
    // confirms both the GPU write and the world-XZ sampling in the floor material.
    let a = sin(uv.x * 24.0 + t * 1.3);
    let b = cos(uv.y * 24.0 - t * 0.9);
    let v = 0.5 + 0.5 * a * b;

    // Grimy-biolum stand-in colours (sickly green/cyan) so Phase A already reads on-theme.
    let veins = smoothstep(0.55, 0.95, v);
    let rgb = vec3<f32>(0.05, 0.9, 0.5) * veins + vec3<f32>(0.0, 0.05, 0.08) * v;
    textureStore(display, vec2<i32>(i32(id.x), i32(id.y)), vec4<f32>(rgb, veins));
}
