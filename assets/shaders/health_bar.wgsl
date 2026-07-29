// Floating health bar drawn on a camera-facing quad (see `src/health.rs`). The single `fraction`
// uniform (0..1) drives both the fill width and its brightness, so one small material serves every
// unit and enemy. Legible combat feedback is a standard difficulty-readability affordance (Gee's
// "just doable" challenge, per McKay et al., "Implementing Adaptive Game Difficulty Balancing in
// Serious Games", IEEE Trans. Games 2018, DOI 10.1109/tg.2018.2791019).
//
// **Health is encoded as LENGTH first, LUMINANCE second — never hue** (`docs/ui.md` §1.3).
//
// This used to be `mix(red, green, frac)`. Two problems with that, and the fill length already
// solved the thing it was for:
//
//  1. Red-vs-green is the canonical red-green colour-vision confusion (~8% of men), so for those
//     players the hue channel carried *nothing* and a nearly-dead unit looked like a healthy one.
//  2. It was the last hue-ramped readout left in the game. The 2026-07-28 UI pass moved threat onto
//     the ACS luminosity scale (`ui::theme::Hazard`) precisely because hue fails in peripheral
//     vision — and the health bar is the readout most often read peripherally, while the player is
//     looking at the world.
//
// The bar's *length* is the primary channel and is untouched: position/length is the most
// accurately-read encoding available (Cleveland & McGill's ordering). Luminance is the redundant
// second channel, and it runs the useful way round — a full bar sits DIM and recedes, a critical one
// burns bright and pulls the eye. One hue throughout, so nothing depends on telling two apart.

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
        // One hue, luminance ramped by how hurt the owner is. `1 - frac` so the brightest bar is the
        // one that needs attention; a healthy squad's bars sit back and stop dominating the frame.
        let hurt = 1.0 - frac;
        let gain = mix(0.34, 1.0, hurt * hurt); // eased, so only real damage lights up
        let fill = vec3<f32>(0.30, 0.85, 0.38) * gain;
        return vec4<f32>(fill, 1.0);
    }
    return vec4<f32>(0.12, 0.12, 0.12, 0.9); // empty track
}
