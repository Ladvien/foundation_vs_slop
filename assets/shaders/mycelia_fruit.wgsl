// MYCELIA fruit body — an ExtendedMaterial<StandardMaterial, MoldFruitExt> fragment for the death cap.
//
// The same organism as the mat on the floor, and it must LOOK like it. It shares the mat's palette
// (FLESH_DEEP / GLOW / FUZZ), its `fiber_scale` filament gauge, its `margin_roughness` mottle, its matte
// felt roughness with wetness confined to the vein cores, its cavity AO, its sheen rim, and its
// conceal-under-gaze reflex — all off the same uniforms, so retuning the mat retunes the mushroom.
//
// What differs is that a mushroom has PARTS, and the mesh tells us which is which.
//
// `COLOR_0` on this mesh is a **part mask**, not artwork: R = cap (pileus) · G = flesh (stipe, gills,
// annulus) · B = volva. There are no textures on the asset at all; the mask *is* the material. Bevy's
// `pbr_input_from_standard_material` multiplies base colour by the vertex colour whenever the mesh carries
// COLOR_0 — which would paint the cap pure red — so we overwrite `base_color` outright below and read the
// mask ourselves. Reading `in.color` unguarded is deliberate: if the asset ever ships without COLOR_0 this
// fails to compile, which is the correct outcome, not a silent grey mushroom.
//
// Colour is driven by `tint`, NOT by `growth`. `tint` chases growth but is rate-limited so no albedo shift
// can complete faster than the slow-change-blindness window (Simons, Franconeri & Reimer 2000,
// 10.1068/p3104). Motion has its own, far tighter budget — see `src/mycelia/perceptual.rs`.

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
    fiber_scale: f32,
    fiber_strength: f32,
    margin_roughness: f32,
    sheen_strength: f32,
    ao_strength: f32,
};

// MUST byte-match `MoldFruitParams` in `src/mycelia/material.rs`.
struct MoldFruitParams {
    tint: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> mold: MoldSurfaceParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var field_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var field_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var control_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var control_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var<uniform> fruit: MoldFruitParams;

// ── Shared with the mat ───────────────────────────────────────────────────────────────────────────────
// Identical to `mycelia_floor.wgsl` / `mycelia_wall.wgsl`. The fruit body is not a botanical illustration
// of Amanita phalloides dropped onto the mold — it is the SAME ORGANISM, so it wears the same palette.
// (An earlier revision used the species' real colours — pale tan cap, white flesh — and the mushroom read
// as a garden prop standing on an alien mat. Realism lost to family resemblance.)
const FLESH_DEEP: vec3<f32> = vec3<f32>(0.030, 0.068, 0.040);
const GLOW: vec3<f32> = vec3<f32>(0.10, 0.46, 0.26);
const FUZZ: vec3<f32> = vec3<f32>(0.17, 0.26, 0.17);
const FOG_DIM: vec3<f32> = vec3<f32>(0.30, 0.30, 0.38);
// A fungus is a dielectric felt and barely reflects; StandardMaterial's default F0 of 0.04 puts a specular
// sheet over the whole body under this scene's brightness-500 ambient. See `mycelia_wall.wgsl`.
const MOLD_REFLECTANCE: f32 = 0.08;

// ── The body, in the mat's hue ────────────────────────────────────────────────────────────────────────
// Everything below is the mold's green-black, lightened by part. The read comes from the bioluminescence,
// not from albedo — exactly as it does on the floor, where a near-black mat is legible only by its veins.
//
// The universal veil is the palest thing on the mushroom: a young cap is a taut membrane stretched over
// the primordium, so it catches the light. As the pileus expands and thins, the flesh beneath shows
// through and it sinks toward the mat's own deep flesh.
const CAP_YOUNG: vec3<f32> = vec3<f32>(0.44, 0.46, 0.36);
const CAP_OLD: vec3<f32> = vec3<f32>(0.11, 0.17, 0.09);
// Stipe, gills and annulus: hyphal felt, the fibrous part. Kept the lightest of the mature parts so the
// body's silhouette still reads at the RTS zoom, and it is where the glow lives.
const STIPE: vec3<f32> = vec3<f32>(0.20, 0.26, 0.19);
// The volva is a torn sac half-buried in the substrate, so it wears the substrate.
const VOLVA: vec3<f32> = vec3<f32>(0.13, 0.15, 0.10);
const SUBSTRATE: vec3<f32> = vec3<f32>(0.040, 0.055, 0.035);

// How far up the body (world units, from its base) the mat it grew out of still clings. The mold does not
// climb the whole mushroom — it pools around the volva, exactly where the sac meets the substrate.
const SKIRT_HEIGHT: f32 = 0.06;

// ── Procedural noise (see `mycelia_floor.wgsl` for provenance) ────────────────────────────────────────
fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i + vec2<f32>(0.0, 0.0));
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var pp = p;
    for (var i = 0; i < 4; i = i + 1) {
        v = v + amp * vnoise(pp);
        pp = pp * 2.0;
        amp = amp * 0.5;
    }
    return v;
}

fn world_to_uv(world_xz: vec2<f32>) -> vec2<f32> {
    return (world_xz - mold.world_origin) / mold.world_extent;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    let n = normalize(pbr_input.world_normal);

    // The part mask. Normalised because the generator writes one channel per part but interpolation across
    // a triangle spanning two parts leaves a blend that need not sum to 1.
    let raw = in.color.rgb;
    let mask = raw / max(raw.r + raw.g + raw.b, 1e-4);
    let cap = mask.r;
    let flesh = mask.g;
    let volva = mask.b;

    // ── Surface coordinates ───────────────────────────────────────────────────────────────────────────
    // The strand frame, established before anything reads it: hyphae run up the body, so the noise is
    // stretched along its axis. Same `fiber_scale` the mat uses, so the filaments are the same gauge on the
    // mushroom as on the floor it grew out of.
    let fs = mold.fiber_scale;
    let up = vec3<f32>(0.0, 1.0, 0.0);
    var tangent = cross(up, n);
    let tlen = length(tangent);
    if (tlen > 1e-3) {
        tangent = tangent / tlen;
    } else {
        tangent = vec3<f32>(1.0, 0.0, 0.0);
    }
    let along = dot(in.world_position.xyz, tangent);
    let sp = vec2<f32>(along * fs, in.world_position.y * fs * 0.6);

    // The body's own vein network, the same fbm ridge the mat's trail field resolves into. This is what
    // carries the family resemblance: the veins do not stop at the floor, they climb into the fruit body.
    let body_vein = smoothstep(0.52, 0.86, fbm(sp * 0.55));

    // ── Albedo by part ────────────────────────────────────────────────────────────────────────────────
    // The pileus darkens toward the mat's own flesh as the veil thins. `tint`, never `growth` — see header.
    let pileus = mix(CAP_YOUNG, CAP_OLD, fruit.tint);
    // Mottle the cap with the same fbm the colony's advancing margin is broken by, so its surface is
    // dappled like the mat rather than a clean painted dome.
    let mottle = smoothstep(0.35, 0.78, fbm(sp * 1.3));
    let cap_col = mix(pileus, FLESH_DEEP, mottle * mold.margin_roughness * 0.55);

    // The volva is a torn sac still half in the ground: fleck it with substrate.
    let grime = fbm(in.world_position.xz * fs * 2.0 + vec2<f32>(in.world_position.y * fs, 0.0));
    let sac = mix(VOLVA, SUBSTRATE, smoothstep(0.45, 0.75, grime) * 0.7);

    var albedo = cap_col * cap + STIPE * flesh + sac * volva;
    // Veins darken and thicken the flesh they run through, exactly as they do on the mat.
    albedo = mix(albedo, FLESH_DEEP, body_vein * 0.45 * saturate(flesh + volva));

    // ── The mat it grew out of ────────────────────────────────────────────────────────────────────────
    // Sample the mold field directly beneath this body. A mushroom standing in thick biomass is skirted by
    // it; one standing on bare floor is clean. Height above the body's base, not world Y — the body sinks
    // below the floor while it emerges, and the skirt must not slide up it as it rises.
    let uv = world_to_uv(in.world_position.xz);
    let inside = f32(uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0);
    let f = textureSampleLevel(field_tex, field_samp, uv, 0.0);
    let veins = smoothstep(mold.vein_lo, mold.vein_hi, f.r);
    let bio = smoothstep(0.10, 0.35, f.g);
    let lit = mix(FOG_DIM, vec3<f32>(1.0), smoothstep(0.5, 1.0, f.a));

    // `h` is 0 at the floor plane and 1 at `SKIRT_HEIGHT` above it. Written out rather than as an inverted
    // smoothstep: WGSL leaves smoothstep undefined when edge0 >= edge1.
    let h = clamp(in.world_position.y / SKIRT_HEIGHT, 0.0, 1.0);
    let ragged = (fbm(vec2<f32>(in.world_position.x, in.world_position.z) * fs * 1.5) - 0.5)
               * mold.margin_roughness;
    let t = clamp(1.0 - (h + ragged), 0.0, 1.0);
    let skirt = t * t * (3.0 - 2.0 * t);
    let coat = saturate(max(bio, veins * 0.9)) * skirt * inside * mold.intensity;

    albedo = mix(albedo, FLESH_DEEP, coat);
    pbr_input.material.base_color = vec4<f32>(albedo * lit, pbr_input.material.base_color.a);

    // ── Surface ───────────────────────────────────────────────────────────────────────────────────────
    // Matte felt everywhere — a fungus is a light-scattering mat, not a waxy toy. The only wet places are
    // the vein cores, precisely as on the floor, where `wet_roughness` is confined to the trail ridges.
    // (The cap was `0.62` "waxy" here before; under a brightness-500 ambient that gave it a plastic
    // highlight the mat never has, and broke the family resemblance more than the palette did.)
    var roughness = mix(0.92, 0.88, cap);
    roughness = mix(roughness, mold.wet_roughness, body_vein * 0.7);
    roughness = mix(roughness, mold.wet_roughness, coat * veins * veins);
    pbr_input.material.perceptual_roughness = roughness;
    pbr_input.material.reflectance = vec3<f32>(MOLD_REFLECTANCE);

    // ── Filaments, over the whole body ────────────────────────────────────────────────────────────────
    // Stipe and volva are hyphal felt; the cap is a membrane stretched over the same hyphae, so it takes
    // the same strands at half relief rather than none. Central-difference the noise in the body's own
    // frame — it has no thickness field of its own to take a gradient of.
    let fibrous = 1.0 - 0.5 * cap;
    let e = 0.02;
    let sa = fbm(sp + vec2<f32>(e * fs, 0.0));
    let sb = fbm(sp - vec2<f32>(e * fs, 0.0));
    let sc = fbm(sp + vec2<f32>(0.0, e * fs * 0.6));
    let sd = fbm(sp - vec2<f32>(0.0, e * fs * 0.6));
    let ridge = mold.fiber_strength * fibrous * 0.5;
    pbr_input.N = normalize(n - tangent * (sa - sb) * ridge - up * (sc - sd) * ridge);

    // Cavity AO. Without it the scene's bright uniform ambient — which ignores normals entirely — flattens
    // every strand and the gills read as a painted disc.
    let strand = fbm(sp);
    let ao = clamp(1.0 - mold.ao_strength * (1.0 - strand) * fibrous, 0.0, 1.0);
    pbr_input.diffuse_occlusion = vec3<f32>(ao);
    pbr_input.specular_occlusion = ao;

    // ── Bioluminescence ───────────────────────────────────────────────────────────────────────────────
    // With a near-black albedo, this is what makes the mushroom legible at all — the same bargain the mat
    // strikes on the floor. The glow lives in the flesh (gills, stipe, annulus) and in the veins climbing
    // it; the cap's cuticle mostly hides it. It conceals itself under a live gaze, off the same control
    // channel and with the same `conceal` term as the mat.
    let lumen = flesh + volva * 0.35 + cap * 0.15;
    let light = textureSampleLevel(control_tex, control_samp, uv, 0.0).g;
    let conceal = 1.0 - 0.7 * light;
    var emissive = GLOW * lumen * (0.30 + 0.85 * body_vein) * conceal * mold.glow_gain * lit;
    // The mat's own veins, where they climb the skirt.
    emissive += GLOW * veins * coat * conceal * mold.glow_gain * lit;
    // Fresnel-shaped stand-in for a sheen lobe; folded in before lighting so it respects exposure.
    let ndv = clamp(dot(pbr_input.N, pbr_input.V), 0.0, 1.0);
    emissive += FUZZ * pow(1.0 - ndv, 5.0) * mold.sheen_strength * fibrous * lit;
    pbr_input.material.emissive = vec4<f32>(emissive, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
