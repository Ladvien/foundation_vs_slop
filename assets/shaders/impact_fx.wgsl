// Laser-impact particle burst. Adapted from a Shadertoy fireworks fragment shader, reduced to a
// single centered burst on a camera-facing quad and driven entirely by the `ImpactSettings`
// uniform so its look is adjustable at runtime (see `src/impact_fx.rs`).
//
// The original samples a noise TEXTURE (iChannel0) for per-particle randomness. We replace it with
// an in-shader hash — procedural noise is randomly-accessible, parametrized, and GPU-evaluated in
// constant time, which is exactly why it suits this (Lagae, Lefebvre, Cook, DeRose, Drettakis,
// Ebert, Lewis, Perlin, Zwicker, "A Survey of Procedural Noise Functions", Computer Graphics Forum
// 2010, DOI 10.1111/j.1467-8659.2010.01827.x — "rocket trails", sparks, etc.).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals
#import foundation::noise::{hash21, rand_dir}

// Compile-time upper bound; `particle_count` cuts the loop short at runtime (WGSL allows a uniform
// loop bound, but a fixed max keeps it unrollable/portable).
const MAX_PARTICLES: i32 = 64;

struct ImpactSettings {
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    intensity: f32,
    spread: f32,
    speed: f32,
    particle_size: f32,
    gravity: f32,
    spawn_time: f32,
    duration: f32,
    seed: f32,
    particle_count: i32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: ImpactSettings;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let age = clamp(
        (globals.time - material.spawn_time) / max(material.duration, 0.0001),
        0.0,
        1.0,
    );
    // Centered quad coordinates in [-1, 1].
    let p = mesh.uv * 2.0 - 1.0;

    var color = vec3<f32>(0.0);
    for (var i: i32 = 0; i < MAX_PARTICLES; i = i + 1) {
        if (i >= material.particle_count) {
            break;
        }
        let fi = f32(i);
        let dir = rand_dir(fi, material.seed, 0.3, 0.7) * material.spread;
        // Fly outward from the center, with a little gravity droop as the burst ages.
        var pos = dir * material.speed * age;
        pos.y = pos.y - material.gravity * age * age;

        // Inverse-distance glow around each particle; `particle_size` tightens/loosens it.
        let term = material.intensity / (length(p - pos) * material.particle_size + 0.001);
        let cmix = hash21(vec2<f32>(fi + 5.0, material.seed + 7.0));
        let col = mix(material.color_a.rgb, material.color_b.rgb, cmix);
        color = color + pow(abs(col * term), vec3<f32>(1.25));
    }

    // Fade the whole burst out over its life (quadratic ease).
    let fade = 1.0 - age;
    // Radial vignette → the inverse-distance glow reaches exactly 0 by the quad edge, so the
    // square quad boundary is never visible (only the round burst shows through the additive pass).
    let vignette = smoothstep(1.0, 0.55, length(p));
    color = color * fade * fade * vignette;

    // Alpha = brightness so dark areas are transparent (the square quad vanishes, only the glow
    // shows). Premultiply so bright cores read as glow over the scene.
    let lum = clamp(max(color.r, max(color.g, color.b)), 0.0, 1.0);
    return vec4<f32>(color, lum);
}
