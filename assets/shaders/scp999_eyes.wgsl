// SCP-999's two big darling eyes — procedural analytic distance-field masks on a camera-facing billboard
// quad, drawn ON the front of the translucent gel dome. The technique (layered `length()`-based masks,
// coverage-as-alpha so the square quad's corners vanish under AlphaMode::Blend, and an iris/pupil that
// track a glance vector fed from Rust) is the same one the smiley enemy uses (`assets/shaders/smiley.wgsl`,
// a WGSL port of BigWings' Shadertoy "Smiley Tutorial", CC BY-NC-SA 3.0) — inverted emotionally: where the
// smiley frowns and panics, SCP-999 has big warm pupils, a bright glossy catch-light, and a gentle blink,
// so it reads as captivated/adoring (the "darling puppy" the design calls for).
//
// Uniforms (must byte-match `Scp999EyesUniform` in src/scp999/eyes.rs, field order + types):
//   look         — glance vector in face space (~[-0.4, 0.4]); the iris/pupil follow the comforted member.
//   bob_l/bob_r  — per-eye bounce offset (eye-space); each eye tracks the body jiggle on its own detuned
//                  spring, so the two bob independently — googly slime eyes.
//   blink        — 0 = open, 1 = fully shut (a top lid sweeps down); driven on a slow per-blob timer.
//   joy          — 0 = calm gaze, 1 = mid-tickle delight (dilates the pupil + brightens the catch-light).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

struct Scp999EyesUniform {
    look: vec2<f32>,
    bob_l: vec2<f32>,
    bob_r: vec2<f32>,
    blink: f32,
    joy: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: Scp999EyesUniform;

fn sat(x: f32) -> f32 {
    return clamp(x, 0.0, 1.0);
}

// Map `uv` into the [0,1] box described by `rect` = (min.xy, max.xy). (Same helper as smiley.wgsl.)
fn within(uv: vec2<f32>, rect: vec4<f32>) -> vec2<f32> {
    return (uv - rect.xy) / (rect.zw - rect.xy);
}

// One eye, centred in its [0,1] sub-rect. `side` (+1 / -1) mirrors the single definition into left+right;
// `m` is the glance target; `joy` dilates the pupil + fattens the catch-light; `blink` sweeps a top lid.
fn eye(uv_in: vec2<f32>, side: f32, m_in: vec2<f32>, joy: f32, blink: f32) -> vec4<f32> {
    var uv = uv_in - vec2<f32>(0.5);
    uv.x *= side;
    // The glance must be mirrored INTO the mirrored space, or the two irises swing apart. `side` mirrors
    // eye-local X so one definition draws both eyes (highlight and lid asymmetries come out handed, which
    // is what we want), but `m` arrives in unmirrored screen space: applied raw, the left eye's iris lands
    // at −m.x while the right eye's lands at +m.x — cross-eyed whenever SCP-999 glances sideways at the
    // member it is comforting. smiley.wgsl's Eye() looks correct only because Smiley() pre-folds with
    // `uv.x = abs(uv.x)`, so there `side` UN-mirrors; these explicit per-eye rects have no such fold.
    let m = vec2<f32>(m_in.x * side, m_in.y);

    var d = length(uv);
    // Warm sclera (a hair of cream, not clinical white) so it sits kindly on the orange gel.
    let sclera = vec3<f32>(1.0, 0.97, 0.92);
    // Friendly amber iris — a warm brown-gold that harmonises with SCP-999's orange body.
    var irisCol = vec3<f32>(0.55, 0.33, 0.13);
    var rgb = mix(sclera, irisCol, smoothstep(0.1, 0.7, d) * 0.35); // faint warm gradient in the white
    let mask = smoothstep(0.5, 0.48, d);                            // round eye boundary → alpha

    // Soft upper-lid shadow, so the eye reads as a rounded ball, not a flat disc.
    rgb *= 1.0 - smoothstep(0.42, 0.5, d) * 0.4 * sat(uv.y + 0.2);

    // Big iris, looking toward `m`.
    d = length(uv - m * 0.35);
    rgb = mix(rgb, vec3<f32>(0.0), smoothstep(0.36, 0.34, d));       // thin dark iris rim
    irisCol *= 1.0 + smoothstep(0.34, 0.05, d);                      // lighter toward the centre
    let irisMask = smoothstep(0.34, 0.31, d);
    rgb = mix(rgb, irisCol, irisMask);

    // Big round pupil, looking toward `m`; dilates with joy (a delighted, tickled stare).
    d = length(uv - m * 0.42);
    let pupilSize = mix(0.2, 0.26, joy);
    let pupilMask = smoothstep(pupilSize, pupilSize * 0.85, d) * irisMask;
    rgb = mix(rgb, vec3<f32>(0.02, 0.01, 0.0), pupilMask);

    // Glossy catch-lights — THE darling glint. A big primary spark (upper-left) + a small secondary, both
    // fattened by joy. A slow shimmer keeps a live, wet sparkle without the smiley's nervous quiver.
    let sh = sin(globals.time * 1.5) * 0.006;
    var hi = smoothstep(0.13, 0.10, length(uv - vec2<f32>(-0.13, 0.15) + vec2<f32>(sh, 0.0))) * (0.9 + 0.3 * joy);
    hi += smoothstep(0.07, 0.05, length(uv + vec2<f32>(0.10, -0.06)));
    rgb = mix(rgb, vec3<f32>(1.0), sat(hi));

    // Blink: a top lid sweeps down. At blink=1 the lid crosses the whole eye and alpha → 0 (fully shut).
    let lid_y = mix(0.62, -0.62, blink);
    let open = smoothstep(lid_y, lid_y - 0.05, uv.y);

    return vec4<f32>(rgb, mask * open);
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // Quad UVs are [0,1] top-left origin (y down); centre and flip y so the eyes sit upright.
    var uv = mesh.uv - vec2<f32>(0.5);
    uv.y = -uv.y;

    let m = material.look;
    var col = vec4<f32>(0.0);

    // Two eyes at explicit centres, each shifted by its own bounce offset so they bob independently with the
    // body jiggle. Gated to a bounding circle so only eye pixels shade; everything else stays transparent
    // and the gel shows through. `side` (-1 / +1) mirrors the glance direction so both irises look the same way.
    let cl = vec2<f32>(-0.24, 0.02) + material.bob_l;
    if (length(uv - cl) < 0.26) {
        let e = eye(within(uv, vec4<f32>(cl.x - 0.2, cl.y - 0.2, cl.x + 0.2, cl.y + 0.2)), -1.0, m, material.joy, material.blink);
        col = mix(col, e, e.a);
    }
    let cr = vec2<f32>(0.24, 0.02) + material.bob_r;
    if (length(uv - cr) < 0.26) {
        let e = eye(within(uv, vec4<f32>(cr.x - 0.2, cr.y - 0.2, cr.x + 0.2, cr.y + 0.2)), 1.0, m, material.joy, material.blink);
        col = mix(col, e, e.a);
    }

    return col;
}
