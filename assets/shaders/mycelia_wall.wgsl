// MYCELIA wall coating — an ExtendedMaterial<StandardMaterial, MoldWallExt> fragment.
//
// The mold creeps up the wall out of the floor/wall corner. It samples the SAME world-XZ field the floor
// does — a wall slab is only 0.14 thick and sits inset from the cell boundary, so its world XZ lands inside
// the floor cell it borders. Whatever mold has pooled against that wall foot is therefore exactly what
// climbs it. Coverage fades with world height over `climb_height`.
//
// Unlike the floor this is an OPAQUE material (it is the wall), so instead of blending a coat on top we
// lerp the wall's own base colour toward the biomass. Only the main-pass fragment is overridden; the
// prepass keeps StandardMaterial's default.

#import bevy_pbr::pbr_fragment::pbr_input_from_standard_material
#import bevy_pbr::pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing}
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}

// MUST byte-match `MoldSurfaceParams` in `src/mycelia/material.rs`.
struct MoldSurfaceParams {
    world_origin: vec2<f32>,
    world_extent: vec2<f32>,
    field_res: vec2<f32>,
    glow_gain: f32,
    intensity: f32,
    vein_lo: f32,
    vein_hi: f32,
    normal_strength: f32,
    wet_roughness: f32,
    climb_height: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> mold: MoldSurfaceParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var field_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var field_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var control_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var control_samp: sampler;

// Kept identical to the floor's, so the coating is visibly one organism crossing the corner.
const FLESH: vec3<f32> = vec3<f32>(0.035, 0.080, 0.048);
const GLOW: vec3<f32> = vec3<f32>(0.10, 0.46, 0.26);

fn world_to_uv(world_xz: vec2<f32>) -> vec2<f32> {
    return (world_xz - mold.world_origin) / mold.world_extent;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    let uv = world_to_uv(in.world_position.xz);
    let inside = f32(uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0);

    let f = textureSampleLevel(field_tex, field_samp, uv, 0.0);
    let veins = smoothstep(mold.vein_lo, mold.vein_hi, f.r);
    let bio = smoothstep(0.10, 0.35, f.g);
    let coverage = f.a;

    // How high off the floor this fragment is. Walls stand on y = 0, so world Y *is* the climb height.
    // Falls to zero at `climb_height`; squared so the mold is dense at the skirting and wispy at its limit.
    let climb = pow(1.0 - saturate(in.world_position.y / max(mold.climb_height, 0.001)), 2.0);

    // Mold only climbs where it has actually pooled at the foot of this wall.
    let coat = saturate(max(bio, veins * 0.9)) * climb * coverage * inside * mold.intensity;

    if (coat > 0.002) {
        // Opaque surface: lerp the wallpaper toward biomass rather than blending a layer over it.
        pbr_input.material.base_color = vec4<f32>(
            mix(pbr_input.material.base_color.rgb, FLESH, coat),
            pbr_input.material.base_color.a,
        );
        pbr_input.material.perceptual_roughness =
            mix(pbr_input.material.perceptual_roughness, mold.wet_roughness, coat);

        let light = textureSampleLevel(control_tex, control_samp, uv, 0.0).g;
        let conceal = 1.0 - 0.7 * light;
        pbr_input.material.emissive =
            vec4<f32>(GLOW * veins * climb * conceal * mold.glow_gain, 1.0);
    }

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
