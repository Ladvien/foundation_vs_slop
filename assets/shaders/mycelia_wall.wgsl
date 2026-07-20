// MYCELIA wall coating — an ExtendedMaterial<StandardMaterial, MoldWallExt> fragment.
//
// The mold creeps up the wall out of the floor/wall corner, so it must sample the field where the mold
// actually POOLED: at the wall's foot, in the room this face looks into. Sampling the wall's own XZ (the
// previous behaviour) lands on the outermost sliver of the floor cell — exactly the strip that used to be
// drained dry by the leaking diffusion, so nothing ever appeared to climb.
//
// Unlike the floor this is an OPAQUE material (it is the wall), so instead of blending a coat on top we lerp
// the wall's own base colour toward the biomass. Only the main-pass fragment is overridden; the prepass
// keeps StandardMaterial's default.

#import bevy_pbr::pbr_fragment::pbr_input_from_standard_material
#import bevy_pbr::pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing}
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}
#import foundation::noise::fbm4

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
    fiber_scale: f32,
    fiber_strength: f32,
    margin_roughness: f32,
    sheen_strength: f32,
    ao_strength: f32,
    reveal_warp_amp: f32,
    reveal_warp_scale: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> mold: MoldSurfaceParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var field_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var field_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var control_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var control_samp: sampler;

// Kept identical to the floor's, so the coating is visibly one organism crossing the corner.
const FLESH_DEEP: vec3<f32> = vec3<f32>(0.048, 0.059, 0.051);
const GLOW: vec3<f32> = vec3<f32>(0.238, 0.396, 0.323);
const FUZZ: vec3<f32> = vec3<f32>(0.213, 0.237, 0.213);
// The fog's dim tint for remembered-but-unseen floor, matching `dungeon::FloorMaterials::dim`
// (0.28, 0.28, 0.36). The mold must dim with the ground it sits on; drawn at full brightness it ignores the
// fog's lighting state even while honouring its reveal state, and a remembered room glows through the dark.
const FOG_DIM: vec3<f32> = vec3<f32>(0.30, 0.30, 0.38);
// Mycelium is a dielectric felt, and a felt barely reflects. StandardMaterial defaults `reflectance` to 0.5
// (F0 = 0.04) which, under this scene's brightness-500 ambient, puts a specular sheet over the whole coat.
// THAT is the shine — not the roughness alone. Dropping F0 by ~6x is what finally kills the wet look.
const MOLD_REFLECTANCE: f32 = 0.08;


// How far off the wall face to sample, in world units. A slab is 0.14 thick with its outer face flush to
// the cell boundary, so its inner face sits 0.14 inside the floor cell; pushing one more field texel
// (0.1875) past that lands the sample squarely on the pooled floor rather than on the slab's own footprint.
// Geometry, not taste — hence a constant, not a dial.
const WALL_FOOT_OFFSET: f32 = 0.19;

fn world_to_uv(world_xz: vec2<f32>) -> vec2<f32> {
    return (world_xz - mold.world_origin) / mold.world_extent;
}

// Fog state from the control texture, which `write_control` rewrites every frame — never from the field's
// alpha, which only advances on a sim tick and lagged the reveal by a whole tick period. See
// `mycelia_floor.wgsl`.
fn is_explored(a: f32) -> f32 {
    return smoothstep(0.45, 0.55, a);
}
fn is_visible(a: f32) -> f32 {
    return smoothstep(0.85, 0.95, a);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    let n = normalize(pbr_input.world_normal);

    // Sample the mold pooled at this face's foot, in the room the face looks into. Note this is
    // `normalize(3D).xz`, NOT `normalize(xz)`: on the slab's +Y cap the horizontal components are ~0, so
    // the offset harmlessly vanishes instead of exploding. (The cap is discarded by `climb` below anyway,
    // but the sample must be finite before we get there.)
    let foot = in.world_position.xz + n.xz * WALL_FOOT_OFFSET;
    let uv = world_to_uv(foot);
    let inside = f32(uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0);

    let f = textureSampleLevel(field_tex, field_samp, uv, 0.0);
    let veins = smoothstep(mold.vein_lo, mold.vein_hi, f.r);
    let bio = smoothstep(0.10, 0.35, f.g);
    // Domain-warp the reveal/coverage tap so the coat's edge stops snapping to the per-cell control
    // grid (see `mycelia_floor.wgsl` for the why). Warp by the foot world-XZ, the point the wall reads.
    let warp = vec2<f32>(
        fbm4(foot * mold.reveal_warp_scale),
        fbm4(foot * mold.reveal_warp_scale + vec2<f32>(31.4, 17.7)),
    ) - 0.5;
    let ctrl_uv = uv + warp * mold.reveal_warp_amp;
    let substrate = textureSampleLevel(control_tex, control_samp, ctrl_uv, 0.0).a;
    let coverage = is_explored(substrate);
    let lit = mix(FOG_DIM, vec3<f32>(1.0), is_visible(substrate));

    // ── Tangent frame on a vertical face ──────────────────────────────────────────────────────────────
    // Wall faces are vertical, so `cross(up, n)` is a well-defined horizontal tangent running along the
    // face. It degenerates only on the +Y cap, where `n` is parallel to up — guard rather than rely on the
    // fact that `climb` happens to zero that fragment out.
    let up = vec3<f32>(0.0, 1.0, 0.0);
    var tangent = cross(up, n);
    let tlen = length(tangent);
    if (tlen > 1e-3) {
        tangent = tangent / tlen;
    } else {
        tangent = vec3<f32>(1.0, 0.0, 0.0);
    }
    // Surface coordinates on the face: how far along it, and how high up it.
    let along = dot(in.world_position.xyz, tangent);
    let height = in.world_position.y;

    // ── Climb, with a ragged margin ───────────────────────────────────────────────────────────────────
    // A plain `pow(1 - y/climb_height, 2)` is a clean bathtub ring. Breaking the upper edge with fbm —
    // stretched vertically, so the noise varies faster along the face than up it — turns the ring into
    // tendrils reaching up out of a dense skirting.
    let h = height / max(mold.climb_height, 0.001);
    let fs = mold.fiber_scale;
    let m = fbm4(vec2<f32>(along * fs * 0.35, height * fs * 0.12));
    // Falls from 1 at the skirting to 0 at `climb_height`. Written out rather than as
    // `smoothstep(1.0, 0.0, ...)`: WGSL leaves smoothstep undefined when `edge0 >= edge1`. It happens to do
    // the right thing under naga/Metal, which is not a guarantee worth depending on.
    let t = clamp(1.0 - (h + (m - 0.5) * mold.margin_roughness), 0.0, 1.0);
    let climb = t * t * (3.0 - 2.0 * t);

    // Mold only climbs where it has actually pooled at the foot of this wall.
    let coat = saturate(max(bio, veins * 0.9)) * climb * coverage * inside * mold.intensity;

    if (coat > 0.002) {
        // Opaque surface: lerp the wallpaper toward biomass rather than blending a layer over it.
        pbr_input.material.base_color = vec4<f32>(
            mix(pbr_input.material.base_color.rgb, FLESH_DEEP * lit, coat),
            pbr_input.material.base_color.a,
        );
        // Matte felt; wet only in the vein cores, exactly as on the floor.
        pbr_input.material.perceptual_roughness =
            mix(pbr_input.material.perceptual_roughness, mold.wet_roughness, coat * veins * veins);
        pbr_input.material.reflectance = mix(pbr_input.material.reflectance, vec3<f32>(MOLD_REFLECTANCE), coat);

        // ── Filaments ─────────────────────────────────────────────────────────────────────────────────
        // Hyphae climbing a wall run vertically. Sample the strand noise and central-difference it in the
        // face's own frame to perturb the normal — the wall has no thickness field of its own to gradient.
        let e = 0.06;
        let sp = vec2<f32>(along * fs, height * fs * 0.25);
        let sa = fbm4(sp + vec2<f32>(e * fs, 0.0));
        let sb = fbm4(sp - vec2<f32>(e * fs, 0.0));
        let sc = fbm4(sp + vec2<f32>(0.0, e * fs * 0.25));
        let sd = fbm4(sp - vec2<f32>(0.0, e * fs * 0.25));
        let ridge = mold.fiber_strength * coat;
        pbr_input.N = normalize(
            n - tangent * (sa - sb) * ridge - up * (sc - sd) * ridge
        );

        // Cavity AO: without it the bright uniform ambient flattens every strand. See the floor shader.
        let strand = fbm4(sp);
        let ao = clamp(1.0 - mold.ao_strength * (1.0 - strand) * coat, 0.0, 1.0);
        pbr_input.diffuse_occlusion = vec3<f32>(ao);
        pbr_input.specular_occlusion = ao * (1.0 - 0.8 * coat);

        let light = textureSampleLevel(control_tex, control_samp, uv, 0.0).g;
        let conceal = 1.0 - 0.7 * light;
        var emissive = GLOW * veins * climb * conceal * mold.glow_gain * lit;
        // Fresnel-shaped stand-in for a sheen lobe; folded in before lighting so it respects exposure.
        let ndv = clamp(dot(pbr_input.N, pbr_input.V), 0.0, 1.0);
        emissive += FUZZ * pow(1.0 - ndv, 5.0) * mold.sheen_strength * coat * lit;
        pbr_input.material.emissive = vec4<f32>(emissive, 1.0);
    }

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
