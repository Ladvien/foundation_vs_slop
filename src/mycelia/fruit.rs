//! Fruit bodies — the mold reproducing.
//!
//! Where the mat has grown thick and eaten its substrate, in the dark, a death cap erupts: a single
//! `death_cap_growth.glb` mesh blended from sealed egg to adult across six glTF morph targets under one
//! `growth: f32`. It grows in real time, and it grows **too slowly to see**.
//!
//! # The biology, and where it already lives in the fields
//!
//! Gray-Scott integrates `U` (substrate) and `V` (biomass) via the autocatalytic `U + 2V → 3V`. Real
//! Agaricomycetes fruit once a colony has accumulated **critical mycelial mass** *and* **exhausted its
//! nutrients** — nitrogen starvation is among the strongest maturation cues (Zhang et al. 2015, PLoS ONE
//! 10:e0123025, 10.1371/journal.pone.0123025; morphogenesis review: Kües & Navarro-González 2015, Fungal
//! Biol. Rev. 29:63, 10.1016/j.fbr.2015.05.001). That is exactly `V > v_fruit && U < u_exhausted`. The
//! trigger needed no new state; it was already being integrated on the GPU every tick.
//!
//! Two more rules, both real phenomena rather than engineering conveniences:
//!
//! - **Dark-dependent initiation, light-induced rupture.** In *Coprinopsis cinerea*, primary hyphal knots
//!   form **in the dark**, and their transition to the compact secondary knot is **light-induced** (Liu et
//!   al. 2006, Genetics 172:873, 10.1534/genetics.105.045542; Kües 2000, MMBR 64:316,
//!   10.1128/mmbr.64.2.316-353.2000). The fog is our light proxy, as it already is for the mold's
//!   photophobia. So a pin commits only in a cell **no unit can currently see**, and the universal veil
//!   only ruptures once a unit **looks at it**. The mold hides from you; its fruiting body wants an
//!   audience.
//! - **Primordium abortion.** Most knots never mature; neighbours compete for translocated nutrient (Kües
//!   & Navarro-González 2015). Hence `pin_min_spacing`, and hence a body whose local `V` collapses below
//!   `maintain_v` running its growth clock **backwards** until it is reabsorbed. Not a fallback branch —
//!   the same ODE with a negative sign.
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
//! Amatoxins concentrate in the pileus rather than the stipe or volva (Enjalbert et al. 1993, Toxicon
//! 31:803, 10.1016/0041-0101(93)90386-w), so [`FruitBody::amatoxin`] is a function of `growth` alone — a
//! body is only poisonous once it has a cap to hold them.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::render::gpu_readback::ReadbackComplete;
use bevy::time::Real;

use crate::dungeon::Dungeon;
use crate::fog::FogGrid;

use super::material::{MoldFruitExt, MoldFruitMaterial};
use super::control::{self, MoldControlImage};
use super::perceptual::{
    growth_rate, stage_weights, v_max, ADULT_HEIGHT_M, CAP_RADIUS_M, EGG_HEIGHT_M, MAX_BEND_M,
    MIN_APPEARANCE_RAMP_SECS, VEIL_RUPTURE_T, VOLVA_RADIUS_M,
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
}

impl FruitBody {
    /// Food value available right now, in arbitrary units. An egg is mostly volva and water; a mature body
    /// is worth eating. Scales with volume, hence the cube.
    pub fn energy(&self) -> f32 {
        self.growth * self.growth * self.growth * self.scale * self.scale * self.scale
    }

    /// Amatoxin load, `0..1`. Zero until the cap has expanded past [`VEIL_RUPTURE_T`], because amatoxins
    /// concentrate in the pileus rather than the stipe or volva (Enjalbert et al. 1993,
    /// 10.1016/0041-0101(93)90386-w). The mesh's `COLOR_0` part mask partitions it the same way, so the
    /// shader and this function agree without either knowing about the other.
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

/// The mold's coarse biomass grid, read back from the GPU each frame.
///
/// One entry per `COARSE_SIZE²` cell: `(max V in the block, U at that same texel, texel x, texel y)`.
/// Written by the `pin_scan` compute pass — see `mycelia_sim.wgsl`.
#[derive(Resource, Default)]
pub struct MoldCoarse {
    /// `COARSE_SIZE * COARSE_SIZE` entries. Empty until the first readback lands.
    cells: Vec<[f32; 4]>,
}

impl MoldCoarse {
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
    Ok(())
}

/// World XZ of the centre of a field texel.
fn field_texel_to_world(texel: Vec2, field_size: f32) -> Vec2 {
    WORLD_ORIGIN + (texel + Vec2::splat(0.5)) / field_size * WORLD_EXTENT
}

/// Integer hash → uniform `f32` in `[0,1)`. Decorrelates each body's yaw, lean and scale, so a flush does
/// not look stamped from one mould. Seeded from the coarse index, so it is reproducible.
fn hash01(x: u64) -> f32 {
    let mut s = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    s ^= s >> 30;
    s = s.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    s ^= s >> 27;
    s = s.wrapping_mul(0x94D0_49BB_1331_11EB);
    s ^= s >> 31;
    (s >> 40) as f32 / (1u64 << 24) as f32
}

/// How many rays the wall probe casts around a candidate site.
const PROBE_RAYS: usize = 24;
/// Step along each probe ray, world units. Well under `WALL_THICKNESS`, so a slab can never be stepped over.
const PROBE_STEP: f32 = 0.03;
/// Breathing room between the volva and the wall face it is nudged clear of, world units.
const VOLVA_MARGIN: f32 = 0.02;
/// Peak random lean, as a fraction of adult height — the natural crookedness of a stem with nothing to
/// avoid. Small: real stipes are near-vertical (negatively gravitropic), just never perfectly so.
const LEAN_FRACTION: f32 = 0.18;
/// Per-body size jitter, ±this fraction of `body_scale`. A flush is not a set of clones.
const SCALE_JITTER: f32 = 0.18;

/// What the geometry around a candidate site demands of a stem planted there.
struct WallEscape {
    /// Unit direction away from the nearest solid, or zero if nothing is within reach.
    away: Vec2,
    /// Distance (world units) to the nearest solid along any probe ray; infinite in the open.
    nearest: f32,
}

/// Probe the solid geometry around `site` and work out which way a stem there would have to lean.
///
/// Casts [`PROBE_RAYS`] rays and marches each until it hits a wall slab or rock. Each hit pushes back with
/// a weight equal to how far it intrudes inside the cap's radius, so a stem in a corner leans out along the
/// diagonal and a stem in the open leans nowhere. It reads the *slab band* — not the cell boundary —
/// through `control::solid_at_world`, so the direction it returns points away from the surface the player
/// actually sees.
fn wall_escape(dungeon: &Dungeon, site: Vec2, cap_radius: f32) -> WallEscape {
    let reach = cap_radius + PROBE_STEP;
    let mut push = Vec2::ZERO;
    let mut nearest = f32::INFINITY;

    for i in 0..PROBE_RAYS {
        let angle = std::f32::consts::TAU * (i as f32) / (PROBE_RAYS as f32);
        let dir = Vec2::from_angle(angle);

        let mut r = PROBE_STEP;
        while r <= reach {
            if control::solid_at_world(dungeon, site + dir * r) {
                nearest = nearest.min(r);
                // Weight by intrusion: a slab grazing the cap's rim barely steers; one under the stipe
                // steers hard.
                push -= dir * (cap_radius - r).max(0.0);
                break;
            }
            r += PROBE_STEP;
        }
    }

    WallEscape { away: push.normalize_or_zero(), nearest }
}

/// Where a body planted near `site` should actually stand, and how far its apex should lean, in **world**
/// XZ metres.
///
/// Thigmotropism first: if the cap would overhang a slab, the stem curves out by exactly the overhang.
/// Then a per-body random lean on top, so no two stems match. The two are clamped together, because a stem
/// bent past [`MAX_BEND_M`] reads as snapped rather than curved.
///
/// The base nudge is a separate, purely geometric thing: a volva cannot occupy rock. It fires only when the
/// site sits closer than the sac's own radius to a wall, and moves the body by at most that radius. It is
/// *not* a keep-out rule — a mushroom may still grow with its sac against the skirting, which is precisely
/// where the mold pools (`wall_affinity`). Only the cap is carried clear, and by bending.
fn plan_body(dungeon: &Dungeon, site: Vec2, scale: f32, seed: u64) -> (Vec2, Vec2) {
    let cap_r = CAP_RADIUS_M * scale;
    let volva_r = VOLVA_RADIUS_M * scale;

    // A volva cannot grow inside a wall. Push the base out just far enough that it does not.
    let probe = wall_escape(dungeon, site, cap_r);
    let want = volva_r + VOLVA_MARGIN;
    let nudge = if probe.nearest < want { probe.away * (want - probe.nearest) } else { Vec2::ZERO };
    let base = site + nudge;

    // Re-probe from where the body will actually stand: the nudge changed what it can see.
    let probe = wall_escape(dungeon, base, cap_r);
    // Thigmotropic lean: carry the cap's rim back out of the slab it would otherwise pass through.
    let overhang = if probe.nearest.is_finite() { (cap_r - probe.nearest).max(0.0) } else { 0.0 };
    let thigmo = probe.away * overhang;

    // ...plus the crookedness every stem has anyway.
    let angle = hash01(seed ^ 0xA1) * std::f32::consts::TAU;
    let lean_mag = hash01(seed ^ 0xB2) * LEAN_FRACTION * ADULT_HEIGHT_M * scale;
    let lean = Vec2::from_angle(angle) * lean_mag;

    let bend = (thigmo + lean).clamp_length_max(MAX_BEND_M * scale);
    (base, bend)
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
    bodies: Query<&Transform, With<FruitBody>>,
) {
    if coarse.cells.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let field_size = cfg.field_size as f32;
    let mut live = bodies.iter().count() as u32;

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

        // Primordium competition: neighbours starve each other out (Kües & Navarro-González 2015).
        let crowded = crate::util::nearest_planar(world, bodies.iter().map(|t| ((), t.translation)))
            .is_some_and(|(_, _, d)| d < cfg.pin_min_spacing);
        if crowded {
            continue;
        }

        let seed = index as u64;
        // Size varies across a flush. Growth time scales with it, since the speed limit bounds vertex
        // *speed* and a bigger body has further to travel — a large mushroom simply takes longer.
        let scale = cfg.body_scale * (1.0 + SCALE_JITTER * (2.0 * hash01(seed ^ 0xC3) - 1.0));
        let yaw = hash01(seed) * std::f32::consts::TAU;

        // Where it can actually stand, and which way its stem must curve to keep its cap out of the wall.
        let (base_xz, bend_world) = plan_body(&dungeon, world_xz, scale, seed);

        // The vertex shader bends in object space, so undo the entity's yaw and scale here rather than
        // handing the GPU a transform it would have to invert per vertex.
        let bend_local = Quat::from_rotation_y(-yaw) * Vec3::new(bend_world.x, 0.0, bend_world.y);
        let bend = Vec2::new(bend_local.x, bend_local.z) / scale;

        let base = Vec3::new(base_xz.x, 0.0, base_xz.y);
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
            },
            // Spawns fully sunk: `rise = 0` puts the egg's crown exactly level with the floor.
            Transform::from_translation(base - Vec3::Y * (EGG_HEIGHT_M * scale))
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(scale)),
            Visibility::default(),
            WorldAssetRoot(scene.0.clone()),
        ));
        live += 1;
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
        let morph_rate = growth_rate(body.growth, body.scale, body.bend.length(), budget);

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
fn drive_morph_weights(
    bodies: Query<(Entity, &FruitBody)>,
    children: Query<&Children>,
    mut weights: Query<&mut MorphWeights>,
) -> Result<(), BevyError> {
    for (root, body) in &bodies {
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
        }
    }

    /// An egg carries no amatoxins; a mature cap carries them all. The threshold is the veil rupture,
    /// because the toxin lives in the pileus (Enjalbert et al. 1993).
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
}

