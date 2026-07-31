// SCP-610's eye — the deliberate perceptual mismatch, and the reason it needs a shader at all.
//
// # Why this exists (the bug)
//
// The eye is a separate material slot, 82 vertices, a flat solid colour. It shares the body's mesh,
// so it shares the mesh's `COLOR_0` — and after FVS-K-1 that channel carries the scar mask, which is
// **0.0 at every one of those 82 vertices** (measured on the shipped file). Bevy's
// `pbr_fragment.wgsl` assigns `base_color = in.color` under `VERTEX_COLORS`, so a stock
// `StandardMaterial` here renders the eye pure black.
//
// It was safe before only by accident: the old asset shipped a forced all-1.0 `COLOR_0`, so the
// assign was a no-op. `assets/scp610/README.md` §6 called that "checked, confirmed benign" and warned
// not to assume it after a re-export. It was right.
//
// # Why it is not just a black-fix
//
// Kätsyri, Mäkäräinen, Förger & Takala (2015), 10.3389/fpsyg.2015.00390, found the strongest support
// of any uncanny-valley hypothesis they reviewed — **4 out of 4 studies** — for H4a, *perceptual
// mismatch between the realism levels of individual features*. Their canonical example of it is,
// almost word for word, this asset: *"Clearly artificial eyes on an otherwise fully human-like
// face."* Two of the four studies (Seyama & Nagayama 2007; MacDorman et al. 2009) measured the most
// negative affinity precisely where the realism gap between eyes and face was widest.
//
// SCP-610 is a photographed-gore body — real PBR flesh, real normal maps — with a flat untextured eye
// in it. That is the single highest-leverage uncanny lever in the whole creature, it was already in
// the asset by accident of how the slot was authored, and this shader's job is to **keep** it and
// commit to it rather than blend it away. So: no texture, no normal map, no roughness variation, a
// pure unbroken colour. The mismatch IS the effect.
//
// The wet specular pinpoint is the one concession, and it is the same argument in miniature: an eye
// that does not catch light reads as a prop rather than as an eye, and a prop is not mismatched with
// anything. It has to look like an eye to look wrong.

#import bevy_pbr::pbr_fragment::pbr_input_from_standard_material
#import bevy_pbr::pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing}
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}

// MUST byte-match `Scp610EyeParams` in `src/scp610/material.rs`.
struct Scp610EyeParams {
    // Linear RGB. Authored in `src/scp610/material.rs`, not here, so it sits with the rest of the
    // creature's constants instead of being a number buried in a shader.
    color: vec3<f32>,
    // Shares the body's ACS Disruption term so the eye brightens with the rest of the creature. The
    // eye is the part a player looks at, so it is where a rising breach is noticed first.
    disruption: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> eye: Scp610EyeParams;

// Wet, but not a mirror. Low enough that the sclera still reads as a surface under the dungeon's
// dim ambient rather than blowing out under bloom.
const EYE_ROUGHNESS: f32 = 0.18;
const DISRUPTION_GAIN: f32 = 0.9;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Overwrite outright. `in.color` is the body's scar mask and means nothing here — reading it at
    // all would be reading another slot's data.
    pbr_input.material.base_color = vec4<f32>(eye.color, 1.0);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    pbr_input.material.perceptual_roughness = EYE_ROUGHNESS;

    // Flat by construction: the geometric normal, with no map and no perturbation. This is the
    // "clearly artificial" half of the mismatch and it must not be softened.
    pbr_input.N = normalize(pbr_input.world_normal);

    pbr_input.material.emissive =
        vec4<f32>(eye.color * eye.disruption * DISRUPTION_GAIN, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
