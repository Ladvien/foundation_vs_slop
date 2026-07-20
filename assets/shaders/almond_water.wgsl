// Almond Water puddle — a slightly-iridescent pool that bubbles up from the concrete. One translucent
// overlay quad over the whole floor footprint, sampling a 192² water-level texture by world XZ (uploaded
// from the gameplay field). Three composited layers:
//
//   A. Bubble-up blooms — procedural blobs that well up from the surface, gated by the sampled level, so
//      they appear only where water actually pools and pulse gently (the "it seeps up" motion).
//   B. Thin-film interference — physically-based oil-slick iridescence. The film is ~a wavelength thick;
//      reflections off its top and bottom surface interfere, and the in/out-of-phase angle differs per
//      wavelength, splitting white light into colour. Optical path difference OPD = 2·n·d·cos(θ₂), with
//      θ₂ the refraction angle inside the film (Snell); per-channel reflectance 0.5 + 0.5·cos(2π·OPD/λ) at
//      λ = (700, 550, 400) nm. Water-on-concrete has n_air < n_water < n_concrete, so a 180° phase flip
//      occurs at BOTH interfaces and cancels → the anti-reflection-coating constructive case (Hecht,
//      *Optics*; the closed form approximates Belcour & Barla, SIGGRAPH 2017). The palette is biased toward
//      muted golds/teals/magentas — real films read as a wavelength mixture, not a clean spectral rainbow.
//   C. Almond base tint, composited under A/B.
//
// Cosmetic and windowed-only (never in the headless harness) — it reads the render clock, so it cannot
// touch a determinism golden. hash: Dave Hoskins, "Hash without Sine".

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::{globals, view}

struct AlmondParams {
    bounds: vec4<f32>,   // (world_origin.xy, world_extent.xy)
    params0: vec4<f32>,  // (field_res, min_visible_norm, film_thickness_nm, film_ior)
    params1: vec4<f32>,  // (iridescence_strength, almond_tint.rgb)
    params2: vec4<f32>,  // (iridescence_mute, poison_tint.rgb)
    params3: vec4<f32>,  // (base_alpha, rim_strength, glint_strength, ripple_strength)
    params4: vec4<f32>,  // (edge_feather, feather_scale, _, _)
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> mat: AlmondParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var level_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var level_smp: sampler;

// Hoskins hash13: a 2D point (+ a scalar) → a reproducible pseudo-random scalar in [0,1). No trig, no
// texture — "Hash without Sine".
fn hash13(p3in: vec3<f32>) -> f32 {
    var p3 = fract(p3in * 0.1031);
    p3 = p3 + dot(p3, p3.zyx + 31.32);
    return fract((p3.x + p3.y) * p3.z);
}

// Smooth 2D value noise + 3-octave fBm from the same hash — used to perturb the pool boundary so the
// per-cell (bilinear) water field breaks into an organic puddle margin instead of tile-aligned diamonds.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f); // smoothstep interpolation
    let a = hash13(vec3<f32>(i + vec2<f32>(0.0, 0.0), 0.0));
    let b = hash13(vec3<f32>(i + vec2<f32>(1.0, 0.0), 0.0));
    let c = hash13(vec3<f32>(i + vec2<f32>(0.0, 1.0), 0.0));
    let d = hash13(vec3<f32>(i + vec2<f32>(1.0, 1.0), 0.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i: i32 = 0; i < 3; i = i + 1) {
        v = v + amp * vnoise(q);
        q = q * 2.03;
        amp = amp * 0.5;
    }
    return v; // ~[0, 0.875], mean ~0.4375
}

// Layer A — bubble-up blooms in cell UV. A handful of blobs per neighbourhood, each with a hashed position
// and phase, growing and fading on a staggered cycle so the field wells continuously. Amplitude is small and
// gated by `wet` outside, so a dry cell shows nothing.
fn blooms(uv: vec2<f32>, t: f32) -> f32 {
    let cell = uv * mat.params0.x; // ~dungeon-cell coordinates (0..192)
    var acc = 0.0;
    // 12 staggered layers is enough to read as a continuous simmer without banding.
    for (var i: i32 = 0; i < 12; i = i + 1) {
        let fi = f32(i);
        // A slow per-layer clock, offset so blooms don't pulse in lockstep.
        let lt = fract(t * 0.18 + hash13(vec3<f32>(fi, 1.0, 3.0)));
        // Random bloom anchor, snapped to a coarse lattice so blooms sit on the concrete, not smeared.
        let anchor = floor(cell / 6.0) + vec2<f32>(
            hash13(vec3<f32>(fi, 2.0, 5.0)),
            hash13(vec3<f32>(fi, 4.0, 7.0)),
        );
        let center = (anchor + 0.5) * 6.0;
        let d = length(cell - center) / (1.5 + 4.0 * lt); // grows outward over the layer's life
        // A ring that swells then fades: bright at birth, gone by the end of the cycle.
        let ring = smoothstep(1.0, 0.0, d) * sin(lt * 3.14159265);
        acc = acc + max(ring, 0.0);
    }
    return acc / 12.0;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let origin = mat.bounds.xy;
    let extent = mat.bounds.zw;
    let uv = (in.world_position.xz - origin) / extent;
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        discard;
    }

    // Field texture: R = normalised water level (0..1), G = belief (0 = cyanide, 1 = heal). `textureSampleLevel`
    // (LOD 0) is derivative-safe after the `discard`.
    let field = textureSampleLevel(level_tex, level_smp, uv, 0.0).rg;
    let belief = field.g;
    // Feather the boundary: shift the water level by fBm noise keyed to WORLD position, so the pool's edge
    // wiggles organically (sub-cell) instead of tracing the bilinear per-cell diamonds that read as tiles.
    // Centred (mean ~0.4375) so it neither grows nor shrinks the pool on average — it just breaks the line.
    let edge_feather = mat.params4.x;
    let feather_scale = mat.params4.y;
    let fn_ = fbm(in.world_position.xz * feather_scale) - 0.4375;
    let level = clamp(field.r + fn_ * edge_feather, 0.0, 1.0);
    let min_vis = mat.params0.y;
    if (level <= min_vis) {
        discard; // dry concrete
    }
    let wet = clamp((level - min_vis) / max(1.0 - min_vis, 1.0e-4), 0.0, 1.0);

    // --- Layer B: thin-film interference ---------------------------------------------------------------
    // View direction from the fragment to the camera; the pool normal is world-up, so cosθ₁ = V.y.
    let v = normalize(view.world_position - in.world_position.xyz);
    let cos1 = clamp(abs(v.y), 1.0e-3, 1.0);
    let n = max(mat.params0.w, 1.0);
    // Snell: cosθ₂ = sqrt(1 − (n_air/n)²·(1 − cos²θ₁)), n_air = 1.
    let sin2_sq = (1.0 / (n * n)) * (1.0 - cos1 * cos1);
    let cos2 = sqrt(max(1.0 - sin2_sq, 0.0));
    let opd = 2.0 * n * mat.params0.z * cos2; // nanometres
    let two_pi = 6.28318530718;
    // Per-channel reflectance at R/G/B wavelengths (constructive, anti-reflection-coating case).
    var irid = vec3<f32>(
        0.5 + 0.5 * cos(two_pi * opd / 700.0),
        0.5 + 0.5 * cos(two_pi * opd / 550.0),
        0.5 + 0.5 * cos(two_pi * opd / 400.0),
    );
    // Mute it: real oil/water films are a wavelength MIXTURE (golds/teals/magentas), not a clean rainbow.
    // Pull each channel toward the mean so the sheen stays tasteful and almond-appropriate (mute now tunable).
    let mean = (irid.r + irid.g + irid.b) / 3.0;
    irid = mix(vec3<f32>(mean), irid, mat.params2.x);

    // --- Layer C: base tint, driven by BELIEF (the inversion mechanic) then tinted by the (muted) film ----
    // A pool the population reads as cyanide (belief→0) shows `poison_tint`; one it trusts (belief→1) shows the
    // almond base. This is the diegetic "this pool is wrong" tell — which an anosmic creature still can't smell.
    let almond = mat.params1.yzw;
    let poison = mat.params2.yzw;
    let base = mix(poison, almond, clamp(belief, 0.0, 1.0));
    let strength = mat.params1.x * wet;
    var color = mix(base, base * (0.5 + irid), strength);

    // Readability/saliency knobs (Itti-Koch 1998 — a pool reads only when it out-contrasts the warm carpet
    // in colour / intensity / MOTION). params3 = (base_alpha, rim_strength, glint_strength, ripple_strength).
    let base_alpha = mat.params3.x;
    let rim_strength = mat.params3.y;
    let glint_strength = mat.params3.z;
    let ripple_strength = mat.params3.w;

    // --- Layer A: bubble-up blooms + ripples, welling where water pools. Motion is the strongest attention
    //     cue, so `ripple_strength` scales the simmer up from the authored baseline. --------------------
    let bloom = blooms(uv, globals.time) * wet * ripple_strength;
    color = color + vec3<f32>(bloom * 0.25);

    // --- Shoreline rim: a glowing waterline at the shallow margin so the pool reads as a distinct object
    //     (figure-ground edge). `wet` is ~0 at the just-wet edge and →1 in the deep, so a band near the
    //     edge traces the shore. A brightened pool colour rims a heal pool cyan, a cyanide pool green. ---
    let rim = smoothstep(0.0, 0.16, wet) * (1.0 - smoothstep(0.16, 0.42, wet));
    let rim_col = base + vec3<f32>(0.35);
    color = color + rim_col * (rim * rim_strength);

    // --- Wet specular glint: a broad view-aligned sheen plus bright sparkles riding the ripple crests, so
    //     the surface catches the room light and glints as it moves (luminance contrast; reads as liquid).
    let sheen = pow(cos1, 8.0) * 0.15;
    let sparkle = pow(clamp(bloom, 0.0, 1.0), 2.0) * 1.5;
    color = color + vec3<f32>((sheen + sparkle) * glint_strength);

    // Opacity: deeper water is more opaque (up to `base_alpha`, no longer a faint 0.6 film); the shoreline
    // rim is a touch crisper so the boundary stays legible even in the shallows.
    let alpha = clamp(wet * base_alpha + rim * rim_strength * 0.25, 0.0, 1.0);
    return vec4<f32>(color, alpha);
}
