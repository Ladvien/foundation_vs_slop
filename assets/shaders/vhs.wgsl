// Full-screen VHS degradation pass. Runs as a Bevy 0.19 `FullscreenMaterial`
// (bevy_core_pipeline::fullscreen_material) — the engine binds the rendered frame at
// @binding(0), a sampler at @binding(1), and our per-camera settings uniform at @binding(2),
// then draws a fullscreen triangle. We read the frame, warp/tint it into a "tracking-error"
// VHS look, and cross-fade base→VHS by `intensity` so intensity 0 is an exact passthrough.
//
// This combines the signature bits of three Shadertoy VHS shaders the author supplied:
//   - RGB channel split + wobble          (ShaderToy "VHS" — chromatic aberration)
//   - bad-tracking horizontal noise waves  (ShaderToy analog-signal displacement)
//   - tape wave + tape crease + switching-noise band + horizontal bloom + AC beat
//                                          (ShaderToy "VHS tape" effect)
//
// The originals sample a value/simplex-noise texture (iChannel0 noise) for the displacement.
// We replace that with an in-shader hash + value-noise fbm (no extra texture binding), which is
// the standard GPU-evaluated, constant-time procedural-noise approach surveyed by
// Lagae et al., "A Survey of Procedural Noise Functions", CGF 2010,
// DOI 10.1111/j.1467-8659.2010.01827.x (in the project's research corpus). The simplex variant
// in one source is Ashima/McEwan (MIT, webgl-noise); we substitute cheaper value noise here.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

const PI: f32 = 3.14159265;

// Must byte-match `VhsSettings` in src/vhs.rs (8 × f32 = 32 bytes, 16-byte aligned).
// Two drive channels: `base` is the always-on texture floor; `spike` is the periodic glitch
// envelope (heavy distortion). See the fragment for how they gate each sub-effect.
struct VhsSettings {
    base: f32,
    spike: f32,
    time: f32,
    chroma: f32,
    wave: f32,
    scanline: f32,
    noise_amt: f32,
    bloom: f32,
}

@group(0) @binding(0) var screen_tex: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: VhsSettings;

// Sample at an explicit LOD 0 so it is legal inside loops / after branches (implicit-derivative
// `textureSample` requires uniform control flow; the frame texture has no mips anyway).
fn tex(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(screen_tex, screen_sampler, uv, 0.0).rgb;
}

// Dave Hoskins hash21 — texture-free, cited via Lagae 2010 (see header).
fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Bilinearly-interpolated value noise on the hash lattice.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i + vec2<f32>(0.0, 0.0));
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// A few octaves of value noise (fbm) — stands in for the sources' iChannel0 noise texture.
fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var pp = p;
    for (var i = 0; i < 5; i = i + 1) {
        v = v + amp * vnoise(pp);
        pp = pp * 2.0;
        amp = amp * 0.5;
    }
    return v;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let src = textureSampleLevel(screen_tex, screen_sampler, in.uv, 0.0);
    // Fully off (no baseline texture and no spike): pass the frame straight through.
    if settings.base <= 0.0 && settings.spike <= 0.0 {
        return src;
    }

    // Two drive amounts:
    //   tex_amt  — always-on texture floor that ramps to full during a spike (grain/scanlines/tint).
    //   dist_amt — the spike alone, gating the geometric distortions so the baseline stays clean.
    let tex_amt = clamp(settings.base + settings.spike, 0.0, 1.0);
    let dist_amt = settings.spike;

    let t = settings.time;
    let dims = vec2<f32>(textureDimensions(screen_tex));
    var uv = in.uv;

    // --- tape wave (slow) + bad-tracking jitter (fast) : horizontal UV displacement ---
    // Spike-gated: at rest (dist_amt ≈ 0) the picture is geometrically undistorted.
    var dx = (fbm(vec2<f32>(uv.y * 3.0, t)) - 0.5) * 0.02;
    dx = dx + (vnoise(vec2<f32>(uv.y * 100.0, t * 10.0)) - 0.5) * 0.01;
    // tape crease: an occasional strong horizontal shove along a moving band.
    let crease = clamp((sin(uv.y * 8.0 - t * 3.7) - 0.92) * fbm(vec2<f32>(t, t)), 0.0, 0.01) * 10.0;
    dx = dx - crease * (vnoise(vec2<f32>(uv.y * 120.0, t * 12.0)) - 0.5);
    uv.x = uv.x + dx * settings.wave * dist_amt;

    // --- switching-noise band rolling along the very bottom of the frame (spike-gated) ---
    let sw = smoothstep(0.06, 0.0, uv.y) * dist_amt;
    uv.y = uv.y + sw * 0.02;
    uv.x = uv.x + sw * (vnoise(vec2<f32>(uv.y * 90.0, t * 9.0)) - 0.5) * 0.25 * settings.wave;

    // --- chromatic aberration / RGB channel split : a faint constant fringe, wider during a spike ---
    let off = vec2<f32>(0.006 * sin(t) + 0.004, 0.0) * settings.chroma * mix(0.15, 1.0, dist_amt);
    var col = vec3<f32>(tex(uv + off).r, tex(uv).g, tex(uv - off).b);

    // --- horizontal chroma-bloom smear: a few one-sided taps, per-channel staggered (spike-gated) ---
    if settings.bloom > 0.0 && dist_amt > 0.0 {
        var glow = vec3<f32>(0.0);
        for (var i = -3; i <= 2; i = i + 1) {
            let o = vec2<f32>(f32(i) * 0.007, 0.0);
            glow = glow + vec3<f32>(
                tex(uv + o).r,
                tex(uv + o - vec2<f32>(0.002, 0.0)).g,
                tex(uv + o - vec2<f32>(0.004, 0.0)).b,
            );
        }
        col = col + glow * 0.03 * settings.bloom * dist_amt;
    }

    // --- scanlines: smoothly darken alternate rows (~2px pitch) — part of the always-on texture ---
    let frag_y = uv.y * dims.y;
    let line = 0.5 + 0.5 * sin(frag_y * PI);
    col = col * (1.0 - settings.scanline * 0.15 * (1.0 - line));

    // --- grain: per-pixel jitter that also crawls with time (always-on texture) ---
    let grain = (hash21(uv * dims + vec2<f32>(t * 137.0, t * 91.0)) - 0.5) * 0.12 * settings.noise_amt;
    col = col + grain;

    // --- AC beat: a subtle vertical brightness pulse ---
    col = col * (1.0 + clamp(vnoise(vec2<f32>(0.0, uv.y + t * 0.2)) * 0.4 - 0.15, 0.0, 0.1));

    let vhs = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    // `tex_amt` floors at `base`, so the grain/scanline texture is always faintly present.
    return vec4<f32>(mix(src.rgb, vhs, tex_amt), src.a);
}
