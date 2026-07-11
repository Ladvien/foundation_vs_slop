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
#import bevy_pbr::forward_io::{Vertex, VertexOutput, FragmentOutput}
#import bevy_pbr::mesh_bindings::mesh
#import bevy_pbr::mesh_functions
#import bevy_pbr::morph::{morph_position, morph_normal, morph_tangent}
#import bevy_pbr::view_transformations::position_world_to_clip

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

// MUST byte-match `MoldFruitParams` in `src/mycelia/material.rs`. `vec2` first: it aligns to 8 bytes, so
// putting the scalar after it costs no padding.
struct MoldFruitParams {
    bend: vec2<f32>,
    tilt: vec2<f32>,
    cap_ab: vec2<f32>,
    tint: f32,
    cap_young: vec3<f32>,
    cap_old: vec3<f32>,
    stipe: vec3<f32>,
    volva: vec3<f32>,
    substrate: vec3<f32>,
    bend_lo: f32,
    bend_hi: f32,
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
const FLESH_DEEP: vec3<f32> = vec3<f32>(0.048, 0.059, 0.051);
const GLOW: vec3<f32> = vec3<f32>(0.238, 0.396, 0.323);
const FUZZ: vec3<f32> = vec3<f32>(0.213, 0.237, 0.213);
const FOG_DIM: vec3<f32> = vec3<f32>(0.30, 0.30, 0.38);
// A fungus is a dielectric felt and barely reflects; StandardMaterial's default F0 of 0.04 puts a specular
// sheet over the whole body under this scene's brightness-500 ambient. See `mycelia_wall.wgsl`.
const MOLD_REFLECTANCE: f32 = 0.08;

// ── Oklab ─────────────────────────────────────────────────────────────────────────────────────────────
// Björn Ottosson (2020); the space CSS Color 4 interpolates in. Used here for exactly one thing: shifting the
// cap's hue and chroma by this body's `cap_ab` while leaving its LIGHTNESS untouched. `L` is what the cavity
// AO, the sheen and this LDR tonemapper were balanced against, so a naive RGB tint would relight the
// mushroom; an Oklab `(a, b)` offset recolours it and nothing else.
//
// Each cluster draws one `(a, b)`, and each member deviates a little from it — so a bunch reads as one
// colour, and its caps as individuals. **Duplicated in `src/mycelia/perceptual.rs`**, where the round-trip
// and the lightness invariant are unit-tested. They must agree.
fn linear_srgb_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
    let m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
    let s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
    let l_ = pow(max(l, 0.0), 0.3333333333);
    let m_ = pow(max(m, 0.0), 0.3333333333);
    let s_ = pow(max(s, 0.0), 0.3333333333);
    return vec3<f32>(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    );
}

fn oklab_to_linear_srgb(c: vec3<f32>) -> vec3<f32> {
    let l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    return vec3<f32>(
         4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

// ── The body, in the mat's hue ────────────────────────────────────────────────────────────────────────
// Everything below is the mold's grey-black, lightened by part, and desaturated in OKLAB by the same factor
// as the mat (see `mycelia_floor.wgsl`) — so the mushroom stays visibly the same organism. The read comes
// from the bioluminescence, not from albedo — exactly as it does on the floor, where a near-black mat is
// legible only by its veins.
//
// The cap (`fruit.cap_young → fruit.cap_old` with maturity), stipe/gills/annulus (`fruit.stipe`) and the
// substrate-buried volva (`fruit.volva`/`fruit.substrate`) are now per-species uniforms from the
// `mycelia.species` table — a young cap is a taut pale membrane that sinks toward the flesh beneath as the
// pileus expands and thins. The death cap carries the values these constants used to hold, so it is
// unchanged; a fly agaric is red, a chanterelle gold. `FLESH_DEEP`/`GLOW` stay shared so every species
// still reads as the same organism as the mat it grew from.

// How far up the body (world units, from its base) the mat it grew out of still clings. The mold does not
// climb the whole mushroom — it pools around the volva, exactly where the sac meets the substrate.
const SKIRT_HEIGHT: f32 = 0.06;

// ── The bend ──────────────────────────────────────────────────────────────────────────────────────────
//
// A mushroom stem curves by *differential cell elongation*, and the extension is concentrated in the upper
// 20–30% of the stem — cells on the outer flank end up four to five times longer than those on the inner
// (Greening, Sánchez & Moore 1997, Can. J. Bot. 75:1174, 10.1139/b97-830). This mesh's stipe spans
// 2.18–11.80 cm, so its upper 30% begins at 8.91 cm and the zone closes at the cap's underside.
//
// `bend_profile` is a smoothstep across that zone, so its SLOPE vanishes at both ends. That is not a
// convenience, it is the anatomy: below the zone the lower stipe and volva stay planted and unsheared;
// above it the profile has saturated, so the cap translates rigidly and stays LEVEL on the curved stem.
// The hymenophore is positively gravitropic and re-levels independently of the stem (Moore 1991,
// New Phytol. 117:3, 10.1111/j.1469-8137.1991.tb00940.x).
//
// Because the profile keys off the stipe's *height*, the bend develops as the stem grows into the zone: an
// egg is perfectly straight and a young button barely leans. That extra vertex travel is charged to the
// perceptual speed limit on the CPU (`perceptual::STAGE_BEND_FRACTION`), so a leaning mushroom grows
// slower rather than swinging over where you can see it.
//
// `tilt` is the other half, and a different animal: a LINEAR term, `tilt * y`, so it leans the whole stem
// from the ground up rather than curving its top. It is the body's growth angle. At `y = 0` it contributes
// nothing, so the volva stays seated on the floor no matter how far off plumb the stem grew — which is why
// this is a vertex displacement and not a rotation of the entity. Its drift is likewise charged to the
// speed limit (`perceptual::STAGE_HEIGHT_DELTA`).
//
// The zone `[fruit.bend_lo, fruit.bend_hi]` is per-species (from the `mycelia.species` table), so a short
// mushroom bends over its own upper stipe. MUST match this species' `bend_lo_m`/`bend_hi_m`, which the CPU
// budgets for in `SpeciesGeometry::stage_bend_fraction`.
fn bend_profile(y: f32) -> f32 {
    let u = clamp((y - fruit.bend_lo) / (fruit.bend_hi - fruit.bend_lo), 0.0, 1.0);
    return u * u * (3.0 - 2.0 * u);
}

/// d(profile)/dy — the local shear, used to tilt the normal with the surface it rides on.
fn bend_slope(y: f32) -> f32 {
    let u = clamp((y - fruit.bend_lo) / (fruit.bend_hi - fruit.bend_lo), 0.0, 1.0);
    return 6.0 * u * (1.0 - u) / (fruit.bend_hi - fruit.bend_lo);
}

#ifdef MORPH_TARGETS
// Overriding the vertex stage means Bevy's own `morph_vertex` (which is defined inside `mesh.wgsl`, not
// exported) no longer runs for this material. Reproduced here verbatim; without it the mushroom would snap
// back to the sealed-egg basis and never grow at all.
fn morph_vertex(vertex_in: Vertex, instance_index: u32) -> Vertex {
    var vertex = vertex_in;
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = vertex.index - first_vertex;

    let weight_count = bevy_pbr::morph::layer_count(instance_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {
        let weight = bevy_pbr::morph::weight_at(i, instance_index);
        if weight == 0.0 {
            continue;
        }
        vertex.position += weight * morph_position(vertex_index, i, instance_index);
#ifdef VERTEX_NORMALS
        vertex.normal += weight * morph_normal(vertex_index, i, instance_index);
#endif
#ifdef VERTEX_TANGENTS
        vertex.tangent += vec4(weight * morph_tangent(vertex_index, i, instance_index), 0.0);
#endif
    }
    return vertex;
}
#endif

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;

#ifdef MORPH_TARGETS
    var vertex = morph_vertex(vertex_no_morph, vertex_no_morph.instance_index);
#else
    var vertex = vertex_no_morph;
#endif

    // Lean the stem (linear) and curve its top (the bend profile). Object space, so both are independent of
    // the body's yaw and scale.
    let b = vec3<f32>(fruit.bend.x, 0.0, fruit.bend.y);
    let t = vec3<f32>(fruit.tilt.x, 0.0, fruit.tilt.y);
    let y = vertex.position.y;
    vertex.position = vertex.position + t * y + b * bend_profile(y);

#ifdef VERTEX_NORMALS
    // The displacement d(y) = t*y + b*p(y) shears the surface, so the normal must tilt with it. For
    // J = I + d'(y) (x) e_y^T, the inverse transpose is I - e_y (x) d'(y)^T to first order, which touches
    // only the normal's Y component. Skipping this leaves the lighting flat on a visibly bent stem.
    let slope = t + b * bend_slope(y);
    let shear = dot(slope.xz, vertex.normal.xz);
    vertex.normal = normalize(vertex.normal - vec3<f32>(0.0, shear, 0.0));
#endif

    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);
    // No `SKINNED` branch: the death cap has no joints, and a shader def we do not handle should fail to
    // compile rather than silently drop the skin.
    var world_from_local = mesh_world_from_local;

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index,
    );
#endif

#ifdef VERTEX_POSITIONS
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_no_morph.instance_index,
    );
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex_no_morph.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index,
        mesh_world_from_local[3],
    );
#endif

    return out;
}

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

// Fog state from the control texture, which `write_control` rewrites every frame — never from the field's
// alpha, which only advances on a sim tick and lagged the reveal by a whole tick period. See
// `mycelia_floor.wgsl`.
fn is_visible(a: f32) -> f32 {
    return smoothstep(0.85, 0.95, a);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    let n = normalize(pbr_input.world_normal);

    // World Y of this body's base. The mesh is authored with its base (the volva) at object y = 0 and the
    // `DeathCap` node's local transform is identity, so the primitive→world translation *is* the body's
    // origin. It tracks the entity as it sinks and rises (`translation.y = -sink * (1 - rise)`), which world
    // Y alone does not — anything keyed off absolute Y slides through the mesh during emergence.
    let base_y = mesh_functions::get_world_from_local(in.instance_index)[3].y;
    let height = in.world_position.y - base_y;

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
    let sp = vec2<f32>(along * fs, height * fs * 0.6);

    // The body's own vein network, the same fbm ridge the mat's trail field resolves into. This is what
    // carries the family resemblance: the veins do not stop at the floor, they climb into the fruit body.
    let body_vein = smoothstep(0.52, 0.86, fbm(sp * 0.55));

    // ── Albedo by part ────────────────────────────────────────────────────────────────────────────────
    // The pileus darkens toward the mat's own flesh as the veil thins. `tint`, never `growth` — see header.
    // Then this body's cluster colour, applied in Oklab so only hue and chroma move: the maturity ramp lives
    // in lightness and must survive untouched, or a differently-coloured cap would also read as a
    // differently-aged one.
    let pileus_lab = linear_srgb_to_oklab(mix(fruit.cap_young, fruit.cap_old, fruit.tint));
    let pileus = max(
        oklab_to_linear_srgb(vec3<f32>(pileus_lab.x, pileus_lab.y + fruit.cap_ab.x, pileus_lab.z + fruit.cap_ab.y)),
        vec3<f32>(0.0),
    );
    // Mottle the cap with the same fbm the colony's advancing margin is broken by, so its surface is
    // dappled like the mat rather than a clean painted dome.
    let mottle = smoothstep(0.35, 0.78, fbm(sp * 1.3));
    let cap_col = mix(pileus, FLESH_DEEP, mottle * mold.margin_roughness * 0.55);

    // The volva is a torn sac still half in the ground: fleck it with substrate. Keyed off height above the
    // base, so the flecks sit on the sac rather than swimming down it as the body rises out of the mat.
    let grime = fbm(in.world_position.xz * fs * 2.0 + vec2<f32>(height * fs, 0.0));
    let sac = mix(fruit.volva, fruit.substrate, smoothstep(0.45, 0.75, grime) * 0.7);

    var albedo = cap_col * cap + fruit.stipe * flesh + sac * volva;
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
    // A body is never drawn on unexplored floor — it cannot pin there — so it needs `is_visible` only, to
    // dim with the fogged floor it stands on. No coverage gate, unlike the mat.
    let lit = mix(FOG_DIM, vec3<f32>(1.0), is_visible(textureSampleLevel(control_tex, control_samp, uv, 0.0).a));

    // `h` is 0 at the body's base and 1 at `SKIRT_HEIGHT` above it, in world metres — so the skirt keeps its
    // physical thickness whatever `body_scale` this mushroom drew. Written out rather than as an inverted
    // smoothstep: WGSL leaves smoothstep undefined when edge0 >= edge1.
    let h = clamp(height / SKIRT_HEIGHT, 0.0, 1.0);
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
