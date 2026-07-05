// Blood pool / wall-splat decal — a flat quad (floor) or upright quad (wall) that grows from the
// death point into an organic, highly-varied splatter with a wet sheen, then holds as a permanent
// stain. The silhouette is an SDF (a wobbly main blob smooth-unioned with random satellite droplets
// and a couple of thrown streaks) so the square quad edge is never visible — only the splat shape
// composites through the alpha-blended pass. Every pool gets a per-pool `seed`, and almost every
// shape parameter is hashed from it, so no two pools look alike.
//
// `clip` masks the pool past a wall face (floor pools only) so blood can't seep through walls.
//
// Radius grows over `grow_time` via `globals.time - spawn_time`. Texture-free value noise (hash21)
// follows Lagae et al., "A Survey of Procedural Noise Functions", CGF 2010
// (DOI 10.1111/j.1467-8659.2010.01827.x). See `src/gore.rs`.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

struct PoolSettings {
    color: vec4<f32>,   // deep blood red (rgb); a=1
    // Per-axis clip in quad-`p` units (+X, -X, +Z, -Z). Large ⇒ no clipping (wall splats).
    clip: vec4<f32>,
    // Per-diagonal clip (+X+Z, -X+Z, +X-Z, -X-Z), same units — stops corner leaks past walls.
    clip_diag: vec4<f32>,
    spawn_time: f32,
    grow_time: f32,
    gloss: f32,         // strength of the wet sheen highlight
    seed: f32,          // per-pool random so no two pools share a silhouette
    dry_time: f32,      // seconds to dry: darken to matte maroon + lose the glint
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: PoolSettings;
// Real blood PBR maps (an atlas of splatters + its normal) for photo-real surface detail + wet glints.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_smp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var normal_smp: sampler;

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Polynomial smooth-min: unions the main pool with satellite droplets/streaks into one blobby mass.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

// SDF of the segment from origin to `e`, minus radius `r` (a thrown streak / tendril).
fn sd_streak(q: vec2<f32>, e: vec2<f32>, r: f32) -> f32 {
    let h = clamp(dot(q, e) / max(dot(e, e), 1e-5), 0.0, 1.0);
    return length(q - e * h) - r;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let grow = clamp(
        (globals.time - material.spawn_time) / max(material.grow_time, 0.0001),
        0.0,
        1.0,
    );
    // Ease-out growth: splooshes out fast, then settles.
    let scale = sqrt(grow);
    let p = mesh.uv * 2.0 - 1.0;              // centered quad, world-aligned, [-1, 1]
    let s = material.seed;

    // --- Per-pool randomization: rotate + anisotropically stretch the shape space `q`, so pools
    //     vary in orientation and elongation. (Clipping below uses the un-rotated `p`.)
    let rot = hash21(vec2<f32>(s, 1.0)) * 6.2831853;
    let ca = cos(rot);
    let sa = sin(rot);
    var q = vec2<f32>(p.x * ca - p.y * sa, p.x * sa + p.y * ca);
    q = q / vec2<f32>(0.65 + 0.7 * hash21(vec2<f32>(s, 2.0)), 0.65 + 0.7 * hash21(vec2<f32>(s, 3.0)));

    let ang = atan2(q.y, q.x);
    // Rich wobble: several harmonics with hashed amplitude, count and phase → jagged, unique rims.
    let ph = hash21(vec2<f32>(s, 7.0)) * 6.2831853;
    let n1 = 2.0 + floor(hash21(vec2<f32>(s, 8.0)) * 3.0);
    let n2 = 4.0 + floor(hash21(vec2<f32>(s, 9.0)) * 4.0);
    let n3 = 7.0 + floor(hash21(vec2<f32>(s, 10.0)) * 5.0);
    let wobble = (0.10 + 0.14 * hash21(vec2<f32>(s, 4.0))) * sin(ang * n1 + ph)
               + (0.05 + 0.11 * hash21(vec2<f32>(s, 5.0))) * sin(ang * n2 - s * 9.0)
               + (0.03 + 0.08 * hash21(vec2<f32>(s, 6.0))) * sin(ang * n3 + s * 3.0);
    let base = 0.40 + 0.28 * hash21(vec2<f32>(s, 11.0));
    var dist = (length(q) - (base + wobble) * scale);

    // Satellite droplets — random count (3..8), position, size, and blend radius.
    let count = i32(3.0 + floor(hash21(vec2<f32>(s, 12.0)) * 6.0));
    for (var i: i32 = 0; i < 8; i = i + 1) {
        if (i >= count) { break; }
        let fi = f32(i);
        let a2 = hash21(vec2<f32>(fi, s + 20.0)) * 6.2831853;
        let rr = (0.35 + 0.75 * hash21(vec2<f32>(fi + 1.0, s + 21.0))) * scale;
        let cpos = vec2<f32>(cos(a2), sin(a2)) * rr;
        let crad = (0.04 + 0.17 * hash21(vec2<f32>(fi + 2.0, s + 22.0))) * scale;
        let k = (0.07 + 0.10 * hash21(vec2<f32>(fi + 4.0, s))) * scale + 0.001;
        dist = smin(dist, length(q - cpos) - crad, k);
    }

    // A couple of thrown streaks so it reads as splatter, not just blobs.
    for (var j: i32 = 0; j < 2; j = j + 1) {
        let fj = f32(j);
        let sa2 = hash21(vec2<f32>(fj, s + 30.0)) * 6.2831853;
        let len = (0.45 + 0.75 * hash21(vec2<f32>(fj + 1.0, s + 31.0))) * scale;
        let e = vec2<f32>(cos(sa2), sin(sa2)) * len;
        let r = (0.015 + 0.03 * hash21(vec2<f32>(fj + 2.0, s + 32.0))) * scale;
        dist = smin(dist, sd_streak(q, e, r), 0.05 * scale + 0.001);
    }

    // Antialiased coverage from the SDF edge.
    let edge = fwidth(dist) + 0.004;
    var coverage = smoothstep(edge, -edge, dist);

    // --- Wall clip (world-aligned `p`): fade coverage to 0 just past a wall face so the pool stops
    //     at the wall instead of seeping through. world +X = +p.x, world +Z = -p.y (quad is rotated
    //     -90° about X). `clip` is in p-units; wall splats pass large values ⇒ no effect.
    let cm = 0.02;
    coverage = coverage * smoothstep(material.clip.x, material.clip.x - cm, p.x);    // +X
    coverage = coverage * smoothstep(-material.clip.y, -material.clip.y + cm, p.x);  // -X
    coverage = coverage * smoothstep(-material.clip.z, -material.clip.z + cm, p.y);  // +Z
    coverage = coverage * smoothstep(material.clip.w, material.clip.w - cm, p.y);    // -Z
    // Diagonal clip: world offset in p-units is (wx, wz) = (p.x, -p.y); project onto each unit
    // diagonal and mask past the wall found along it. Stops corners leaking past a wall.
    let wx = p.x;
    let wz = -p.y;
    let r = 0.70710678;
    let dpp = (wx + wz) * r;   // +X+Z
    let dnp = (-wx + wz) * r;  // -X+Z
    let dpn = (wx - wz) * r;   // +X-Z
    let dnn = (-wx - wz) * r;  // -X-Z
    coverage = coverage * smoothstep(material.clip_diag.x, material.clip_diag.x - cm, dpp);
    coverage = coverage * smoothstep(material.clip_diag.y, material.clip_diag.y - cm, dnp);
    coverage = coverage * smoothstep(material.clip_diag.z, material.clip_diag.z - cm, dpn);
    coverage = coverage * smoothstep(material.clip_diag.w, material.clip_diag.w - cm, dnn);

    if (coverage <= 0.001) {
        discard;
    }

    // How deep inside the pool this pixel is (1 = interior, 0 = rim).
    let inside = smoothstep(0.0, -0.22, dist);

    // Wet → dry over lifetime: fresh blood is a deep saturated red with a bright glossy glint; as it
    // dries it darkens toward matte maroon and the glint dies (codex: wet = low roughness/high spec).
    let dryness = clamp((globals.time - material.spawn_time) / max(material.dry_time, 0.001), 0.0, 1.0);
    let wet = material.color.rgb * (0.55 + 0.45 * inside);
    let dried = material.color.rgb * (0.24 + 0.14 * inside);
    var color = mix(wet, dried, dryness);

    // --- Real blood PBR detail: sample a splatter blob from the atlas, rotated per pool to vary it.
    //     `textureSampleLevel` (LOD 0) avoids derivative/uniformity issues after the `discard`.
    let trot = hash21(vec2<f32>(s, 40.0)) * 6.2831853;
    let tcs = cos(trot);
    let tsn = sin(trot);
    let uvc = mesh.uv - 0.5;
    let uvr = vec2<f32>(uvc.x * tcs - uvc.y * tsn, uvc.x * tsn + uvc.y * tcs);
    let atlas = vec2<f32>(0.26, 0.27) + uvr * 0.30;   // the top-left blood blob of the atlas
    let btex = textureSampleLevel(base_tex, base_smp, atlas, 0.0).rgb;
    let nrm = textureSampleLevel(normal_tex, normal_smp, atlas, 0.0).xyz * 2.0 - 1.0;
    let tex_luma = max(btex.r, max(btex.g, btex.b));
    let tex_present = smoothstep(0.03, 0.18, tex_luma);
    // Multiply real blood speckle/mottle into the fill (darker clumps), only where the atlas has blood.
    color = mix(color, color * (0.5 + 1.0 * btex.r), tex_present * inside * 0.7);

    // Tight wet specular glint (surface tension), perturbed by the blood normal for micro wet variation;
    // a small hot spot, not a big wash, and it dies as the blood dries.
    let hlc = vec2<f32>(-0.12, 0.16) + nrm.xy * 0.12;
    let hl = pow(smoothstep(0.34, 0.0, length((q - hlc) * vec2<f32>(1.0, 1.6))), 3.0);
    color = color + vec3<f32>(hl * inside * material.gloss * (1.0 - dryness));
    return vec4<f32>(color, coverage);
}
