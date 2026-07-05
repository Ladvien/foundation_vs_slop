// Dimensional nest portal — a pulsating swirling fractal-noise dome the interdimensional crabs emerge
// from and haul meat into. Ported to WGSL from a Shadertoy by @zozuar
// (https://twitter.com/zozuar/status/1621229990267310081); the algorithm (20-iteration rotating
// domain-warp accumulation) is unchanged. Adaptations, mirroring `smiley.wgsl`:
//   * iTime        -> globals.time  (Bevy per-frame global; no CPU time uniform needed)
//   * fragCoord/uv -> mesh.uv centered to [-1,1]
//   * a `hoard` uniform brightens the portal as the crabs fill it (visual feedback)
// NOTE: WGSL has no implicit scalar→vector broadcast, so the GLSL `p*S + t*4. + …` scalar adds are
// folded into one scalar `s` and broadcast explicitly.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

struct NestSettings {
    // Meat hauled in so far (world units of it) — drives the portal glow.
    hoard: f32,
    radius: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: NestSettings;

fn rotate2D(r: f32) -> mat2x2<f32> {
    return mat2x2<f32>(cos(r), sin(r), -sin(r), cos(r));
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // Sphere UVs are [0,1]; center to [-1,1] so the swirl sits on the dome.
    let uv = (mesh.uv - vec2<f32>(0.5)) * 2.0;
    let t = globals.time;

    var p = uv;
    var n = vec2<f32>(0.0);
    var q = vec2<f32>(0.0);
    let d = dot(p, p);
    var S = 12.0;
    var a = 0.0;
    let m = rotate2D(5.0);

    for (var j = 0.0; j < 20.0; j = j + 1.0) {
        p = m * p;
        n = m * n;
        // scalar terms folded (WGSL: no scalar+vec broadcast)
        let s = t * 4.0 + sin(t * 4.0 - d * 6.0) * 0.8 + j;
        q = p * S + vec2<f32>(s) + n;
        a = a + dot(cos(q) / S, vec2<f32>(0.2));
        n = n - sin(q);
        S = S * 1.2;
    }

    var col = vec3<f32>(4.0, 2.0, 1.0) * (a + 0.2) + a + a - d;
    // Brighter as the hoard fills (0 → full over ~20 units of delivered meat).
    let glow = mix(0.6, 1.6, clamp(material.hoard / 20.0, 0.0, 1.0));
    col = col * glow;
    return vec4<f32>(max(col, vec3<f32>(0.0)), 1.0);
}
