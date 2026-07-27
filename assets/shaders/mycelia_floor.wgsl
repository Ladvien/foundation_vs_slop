// MYCELIA floor coating — an ExtendedMaterial<StandardMaterial, MoldFloorExt> fragment.
//
// The compute chain hands us raw simulation fields; this shader turns them into a LIT surface. Sampling is
// by WORLD XZ (not mesh UV: every floor tile shares one Plane3d with UV 0..1, so world position is the only
// stable index).
//
// The surface must read as a FIBROUS MYCELIAL MAT, not a fluid. Four things do that work, in rough order of
// importance:
//   1. A matte body (roughness ~0.92). Only the vein CORES go wet. A low roughness smeared across the whole
//      sheet is precisely what makes a biofilm look like spilled liquid.
//   2. Cavity AO into `diffuse_occlusion`. The scene's ambient is a bright UNIFORM fill (brightness 500),
//      and uniform ambient ignores surface normals entirely — so without an occlusion term the filaments
//      render flat no matter how hard we perturb the normal. This is the dial that makes the strands exist.
//   3. A dendritic, fbm-broken colony margin. Real fungal colonies have a feathery fractal advancing edge;
//      a smooth iso-contour reads as the meniscus of a puddle.
//   4. Filaments: fbm stretched ALONG the trail gradient's iso-contours, so strands run with the veins.
//
// Only the main-pass fragment is overridden; the prepass uses StandardMaterial's default.

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
// R = trail · G = biomass V · B = wall contact · A = unused. Interpolated between the last two sim ticks by
// `mycelia_blend.wgsl`, so this is continuous in time even though the simulation behind it is not. Coverage
// used to live in `A`; it now comes from `control_tex.a` per frame — see `is_explored` below.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var field_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var field_samp: sampler;
// R = chemo · G = light/gaze · B = disturbance · A = substrate
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var control_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var control_samp: sampler;

// ── Palette: damp grey, not green ─────────────────────────────────────────────────────────────────────
// Every colour below was desaturated in OKLAB (Ottosson 2020, the space CSS Color 4 interpolates in),
// scaling chroma toward the neutral axis while holding LIGHTNESS EXACTLY. That matters: `L` is what the AO,
// the sheen and the LDR tonemapper were balanced against, so draining the colour cannot disturb the read of
// the surface. Chroma fell ~70% (e.g. FLESH_DEEP 0.043 -> 0.013). The residual hue sits near 150 deg — a
// cold olive — so the mat is grey and dank first and organic second, rather than a vivid mould green.
//
// Mature biomass: dark, wet, colourless. Dark enough that the emissive veins read as light coming *out of*
// the flesh, but not so dark that the specular highlight is all you see — the scene's 500-brightness ambient
// will otherwise render a near-black albedo as a grey mirror.
const FLESH_DEEP: vec3<f32> = vec3<f32>(0.048, 0.059, 0.051);
// The advancing margin of a real colony is paler than its mature centre — young hyphae, no pigment yet.
const FLESH_EDGE: vec3<f32> = vec3<f32>(0.092, 0.103, 0.087);
// Phosphorescence: a pale, sickly grey-green — the ONE place any colour survives, because a colourless glow
// is just a lamp. Desaturated less hard than the albedo (chroma 0.108 -> 0.044) so the veins still read as
// something alive lit from within. The camera is LDR (no HDR, no bloom) and the scene is brightly lit, so
// this must be bright enough to compete with the ambient fill yet stay under the tonemapper's clip.
const GLOW: vec3<f32> = vec3<f32>(0.238, 0.396, 0.323);
// Colour of the grazing-angle fuzz. Desaturated: it is light scattering off filament tips, not pigment.
const FUZZ: vec3<f32> = vec3<f32>(0.213, 0.237, 0.213);
// The fog's dim tint for remembered-but-unseen floor, matching `dungeon::FloorMaterials::dim`
// (0.28, 0.28, 0.36). The mold must dim with the ground it sits on; drawn at full brightness it ignores the
// fog's lighting state even while honouring its reveal state, and a remembered room glows through the dark.
const FOG_DIM: vec3<f32> = vec3<f32>(0.30, 0.30, 0.38);
// Mycelium is a dielectric felt, and a felt barely reflects. StandardMaterial defaults `reflectance` to 0.5
// (F0 = 0.04) which, under this scene's brightness-500 ambient, puts a specular sheet over the whole coat.
// THAT is the shine — not the roughness alone. Dropping F0 by ~6x is what finally kills the wet look.
const MOLD_REFLECTANCE: f32 = 0.08;


fn world_to_uv(world_xz: vec2<f32>) -> vec2<f32> {
    return (world_xz - mold.world_origin) / mold.world_extent;
}

// ── Fog state, straight from the control texture ───────────────────────────────────────────────────────
// `control.a` is the four-state substrate mask: 0 void · 0.33 floor never seen · 0.67 remembered · 1 visible.
// The compute chain thresholds it with `step()`; here we use a narrow `smoothstep` instead, because the
// control texture is one texel per dungeon CELL and a bare step would alias along the reveal boundary.
//
// Read from `control_tex`, NOT from the field's alpha. The field only advances on a sim tick, so a coverage
// baked into it lagged the fog by a whole tick period and the mat visibly arrived *after* the floor tile it
// sits on. `write_control` rewrites this texture every frame, so the reveal is exact.
//
// This is the one mold signal that is deliberately NOT rate-limited: it is caused by the player walking into
// a room, not by the mold acting on its own, and it must land on the same frame as the floor's own material
// swap in `fog::apply_floor_fog`.
fn is_explored(a: f32) -> f32 {
    return smoothstep(0.45, 0.55, a);
}
fn is_visible(a: f32) -> f32 {
    return smoothstep(0.85, 0.95, a);
}

// `field_tex` is the one buffer in this pipeline stored `Rgba16Float` (see `mycelia/mod.rs::DISPLAY_FORMAT`)
// — every other mold texture this shader reads is a normalized 8-bit format, which cannot hold NaN/Inf by
// construction. `mycelia_sim.wgsl` clamps U/V to [0,1] every tick, but WGSL's `clamp`/`min`/`max` are not
// specified to filter a NaN INPUT on every backend (`mycelia_wall.wgsl` already carries a documented
// naga/Metal undefined-behavior workaround for a different intrinsic), so a transient NaN from the reaction
// step could in principle survive the sim's own clamp and land here. Sanitizing at this last consumption
// point means a numerically unstable sim frame reads as "no mold here" instead of a raw NaN pixel (which
// upstream tonemapping/exposure can turn into a solid, screen-filling wrong color).
//
// The test must be a BIT-PATTERN test, not `x != x`. `x != x` is the textbook IEEE 754 NaN check, but it
// does not survive this toolchain: naga lowers a WGSL float `!=` to SPIR-V `OpFOrdNotEqual` (an ORDERED
// compare, false whenever either operand is NaN — `naga/src/back/spv/block.rs`, which reserves the unordered
// `FUnordNotEqual` for float→bool casts), so `select(x, 0.0, x != x)` folds to the identity on Vulkan; the
// Metal backend builds `MTLCompileOptions` with fast-math left on, which licenses the same fold. An
// all-ones exponent is NaN or Inf by definition, and `bitcast` is opaque to finite-math assumptions. Inf is
// deliberately caught alongside NaN: it is just as fatal once it reaches tonemapping.
fn no_nan(x: f32) -> f32 {
    return select(x, 0.0, (bitcast<u32>(x) & 0x7f800000u) == 0x7f800000u);
}

// ── Control-texture taps: one texel is exactly one dungeon cell ────────────────────────────────────────
// `CONTROL_SIZE` (192) equals `WORLD_EXTENT` (192) at `TILE_SIZE` 1.0, so control-texel space IS cell space.
fn control_dims() -> vec2<f32> {
    return vec2<f32>(textureDimensions(control_tex, 0));
}

// Substrate of the cell containing `uv`, read with NEAREST semantics.
//
// `field::control_texture` leaves `Image::sampler` at `ImageSampler::Default`, which `DefaultPlugins`
// resolves to LINEAR filtering, so `textureSampleLevel` returns a bilinear blend of the four surrounding
// cells rather than this cell's value. Thresholding that blend does NOT split floor from void: across the
// boundary between a floor cell (alpha F >= 0.33) and the void cell beside it the reconstruction is the ramp
// F·(1 − d/Δ), so a `step(0.1, …)` on it puts the iso-contour at d = Δ·(1 − 0.1/F) — for a visible cell
// (F = 1) that is 0.9 cells, i.e. ~0.4 world units PAST the boundary, against a wall slab only
// WALL_THICKNESS (0.14) thick. That dilated mask is what let the coat paint into solid rock ("mold seeps
// under the walls"). `textureLoad` bypasses the sampler entirely, so the four-state mask stays four-state.
fn substrate_at_cell(uv: vec2<f32>) -> f32 {
    let dims = control_dims();
    let coord = vec2<i32>(clamp(floor(uv * dims), vec2<f32>(0.0), dims - 1.0));
    return textureLoad(control_tex, coord, 0).a;
}

// The domain-warped tap position, fenced inside the cell `uv` belongs to.
//
// The warp exists to stop the reveal edge snapping to the cell grid, but `reveal_warp_amp` (0.012 UV over a
// 192-unit extent) reaches ~1.15 world units — far enough to hop a single-cell-thick wall and read the NEXT
// room's fog state. Clamping the tap to its own cell's footprint makes that structurally impossible rather
// than merely unlikely: bilinear reconstruction at a point inside a texel only ever mixes that texel with
// its IMMEDIATE neighbours, and the floor on the far side of a wall is two cells away, so it carries zero
// weight regardless of warp direction. The edge still wanders sub-cell, which is what breaks up the grid.
fn warped_tap(uv: vec2<f32>, warp: vec2<f32>) -> vec2<f32> {
    let dims = control_dims();
    let lo = floor(uv * dims) / dims;
    return clamp(uv + warp * mold.reveal_warp_amp, lo, lo + 1.0 / dims);
}

/// How physically thick the mold is at `uv`, in arbitrary units. Drives the surface normal.
fn thickness(uv: vec2<f32>) -> f32 {
    let f = textureSampleLevel(field_tex, field_samp, uv, 0.0);
    let veins = smoothstep(mold.vein_lo, mold.vein_hi, no_nan(f.r));
    let bio = smoothstep(0.10, 0.35, no_nan(f.g));
    // Veins are raised cords; biomass is a swollen sheet; mold piles up in the wall corner.
    return bio + veins * 0.55 + no_nan(f.b) * 0.30;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let world_xz = in.world_position.xz;
    let uv = world_to_uv(world_xz);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        discard;
    }

    let f = textureSampleLevel(field_tex, field_samp, uv, 0.0);
    let f_r = no_nan(f.r);
    let veins = smoothstep(mold.vein_lo, mold.vein_hi, f_r);
    let sheen = smoothstep(mold.vein_lo * 0.17, mold.vein_lo, f_r);
    let bio = smoothstep(0.10, 0.35, no_nan(f.g));
    let contact = no_nan(f.b);
    // Coverage gates drawing (explored floor only, or the coat traces the map through the fog); `lit` dims
    // the mold to match the fogged floor under it. Both are player-caused and therefore instantaneous.
    // The control texture is one texel per dungeon CELL, so tapping it at the bare `uv` snaps the coat's
    // reveal/coverage edge to the cell grid — the coat reads as square tiles. Domain-warp the tap with the
    // same world-space fbm the colony margin uses (Lagae et al. 2010, procedural-noise survey) so the edge
    // wanders off the grid. Kept small (~1-2 cells) so mold never bleeds across a wall.
    let warp = vec2<f32>(
        fbm4(world_xz * mold.reveal_warp_scale),
        fbm4(world_xz * mold.reveal_warp_scale + vec2<f32>(31.4, 17.7)),
    ) - 0.5;
    // `warped_tap` fences the warp inside this fragment's own cell, so the tap can never reach across a
    // wall (see its comment). That handles the "which room's fog state" half of the problem.
    let substrate_warped = textureSampleLevel(control_tex, control_samp, warped_tap(uv, warp), 0.0).a;
    // The remaining half: this overlay is a SINGLE quad spanning the whole world, so it covers solid rock
    // too. Kill the coat wherever the fragment's own cell is void. Void is exactly 0.0 and every floor state
    // is >= 0.33 (see `control.rs::write_control`), so 0.1 is a clean binary split — but only against the
    // NEAREST tap. Read through the linear sampler this same threshold dilates the floor mask ~0.4 world
    // units into the rock; that is the bug, not the constant.
    let is_floor_direct = step(0.1, substrate_at_cell(uv));
    let substrate = substrate_warped * is_floor_direct;
    let coverage = is_explored(substrate);
    let lit = mix(FOG_DIM, vec3<f32>(1.0), is_visible(substrate));

    // ── Thickness gradient → the filament frame ───────────────────────────────────────────────────────
    // The overlay is a horizontal plane, so its tangent frame is trivial: +uv.x is +world.x, +uv.y is
    // +world.z, geometric normal is +Y. Building a perturbed normal straight in world space is exact.
    let texel = 1.0 / mold.field_res;
    let hx = thickness(uv + vec2<f32>(texel.x, 0.0)) - thickness(uv - vec2<f32>(texel.x, 0.0));
    let hz = thickness(uv + vec2<f32>(0.0, texel.y)) - thickness(uv - vec2<f32>(0.0, texel.y));

    // Hyphae grow ALONG a vein, i.e. along the thickness field's iso-contours — perpendicular to its
    // gradient. Where the field is flat the gradient is meaningless, so fall back to a fixed axis rather
    // than normalising a zero vector into NaN.
    let grad = vec2<f32>(hx, hz);
    let glen = length(grad);
    var along = vec2<f32>(1.0, 0.0);
    if (glen > 1e-5) {
        along = vec2<f32>(-hz, hx) / glen;
    }
    let across = vec2<f32>(-along.y, along.x);

    // Sample noise in that frame, compressed along the strand and stretched across it: slow variation down
    // a filament, fast variation between neighbouring filaments. That anisotropy is what makes it read as
    // fibres rather than isotropic lumps.
    let fiber_uv = vec2<f32>(dot(world_xz, along) * 0.22, dot(world_xz, across)) * mold.fiber_scale;
    let strand = fbm4(fiber_uv);

    // ── Coat, with a dendritic margin ─────────────────────────────────────────────────────────────────
    let body = clamp(max(max(veins * 0.85, bio * 0.55), sheen * 0.14) + contact * bio * 0.35, 0.0, 1.0);

    // Break the outer contour with low-frequency fbm so the colony edge is feathery and dendritic rather
    // than a smooth iso-contour (a meniscus, i.e. a puddle).
    //
    // The noise must ERODE AND DILATE AN EXISTING EDGE, never conjure coat out of nothing: added
    // unconditionally it lifts bare carpet — where `body` is exactly 0 — to as much as +margin_roughness/2,
    // far above the discard threshold, hazing the whole floor with phantom mold. `gate` is zero wherever
    // there is no mold to feather, so bare floor stays bare and only the fringe (0 < body < 0.12) moves.
    let lobes = fbm4(world_xz * mold.fiber_scale * 0.25);
    let gate = smoothstep(0.0, 0.12, body);
    let coat = clamp(body + (lobes - 0.5) * mold.margin_roughness * gate, 0.0, 1.0)
             * coverage * mold.intensity;

    // Bare carpet: nothing to draw. Discarding (rather than emitting alpha 0) skips the lighting work and
    // avoids a full-footprint transparent quad blending over the whole floor every frame.
    if (coat < 0.004) {
        discard;
    }

    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // ── Surface ───────────────────────────────────────────────────────────────────────────────────────
    // Pale at the growing fringe, dark in the mature centre.
    let albedo = mix(FLESH_EDGE, FLESH_DEEP, smoothstep(0.05, 0.65, body)) * lit;
    pbr_input.material.base_color = vec4<f32>(albedo, coat);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    // Matte felt everywhere; wet ONLY in the vein cores. Squaring `veins` keeps that wet band narrow — this
    // single line is most of the difference between "mycelium" and "spill".
    pbr_input.material.perceptual_roughness = mix(0.96, mold.wet_roughness, veins * veins);
    pbr_input.material.metallic = 0.0;
    pbr_input.material.reflectance = vec3<f32>(MOLD_REFLECTANCE);

    // ── Normal ────────────────────────────────────────────────────────────────────────────────────────
    // Low-frequency lumps from the simulated thickness, high-frequency ridges from the filament noise.
    // The field is only ~5.3 texels per tile, so on its own it can only ever produce rolling liquid lobes.
    let ridge = (strand - 0.5) * mold.fiber_strength * coat;
    let bumpy = normalize(vec3<f32>(
        -hx * mold.normal_strength + across.x * ridge,
        1.0,
        -hz * mold.normal_strength + across.y * ridge,
    ));
    pbr_input.N = normalize(mix(pbr_input.world_normal, bumpy, coat));

    // ── Occlusion ─────────────────────────────────────────────────────────────────────────────────────
    // The gaps between filaments are shadowed by the filaments around them. `diffuse_occlusion` is what
    // gates the ambient term (bevy_pbr `pbr_functions.wgsl`), and the ambient here is a bright *uniform*
    // fill that would otherwise wash the whole structure flat.
    let cavity = 1.0 - strand;
    let ao = clamp(1.0 - mold.ao_strength * cavity * coat, 0.0, 1.0);
    pbr_input.diffuse_occlusion = vec3<f32>(ao);
    // Occlude the specular far harder than the diffuse: light that reaches deep between filaments comes back
    // scattered, not mirrored.
    pbr_input.specular_occlusion = ao * (1.0 - 0.8 * coat);

    // ── Emission: bioluminescence + fuzz ──────────────────────────────────────────────────────────────
    // The mold conceals its glow under a direct gaze — brightest in the dark. `control.g` is rate-limited on
    // the CPU to the slow-change window (see `control.rs`), so the flinch is a slow bleed rather than a
    // pulse: you never catch the mold reacting, you only notice later that it has. The structural retreat is
    // the same signal steering the agents away.
    let light = textureSampleLevel(control_tex, control_samp, uv, 0.0).g;
    let conceal = 1.0 - 0.7 * light;
    var emissive = GLOW * veins * conceal * mold.glow_gain * lit;

    // Grazing-angle fuzz. A real fuzz/sheen lobe (e.g. Estévez & Kulla, "Production Friendly Microfacet
    // Sheen BRDF", Sony Imageworks 2017) is not available: bevy 0.19's StandardMaterial has no sheen layer.
    // This is a cheap Fresnel-shaped APPROXIMATION of it, folded into emissive *before* lighting so it goes
    // through exposure and tonemapping with everything else — added afterwards it clips straight to white
    // on this LDR, bloom-free camera.
    let ndv = clamp(dot(pbr_input.N, pbr_input.V), 0.0, 1.0);
    emissive += FUZZ * pow(1.0 - ndv, 5.0) * mold.sheen_strength * coat * lit;
    pbr_input.material.emissive = vec4<f32>(emissive, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
