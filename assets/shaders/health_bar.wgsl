// Floating health bar drawn on a camera-facing quad (see `src/health.rs`). The single `fraction`
// uniform (0..1) drives both the fill width and its green→red color, so one small material serves
// every unit and enemy. Legible combat feedback is a standard difficulty-readability affordance
// (Gee's "just doable" challenge, per McKay et al., "Implementing Adaptive Game Difficulty Balancing
// in Serious Games", IEEE Trans. Games 2018, DOI 10.1109/tg.2018.2791019).

#import bevy_pbr::forward_io::VertexOutput

struct HealthBarSettings {
    fraction: f32,
    // Pad to a 16-byte uniform slot (mirror the Rust `HealthBarUniform` field order exactly).
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: HealthBarSettings;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let uv = mesh.uv; // [0,1], origin top-left
    let frac = clamp(material.fraction, 0.0, 1.0);

    // Dark frame around the bar.
    let bx = 0.05;
    let by = 0.14;
    let inside = uv.x > bx && uv.x < 1.0 - bx && uv.y > by && uv.y < 1.0 - by;
    if (!inside) {
        return vec4<f32>(0.02, 0.02, 0.02, 0.9);
    }

    // Inner track, remapped to [0,1] across the fillable width.
    let fx = (uv.x - bx) / (1.0 - 2.0 * bx);
    if (fx <= frac) {
        let fill = mix(vec3<f32>(0.85, 0.12, 0.12), vec3<f32>(0.25, 0.85, 0.25), frac); // red→green
        return vec4<f32>(fill, 1.0);
    }
    return vec4<f32>(0.12, 0.12, 0.12, 0.9); // empty track
}
