#define_import_path foundation::noise

// Shared procedural-noise helpers, previously copy-pasted across ~7 shaders (2026-07-19 review Finding E;
// deferred from the 2026-07-05 review [9]). One home for the whole chain — Dave-Hoskins hash → value
// noise → fbm — plus the impact/blood `rand_dir`. Procedural noise is compact, randomly-accessible,
// constant-time, and *parametrized* (Lagae, Lefebvre, Cook, DeRose, Drettakis, Ebert, Lewis, Perlin,
// Zwicker, "A Survey of Procedural Noise Functions", Computer Graphics Forum 2010,
// DOI 10.1111/j.1467-8659.2010.01827.x), so the two call-site-varying knobs — fbm octave count and
// rand_dir radius band — are function parameters, not per-shader forks.
//
// Loaded as an embedded shader library (`lib::run` → `load_shader_library!`), so every asset-loaded
// consumer resolves `#import foundation::noise::{hash21, vnoise, fbm, rand_dir}`.

// Dave-Hoskins hash → [0,1). Texture-free, so it tiles infinitely and needs no repeat-address sampler.
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

// fbm with a caller-chosen octave count. Same 0.5 amplitude / ×2 lacunarity as every prior copy.
fn fbm(p: vec2<f32>, octaves: i32) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var pp = p;
    for (var i = 0; i < octaves; i = i + 1) {
        v = v + amp * vnoise(pp);
        pp = pp * 2.0;
        amp = amp * 0.5;
    }
    return v;
}

// Named entry points for the two octave counts this project uses, so a call site is a plain rename with no
// signature change: `fbm5` for the vhs post-process quad; `fbm4` for the mycelia surfaces (the 5th octave
// is sub-pixel over a whole-floor footprint, so it's dropped there).
fn fbm4(p: vec2<f32>) -> f32 { return fbm(p, 4); }
fn fbm5(p: vec2<f32>) -> f32 { return fbm(p, 5); }

// A pseudo-random launch direction for particle `i`, filling a disc of radius `[r0, r0 + r1]`. impact_fx
// uses (0.3, 0.7); blood_spray uses (0.25, 0.75) — a wider inner void — passed as params to preserve both.
fn rand_dir(i: f32, seed: f32, r0: f32, r1: f32) -> vec2<f32> {
    let angle = hash21(vec2<f32>(i, seed)) * 6.2831853;
    let radius = r0 + r1 * hash21(vec2<f32>(i + 11.0, seed + 3.0));
    return vec2<f32>(cos(angle), sin(angle)) * radius;
}
