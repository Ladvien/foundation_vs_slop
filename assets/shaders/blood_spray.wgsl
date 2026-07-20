// Blood spray burst — a camera-facing quad of liquid droplets flung out from a hit/death point and
// arcing down under gravity. Forked from `impact_fx.wgsl` (the orange spark burst) but recolored to
// blood and made coverage-based (opaque droplets) instead of additive glow, so it reads as wet
// liquid rather than fire. Driven entirely by the `BloodSettings` uniform (see `src/gore.rs`) so the
// look is tunable at runtime from `gore.ron`.
//
// Per-droplet randomness uses a texture-free in-shader hash (Lagae, Lefebvre, Cook, DeRose,
// Drettakis, Ebert, Lewis, Perlin, Zwicker, "A Survey of Procedural Noise Functions", Computer
// Graphics Forum 2010, DOI 10.1111/j.1467-8659.2010.01827.x).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals
#import foundation::noise::{hash21, rand_dir}

// Compile-time upper bound; `particle_count` cuts the loop short at runtime.
const MAX_PARTICLES: i32 = 64;

struct BloodSettings {
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

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: BloodSettings;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let age = clamp(
        (globals.time - material.spawn_time) / max(material.duration, 0.0001),
        0.0,
        1.0,
    );
    // Centered quad coordinates in [-1, 1].
    let p = mesh.uv * 2.0 - 1.0;

    // Accumulate coverage of the nearest droplet and carry that droplet's color, so overlapping
    // droplets composite as opaque blood (max coverage) rather than summing into a glow.
    var coverage = 0.0;
    var col = material.color_a.rgb;
    for (var i: i32 = 0; i < MAX_PARTICLES; i = i + 1) {
        if (i >= material.particle_count) {
            break;
        }
        let fi = f32(i);
        let dir = rand_dir(fi, material.seed, 0.25, 0.75) * material.spread;
        // Fly outward from the center; gravity droops the path quadratically in age → an arc.
        var pos = dir * material.speed * age;
        pos.y = pos.y - material.gravity * age * age;

        // Soft round droplet; `particle_size` scales its radius in quad space.
        let radius = 0.02 * material.particle_size * (0.6 + 0.8 * hash21(vec2<f32>(fi + 2.0, material.seed)));
        let cov = smoothstep(radius, radius * 0.35, length(p - pos));
        if (cov > coverage) {
            coverage = cov;
            let cmix = hash21(vec2<f32>(fi + 5.0, material.seed + 7.0));
            col = mix(material.color_a.rgb, material.color_b.rgb, cmix);
        }
    }

    // Fade the whole spray out over its life (quadratic ease). Blood is opaque, so the alpha (not the
    // brightness) carries the droplet shape — the square quad edge vanishes where coverage is 0.
    let fade = 1.0 - age * age;
    let alpha = clamp(coverage * fade * material.intensity, 0.0, 1.0);
    return vec4<f32>(col, alpha);
}
