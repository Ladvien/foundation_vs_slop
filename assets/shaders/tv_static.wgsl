// Animated CRT "dead channel" static for a TV screen mesh. Ported from the Shadertoy analog-TV
// distortion shader the player supplied (vertical jerk/roll, bottom static, scanlines, horizontal
// fuzz). The original distorts an input image (iChannel0); a powered-on-but-untuned TV has no signal,
// so we drop the image and render the *snow itself* — the `staticV` term becomes the whole picture,
// rolled and fuzzed like a detuned set. Driven by `globals.time`; the material is unlit, so its output
// IS the emitted colour (the screen self-glows in the dark). See `src/light.rs` (`TvStaticMaterial`,
// `glow_screens`). Simplex noise from ashima/webgl-noise (webgl-noise, MIT).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

struct TvStatic {
    // rgb = cool CRT tint applied to the snow; a = overall glow/brightness multiplier.
    tint: vec4<f32>,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: TvStatic;

// --- Effect toggles (compile-time), matching the source shader's per-effect switches. ---
const VERT_JERK_OPT: f32 = 1.0;
const VERT_MOVEMENT_OPT: f32 = 1.0;
const BOTTOM_STATIC_OPT: f32 = 1.0;
const SCANLINES_OPT: f32 = 1.0;
const HORZ_FUZZ_OPT: f32 = 1.0;
// The source shader's static was a subtle overlay on an image (weights sum to ~0.1); as the whole
// picture it must be amplified to read as bright snow, with a dim base so the dark gaps still glow "on".
const SNOW_GAIN: f32 = 6.0;
const SNOW_BASE: f32 = 0.1;

// --- ashima/webgl-noise simplex 2D (mod289 / permute / snoise). ---
fn mod289v3(x: vec3<f32>) -> vec3<f32> { return x - floor(x * (1.0 / 289.0)) * 289.0; }
fn mod289v2(x: vec2<f32>) -> vec2<f32> { return x - floor(x * (1.0 / 289.0)) * 289.0; }
fn permute3(x: vec3<f32>) -> vec3<f32> { return mod289v3(((x * 34.0) + 1.0) * x); }

fn snoise(v: vec2<f32>) -> f32 {
    let C = vec4<f32>(0.211324865405187, 0.366025403784439, -0.577350269189626, 0.024390243902439);
    // First corner.
    var i = floor(v + dot(v, C.yy));
    let x0 = v - i + dot(i, C.xx);
    // Other corners.
    var i1 = vec2<f32>(0.0, 1.0);
    if (x0.x > x0.y) { i1 = vec2<f32>(1.0, 0.0); }
    // x12 = x0.xyxy + C.xxzz, then subtract i1 from its xy (WGSL has no multi-component swizzle assign).
    var x12 = x0.xyxy + C.xxzz;
    x12 = vec4<f32>(x12.x - i1.x, x12.y - i1.y, x12.z, x12.w);
    // Permutations.
    i = mod289v2(i);
    let p = permute3(permute3(i.y + vec3<f32>(0.0, i1.y, 1.0)) + i.x + vec3<f32>(0.0, i1.x, 1.0));
    var m = max(0.5 - vec3<f32>(dot(x0, x0), dot(x12.xy, x12.xy), dot(x12.zw, x12.zw)), vec3<f32>(0.0));
    m = m * m;
    m = m * m;
    // Gradients.
    let x = 2.0 * fract(p * C.www) - 1.0;
    let h = abs(x) - 0.5;
    let ox = floor(x + 0.5);
    let a0 = x - ox;
    m = m * (1.79284291400159 - 0.85373472095314 * (a0 * a0 + h * h));
    // Final noise.
    let gyz = a0.yz * x12.xz + h.yz * x12.yw;
    let g = vec3<f32>(a0.x * x0.x + h.x * x0.y, gyz.x, gyz.y);
    return 130.0 * dot(m, g);
}

// The snow field: a time-varying threshold on high-frequency noise, scaled by a fluctuating strength.
fn static_v(uv: vec2<f32>, t: f32) -> f32 {
    let static_height = snoise(vec2<f32>(9.0, t * 1.2 + 3.0)) * 0.3 + 5.0;
    let static_amount = snoise(vec2<f32>(1.0, t * 1.2 - 6.0)) * 0.1 + 0.3;
    let static_strength = snoise(vec2<f32>(-9.75, t * 0.6 - 3.0)) * 2.0 + 2.0;
    let roll = (t - floor(t / 100.0) * 100.0 + 100.0) * uv.y * 0.3 + 3.0; // mod(t,100)+100 for the y term
    let n = snoise(vec2<f32>(5.0 * pow(t, 2.0) + pow(uv.x * 7.0, 1.2), pow(roll, static_height)));
    return (1.0 - step(n, static_amount)) * static_strength;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time;
    let uv = mesh.uv;

    // Vertical roll + jerk: an untuned set drifts and hitches vertically.
    let vert_movement_on = (1.0 - step(snoise(vec2<f32>(t * 0.2, 8.0)), 0.4)) * VERT_MOVEMENT_OPT;
    let vert_jerk = (1.0 - step(snoise(vec2<f32>(t * 1.5, 5.0)), 0.6)) * VERT_JERK_OPT;
    let vert_jerk2 = (1.0 - step(snoise(vec2<f32>(t * 5.5, 5.0)), 0.2)) * VERT_JERK_OPT;
    let y_offset = abs(sin(t) * 4.0) * vert_movement_on + vert_jerk * vert_jerk2 * 0.3;

    // Horizontal fuzz: sub-pixel jitter of the sampled column (rolls the snow sideways).
    let fuzz = snoise(vec2<f32>(t * 15.0, uv.y * 80.0)) * 0.003;
    let large_fuzz = snoise(vec2<f32>(t * 1.0, uv.y * 25.0)) * 0.004;
    let x_offset = (fuzz + large_fuzz) * HORZ_FUZZ_OPT;
    let sample_uv = vec2<f32>(uv.x + x_offset, fract(uv.y + y_offset));

    // Accumulate the snow over a small vertical neighbourhood (the source's "bottom static" blur).
    var static_val = 0.0;
    for (var yy: f32 = -1.0; yy <= 1.0; yy = yy + 1.0) {
        let max_dist = 5.0 / 200.0;
        let dist = yy / 200.0;
        static_val = static_val + static_v(vec2<f32>(sample_uv.x, sample_uv.y + dist), t) * (max_dist - abs(dist)) * 1.5;
    }
    static_val = static_val * BOTTOM_STATIC_OPT;

    // Dead channel: no signal, so the snow IS the whole picture. In the source shader the static was a
    // faint overlay on a full-brightness image; standing alone it needs amplifying (SNOW_GAIN), plus a
    // dim base so the screen reads as powered-on ("on" grey) between the bright specks. Cool-tint it (a
    // CRT's bluish cast) and scale by the glow multiplier, then carve the scanlines.
    let snow = static_val * SNOW_GAIN + SNOW_BASE;
    var color = vec3<f32>(snow) * material.tint.rgb * material.tint.a;
    let scanline = sin(uv.y * 800.0) * 0.04 * SCANLINES_OPT;
    color = color - scanline;

    // Unlit: this colour is emitted directly, so the screen glows on its own in the dark room.
    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
