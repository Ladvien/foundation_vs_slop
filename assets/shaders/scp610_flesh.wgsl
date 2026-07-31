// SCP-610 flesh — an ExtendedMaterial<StandardMaterial, Scp610FleshExt> fragment for the infected body.
//
// Two PBR gore sets cross-faded by a per-vertex mask, plus the ACS luminosity term. The mask is
// `COLOR_0.r`, baked by the asset builder as a BFS feather out from each mutant stub
// (`monsters/infected.py::_bake_scar_blend_weights`): 1.0 on the stub itself, 0.0 four rings out.
//
// # Why the vertex colour has to be overwritten
//
// `pbr_fragment.wgsl` does `pbr_input.material.base_color = in.color` under `VERTEX_COLORS` — an
// ASSIGN, not a multiply, in Bevy 0.19 — and then multiplies the uniform and the texture into it.
// Our mask lives in R with G = B = 0, so left alone the body would render pure red where the mask is
// high and black everywhere else. So the albedo is rebuilt here from `base_color_texture` sampled
// directly, exactly the way `mycelia_fruit.wgsl` rebuilds its own: **the mask is data, not artwork.**
//
// Reading `in.color` unguarded is deliberate. If the asset ever ships without `COLOR_0` this fails to
// compile, which is the correct outcome rather than a silently unblended creature — the failure the
// old asset had for months without anyone seeing it.
//
// # The slop reading (FVS-K-1, the SCP-9191 aesthetic)
//
// Kätsyri, Mäkäräinen, Förger & Takala (2015), *A review of empirical evidence on different uncanny
// valley hypotheses*, 10.3389/fpsyg.2015.00390, is the grounding and it is unusually specific. Of the
// hypotheses it reviews, the two that actually survived contact with data are:
//
//   * **H4a, inconsistent realism levels between features — supported by 4 of 4 studies.** Eeriness
//     peaks when the realism of one feature disagrees with the realism of the rest. See
//     `scp610_eye.wgsl`, which is where this project spends that.
//   * **H4b, sensitivity to atypical features — 3 of 4 studies**, and the review's summary of the
//     mechanism is the load-bearing part: *"individuals are more sensitive and less tolerant to
//     deviations from typical norms when judging human faces."* Atypicality is MORE unsettling on a
//     MORE humanlike base — which is why 610's `mutation` morph starts at a passing human and grows,
//     rather than spawning fully turned.
//
// `docs/lore/2026-07-12-scp-color-language.md` §6 names the visual grammar for SCP-9191's output:
// *"Mush. Colors that are nearly right. Palettes with no contrast. Gradients where there should be
// edges."* Both halves are literal here — the feathered mask IS a gradient where a boundary should
// be, and `SLOP_CONTRAST` below compresses the scar side's albedo toward its own mean so the infected
// tissue reads as flatter and less articulated than the body it is eating. (That document carries a
// deprecation banner for its *semiotic-decay* framing; only its colour grammar is used here, and
// `tests/lore_canon.rs` fails the build on the deprecated vocabulary.)
//
// # Luminosity, not hue
//
// The same document §1 takes the Foundation's ACS Disruption Class — Dark → Vlam → Keneq → Ekhi →
// Amida — and points out it is a scale of *illumination*, not colour: **containment is darkness, and
// a breach is a fire getting brighter.** `disruption` is that scale, driven from the containment
// phase. The emissive it feeds is deliberately **hueless** (`vec3(1.0) * disruption`): §7's rule is
// that colour means *deviation*, never danger, and light added to this scene must carry no hue of its
// own or it fights the dungeon's authored palette.

#import bevy_pbr::pbr_fragment::pbr_input_from_standard_material
#import bevy_pbr::pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing}
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}
#import bevy_pbr::pbr_bindings

// MUST byte-match `Scp610FleshParams` in `src/scp610/material.rs`.
struct Scp610FleshParams {
    // Tiling of the scar set relative to the body's own UVs. The two gore sets were photographed at
    // different native densities, so they cannot share one scale (the Blender-side material makes the
    // same split with INFECTED_FLESH_UV_SCALE / INFECTED_SCAR_UV_SCALE).
    scar_uv_scale: f32,
    // ACS Disruption, 0 = contained (dark) .. 1 = breach. See the header.
    disruption: f32,
    // How far the scar normal map is allowed to pull the shading normal.
    normal_strength: f32,
    // 0 = still passing for human, 1 = fully turned. Mirrors `Scp610Mutation::current` so the albedo
    // can turn WITH the silhouette instead of arriving already infected on a human-shaped mesh.
    mutation: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> flesh: Scp610FleshParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var scar_color_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var scar_color_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var scar_rough_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var scar_rough_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var scar_normal_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var scar_normal_samp: sampler;

// How far the scar albedo is pulled toward its own mean. The colour doc's slop grammar is "palettes
// with no contrast"; this is that, bounded so the tissue still reads as tissue rather than as a flat
// decal. Not a uniform: it is the look, not a dial, and every knob that reaches `config.ron` obliges
// `world_genome` to learn it (see `src/scp610/mod.rs`'s "tunables are constants here").
const SLOP_CONTRAST: f32 = 0.45;

// Emissive gain at full Disruption. Small: this is a candle (Vlam) escaping a room, not a light
// source. The scene is HDR + bloom, so a little goes further than it reads here.
const DISRUPTION_GAIN: f32 = 0.6;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // The mask. Only R carries the factor (G/B are spare channels, A is 1 — see the builder), and it
    // is clamped because interpolation across a triangle spanning the feather can overshoot slightly.
    let scar = clamp(in.color.r, 0.0, 1.0);

    // ── Albedo ────────────────────────────────────────────────────────────────────────────────────
    // Rebuilt, not adjusted: `pbr_input.material.base_color` has the mask multiplied into it by now.
    let flesh_albedo = textureSample(
        pbr_bindings::base_color_texture,
        pbr_bindings::base_color_sampler,
        in.uv,
    ).rgb;
    var scar_albedo = textureSample(scar_color_tex, scar_color_samp, in.uv * flesh.scar_uv_scale).rgb;
    // "Palettes with no contrast" — pull the scar toward its own luminance. Rec. 709 luma, so the
    // compression is perceptual rather than a flat RGB average.
    let scar_luma = dot(scar_albedo, vec3<f32>(0.2126, 0.7152, 0.0722));
    scar_albedo = mix(scar_albedo, vec3<f32>(scar_luma), SLOP_CONTRAST);

    // `mutation` gates how far the scar set is allowed in, so a fresh bloom is a person with a
    // discoloured patch and a turned one is mostly gore. The mask says WHERE, the morph says HOW MUCH.
    let blend = scar * flesh.mutation;
    let albedo = mix(flesh_albedo, scar_albedo, blend);
    pbr_input.material.base_color = vec4<f32>(albedo, 1.0);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    // ── Surface ───────────────────────────────────────────────────────────────────────────────────
    let scar_rough = textureSample(scar_rough_tex, scar_rough_samp, in.uv * flesh.scar_uv_scale).r;
    pbr_input.material.perceptual_roughness =
        mix(pbr_input.material.perceptual_roughness, scar_rough, blend);

    // The scar normal, into world space through the mesh's own tangent frame. `pbr_input.N` already
    // carries the flesh normal map, so this leans it toward the scar one by the mask rather than
    // replacing it — the two maps describe the same surface at different stages.
    let t = normalize(in.world_tangent.xyz);
    let n0 = normalize(pbr_input.world_normal);
    let b = cross(n0, t) * in.world_tangent.w;
    let ts = textureSample(scar_normal_tex, scar_normal_samp, in.uv * flesh.scar_uv_scale).rgb * 2.0 - 1.0;
    let scar_n = normalize(t * ts.x + b * ts.y + n0 * ts.z);
    pbr_input.N = normalize(mix(pbr_input.N, scar_n, blend * flesh.normal_strength));

    // ── Disruption ────────────────────────────────────────────────────────────────────────────────
    // Hueless by rule (see the header). Weighted toward the infected tissue so what glows is the
    // anomaly, not the person it used to be — the light is escaping from the thing that is wrong.
    let lumen = mix(0.35, 1.0, blend);
    pbr_input.material.emissive =
        vec4<f32>(vec3<f32>(flesh.disruption * DISRUPTION_GAIN * lumen), 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
