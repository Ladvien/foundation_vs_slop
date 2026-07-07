// Smiley enemy face, ported to WGSL from the Shadertoy "Smiley Tutorial" by Martijn Steinrucken
// (BigWings), 2017 — https://www.youtube.com/watch?v=ZlNnrpM0TRg
// Original licensed Creative Commons Attribution-NonCommercial-ShareAlike 3.0 Unported. This port
// keeps that attribution; the algorithm (layered analytic SDF-style masks — head, eyes, brows,
// mouth) is unchanged. Adaptations for use as an in-world enemy on a camera-facing quad:
//   * `iTime`  -> `globals.time`   (Bevy's per-frame global; see impact_fx.wgsl for the same trick)
//   * `iMouse` -> `material.look`  (the face glances toward the nearest squad unit, fed from Rust)
//   * `smile`/`menace` are uniform inputs so the enemy can frown and tint hostile at runtime
//   * output alpha = head coverage, so the square quad vanishes under AlphaMode::Blend.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

struct SmileySettings {
    // Look/glance vector in face space (~[-0.4, 0.4]); drives eye + head parallax.
    look: vec2<f32>,
    // 0 = full frown (hostile), 1 = full grin. Enemies sit near 0.
    smile: f32,
    // 0 = faithful yellow smiley, 1 = fully bled to hostile red.
    menace: f32,
    // 0 = normal, 1 = full panic: pin-prick pupils + a cold, drained pallor as the swarm overwhelms it.
    panic: f32,
    // 0 = neutral, 1 = full "saddish" idle: the lonely watcher waiting — desaturated, dimmed, and cooled
    // toward a grey-blue pallor. The lonely-idle read the README asks for (emotional loneliness / the need
    // to belong: Weiss 1973; Baumeister & Leary 1995). Distinct from `panic` (fear) — this is desolation,
    // not terror — so it only cools + dims, it does not shrink the pupils.
    sad: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: SmileySettings;

fn sat(x: f32) -> f32 {
    return clamp(x, 0.0, 1.0);
}

fn remap01(a: f32, b: f32, t: f32) -> f32 {
    return sat((t - a) / (b - a));
}

fn remap(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    return sat((t - a) / (b - a)) * (d - c) + c;
}

// Map `uv` into the [0,1] box described by `rect` = (min.xy, max.xy).
fn within(uv: vec2<f32>, rect: vec4<f32>) -> vec2<f32> {
    return (uv - rect.xy) / (rect.zw - rect.xy);
}

fn Brow(uv_in: vec2<f32>, smile: f32) -> vec4<f32> {
    var uv = uv_in;
    let offs = mix(0.2, 0.0, smile);
    uv.y += offs;

    let y = uv.y;
    uv.y += uv.x * mix(0.5, 0.8, smile) - mix(0.1, 0.3, smile);
    uv.x -= mix(0.0, 0.1, smile);
    uv -= vec2<f32>(0.5);

    var col = vec4<f32>(0.0);

    var blur = 0.1;

    var d1 = length(uv);
    var s1 = smoothstep(0.45, 0.45 - blur, d1);
    var d2 = length(uv - vec2<f32>(0.1, -0.2) * 0.7);
    var s2 = smoothstep(0.5, 0.5 - blur, d2);

    let browMask = sat(s1 - s2);

    var colMask = remap01(0.7, 0.8, y) * 0.75;
    colMask *= smoothstep(0.6, 0.9, browMask);
    colMask *= smile;
    let browCol = mix(vec4<f32>(0.4, 0.2, 0.2, 1.0), vec4<f32>(1.0, 0.75, 0.5, 1.0), colMask);

    uv.y += 0.15 - offs * 0.5;
    blur += mix(0.0, 0.1, smile);
    d1 = length(uv);
    s1 = smoothstep(0.45, 0.45 - blur, d1);
    d2 = length(uv - vec2<f32>(0.1, -0.2) * 0.7);
    s2 = smoothstep(0.5, 0.5 - blur, d2);
    let shadowMask = sat(s1 - s2);

    col = mix(col, vec4<f32>(0.0, 0.0, 0.0, 1.0), smoothstep(0.0, 1.0, shadowMask) * 0.5);
    col = mix(col, browCol, smoothstep(0.2, 0.4, browMask));

    return col;
}

fn Eye(uv_in: vec2<f32>, side: f32, m: vec2<f32>, smile: f32) -> vec4<f32> {
    var uv = uv_in - vec2<f32>(0.5);
    uv.x *= side;

    var d = length(uv);
    var irisCol = vec3<f32>(0.3, 0.5, 1.0);
    var rgb = mix(vec3<f32>(1.0), irisCol, smoothstep(0.1, 0.7, d) * 0.5);   // gradient in eye-white
    let a = smoothstep(0.5, 0.48, d);                                        // eye mask

    rgb *= 1.0 - smoothstep(0.45, 0.5, d) * 0.5 * sat(-uv.y - uv.x * side);  // eye shadow

    d = length(uv - m * 0.4);                                               // iris looks toward `m`
    rgb = mix(rgb, vec3<f32>(0.0), smoothstep(0.3, 0.28, d));                // iris outline

    irisCol *= 1.0 + smoothstep(0.3, 0.05, d);                              // iris lighter in center
    let irisMask = smoothstep(0.28, 0.25, d);
    rgb = mix(rgb, irisCol, irisMask);                                      // blend in iris

    d = length(uv - m * 0.45);                                             // pupil looks toward `m`
    // Panic shrinks the pupils to terrified pin-pricks.
    let pupilSize = mix(mix(0.4, 0.16, smile), 0.05, material.panic);
    var pupilMask = smoothstep(pupilSize, pupilSize * 0.85, d);
    pupilMask *= irisMask;
    rgb = mix(rgb, vec3<f32>(0.0), pupilMask);                             // blend in pupil

    let t = globals.time * 3.0;
    var offs = vec2<f32>(sin(t + uv.y * 25.0), sin(t + uv.x * 25.0));
    offs *= 0.01 * (1.0 - smile);

    uv += offs;
    var highlight = smoothstep(0.1, 0.09, length(uv - vec2<f32>(-0.15, 0.15)));
    highlight += smoothstep(0.07, 0.05, length(uv + vec2<f32>(-0.08, 0.08)));
    rgb = mix(rgb, vec3<f32>(1.0), highlight);                             // blend in highlight

    return vec4<f32>(rgb, a);
}

fn Mouth(uv_in: vec2<f32>, smile: f32) -> vec4<f32> {
    var uv = uv_in - vec2<f32>(0.5);
    var rgb = vec3<f32>(0.5, 0.18, 0.05);

    uv.y *= 1.5;
    uv.y -= uv.x * uv.x * 2.0 * smile;

    uv.x *= mix(2.5, 1.0, smile);

    let d = length(uv);
    let a = smoothstep(0.5, 0.48, d);

    var tUv = uv;
    tUv.y += (abs(uv.x) * 0.5 + 0.1) * (1.0 - smile);
    var td = length(tUv - vec2<f32>(0.0, 0.6));

    let toothCol = vec3<f32>(1.0) * smoothstep(0.6, 0.35, d);
    rgb = mix(rgb, toothCol, smoothstep(0.4, 0.37, td));

    td = length(uv + vec2<f32>(0.0, 0.5));
    rgb = mix(rgb, vec3<f32>(1.0, 0.5, 0.5), smoothstep(0.5, 0.2, td));
    return vec4<f32>(rgb, a);
}

fn Head(uv: vec2<f32>) -> vec4<f32> {
    var rgb = vec3<f32>(0.9, 0.65, 0.1);

    var d = length(uv);

    let a = smoothstep(0.5, 0.49, d);

    var edgeShade = remap01(0.35, 0.5, d);
    edgeShade *= edgeShade;
    rgb *= 1.0 - edgeShade * 0.5;

    rgb = mix(rgb, vec3<f32>(0.6, 0.3, 0.1), smoothstep(0.47, 0.48, d));

    var highlight = smoothstep(0.41, 0.405, d);
    highlight *= remap(0.41, -0.1, 0.75, 0.0, uv.y);
    highlight *= smoothstep(0.18, 0.19, length(uv - vec2<f32>(0.21, 0.08)));
    rgb = mix(rgb, vec3<f32>(1.0), highlight);

    d = length(uv - vec2<f32>(0.25, -0.2));
    var cheek = smoothstep(0.2, 0.01, d) * 0.4;
    cheek *= smoothstep(0.17, 0.16, d);
    rgb = mix(rgb, vec3<f32>(1.0, 0.1, 0.1), cheek);

    return vec4<f32>(rgb, a);
}

fn Smiley(uv_in: vec2<f32>, m: vec2<f32>, smile: f32) -> vec4<f32> {
    var col = vec4<f32>(0.0);
    var uv = uv_in;

    if (length(uv) < 0.5) {                    // only shade pixels inside the head
        let side = sign(uv.x);
        uv.x = abs(uv.x);
        let head = Head(uv);
        col = mix(col, head, head.a);

        if (length(uv - vec2<f32>(0.2, 0.075)) < 0.175) {
            let eye = Eye(within(uv, vec4<f32>(0.03, -0.1, 0.37, 0.25)), side, m, smile);
            col = mix(col, eye, eye.a);
        }

        if (length(uv - vec2<f32>(0.0, -0.15)) < 0.3) {
            let mouth = Mouth(within(uv, vec4<f32>(-0.3, -0.43, 0.3, -0.13)), smile);
            col = mix(col, mouth, mouth.a);
        }

        if (length(uv - vec2<f32>(0.185, 0.325)) < 0.18) {
            let brow = Brow(within(uv, vec4<f32>(0.03, 0.2, 0.4, 0.45)), smile);
            col = mix(col, brow, brow.a);
        }
    }

    return col;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // Quad UVs are [0,1] with origin top-left (y down); center and flip y so the face is upright.
    var uv = mesh.uv - vec2<f32>(0.5);
    uv.y = -uv.y;

    let m = material.look;

    // Whole-face parallax lean toward the look target (original: `uv -= m*sat(.23-d)`).
    let d = dot(uv, uv);
    uv -= m * sat(0.23 - d);

    var col = Smiley(uv, m, material.smile);

    // Menace: bleed the lit face toward hostile red (multiplied by coverage so the outside stays clear).
    col = vec4<f32>(mix(col.rgb, vec3<f32>(0.85, 0.05, 0.05), material.menace * col.a), col.a);
    // Panic: drain the face toward a cold, sickly pallor (over coverage so the outside stays clear).
    col = vec4<f32>(mix(col.rgb, vec3<f32>(0.75, 0.82, 0.92), material.panic * 0.7 * col.a), col.a);
    // Sad idle: desaturate toward luminance, dim, and cool toward grey-blue — the lonely watcher waiting.
    // Applied last so it colours whatever expression is showing; gated by coverage so the quad stays clear.
    let lum = dot(col.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let melancholy = mix(vec3<f32>(lum), vec3<f32>(0.34, 0.40, 0.52), 0.5) * 0.8;
    col = vec4<f32>(mix(col.rgb, melancholy, material.sad * col.a), col.a);
    return col;
}
