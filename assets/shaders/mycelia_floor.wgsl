// MYCELIA floor coating — an ExtendedMaterial<StandardMaterial, MoldFloorExt> fragment.
//
// The compute chain hands us raw simulation fields; this shader turns them into a LIT surface. Sampling is
// by WORLD XZ (not mesh UV: every floor tile shares one Plane3d with UV 0..1, so world position is the only
// stable index). The surface normal is derived by finite-differencing the mold's thickness, which is what
// stops the coating reading as a flat decal — it becomes a lumpy, wet, lit film.
//
// Only the main-pass fragment is overridden; the prepass uses StandardMaterial's default.

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
// R = trail · G = biomass V · B = wall contact · A = coverage (explored floor only)
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var field_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var field_samp: sampler;
// R = chemo · G = light/gaze · B = disturbance · A = substrate
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var control_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var control_samp: sampler;

// The mold's dead-matter colour: a dark, sickly, saturated green. Dark enough that the emissive veins read
// as light coming *out of* the biomass, but not so dark that the specular highlight is all you see — the
// scene's 500-brightness ambient will otherwise render a near-black albedo as a grey mirror.
const FLESH: vec3<f32> = vec3<f32>(0.035, 0.080, 0.048);
// Sickly green/cyan phosphorescence. The camera is LDR (no HDR, no bloom) and the scene is brightly lit, so
// this must be bright enough to compete with the ambient fill yet stay under the tonemapper's clip.
const GLOW: vec3<f32> = vec3<f32>(0.10, 0.46, 0.26);

fn world_to_uv(world_xz: vec2<f32>) -> vec2<f32> {
    return (world_xz - mold.world_origin) / mold.world_extent;
}

// How physically thick the mold is at `uv`, in arbitrary units. Drives the surface normal.
fn thickness(uv: vec2<f32>) -> f32 {
    let f = textureSampleLevel(field_tex, field_samp, uv, 0.0);
    let veins = smoothstep(mold.vein_lo, mold.vein_hi, f.r);
    let bio = smoothstep(0.10, 0.35, f.g);
    // Veins are raised cords; biomass is a swollen sheet; mold piles up in the wall corner.
    return bio + veins * 0.55 + f.b * 0.30;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let uv = world_to_uv(in.world_position.xz);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        discard;
    }

    let f = textureSampleLevel(field_tex, field_samp, uv, 0.0);
    let veins = smoothstep(mold.vein_lo, mold.vein_hi, f.r);
    let sheen = smoothstep(mold.vein_lo * 0.17, mold.vein_lo, f.r);
    let bio = smoothstep(0.10, 0.35, f.g);
    let contact = f.b;
    let coverage = f.a;

    // Coverage already encodes "explored floor only". Mold pools in the wall corner, so contact thickens it.
    let coat = clamp(max(max(veins * 0.85, bio * 0.55), sheen * 0.14) + contact * bio * 0.35, 0.0, 1.0)
             * coverage * mold.intensity;

    // Bare carpet: nothing to draw. Discarding (rather than emitting alpha 0) skips the lighting work and
    // avoids a full-footprint transparent quad blending over the whole floor every frame.
    if (coat < 0.004) {
        discard;
    }

    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // ── Surface ───────────────────────────────────────────────────────────────────────────────────────
    pbr_input.material.base_color = vec4<f32>(FLESH, coat);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    // Wet where thick, dull where it is only a scent sheen.
    pbr_input.material.perceptual_roughness = mix(0.95, mold.wet_roughness, max(bio, veins));
    pbr_input.material.metallic = 0.0;

    // The mold conceals its bioluminescence under a direct gaze — brightest in the dark. (The slow
    // structural retreat comes from the agents themselves fleeing the light; this is the instant flinch.)
    let light = textureSampleLevel(control_tex, control_samp, uv, 0.0).g;
    let conceal = 1.0 - 0.7 * light;
    pbr_input.material.emissive = vec4<f32>(GLOW * veins * conceal * mold.glow_gain, 1.0);

    // ── Normal from the thickness field ───────────────────────────────────────────────────────────────
    // Central differences over one field texel. The overlay is a horizontal plane, so its tangent frame is
    // trivial: +uv.x is +world.x and +uv.y is +world.z, and the geometric normal is +Y. Building the
    // perturbed normal straight in world space is therefore exact, not an approximation.
    let texel = 1.0 / mold.field_res;
    let hx = thickness(uv + vec2<f32>(texel.x, 0.0)) - thickness(uv - vec2<f32>(texel.x, 0.0));
    let hz = thickness(uv + vec2<f32>(0.0, texel.y)) - thickness(uv - vec2<f32>(0.0, texel.y));
    let bumpy = normalize(vec3<f32>(-hx * mold.normal_strength, 1.0, -hz * mold.normal_strength));
    // Fade the perturbation in with coverage so the bare-carpet fringe doesn't get a hard normal seam.
    pbr_input.N = normalize(mix(pbr_input.world_normal, bumpy, coat));

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
