//! Fruit bodies — the mold reproducing.
//!
//! Where the mat has grown thick and eaten its substrate, in the dark, a death cap erupts: a single
//! `death_cap_growth.glb` mesh blended from sealed egg to adult across six glTF morph targets under one
//! `growth: f32`. It grows in real time, and it grows **too slowly to see**.
//!
//! # The biology, and where it already lives in the fields
//!
//! Gray-Scott integrates `U` (substrate) and `V` (biomass) via the autocatalytic `U + 2V → 3V`. The pin gate
//! `V > v_fruit && U < u_exhausted` is thick mat over spent substrate, and that is what a real primary hyphal
//! knot forms on: "In the dark, upon nutritional depletion, single hyphae locally undergo intense branching to
//! form microscopic primary hyphal knots" (Liu et al. 2006, Genetics 172:873, 10.1534/genetics.105.045542).
//! Nutrient exhaustion is an **initiation** cue there. Nitrogen starvation is separately implicated in
//! **maturation** — the transcriptome of *Hypsizygus marmoreus* puts it among the strongest such cues (Zhang
//! et al. 2015, PLoS ONE 10:e0123025, 10.1371/journal.pone.0123025, which hedges: "might be one of the most
//! important factors"). Either way the trigger needed no new state; it was already integrated on the GPU.
//!
//! Two more rules, both real phenomena rather than engineering conveniences:
//!
//! - **Dark-dependent initiation, light-induced rupture.** In *Coprinopsis cinerea*, primary hyphal knots
//!   form **in the dark**, and "following a light signal, radial growth of primary hyphal knots and hyphal
//!   interaction lead to the formation of compact hyphal aggregates, secondary hyphal knots" (Liu et al.
//!   2006, ibid.; corroborated by Kües 2000, MMBR 64:316, 10.1128/mmbr.64.2.316-353.2000: knots "are formed
//!   in the dark", are "repressed by illumination with blue light", and "continuation of development toward
//!   fruiting-body formation is light dependent"). The fog is our light proxy, as it already is for the
//!   mold's photophobia. So a pin commits only in a cell **no unit can currently see**, and the universal
//!   veil only ruptures once a unit **looks at it**. Mapping that light step onto *veil rupture* rather than
//!   onto the knot→secondary-knot transition is ours; the phenomenon is theirs. The mold hides from you; its
//!   fruiting body wants an audience.
//! - **Primordium abortion.** A body whose local `V` collapses below `maintain_v` runs its growth clock
//!   **backwards** until it is reabsorbed — not a fallback branch, the same ODE with a negative sign.
//!
//!   `pin_min_spacing` is a **geometric stand-in, not a cited mechanism.** This module used to attribute
//!   "neighbours compete for translocated nutrient" to Kües & Navarro-González 2015 (Fungal Biol. Rev. 29:63,
//!   10.1016/j.fbr.2015.05.001). That review is paywalled and its green-OA copy 403s, so the attribution was
//!   never checked; the two sources above that *are* readable — Liu 2006 and Kües 2000 — say nothing about
//!   primordium abortion or inter-primordium nutrient competition. Do not restore the claim without reading
//!   the review. What the spacing *is* defensible as: local activation with long-range inhibition, where the
//!   inhibition takes Oster's "movement away from a local zone of influence" form (Jones 2010,
//!   10.1162/artl.2010.16.2.16202 — already this module's Physarum reference, and in the home-still corpus).
//!
//! # The speed limit
//!
//! Every autonomous motion here — the egg rising out of the mat, the veil tearing, the cap flattening — is
//! held below the human motion-detection threshold by [`super::perceptual`]. See that module for the
//! psychophysics. Being eaten or crushed is deliberately exempt: that is meant to be seen.
//!
//! # The determinism firewall
//!
//! This module opens the mold's **only** GPU→CPU channel: a readback of the coarse biomass grid that
//! `pin_scan` writes. That makes fruit-body positions non-reproducible across hardware, the same
//! non-determinism class as the Avian physics and FX layers. It is safe because a `FruitBody` carries a
//! `Transform` but **never a `Health`**, and `sim_harness::snapshot_hash` queries `(&Transform, &Health)` —
//! so fruit bodies are excluded from the replay oracle for exactly the reason `gore::GibChunk` is. Every
//! system here is on `Update`, never `FixedUpdate`.
//!
//! # Food (the seam, not built here)
//!
//! Fruit bodies are first-class world entities so the ecosystem can eat them: crabs already forage the
//! `ai::field::FieldId::MEAT` stigmergy field against an accumulating `DriveId::HUNGER`, scored by
//! `Mode::SeekMeat`/`TargetKind::MeatHotspot`, and haul `gore::Carryable` back to nests. A mature body
//! splatting into that field would be foraged with no AI changes at all. [`FruitBody::consume`] is the hook.
//! Amatoxins live in the **hymenophore and cap**, and are scarce in the volva: gills 13.38 > pileus 10.16 >
//! stipe 9.99 >> volva 2.85 mg/g dry matter (Enjalbert et al. 1999, C. R. Acad. Sci. III 322:855,
//! 10.1016/s0764-4469(00)86651-2, as tabulated by Vetter 2023, 10.3390/molecules28155932). Gills and cap both
//! appear only when the veil tears, so [`FruitBody::amatoxin`] is a function of `growth` alone — a body is
//! only poisonous once it has a cap and gills to hold the toxin.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::render::gpu_readback::ReadbackComplete;
use bevy::time::Real;

use crate::dungeon::Dungeon;
use crate::fog::FogGrid;
use crate::util::hash01_u32;

use super::material::{MoldFruitExt, MoldFruitMaterial};
use super::control::{self, MoldControlImage};
use super::perceptual::{
    bend_profile, growth_rate, radius_slice_height, stage_weights, v_max, ADULT_HEIGHT_M,
    BENDABLE_MIN_PROFILE, CAP_RADIUS_M, EGG_HEIGHT_M, MAX_BEND_M, MAX_TILT, MIN_APPEARANCE_RAMP_SECS,
    RADIUS_PROFILE, VEIL_RUPTURE_T, VOLVA_RADIUS_M,
};
use super::{MoldImages, MyceliaConfig, COARSE_SIZE, WORLD_EXTENT, WORLD_ORIGIN};

/// The death cap, as six morph targets over one `growth` scalar. No animation clips ship with it.
const DEATH_CAP_GLB: &str = "death_cap/death_cap_growth.glb";

/// How many morph targets the mesh must expose. If the asset is regenerated with a different `STAGES` list
/// this stops matching, and [`super::perceptual::STAGE_MAX_DISP`] — from which the entire speed limit is
/// derived — would silently be describing a different mesh. So a mismatch is a hard error, not a warning.
const MORPH_TARGET_COUNT: usize = 6;

/// Marks the entity that carries the [`bevy::render::gpu_readback::Readback`] for the coarse biomass grid.
#[derive(Component)]
pub struct CoarseReadback;

/// A death cap. Growth state only; the geometry lives in the glTF scene spawned beneath this entity.
///
/// **Deliberately carries no `Health`.** `sim_harness::snapshot_hash` queries `(&Transform, &Health)`, and
/// its doc states the invariant: every gameplay actor carries `Health`, gib chunks do not. A fruit body is
/// in the second class. Eating one drives [`FruitBody::consume`], never damage.
#[derive(Component, Debug, Clone)]
pub struct FruitBody {
    /// Morph blend parameter, `0` = sealed egg, `1` = adult. Drives the six morph-target weights.
    pub growth: f32,
    /// How far the body has emerged from the mat, `0` = fully sunk, `1` = standing on the floor. A 4.85 cm
    /// egg *appearing* would be an enormous change signal, so it rises out of the substrate at the same
    /// speed limit everything else obeys — which is also what a primary hyphal knot really does.
    pub rise: f32,
    /// Uniform scale applied to the native 13.9 cm mesh.
    pub scale: f32,
    /// Dungeon cell this body stands in, for the fog (light) lookups.
    pub cell: IVec2,
    /// Latched once a unit has *seen* this body. The universal veil cannot rupture until then (Liu et al.
    /// 2006): a mushroom left in a dark room grows to a closed button and waits for an audience.
    pub veil_triggered: bool,
    /// Colour-transition parameter, chasing `growth` but rate-limited so no albedo shift ever completes
    /// faster than [`MIN_APPEARANCE_RAMP_SECS`]. Motion has its own, far tighter budget; this bounds the
    /// *non-moving* half of the change signal (Simons, Franconeri & Reimer 2000, 10.1068/p3104).
    pub tint: f32,
    /// Apex deflection of the stipe, in the body's **own object space** and in native-scale metres, so the
    /// vertex shader can apply it without knowing the entity's yaw or scale. Fixed at spawn; a bent stem
    /// does not un-bend.
    ///
    /// Two things go into it, and both are real tropisms (Moore 1991, 10.1111/j.1469-8137.1991.tb00940.x,
    /// which ranks them thigmotropism < gravitropism < anemotropism < phototropism):
    /// **thigmotropism** — the stem curves away from what it touches, which is what keeps a 22 cm cap out
    /// of a wall its 9 cm volva already clears — and a per-body random lean, because no two stems in a
    /// flush grow alike.
    pub bend: Vec2,
    /// The body's **growth angle**, as a slope in its own object space: horizontal drift per unit of height.
    /// A *linear* term, so it leans the whole stem from the ground up while leaving the volva seated at
    /// `y = 0`. Distinct from [`FruitBody::bend`], which is a *curve* confined to the stipe's upper third —
    /// together they are a stem that grew off-plumb and then turned away from what it touched.
    ///
    /// The youngest fruit-body initials grow perpendicular to their substratum, and negative gravitropism
    /// only asserts itself later (Moore 1991, 10.1111/j.1469-8137.1991.tb00940.x). No stem ends up plumb.
    pub tilt: Vec2,
}

impl FruitBody {
    /// Food value available right now, in arbitrary units. An egg is mostly volva and water; a mature body
    /// is worth eating. Scales with volume, hence the cube.
    pub fn energy(&self) -> f32 {
        self.growth * self.growth * self.growth * self.scale * self.scale * self.scale
    }

    /// Amatoxin load, `0..1`. Zero until the cap has expanded past [`VEIL_RUPTURE_T`], because the toxin is
    /// carried by the tissues that appear with the cap: gills (13.38 mg/g DM) and pileus (10.16) hold it, the
    /// stipe about as much as the pileus (9.99), and the volva almost none (2.85) — Enjalbert et al. 1999,
    /// 10.1016/s0764-4469(00)86651-2, tabulated by Vetter 2023, 10.3390/molecules28155932.
    ///
    /// Note the `COLOR_0` part mask does **not** partition the body by toxin: it groups gills with the stipe
    /// and annulus under `G` (flesh), and the gills are the *most* toxic tissue of all. The mask is anatomy;
    /// this function is chemistry. They agree only on the volva being inert and the egg being harmless.
    pub fn amatoxin(&self) -> f32 {
        ((self.growth - VEIL_RUPTURE_T) / (1.0 - VEIL_RUPTURE_T)).clamp(0.0, 1.0)
    }

    /// Take a bite. Runs the growth clock backwards along the same path abortion uses; at `growth <= 0` the
    /// body is reabsorbed and despawned by [`grow_fruit_bodies`].
    ///
    /// **Exempt from the perceptual speed limit** — unlike autonomous growth, being eaten is meant to be
    /// seen. This is the same distinction the mold already draws: it hides from a gaze, but visibly scatters
    /// from footsteps.
    pub fn consume(&mut self, bite: f32) {
        self.growth = (self.growth - bite).max(0.0);
    }
}

/// Cached handle to the per-body extended material, so `tint_fruit_bodies` can find it without a descendant
/// walk. One material per body: they are few (`max_fruit_bodies`), and each needs its own `tint`.
#[derive(Component)]
struct FruitMaterial(Handle<MoldFruitMaterial>);

/// Marks a fruit-body mesh whose `StandardMaterial` has already been swapped, so the coating system never
/// reprocesses it. Mirrors `MoldCoated` for furniture.
#[derive(Component)]
struct FruitCoated;

/// The mold's coarse biomass grid, read back from the GPU once per sim tick.
///
/// One entry per `COARSE_SIZE²` cell: `(max V in the block, U at that same texel, texel x, texel y)`.
/// Written by the `pin_scan` compute pass — see `mycelia_sim.wgsl`.
#[derive(Resource, Default)]
pub struct MoldCoarse {
    /// `COARSE_SIZE * COARSE_SIZE` entries. Empty until the first readback lands.
    cells: Vec<[f32; 4]>,
    /// Bumped each time a readback lands. `pin_fruit_bodies` rescans all `COARSE_SIZE²` cells, which is only
    /// worth doing when the contents actually changed — the GPU rewrites this buffer at `sim_hz`, not per frame.
    generation: u64,
}

impl MoldCoarse {
    /// How many readbacks have landed. **Not** the same as ticks dispatched until [`Self::has_run`] is
    /// true: `bevy_render` copies the buffer for every entity carrying a `Readback`, whether or not the
    /// compute chain ran, so early frames (while the pipelines are still compiling) bump this against a
    /// buffer nothing has written. `advance_mold_time` takes a baseline at the first live readback.
    pub fn ticks_elapsed(&self) -> u64 {
        self.generation
    }

    /// Has the compute chain ever actually dispatched?
    ///
    /// The coarse buffer is zero-initialised, and `pin_scan` writes each cell's `(V, U, x, y)` with the
    /// texel coordinates `x, y` of its block — unconditionally, even where the block is solid rock. Those
    /// coordinates are non-zero for every cell but the first. So a buffer whose texel coordinates are all
    /// zero has never been written by the GPU, and one where any is non-zero has. That is a fact about the
    /// data rather than about the render world's private `MoldState::ready`, which the main world cannot
    /// see.
    pub fn has_run(&self) -> bool {
        self.cells.iter().any(|c| c[2] != 0.0 || c[3] != 0.0)
    }

    /// Biomass `V` at a world position, or `0.0` before the first readback. Coarse: one sample per
    /// `WORLD_EXTENT / COARSE_SIZE` (1.5 world units) block.
    fn v_at(&self, world_xz: Vec2) -> f32 {
        self.cell_at(world_xz).map_or(0.0, |c| c[0])
    }

    fn cell_at(&self, world_xz: Vec2) -> Option<&[f32; 4]> {
        if self.cells.is_empty() {
            return None;
        }
        let uv = (world_xz - WORLD_ORIGIN) / WORLD_EXTENT;
        if uv.x < 0.0 || uv.x >= 1.0 || uv.y < 0.0 || uv.y >= 1.0 {
            return None;
        }
        let x = (uv.x * COARSE_SIZE as f32) as usize;
        let y = (uv.y * COARSE_SIZE as f32) as usize;
        self.cells.get(y * COARSE_SIZE as usize + x)
    }
}

/// How long each coarse cell has continuously held the pin condition. Keyed by coarse index, so iteration
/// for pinning is over the *grid* (deterministic order), never over this map.
#[derive(Resource, Default)]
pub struct PinDwell(HashMap<usize, f32>);

/// The loaded death cap scene. Loaded once at startup; `WorldAssetRoot` instantiates it asynchronously.
#[derive(Resource)]
pub struct DeathCapScene(Handle<WorldAsset>);

impl DeathCapScene {
    /// A clone of the scene handle, for anything that spawns a body outside the normal pin path.
    pub fn handle(&self) -> Handle<WorldAsset> {
        self.0.clone()
    }
}

pub(super) fn build(app: &mut App) {
    app.init_resource::<MoldCoarse>()
        .init_resource::<PinDwell>()
        .add_plugins(MaterialPlugin::<MoldFruitMaterial>::default())
        .add_systems(Startup, load_death_cap)
        // Cosmetic, and reads a GPU readback: `Update` only, never `FixedUpdate`. See the module header.
        .add_systems(
            Update,
            (
                pin_fruit_bodies,
                grow_fruit_bodies,
                drive_morph_weights,
                coat_fruit_bodies,
                tint_fruit_bodies,
            )
                .chain(),
        );
}

fn load_death_cap(mut commands: Commands, assets: Res<AssetServer>) {
    let scene: Handle<WorldAsset> = assets.load(GltfAssetLabel::Scene(0).from_asset(DEATH_CAP_GLB));
    commands.insert_resource(DeathCapScene(scene));
}

/// Observer for the coarse-grid readback. Decodes `vec4<f32>` little-endian into [`MoldCoarse`].
///
/// A size mismatch means the shader's `coarse_res` and [`COARSE_SIZE`] have diverged, which would silently
/// place mushrooms at the wrong world positions. Returning `Err` here surfaces as a loud panic through
/// Bevy's default error handler, rather than a plausible lie.
pub(super) fn receive_coarse(
    trigger: On<ReadbackComplete>,
    mut coarse: ResMut<MoldCoarse>,
) -> Result<(), BevyError> {
    let bytes = &trigger.event().data;
    let expected = (COARSE_SIZE * COARSE_SIZE) as usize * 4 * size_of::<f32>();
    if bytes.len() != expected {
        return Err(format!(
            "mycelia: coarse readback is {} bytes, expected {expected} ({COARSE_SIZE}² × vec4<f32>); the \
             shader's coarse_res and COARSE_SIZE have diverged",
            bytes.len()
        )
        .into());
    }

    let n = (COARSE_SIZE * COARSE_SIZE) as usize;
    coarse.cells.clear();
    coarse.cells.reserve(n);
    for i in 0..n {
        let mut cell = [0.0f32; 4];
        for (c, slot) in cell.iter_mut().enumerate() {
            let o = (i * 4 + c) * 4;
            // Indices are bounded by the length check above, so the slice cannot be short.
            let quad: [u8; 4] = bytes[o..o + 4].try_into().map_err(|_| "short coarse readback")?;
            *slot = f32::from_le_bytes(quad);
        }
        coarse.cells.push(cell);
    }
    coarse.generation = coarse.generation.wrapping_add(1);
    Ok(())
}

/// World XZ of the centre of a field texel.
fn field_texel_to_world(texel: Vec2, field_size: f32) -> Vec2 {
    WORLD_ORIGIN + (texel + Vec2::splat(0.5)) / field_size * WORLD_EXTENT
}


/// How many rays the wall probe casts around a candidate site.
const PROBE_RAYS: usize = 24;
/// Step along each probe ray, world units. Well under `WALL_THICKNESS`, so a slab can never be stepped over.
const PROBE_STEP: f32 = 0.01;
/// Angular samples around the body when testing its silhouette against the walls.
const SILHOUETTE_ANGLES: usize = 16;
/// How far a buried silhouette sample will march before giving up, world units.
const MARCH_MAX: f32 = 0.6;
/// Clearance left between the body and the wall face it was pushed off, world units. Zero would leave the
/// cap's rim exactly coplanar with the slab, which reads as clipping the moment anything is interpolated.
const WALL_MARGIN: f32 = 0.03;
/// How many times `plan_body` will back the base off and re-solve before giving the site up.
const RESEAT_ATTEMPTS: usize = 4;
/// Peak random lean, as a fraction of adult height — the natural crookedness of a stem with nothing to
/// avoid. Small: real stipes are near-vertical (negatively gravitropic), just never perfectly so.
const LEAN_FRACTION: f32 = 0.18;
/// Per-body size jitter, ±this fraction of `body_scale`. A flush is not a set of clones.
const SCALE_JITTER: f32 = 0.18;

/// Radius of the disc, in **native-scale metres**, that any admissible adult pose can reach around its base.
///
/// The cap is not the widest thing about a fruit body: `tilt` carries the whole silhouette sideways by up to
/// `MAX_TILT * ADULT_HEIGHT_M`, and the stem's curve adds up to `MAX_BEND_M` on top of that (before the solve
/// runs, the curve is just the random `lean`, bounded by `LEAN_FRACTION * ADULT_HEIGHT_M`). A wall probe that
/// only reaches `CAP_RADIUS_M` is blind to slabs the body's own lean can drive it into.
fn pose_envelope_m() -> f32 {
    let lean_max = LEAN_FRACTION * ADULT_HEIGHT_M;
    CAP_RADIUS_M + MAX_TILT * ADULT_HEIGHT_M + MAX_BEND_M.max(lean_max)
}

/// Everything a body's pose needs, worked out once at spawn.
pub struct BodyPlan {
    /// Where the volva actually sits, world XZ.
    pub base: Vec2,
    /// Apex deflection of the curving stem, world XZ, native-scale metres (i.e. pre-`scale`).
    pub bend: Vec2,
    /// Growth angle as a slope, world XZ: horizontal drift per unit height.
    pub tilt: Vec2,
}

/// Unit direction away from the solids within `reach` of `site`, weighted by how deeply each intrudes.
/// `Vec2::ZERO` when nothing solid is in reach, or when opposing slabs cancel exactly.
fn wall_escape(dungeon: &Dungeon, site: Vec2, reach: f32) -> Vec2 {
    let mut push = Vec2::ZERO;
    for i in 0..PROBE_RAYS {
        let dir = Vec2::from_angle(std::f32::consts::TAU * (i as f32) / (PROBE_RAYS as f32));
        let mut r = PROBE_STEP;
        while r <= reach {
            if control::solid_at_world(dungeon, site + dir * r) {
                // Weight by intrusion, so a slab under the stipe steers harder than one grazing the rim.
                push -= dir * (reach - r);
                break;
            }
            r += PROBE_STEP;
        }
    }
    push.normalize_or_zero()
}

/// How far `p` must travel along `away` to leave solid matter and gain [`WALL_MARGIN`]. Zero if it is
/// already clear.
fn march_out(dungeon: &Dungeon, p: Vec2, away: Vec2) -> f32 {
    if away == Vec2::ZERO {
        return 0.0;
    }
    let mut d = 0.0;
    while d <= MARCH_MAX {
        if !control::solid_at_world(dungeon, p + away * d) {
            // Keep going until the margin is also clear, so the rim never rests on the face.
            let mut m = d;
            while m <= d + WALL_MARGIN {
                if control::solid_at_world(dungeon, p + away * m) {
                    break;
                }
                m += PROBE_STEP;
            }
            if m > d + WALL_MARGIN {
                return d + WALL_MARGIN;
            }
        }
        d += PROBE_STEP;
    }
    MARCH_MAX
}

/// Deepest push, along `away`, needed by any silhouette sample in the given profile band.
///
/// Samples the **adult** silhouette (`RADIUS_PROFILE`) — the widest the body will ever be — displaced by the
/// pose it is being asked to hold. `select` picks which bands to consider: the rings a bend can move, or the
/// rings it cannot.
fn deepest_push(
    dungeon: &Dungeon,
    base: Vec2,
    scale: f32,
    tilt: Vec2,
    bend: Vec2,
    away: Vec2,
    bendable: bool,
) -> f32 {
    let mut worst = 0.0f32;
    for (i, &radius) in RADIUS_PROFILE.iter().enumerate() {
        let y = radius_slice_height(i);
        let p = bend_profile(y);
        if (p >= BENDABLE_MIN_PROFILE) != bendable {
            continue;
        }
        let centre = base + (tilt * y + bend * p) * scale;
        for a in 0..SILHOUETTE_ANGLES {
            let dir = Vec2::from_angle(std::f32::consts::TAU * (a as f32) / (SILHOUETTE_ANGLES as f32));
            let sample = centre + dir * (radius * scale);
            if control::solid_at_world(dungeon, sample) {
                let push = march_out(dungeon, sample, away);
                // A bendable ring is moved by `bend * p`, so it needs `push / p` of bend to clear.
                worst = worst.max(if bendable { push / p } else { push });
            }
        }
    }
    worst
}

/// Work out where a body near `site` can actually stand, how its stem must curve, and which way it leans.
/// Returns `None` when no pose seats the body clear of the geometry.
///
/// This is a **clearance solve against the real silhouette**, not a heuristic push, and it **verifies its own
/// answer** before returning it. Both halves were learned the hard way:
///
/// - An earlier version added the thigmotropic escape and the random lean together and clamped the sum. So a
///   lean pointing *into* the wall silently cancelled the escape; a corner was under-cleared by `1/√2`,
///   because one diagonal push serves two faces; and the clamp could truncate the very displacement it had
///   just computed. Here the lean and tilt are projected so they can never point into a wall, and the bend is
///   derived from how deep the adult cap's own rings actually sit inside solid matter.
/// - Even then, `pin_scan` can hand us a site *inside* a wall. It rejects texels whose dungeon **cell** is
///   not walkable, but a slab occupies the outer `WALL_THICKNESS` strip of a perfectly walkable floor cell.
///   Solving from inside rock produces a confident, wrong answer. So the pose is checked against
///   [`penetration`] and the base retried further out; a site that cannot host a body does not host one.
///
/// The base nudge is separate and purely geometric: a volva cannot occupy rock. It is bounded by the volva's
/// own radius, not the cap's, so a mushroom may still grow with its sac against the skirting — which is
/// precisely where the mold pools (`wall_affinity`). Only the cap is carried clear, and only by bending.
pub fn plan_body(dungeon: &Dungeon, site: Vec2, scale: f32, seed: u32) -> Option<BodyPlan> {
    let volva_r = VOLVA_RADIUS_M * scale;
    // Probe the whole disc any admissible pose can reach, not just the cap's own radius. `lean` and `tilt` are
    // drawn *after* this call and need `away` to be projected safely, so the probe must already have found any
    // slab they could swing the silhouette into. Reaching only `CAP_RADIUS_M` left a blind band — a wall just
    // outside it returned `away == ZERO`, which disabled the projection *and* the solve, and the body leaned
    // into the slab it never saw.
    let away = wall_escape(dungeon, site, pose_envelope_m() * scale + WALL_MARGIN);

    // The crookedness every stem has anyway, and the angle it grew at. Where a wall is near, strip the
    // component pointing into it: random variation must never eat into clearance.
    let project = |v: Vec2| {
        if away == Vec2::ZERO {
            v
        } else {
            v - away * v.dot(away).min(0.0)
        }
    };

    let lean_dir = Vec2::from_angle(hash01_u32(seed ^ 0xA1) * std::f32::consts::TAU);
    let lean = project(lean_dir * (hash01_u32(seed ^ 0xB2) * LEAN_FRACTION * ADULT_HEIGHT_M));

    let tilt_dir = Vec2::from_angle(hash01_u32(seed ^ 0xD4) * std::f32::consts::TAU);
    let tilt = project(tilt_dir * (hash01_u32(seed ^ 0xE5) * MAX_TILT));

    // No early-out for "nothing near". When the probe finds nothing, `away` is zero, so the loop below pushes
    // the base nowhere and bends the stem not at all — it lands on `bend = lean` and verifies it, which is
    // exactly the answer the old special case returned, minus the special case. One path, and every plan that
    // leaves this function has been checked against [`penetration`]. (The old branch returned *unverified*,
    // which is how a body could lean into a slab the cap-radius probe never saw.)

    // Retry from progressively further out. A site inside the slab band cannot be solved from where it sits,
    // and one wedged in a corner may need more room than a single pass concedes.
    let mut base = site;
    for _ in 0..RESEAT_ATTEMPTS {
        // 1. Seat the volva. Only the rings a bend cannot move constrain the base.
        base += away * deepest_push(dungeon, base, scale, tilt, lean, away, false);

        // 2. Curve the stem until the cap's rings are clear of the slab.
        let bend_push = deepest_push(dungeon, base, scale, tilt, lean, away, true);
        let mut bend = lean + away * (bend_push / scale);

        // 3. A stem bent past the ceiling reads as snapped. Spend the excess on moving the whole body —
        //    the base is free to travel, the volva having already been seated.
        let excess = bend.length() - MAX_BEND_M;
        if excess > 0.0 {
            bend = bend.clamp_length_max(MAX_BEND_M);
            base += away * (excess * scale);
        }

        let plan = BodyPlan { base, bend, tilt };
        if penetration(dungeon, &plan, scale) <= 0.0 {
            return Some(plan);
        }
        // Still buried. Back off by half a volva and try again.
        base += away * (volva_r * 0.5);
    }
    None
}

/// Deepest penetration of the adult body's silhouette into solid matter, world units. `0.0` means the pose
/// is entirely clear. Used by the fruit-body testbed to assert what a screenshot can only suggest.
///
/// Sampled **deliberately finer than [`deepest_push`] solves** — four times the angular resolution, and
/// heights interpolated between the profile's slices rather than taken at their centres. A checker that
/// samples exactly where the solver sampled can only ever agree with it; this one can catch a cap rim that
/// slips between two of the solver's rays.
pub fn penetration(dungeon: &Dungeon, plan: &BodyPlan, scale: f32) -> f32 {
    const CHECK_ANGLES: usize = SILHOUETTE_ANGLES * 4;
    const CHECK_HEIGHTS: usize = 64;

    let mut worst = 0.0f32;
    for h in 0..CHECK_HEIGHTS {
        // Interpolate the silhouette between profile slices, so the check is not blind between them.
        let t = (h as f32 + 0.5) / CHECK_HEIGHTS as f32 * (RADIUS_PROFILE.len() as f32) - 0.5;
        let i = (t.floor().max(0.0) as usize).min(RADIUS_PROFILE.len() - 1);
        let j = (i + 1).min(RADIUS_PROFILE.len() - 1);
        let f = (t - i as f32).clamp(0.0, 1.0);
        let radius = RADIUS_PROFILE[i] * (1.0 - f) + RADIUS_PROFILE[j] * f;
        let y = radius_slice_height(i) * (1.0 - f) + radius_slice_height(j) * f;

        let centre = plan.base + (plan.tilt * y + plan.bend * bend_profile(y)) * scale;
        for a in 0..CHECK_ANGLES {
            let dir = Vec2::from_angle(std::f32::consts::TAU * (a as f32) / (CHECK_ANGLES as f32));
            let sample = centre + dir * (radius * scale);
            if control::solid_at_world(dungeon, sample) {
                // How far in? March out along the local escape direction.
                let away = wall_escape(dungeon, sample, PROBE_STEP * 8.0);
                let out = if away == Vec2::ZERO { Vec2::Y } else { away };
                worst = worst.max(march_out(dungeon, sample, out));
            }
        }
    }
    worst
}

/// Commit primordia where the mat is thick, the substrate spent, and nothing is watching.
///
/// Iterates the coarse grid in index order, so the pin decision is a deterministic function of the grid's
/// contents rather than of query or atomic-append order.
#[allow(clippy::too_many_arguments)]
fn pin_fruit_bodies(
    mut commands: Commands,
    cfg: Res<MyceliaConfig>,
    coarse: Res<MoldCoarse>,
    dungeon: Res<Dungeon>,
    fog: Res<FogGrid>,
    scene: Res<DeathCapScene>,
    time: Res<Time<Real>>,
    mut dwell: ResMut<PinDwell>,
    mut last_gen: Local<u64>,
    mut last_scan: Local<Option<f32>>,
    bodies: Query<&Transform, With<FruitBody>>,
) {
    if coarse.cells.is_empty() {
        return;
    }
    // Rescanning all `COARSE_SIZE²` cells is only meaningful when a new readback has landed. The GPU rewrites
    // the coarse buffer at `sim_hz` (1.5 Hz), so at 120 fps an ungated scan repeats the same work ~80×.
    if coarse.generation == *last_gen {
        return;
    }
    *last_gen = coarse.generation;

    // `pin_dwell_secs` is real seconds. This system now fires once per readback rather than once per frame, so
    // a cell must be credited the whole interval since the previous scan — a frame delta would undercount it by
    // the same ~80×. Measuring the elapsed span (rather than assuming `1.0 / sim_hz`) also stays exact when
    // `advance_mold_time` drops a tick under load.
    let now = time.elapsed_secs();
    let dt = last_scan.map_or(0.0, |t| now - t);
    *last_scan = Some(now);

    let field_size = cfg.field_size as f32;
    let mut live = bodies.iter().count() as u32;
    // `commands.spawn` is deferred, so `bodies` cannot see a body pinned earlier in this same run. Without
    // this, two cells that ripen on the same pass both clear the spacing check and erupt on top of each other.
    let mut pinned_this_run: Vec<Vec3> = Vec::new();

    for (index, cell) in coarse.cells.iter().enumerate() {
        let (v, u) = (cell[0], cell[1]);
        let world_xz = field_texel_to_world(Vec2::new(cell[2], cell[3]), field_size);
        let world = Vec3::new(world_xz.x, 0.0, world_xz.y);
        let dcell = dungeon.world_to_cell(world);

        // Thick mat, spent substrate (Zhang 2015) — on real floor, in a room the squad has explored but is
        // not currently looking into. Dark-dependent knot initiation (Liu 2006), and it also means a
        // mushroom can never appear in a room whose floor tiles the fog has not yet revealed.
        let ripe = v > cfg.v_fruit && u < cfg.u_exhausted;
        let sheltered = dungeon.is_floor(dcell) && fog.seen_at(dcell) && !fog.visible_at(dcell);
        if !(ripe && sheltered) {
            dwell.0.remove(&index);
            continue;
        }

        let held = dwell.0.entry(index).or_insert(0.0);
        *held += dt;
        if *held < cfg.pin_dwell_secs {
            continue;
        }
        dwell.0.remove(&index);

        if live >= cfg.max_fruit_bodies {
            // A budget, loudly. Silently dropping the pin would read as "the mold stopped fruiting".
            debug!(
                "mycelia: fruit body budget of {} reached; not pinning at {:?}",
                cfg.max_fruit_bodies, dcell
            );
            continue;
        }

        // Primordium competition: neighbours starve each other out (Kües & Navarro-González 2015). Committed
        // bodies and the ones pinned earlier in this run are one population — `nearest_planar` ranks by
        // (distance bits, position bits), so chaining the two iterators cannot perturb the deterministic order.
        let candidates = bodies
            .iter()
            .map(|t| ((), t.translation))
            .chain(pinned_this_run.iter().map(|&p| ((), p)));
        let crowded = crate::util::nearest_planar(world, candidates)
            .is_some_and(|(_, _, d)| d < cfg.pin_min_spacing);
        if crowded {
            continue;
        }

        // Bounded by `COARSE_SIZE² = 16_384`, so the cast is lossless and the salts stay in range.
        let seed = index as u32;
        // Size varies across a flush. Growth time scales with it, since the speed limit bounds vertex
        // *speed* and a bigger body has further to travel — a large mushroom simply takes longer.
        let scale = cfg.body_scale * (1.0 + SCALE_JITTER * (2.0 * hash01_u32(seed ^ 0xC3) - 1.0));
        let yaw = hash01_u32(seed) * std::f32::consts::TAU;

        // Where it can actually stand, which way its stem curves, and how far off plumb it grew. A site that
        // cannot seat a body clear of the geometry grows nothing — `pin_scan` works at cell resolution and
        // will happily nominate a texel inside a wall slab.
        let Some(plan) = plan_body(&dungeon, world_xz, scale, seed) else {
            debug!("mycelia: no clear pose for a fruit body at {world_xz:?}; skipping the pin");
            continue;
        };

        // The vertex shader works in object space, so undo the entity's yaw here rather than handing the GPU
        // a transform it would have to invert per vertex. (Both are already in native-scale units.)
        let unyaw = |v: Vec2| {
            let r = Quat::from_rotation_y(-yaw) * Vec3::new(v.x, 0.0, v.y);
            Vec2::new(r.x, r.z)
        };
        let bend = unyaw(plan.bend);
        let tilt = unyaw(plan.tilt);

        let base = Vec3::new(plan.base.x, 0.0, plan.base.y);
        commands.spawn((
            Name::new("mycelia_fruit_body"),
            FruitBody {
                growth: 0.0,
                rise: 0.0,
                scale,
                cell: dungeon.world_to_cell(base),
                veil_triggered: false,
                tint: 0.0,
                bend,
                tilt,
            },
            // Spawns fully sunk: `rise = 0` puts the egg's crown exactly level with the floor.
            Transform::from_translation(base - Vec3::Y * (EGG_HEIGHT_M * scale))
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(scale)),
            Visibility::default(),
            WorldAssetRoot(scene.0.clone()),
        ));
        live += 1;
        pinned_this_run.push(base);
    }
}

/// The growth ODE. One expression, evaluated against the live zoom every frame.
///
/// `gate` carries the biology and is the only sign in the system: `+1` growing, `0` stalled at the veil
/// waiting to be seen, `-1` reabsorbing because the patch beneath it has collapsed.
fn grow_fruit_bodies(
    mut commands: Commands,
    cfg: Res<MyceliaConfig>,
    coarse: Res<MoldCoarse>,
    fog: Res<FogGrid>,
    view: Res<crate::camera::CameraView>,
    time: Res<Time<Real>>,
    mut bodies: Query<(Entity, &mut FruitBody, &mut Transform)>,
) {
    let dt = time.delta_secs();
    // The whole perceptual budget, in world units per second, at the zoom the player is actually at.
    let budget = v_max(cfg.motion_threshold_deg_per_s, cfg.screen_fov_deg_v, view.viewport_height);

    for (entity, mut body, mut transform) in &mut bodies {
        // Light-induced transition: once seen, the veil may rupture — and stays permitted thereafter.
        if fog.visible_at(body.cell) {
            body.veil_triggered = true;
        }

        let local_v = coarse.v_at(Vec2::new(transform.translation.x, transform.translation.z));
        let stalled = body.growth >= VEIL_RUPTURE_T && !body.veil_triggered;
        let gate = if local_v < cfg.maintain_v {
            -1.0
        } else if stalled {
            0.0
        } else {
            1.0
        };

        // Emergence and morph are one continuous clock: the body rises out of the mat first, then blends.
        // Reabsorption runs the same clock backwards, in the same order, reversed.
        let sink = EGG_HEIGHT_M * body.scale;
        let rise_rate = budget / sink;
        // A bent stem spends growth on curvature, so it crosses its bending segment slower. The bend's
        // extra vertex travel is charged to the same speed limit as the morph's — see `perceptual`.
        let morph_rate =
            growth_rate(body.growth, body.scale, body.bend.length(), body.tilt.length(), budget);

        if gate > 0.0 {
            if body.rise < 1.0 {
                body.rise = (body.rise + rise_rate * dt).min(1.0);
            } else {
                body.growth = (body.growth + morph_rate * dt).min(1.0);
            }
        } else if gate < 0.0 {
            if body.growth > 0.0 {
                body.growth = (body.growth - morph_rate * dt).max(0.0);
            } else if body.rise > 0.0 {
                body.rise = (body.rise - rise_rate * dt).max(0.0);
            } else {
                // Fully reabsorbed into the mat it came from.
                commands.entity(entity).despawn();
                continue;
            }
        }

        // The albedo shift chases `growth`, but never completes faster than a human's slow-change blindness
        // window. At max zoom-in `growth` is already slower than this and the limiter never binds; zoomed
        // out, where motion may run 7x faster, it does.
        let tint_step = dt / MIN_APPEARANCE_RAMP_SECS;
        body.tint += (body.growth - body.tint).clamp(-tint_step, tint_step);

        transform.translation.y = -sink * (1.0 - body.rise);
    }
}

/// Push each body's `growth` into its glTF morph weights.
///
/// Bevy 0.19 puts `MorphWeights` on the **node** entity and gives each primitive a
/// `MeshMorphWeights::Reference(parent)`, so only the parent is written — and only the parent *beneath this
/// body's own root*, never a global query, or every mushroom would stomp every other.
///
/// The scene instantiates asynchronously, so `MorphWeights` is absent for the first frame or two. That is
/// expected and skipped. A *wrong* number of targets is not: it would mean the mesh no longer matches
/// `perceptual::STAGE_MAX_DISP`, from which the entire speed limit is derived.
///
/// A non-finite `growth` is likewise fatal. `f32::clamp` propagates NaN rather than absorbing it, so
/// `stage_weights` would hand glTF a set of NaN blend weights and the mesh would collapse to garbage. No
/// current path produces one — `growth` is a clamped accumulation seeded at `0.0` — so if it ever happens the
/// integrator is broken and the only honest thing to do is say so.
fn drive_morph_weights(
    bodies: Query<(Entity, &FruitBody)>,
    children: Query<&Children>,
    mut weights: Query<&mut MorphWeights>,
) -> Result<(), BevyError> {
    for (root, body) in &bodies {
        if !body.growth.is_finite() {
            return Err(format!(
                "mycelia: fruit body {root} has growth = {}, which is not finite; the growth integrator \
                 produced a value that would drive the death cap's morph weights to NaN",
                body.growth
            )
            .into());
        }
        let target = stage_weights(body.growth);
        for descendant in children.iter_descendants(root) {
            let Ok(mut mw) = weights.get_mut(descendant) else {
                continue;
            };
            let slots = mw.weights_mut();
            if slots.len() != MORPH_TARGET_COUNT {
                return Err(format!(
                    "mycelia: {DEATH_CAP_GLB} exposes {} morph targets, expected {MORPH_TARGET_COUNT}; \
                     perceptual::STAGE_MAX_DISP describes a different mesh and the growth speed limit \
                     would be a lie",
                    slots.len()
                )
                .into());
            }
            slots.copy_from_slice(&target);
            // Exactly one node in the scene carries `MorphWeights`; the primitives reference it.
            break;
        }
    }
    Ok(())
}

/// Swap the glTF's flat `StandardMaterial`s for the mold-aware fruit material, once each, as the scene's
/// meshes finish loading. Mirrors `coat_furniture`, but mints **exactly one material per body** so each
/// mushroom can carry its own `tint`.
///
/// The `minted` map is load-bearing, not an optimisation. `commands.insert(FruitMaterial(..))` is deferred
/// to the end of the schedule, so within a single frame the `Option<&FruitMaterial>` lookup still reports
/// `None` for every subsequent descendant. The death cap has **three primitives** (cap, flesh, volva); mint
/// per descendant and you get three materials of which only the last is tracked, and `tint_fruit_bodies`
/// then updates one of them. The cap keeps `tint = 0` for ever and never greens.
#[allow(clippy::too_many_arguments)]
fn coat_fruit_bodies(
    mut commands: Commands,
    cfg: Res<MyceliaConfig>,
    images: Res<MoldImages>,
    control_image: Res<MoldControlImage>,
    bodies: Query<(Entity, &FruitBody, Option<&FruitMaterial>)>,
    children: Query<&Children>,
    painted: Query<&MeshMaterial3d<StandardMaterial>, Without<FruitCoated>>,
    std_materials: Res<Assets<StandardMaterial>>,
    mut fruit_materials: ResMut<Assets<MoldFruitMaterial>>,
) {
    let mut minted: HashMap<Entity, Handle<MoldFruitMaterial>> = HashMap::new();
    for (root, body, existing) in &bodies {
        for descendant in children.iter_descendants(root) {
            let Ok(mat) = painted.get(descendant) else {
                continue;
            };
            // The glTF material may not have finished loading; try again next frame.
            let Some(base) = std_materials.get(&mat.0) else {
                continue;
            };
            let already = existing.map(|f| f.0.clone()).or_else(|| minted.get(&root).cloned());
            let handle = match already {
                Some(h) => h,
                None => {
                    let h = fruit_materials.add(MoldFruitMaterial {
                        base: base.clone(),
                        extension: MoldFruitExt::new(
                            &cfg,
                            images.display.clone(),
                            control_image.dynamic.clone(),
                            body.tint,
                            body.bend,
                            body.tilt,
                        ),
                    });
                    commands.entity(root).insert(FruitMaterial(h.clone()));
                    minted.insert(root, h.clone());
                    h
                }
            };
            commands
                .entity(descendant)
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert((MeshMaterial3d(handle), FruitCoated));
        }
    }
}

/// Publish each body's rate-limited `tint` into its material uniform, so the cap can shift pale → olive as
/// it matures without ever crossing the slow-change-blindness threshold.
fn tint_fruit_bodies(
    bodies: Query<(&FruitBody, &FruitMaterial)>,
    mut materials: ResMut<Assets<MoldFruitMaterial>>,
) {
    for (body, FruitMaterial(handle)) in &bodies {
        // `Assets::get_mut` emits `AssetEvent::Modified`, which re-uploads the uniform. A mature body's
        // tint stops changing, so only touch the asset when it actually moved.
        let unchanged = materials.get(handle).is_some_and(|m| (m.extension.tint() - body.tint).abs() < 1e-5);
        if unchanged {
            continue;
        }
        let Some(mut material) = materials.get_mut(handle) else {
            continue;
        };
        material.extension.set_tint(body.tint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{MAX_ZOOM, MIN_ZOOM};
    use crate::mycelia::perceptual::{v_max as vmax, STAGE_T};

    const THRESH: f32 = 0.02;
    const FOV: f32 = 30.0;

    fn body() -> FruitBody {
        FruitBody {
            growth: 0.0,
            rise: 0.0,
            scale: 4.0,
            cell: IVec2::ZERO,
            veil_triggered: false,
            tint: 0.0,
            bend: Vec2::ZERO,
            tilt: Vec2::ZERO,
        }
    }

    /// An egg carries no amatoxins; a mature cap carries them all. The threshold is the veil rupture,
    /// because the toxin lives in the gills and cap, not the volva (Enjalbert et al. 1999).
    #[test]
    fn amatoxin_appears_only_once_the_cap_does() {
        let mut b = body();
        assert_eq!(b.amatoxin(), 0.0);
        b.growth = VEIL_RUPTURE_T;
        assert_eq!(b.amatoxin(), 0.0);
        b.growth = 1.0;
        assert_eq!(b.amatoxin(), 1.0);
        b.growth = (VEIL_RUPTURE_T + 1.0) * 0.5;
        assert!((b.amatoxin() - 0.5).abs() < 1e-5);
        // The stage list must actually contain the veil rupture where we think it does.
        assert_eq!(VEIL_RUPTURE_T, STAGE_T[3]);
    }

    /// Eating is exempt from the speed limit, and is monotonic and clamped: a bite bigger than what's left
    /// reabsorbs the body rather than driving `growth` negative.
    #[test]
    fn consume_runs_the_clock_backwards_and_clamps() {
        let mut b = body();
        b.growth = 0.5;
        b.consume(0.2);
        assert!((b.growth - 0.3).abs() < 1e-6);
        b.consume(10.0);
        assert_eq!(b.growth, 0.0);
        assert_eq!(b.energy(), 0.0);
    }

    /// Energy rises with maturity — an egg is not worth foraging, an adult is.
    #[test]
    fn energy_increases_with_growth() {
        let mut b = body();
        let mut last = -1.0;
        for i in 0..=10 {
            b.growth = i as f32 / 10.0;
            let e = b.energy();
            assert!(e > last, "energy must increase: {e} <= {last}");
            last = e;
        }
    }

    /// **The temporal-contrast invariant.** The albedo ramp is rate-limited independently of zoom, so it can
    /// never complete faster than the slow-change-blindness window even when motion is allowed to run 7×
    /// faster zoomed out. Simulated at a high frame rate against the fastest possible growth.
    #[test]
    fn tint_never_ramps_faster_than_the_slow_change_window() {
        let dt = 1.0 / 240.0;
        let mut tint = 0.0f32;
        let mut elapsed = 0.0f32;
        // Worst case: growth pinned at 1.0 from t=0, so the limiter is the only thing holding tint back.
        while tint < 1.0 && elapsed < 60.0 {
            let step = dt / MIN_APPEARANCE_RAMP_SECS;
            tint += (1.0 - tint).clamp(-step, step);
            elapsed += dt;
        }
        assert!(
            elapsed >= MIN_APPEARANCE_RAMP_SECS - 0.05,
            "tint completed in {elapsed}s, faster than the {MIN_APPEARANCE_RAMP_SECS}s window",
        );
    }

    /// The emergence rise obeys the same budget as everything else: the egg's crown never climbs out of the
    /// mat faster than the motion threshold, at any zoom.
    #[test]
    fn emergence_rise_obeys_the_speed_limit() {
        for viewport in [MIN_ZOOM, 12.0, MAX_ZOOM] {
            let budget = vmax(THRESH, FOV, viewport);
            let b = body();
            let sink = EGG_HEIGHT_M * b.scale;
            let rise_rate = budget / sink; // per second, in `rise` units
            // `rise` spans [0,1] over `sink` metres, so world speed is `rise_rate * sink`.
            let world_speed = rise_rate * sink;
            assert!(
                (world_speed - budget).abs() < 1e-6,
                "viewport {viewport}: rise speed {world_speed} != budget {budget}",
            );
        }
    }

    // ── plan_body's clearance contract ────────────────────────────────────────────────────────────────

    const TEST_SCALE: f32 = 4.0;

    /// A `CONTROL_SIZE`-square dungeon whose floor is the block `lo..=hi`; everything else is rock. The
    /// slab therefore stands on the outer edge of cell `hi` — the face a body near `x = hi` must clear.
    fn dungeon_with_floor_block(lo: i32, hi: i32) -> Dungeon {
        let size = crate::mycelia::CONTROL_SIZE as usize;
        let mut walkable = vec![false; size * size];
        for y in lo..=hi {
            for x in lo..=hi {
                walkable[y as usize * size + x as usize] = true;
            }
        }
        Dungeon::from_walkable(size, size, walkable)
    }

    /// `plan_body` documents that it "verifies its own answer" and that "a site that cannot host a body does
    /// not host one". Both halves are asserted here: every plan it hands back must be clear of solid matter.
    ///
    /// This is swept over seeds rather than fixed at one, because the pose is a *function of the draw*: the
    /// stem's lean and tilt are hashed from the coarse index. A single seed proves nothing about the site —
    /// it only samples one of the poses the site can produce. (Exactly this blind spot let the bug through:
    /// `MYCELIA_FRUIT_TESTBED` pins six seeds, and those six happened to clear.)
    #[test]
    fn plan_body_never_returns_a_pose_that_clips_a_wall() {
        let dungeon = dungeon_with_floor_block(40, 80);
        // The east slab stands on the outer edge of cell 80, i.e. at world x = 80.5 - WALL_THICKNESS.
        let face_x = 80.5 - crate::dungeon::WALL_THICKNESS;

        let mut clipped = Vec::new();
        // Negative offsets sit **inside** the slab strip. `pin_scan` really does hand those over: it rejects
        // texels whose dungeon *cell* is not walkable, but a slab occupies the outer `WALL_THICKNESS` of a
        // perfectly walkable cell. Solving from inside rock is where the verify-and-reseat loop earns its keep.
        // Positive offsets step out across the whole band a pose can reach.
        for step in -8..=60 {
            let offset = step as f32 * 0.01;
            let site = Vec2::new(face_x - offset, 60.0);
            for seed in 0..64u32 {
                let Some(plan) = plan_body(&dungeon, site, TEST_SCALE, seed) else {
                    continue; // Refusing the site is always a legal answer.
                };
                let depth = penetration(&dungeon, &plan, TEST_SCALE);
                if depth > 0.0 {
                    clipped.push((offset, seed, depth));
                }
            }
        }

        assert!(
            clipped.is_empty(),
            "plan_body returned {} poses that clip the slab; worst {:?}. \
             A returned plan must always be clear — refuse the site instead.",
            clipped.len(),
            clipped
                .iter()
                .max_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(o, s, d)| format!("offset {o:.2} m, seed {s}, {d:.4} m deep")),
        );
    }

    // ── pin_fruit_bodies: spacing, and the dwell clock ────────────────────────────────────────────────

    /// A world where every coarse cell is barren except the ones named, which are ripe (`V` high, `U`
    /// spent). Texel coordinates are chosen so the sites land on open floor, far from any slab.
    ///
    /// `Time<Real>` is inserted by hand rather than via `TimePlugin`, so the clock only moves when the test
    /// says so. The system's `Local`s persist across `app.update()`, which is the whole point — the readback
    /// gate lives in one.
    fn app_with_ripe_cells(cfg: MyceliaConfig, texels: &[f32]) -> App {
        let mut cells = vec![[0.0f32; 4]; (COARSE_SIZE * COARSE_SIZE) as usize];
        for (i, &tx) in texels.iter().enumerate() {
            // (V above v_fruit, U below u_exhausted, texel x, texel y)
            cells[i] = [0.9, 0.1, tx, 320.0];
        }

        let mut app = App::new();
        app.insert_resource(cfg)
            .insert_resource(MoldCoarse { cells, generation: 0 })
            .insert_resource(dungeon_with_floor_block(40, 80))
            .insert_resource(crate::fog::FogGrid::all_explored(
                crate::mycelia::CONTROL_SIZE as usize,
                crate::mycelia::CONTROL_SIZE as usize,
            ))
            .insert_resource(DeathCapScene(Handle::default()))
            .insert_resource(PinDwell::default())
            .insert_resource(Time::<Real>::default())
            .add_systems(Update, pin_fruit_bodies);
        app
    }

    fn body_count(app: &mut App) -> usize {
        let mut q = app.world_mut().query_filtered::<(), With<FruitBody>>();
        let n = q.iter(app.world()).count();
        n
    }

    /// Drive the app the way the game drives it: the render loop ticks at `fps`, and a readback lands only
    /// every `frames_per_scan`th frame. `pin_fruit_bodies` runs on `Update` every frame and gates itself.
    ///
    /// Advancing the clock one *frame* at a time is the whole point. A harness that advanced it one *scan* at
    /// a time would make `Time::delta_secs()` and the true inter-scan interval identical, and could not tell a
    /// correct dwell accumulator from one that credits a render frame.
    fn run_frames(app: &mut App, frames: usize, fps: f32, frames_per_scan: usize) -> f32 {
        let frame = std::time::Duration::from_secs_f32(1.0 / fps);
        for i in 1..=frames {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(frame);
            if i % frames_per_scan == 0 {
                app.world_mut().resource_mut::<MoldCoarse>().generation += 1;
            }
            app.update();
        }
        app.world().resource::<Time<Real>>().elapsed_secs()
    }

    /// Run frame-by-frame until a body pins, returning the real time on the clock when it did. `None` if it
    /// never pins within `max_frames`.
    fn time_to_first_pin(app: &mut App, max_frames: usize, fps: f32, frames_per_scan: usize) -> Option<f32> {
        let frame = std::time::Duration::from_secs_f32(1.0 / fps);
        for i in 1..=max_frames {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(frame);
            if i % frames_per_scan == 0 {
                app.world_mut().resource_mut::<MoldCoarse>().generation += 1;
            }
            app.update();
            if body_count(app) > 0 {
                return Some(app.world().resource::<Time<Real>>().elapsed_secs());
            }
        }
        None
    }

    /// Two cells that ripen together must not both pin: `commands.spawn` is deferred, so the second cell's
    /// crowding check cannot see the first body in the `World`. It has to see the pending position instead.
    ///
    /// The sites here are 1.5 world units apart against a `pin_min_spacing` of 3.0 — unambiguously crowded.
    #[test]
    fn two_cells_ripening_on_the_same_scan_pin_only_one_body() {
        let cfg = crate::config::load_game_config().expect("game config").mycelia;
        let spacing = cfg.pin_min_spacing;
        let (fps, per_scan) = frame_clock(&cfg);
        let budget_frames = frames_to_cover_the_dwell(&cfg, fps, per_scan);

        // 8 texels apart = 8 * 192/1024 = 1.5 world units.
        let mut app = app_with_ripe_cells(cfg, &[320.0, 328.0]);

        // Run well past the dwell threshold, so both cells certainly cross it on the same scan.
        run_frames(&mut app, budget_frames, fps, per_scan);

        let n = body_count(&mut app);
        assert_eq!(
            n, 1,
            "two cells 1.5 units apart (pin_min_spacing = {spacing}) ripened together and produced {n} \
             bodies; the second must be rejected by the same-scan crowding check",
        );
    }

    /// `pin_dwell_secs` is **real seconds**. The scan now runs once per readback (~`sim_hz`) rather than once
    /// per rendered frame, so it must credit the whole inter-scan interval — the elapsed span since the last
    /// scan, *not* `Time::delta_secs()`, which is one render frame.
    ///
    /// At 120 fps and `sim_hz` 1.5 that is an 80x error: a 6 s dwell would become 480 s and mushrooms would
    /// effectively stop appearing, with every other test in this suite still green.
    #[test]
    fn dwell_is_credited_in_real_seconds_not_render_frames() {
        let cfg = crate::config::load_game_config().expect("game config").mycelia;
        let (fps, per_scan) = frame_clock(&cfg);
        let scan_secs = per_scan as f32 / fps;
        let dwell = cfg.pin_dwell_secs;

        let max_frames = frames_to_cover_the_dwell(&cfg, fps, per_scan);
        let mut app = app_with_ripe_cells(cfg, &[320.0]);

        // Budget the dwell plus a couple of scans. A frame-delta accumulator needs ~80x that and will not
        // arrive, which is the regression this test exists to catch.
        let t = time_to_first_pin(&mut app, max_frames, fps, per_scan).unwrap_or_else(|| {
            panic!(
                "a lone ripe cell never pinned within {:.0} s of real time, though `pin_dwell_secs` is \
                 {dwell} s. The dwell accumulator is crediting far less than the elapsed interval.",
                2.0 * dwell,
            )
        });

        // The first scan credits nothing (no previous scan to measure from), so the pin lands one scan late.
        let expected = dwell + scan_secs;
        assert!(
            (t - expected).abs() <= scan_secs + 1e-3,
            "pinned after {t:.3} s of real time; expected near {expected:.3} s ({dwell} s dwell + one scan)",
        );
    }

    /// A non-finite `growth` must stop the frame, not silently reach glTF. `f32::clamp` propagates NaN, so
    /// `stage_weights` would emit NaN blend weights and the mesh would collapse. The guard sits ahead of the
    /// descendant walk, so it fires even before the scene has instantiated.
    #[test]
    fn drive_morph_weights_rejects_a_non_finite_growth() {
        use bevy::ecs::system::RunSystemOnce;

        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut world = World::new();
            let mut b = body();
            b.growth = bad;
            world.spawn(b);
            let out: Result<(), BevyError> =
                world.run_system_once(drive_morph_weights).expect("system should run");
            assert!(out.is_err(), "growth = {bad} must be a hard error, not a silent NaN morph weight");
        }

        // And a healthy body is not disturbed by the guard.
        let mut world = World::new();
        let mut b = body();
        b.growth = 0.5;
        world.spawn(b);
        let out: Result<(), BevyError> =
            world.run_system_once(drive_morph_weights).expect("system should run");
        assert!(out.is_ok(), "a finite growth must pass the guard");
    }

    /// A one-cell-wide corridor: slabs on both flanks, symmetric, so `wall_escape`'s weighted push cancels to
    /// `Vec2::ZERO`. There is no direction to solve along — `deepest_push` cannot move the base and the stem
    /// cannot bend away from one wall without bending into the other.
    ///
    /// This is the case that makes the `penetration` gate load-bearing rather than decorative. A pose whose
    /// lean drives the cap into a flank must be **refused**, and only the check catches it: the solve loop
    /// happily returns a clipping plan on its first pass, because it computed a zero push and believes it.
    #[test]
    fn plan_body_refuses_a_corridor_pose_it_cannot_solve() {
        let size = crate::mycelia::CONTROL_SIZE as usize;
        let mut walkable = vec![false; size * size];
        for y in 40..=80 {
            walkable[y * size + 60] = true; // a single column of floor: rock at x = 59 and x = 61
        }
        let dungeon = Dungeon::from_walkable(size, size, walkable);

        let mut clipped = Vec::new();
        let mut refused = 0;
        for seed in 0..256u32 {
            match plan_body(&dungeon, Vec2::new(60.0, 60.0), TEST_SCALE, seed) {
                None => refused += 1,
                Some(plan) => {
                    let depth = penetration(&dungeon, &plan, TEST_SCALE);
                    if depth > 0.0 {
                        clipped.push((seed, depth));
                    }
                }
            }
        }

        assert!(
            clipped.is_empty(),
            "{} corridor poses clip a flank; worst {:?}. With no escape direction the only correct answer \
             is to refuse the site — verify the pose before returning it.",
            clipped.len(),
            clipped.iter().max_by(|a, b| a.1.total_cmp(&b.1)),
        );
        assert!(
            refused > 0,
            "a 1-cell corridor should defeat at least some poses; none were refused, so this test is not \
             exercising the unsolvable case it claims to",
        );
    }

    /// The case a single escape direction serves worst: an inside corner, where one diagonal push must clear
    /// two faces at once and under-clears each by `1/√2`. The first solve iteration is not enough here — the
    /// pose has to be checked and the base re-seated. This is what `plan_body`'s `penetration` gate is *for*.
    #[test]
    fn plan_body_clears_an_inside_corner() {
        let dungeon = dungeon_with_floor_block(40, 80);
        let wt = crate::dungeon::WALL_THICKNESS;
        // The south-west inside corner of the floor block: slabs on the west face of cell 40 and its south.
        let corner = Vec2::new(39.5 + wt, 39.5 + wt);
        let diag = Vec2::new(1.0, 1.0).normalize();

        let mut clipped = Vec::new();
        for step in 0..=60 {
            let site = corner + diag * (step as f32 * 0.01);
            for seed in 0..64u32 {
                let Some(plan) = plan_body(&dungeon, site, TEST_SCALE, seed) else {
                    continue;
                };
                let depth = penetration(&dungeon, &plan, TEST_SCALE);
                if depth > 0.0 {
                    clipped.push((step, seed, depth));
                }
            }
        }

        assert!(
            clipped.is_empty(),
            "{} corner poses clip; worst {:?}. A returned plan must be verified and re-seated, \
             not trusted after one solve pass.",
            clipped.len(),
            clipped.iter().max_by(|a, b| a.2.total_cmp(&b.2)),
        );
    }

    /// The 16,384-cell scan must run once per readback, not once per rendered frame. `MoldCoarse` only
    /// changes at `sim_hz`, so rescanning at the display's refresh rate repeats identical work ~80x.
    ///
    /// This is a *performance* invariant, and the dwell clock cannot detect it: because the accumulator
    /// credits elapsed time, an ungated scan still pins on schedule — it just burns 80x the CPU getting
    /// there. So assert the gate directly: with no new readback, the scan must not touch `PinDwell` at all.
    #[test]
    fn the_coarse_scan_is_skipped_when_no_new_readback_landed() {
        let cfg = crate::config::load_game_config().expect("game config").mycelia;
        let (fps, _) = frame_clock(&cfg);
        let mut app = app_with_ripe_cells(cfg, &[320.0]);

        // One readback: the cell is seen, and starts its dwell at zero (no prior scan to measure from).
        let frame = std::time::Duration::from_secs_f32(1.0 / fps);
        app.world_mut().resource_mut::<Time<Real>>().advance_by(frame);
        app.world_mut().resource_mut::<MoldCoarse>().generation += 1;
        app.update();
        let after_scan = app.world().resource::<PinDwell>().0.get(&0).copied();
        assert_eq!(after_scan, Some(0.0), "the first scan must register the cell with zero dwell");

        // Now run 200 frames with no new readback. The buffer has not changed, so neither may the dwell.
        for _ in 0..200 {
            app.world_mut().resource_mut::<Time<Real>>().advance_by(frame);
            app.update();
        }

        let held = app.world().resource::<PinDwell>().0.get(&0).copied();
        assert_eq!(
            held,
            Some(0.0),
            "dwell advanced without a new readback: the scan ran on frames where `MoldCoarse` was unchanged",
        );
        assert_eq!(body_count(&mut app), 0, "no body may pin from re-scanning stale data");
    }

    /// Sites in the band between the cap's radius and the pose envelope must still be *plannable*, not merely
    /// refused. Verifying the pose without widening the probe would make `plan_body` reject them — the bodies
    /// would stop clipping, and also stop existing. Both halves of the fix are load-bearing.
    #[test]
    fn sites_inside_the_old_blind_band_still_get_a_pose() {
        let dungeon = dungeon_with_floor_block(40, 80);
        let face_x = 80.5 - crate::dungeon::WALL_THICKNESS;
        let cap_reach = CAP_RADIUS_M * TEST_SCALE + WALL_MARGIN;
        let envelope = pose_envelope_m() * TEST_SCALE + WALL_MARGIN;

        let mut refused = 0;
        let mut total = 0;
        let mut offset = cap_reach;
        while offset < envelope {
            let site = Vec2::new(face_x - offset, 60.0);
            for seed in 0..64u32 {
                total += 1;
                if plan_body(&dungeon, site, TEST_SCALE, seed).is_none() {
                    refused += 1;
                }
            }
            offset += 0.01;
        }

        assert!(total > 0, "the blind band must be non-empty");
        assert_eq!(
            refused, 0,
            "{refused}/{total} sites between the cap radius ({cap_reach:.3} m) and the pose envelope \
             ({envelope:.3} m) were refused. Widen the wall probe to the envelope rather than rejecting them.",
        );
    }

    /// The shipped render/sim clock, as the pinning path actually sees it.
    fn frame_clock(cfg: &MyceliaConfig) -> (f32, usize) {
        let fps = 120.0;
        (fps, (fps / cfg.sim_hz) as usize)
    }

    /// Frames enough for a lone ripe cell to certainly pin.
    ///
    /// The dwell is credited **once per scan**, not per frame, and the first scan credits nothing (it has no
    /// previous scan to measure from). So the pin lands on scan `ceil(dwell / scan) + 1`, and a budget
    /// expressed in dwell-seconds is only enough while a scan is short compared to the dwell. It no longer
    /// is: at the shipped `sim_hz` a scan is 13.3 s against a 20 s dwell. Budget in *scans*, from the config,
    /// so this keeps holding whichever way the clock is tuned.
    fn frames_to_cover_the_dwell(cfg: &MyceliaConfig, fps: f32, per_scan: usize) -> usize {
        let scan_secs = per_scan as f32 / fps;
        let scans = (cfg.pin_dwell_secs / scan_secs).ceil() + 2.0;
        (scans * scan_secs * fps) as usize
    }

    /// The pose envelope really is wider than the cap: a body may lean and tilt its silhouette out beyond
    /// `CAP_RADIUS_M`. Any wall probe that only reaches the cap radius is blind to slabs the body can hit.
    #[test]
    fn the_pose_envelope_exceeds_the_cap_radius() {
        let lean_max = LEAN_FRACTION * ADULT_HEIGHT_M;
        let sway = MAX_TILT * ADULT_HEIGHT_M + MAX_BEND_M.max(lean_max);
        assert!(sway > 0.0);
        assert!(
            pose_envelope_m() > CAP_RADIUS_M,
            "envelope {} must exceed the bare cap radius {CAP_RADIUS_M}",
            pose_envelope_m(),
        );
        // And the miss is large, not a rounding detail: at the shipped scale it is centimetres of blind band.
        let blind = (pose_envelope_m() - CAP_RADIUS_M) * TEST_SCALE;
        assert!(blind > 0.2, "blind band {blind} m is suspiciously small");
    }
}

